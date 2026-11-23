//! The "New lighting" section of the Settings page: for KOTOR and KOTOR II,
//! whether the new lighting is installed, and the buttons to download it
//! and to remove it. kjk is asked on a background thread, and the download
//! runs as a job, like an install of a version.

use super::*;
use crate::lighting::{self as light, Offer, State};

/// What the selected version says about the new lighting.
pub(super) enum Lighting {
    /// Not asked for the selected version yet, or asked again.
    Stale,
    /// Asked, and waiting for the answer for this version.
    Asking(String),
    /// The answer for this version.
    Known(String, light::Status),
}

impl Ready {
    /// Asks the selected version about the new lighting on a background
    /// thread, unless it was asked already.
    fn ask_lighting(&mut self, ctx: &egui::Context, current: &Installed) {
        let fresh = match &self.lighting {
            Lighting::Stale => false,
            Lighting::Asking(tag) | Lighting::Known(tag, _) => *tag == current.tag,
        };
        if fresh {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let kjk = install::game_binary(&current.dir);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(light::status(&kjk));
            ctx.request_repaint();
        });
        self.lighting_asking = Some(rx);
        self.lighting = Lighting::Asking(current.tag.clone());
    }

    /// Takes kjk's answer about the new lighting, when it has come.
    pub(super) fn poll_lighting(&mut self) {
        let Some(rx) = &self.lighting_asking else {
            return;
        };
        let Ok(status) = rx.try_recv() else { return };
        self.lighting_asking = None;
        if let Lighting::Asking(tag) = &self.lighting {
            self.lighting = Lighting::Known(tag.clone(), status);
        }
    }

    /// The "New lighting" section.
    pub(super) fn lighting_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(current) = self.current().cloned() else {
            return;
        };
        self.ask_kjk(ctx, &current);
        self.ask_lighting(ctx, &current);
        widgets::section_label(ui, "New lighting");
        let status = match &self.lighting {
            Lighting::Known(_, status) => status.clone(),
            _ => {
                widgets::muted(ui, "The launcher is asking kjk about the new lighting.");
                ui.add_space(theme::SPACE_6);
                return;
            }
        };
        if status == light::Status::TooOld {
            widgets::muted(ui, "The new lighting needs a newer version of kjk.");
            ui.add_space(theme::SPACE_6);
            return;
        }
        widgets::muted(
            ui,
            "The new lighting is an optional download. It is made for KOTOR with the K1 Community Patch installed, and for KOTOR II with TSLRCM installed.",
        );
        ui.add_space(theme::SPACE_2);
        // Only the games kjk finds get a row.
        let found: Vec<Game> = match &self.games {
            Games::Known(_, Some(report), _) => light::GAMES
                .into_iter()
                .filter(|game| matches!(report.status(*game), Some(GameStatus::Found { .. })))
                .collect(),
            _ => Vec::new(),
        };
        if found.is_empty() {
            let words = if matches!(self.games, Games::Known(..)) {
                "kjk has not found KOTOR or KOTOR II, so the new lighting cannot be installed."
            } else {
                "The launcher is asking kjk where the games are."
            };
            widgets::muted(ui, words);
            ui.add_space(theme::SPACE_6);
            return;
        }
        for game in found {
            widgets::hairline(ui);
            let state = status.state(game).cloned().unwrap_or(State::Missing);
            self.lighting_row(ui, ctx, &current, game, &state);
        }
        widgets::hairline(ui);
        ui.add_space(theme::SPACE_6);
    }

    /// One game's row: its state, what the last download found, and the
    /// buttons.
    fn lighting_row(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        current: &Installed,
        game: Game,
        state: &State,
    ) {
        let offers = light::available(&self.releases, game);
        let offer = offers.first().cloned();
        let newer = offer.as_ref().is_some_and(|o| light::newer(state, o));
        let idle = self.job.is_none();
        let (mut download, mut remove, mut toggle) = (false, false, false);
        let open = self.lighting_open.contains(game.key());
        let done = self.lighting_done.get(game.key()).cloned();
        widgets::row(
            ui,
            |ui| {
                ui.label(widgets::text(game.name(), Text::Heading, theme::INK));
                match state {
                    State::Installed { areas, .. } => widgets::muted(
                        ui,
                        format!("The new lighting is installed, with {}.", areas_word(*areas)),
                    ),
                    State::Broken(_) => widgets::muted(
                        ui,
                        "The new lighting does not read, so the game does not use it. Remove it, then download it again.",
                    ),
                    State::Failed(why) => widgets::muted(ui, why.as_str()),
                    State::Missing => {}
                }
                if newer {
                    widgets::muted(ui, "A newer download of the new lighting is available.");
                }
                if let Some(done) = &done {
                    toggle = outcome_lines(ui, game, done, open);
                }
                let wanted = matches!(state, State::Missing | State::Broken(_)) || newer;
                match &offer {
                    Some(offer) if wanted => {
                        ui.add_space(theme::SPACE_2);
                        let label = download_label(game, state, offer);
                        download = ui
                            .add_enabled_ui(idle, |ui| widgets::compact_button(ui, &label))
                            .inner
                            .clicked();
                    }
                    None if matches!(state, State::Missing) => {
                        widgets::muted(ui, "The new lighting is not available yet.");
                    }
                    _ => {}
                }
            },
            |ui| {
                if matches!(state, State::Installed { .. } | State::Broken(_)) {
                    remove = ui
                        .add_enabled_ui(idle, |ui| widgets::compact_button(ui, "Remove"))
                        .inner
                        .clicked();
                    ui.add_space(theme::SPACE_4);
                }
                let (dot, word) = match state {
                    State::Installed { .. } if newer => (Status::Warn, "Update available"),
                    State::Installed { .. } => (Status::Ok, "Installed"),
                    State::Broken(_) | State::Failed(_) => (Status::Error, "Not usable"),
                    State::Missing => (Status::Warn, "Not installed"),
                };
                widgets::status(ui, dot, word);
            },
        );
        if toggle {
            if open {
                self.lighting_open.remove(game.key());
            } else {
                self.lighting_open.insert(game.key());
            }
        }
        if download {
            self.start_lighting(ctx, current, game, offers);
        }
        if remove {
            self.start_lighting_remove(ctx, current, game);
        }
    }

    /// Downloads and installs the new lighting for `game` as a job.
    fn start_lighting(
        &mut self,
        ctx: &egui::Context,
        current: &Installed,
        game: Game,
        offers: Vec<Offer>,
    ) {
        if self.job.is_some() {
            return;
        }
        self.clear_error();
        self.lighting_done.remove(game.key());
        let progress = Arc::new(Progress::default());
        let (tx, rx) = mpsc::channel();
        let kjk = install::game_binary(&current.dir);
        let downloads = self.paths.downloads.clone();
        let worker = progress.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = light::download(&kjk, &offers, &downloads, &worker);
            let _ = tx.send(Outcome::Lighting { game, result });
            ctx.request_repaint();
        });
        self.job = Some(Job {
            title: format!("Downloading the new lighting for {}", game.name()),
            progress,
            cancellable: true,
            items: false,
            done: rx,
        });
    }

    /// Removes the new lighting of `game` as a job.
    fn start_lighting_remove(&mut self, ctx: &egui::Context, current: &Installed, game: Game) {
        if self.job.is_some() {
            return;
        }
        self.clear_error();
        self.lighting_done.remove(game.key());
        let progress = Arc::new(Progress::default());
        progress.stage("Removing");
        let (tx, rx) = mpsc::channel();
        let kjk = install::game_binary(&current.dir);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = light::remove(&kjk, game);
            let _ = tx.send(Outcome::LightingRemoved { game, result });
            ctx.request_repaint();
        });
        self.job = Some(Job {
            title: format!("Removing the new lighting for {}", game.name()),
            progress,
            cancellable: false,
            items: false,
            done: rx,
        });
    }

    /// Takes the end of a download or a removal of the new lighting.
    pub(super) fn lighting_finished(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Lighting { game, result } => {
                self.notice = match &result {
                    Ok(light::Outcome::Installed { .. }) => Some(Notice::Info(format!(
                        "The new lighting for {} is installed.",
                        game.name()
                    ))),
                    Ok(_) => None,
                    Err(e) if e == "cancelled" => Some(Notice::Info(
                        "The download was cancelled. Nothing was installed.".into(),
                    )),
                    Err(e) => Some(Notice::Error(format!(
                        "Downloading the new lighting failed. Nothing was installed. {e}"
                    ))),
                };
                if let Ok(done) = result {
                    self.lighting_done.insert(game.key(), done);
                }
            }
            Outcome::LightingRemoved { game, result } => {
                self.notice = Some(match result {
                    Ok(()) => {
                        Notice::Info(format!("The new lighting for {} is removed.", game.name()))
                    }
                    Err(e) => Notice::Error(format!(
                        "The new lighting for {} could not be removed. {e}",
                        game.name()
                    )),
                });
            }
            _ => return,
        }
        self.lighting = Lighting::Stale;
    }
}

/// "1 area" or "35 areas".
fn areas_word(count: usize) -> String {
    if count == 1 {
        "1 area".into()
    } else {
        format!("{count} areas")
    }
}

/// "Download the new lighting for KOTOR (about 300 MB)", or for an update
/// "Download the update (about 300 MB)".
fn download_label(game: Game, state: &State, offer: &Offer) -> String {
    let bytes = if offer.zip.size > 0 {
        offer.zip.size
    } else {
        light::usual_size(game)
    };
    match state {
        State::Installed { .. } => format!("Download the update (about {})", size(bytes)),
        _ => format!(
            "Download the new lighting for {} (about {})",
            game.name(),
            size(bytes)
        ),
    }
}

/// What the last download for `game` found, with the list of the areas
/// that keep the game's own lighting while `open`. Returns whether the
/// player pressed the button that opens or closes that list.
fn outcome_lines(ui: &mut egui::Ui, game: Game, done: &light::Outcome, open: bool) -> bool {
    let mut toggle = false;
    match done {
        light::Outcome::Installed { check, .. } => {
            widgets::muted(
                ui,
                "To use it, close kjk if it is running and start it again. Then open Options and set Lighting to Enhanced Lightmaps.",
            );
            if !check.placeables_differ.is_empty() {
                widgets::muted(
                    ui,
                    format!(
                        "In {}, the objects in the room differ from the ones the lighting was made with. Those objects keep their usual light.",
                        areas_word(check.placeables_differ.len())
                    ),
                );
            }
            if !check.fall_back.is_empty() {
                widgets::muted(
                    ui,
                    format!(
                        "{} of the {} keep the game's own lighting, because your game files there differ from the ones the lighting was made from.",
                        check.fall_back.len(),
                        areas_word(check.areas)
                    ),
                );
                let label = if open {
                    "Hide the areas"
                } else {
                    "Show the areas"
                };
                toggle = widgets::text_button(ui, label).clicked();
                if open {
                    let names: Vec<&str> = check.fall_back.iter().map(|f| f.area.as_str()).collect();
                    widgets::wrapped_mono(ui, &names.join(", "), theme::INK_MUTED);
                }
            }
        }
        light::Outcome::Refused { why } => widgets::muted(ui, why.as_str()),
        light::Outcome::NeedsNewerKjk => widgets::muted(
            ui,
            "This download of the new lighting needs a newer version of kjk. Update kjk, then try again.",
        ),
        light::Outcome::NeedsNewerDownload => widgets::muted(
            ui,
            format!(
                "This version of kjk needs a newer download of the new lighting for {}. There is none yet.",
                game.name()
            ),
        ),
    }
    toggle
}
