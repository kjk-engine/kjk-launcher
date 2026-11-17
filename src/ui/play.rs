//! The Play page: the selected version, Play, the update notice and the
//! "Before you play" rows for the games and ffmpeg.

use super::*;

impl Ready {
    pub(super) fn play_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.progress(ui);

        let Some(current) = self.current().cloned() else {
            self.nothing_installed(ui, ctx);
            return;
        };

        self.ask_kjk(ctx, &current);
        let (report, ffmpeg) = match &self.games {
            Games::Known(_, report, ffmpeg) => (report.clone(), *ffmpeg),
            _ => (None, true),
        };
        let blocked = kjk_report::blocked_by(report.as_ref());

        ui.horizontal(|ui| {
            self.version_menu(ui, ctx, &current.tag);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let play = ui
                    .add_enabled_ui(blocked.is_none(), |ui| widgets::primary_button(ui, "Play"))
                    .inner;
                if play.clicked() {
                    match launch::play(&self.paths, &mut self.config, &self.args) {
                        Ok(_) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                        Err(e) => self.notice = Some(Notice::Error(e)),
                    }
                }
                if let Some(game) = blocked {
                    ui.label(widgets::text(
                        format!("Choose the {} folder to play.", game.name()),
                        Text::Body,
                        theme::INK_MUTED,
                    ));
                }
                if install::editor_exe(&current.dir).is_some()
                    && widgets::secondary_button(ui, "Open the save editor").clicked()
                {
                    if let Err(e) = launch::save_editor(&self.paths, &self.config) {
                        self.notice = Some(Notice::Error(e));
                    }
                }
            });
        });
        if !self.config.launched.contains(&current.tag) && self.config.backup_before_new_version {
            ui.label(widgets::text(
                "This version has not started before. Your saves are backed up when it starts.",
                Text::Body,
                theme::INK_MUTED,
            ));
        }
        ui.add_space(theme::SPACE_6);

        if let Some(offer) = self.offer().filter(|_| !self.not_now) {
            self.offer_box(ui, ctx, offer);
            ui.add_space(theme::SPACE_6);
        }

        self.before_you_play(ui, &current, report.as_ref(), ffmpeg);
        self.ffmpeg_window(ui.ctx());
    }

    /// Asks the selected version for its report on a background thread,
    /// unless it was asked already.
    pub(super) fn ask_kjk(&mut self, ctx: &egui::Context, current: &Installed) {
        let fresh = match &self.games {
            Games::Stale => false,
            Games::Asking(tag) | Games::Known(tag, ..) => *tag == current.tag,
        };
        if fresh {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let kjk = install::game_binary(&current.dir);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let report = kjk_report::ask(&kjk);
            // A kjk without the command, or a report without ffmpeg, leaves
            // the launcher to look for ffmpeg itself.
            let ffmpeg = report
                .as_ref()
                .and_then(|r| r.ffmpeg)
                .unwrap_or_else(ffmpeg::installed);
            let _ = tx.send((report, ffmpeg));
            ctx.request_repaint();
        });
        self.asking = Some(rx);
        self.games = Games::Asking(current.tag.clone());
    }

    /// The "Before you play" section: a row for each game.
    pub(super) fn before_you_play(
        &mut self,
        ui: &mut egui::Ui,
        current: &Installed,
        report: Option<&Report>,
        ffmpeg: bool,
    ) {
        widgets::section_label(ui, "Before you play");
        ui.add_space(theme::SPACE_2);
        if matches!(self.games, Games::Asking(_) | Games::Stale) {
            ui.label(widgets::text(
                "The launcher is asking kjk where the games are.",
                Text::Body,
                theme::INK_MUTED,
            ));
            return;
        }
        let Some(report) = report else {
            ui.label(widgets::text(
                "This version of kjk cannot report its game folders. It finds them itself when it starts.",
                Text::Body,
                theme::INK_MUTED,
            ));
            ui.add_space(theme::SPACE_2);
            widgets::hairline(ui);
            self.ffmpeg_row(ui, ffmpeg);
            widgets::hairline(ui);
            return;
        };
        if kjk_report::all_ready(report, ffmpeg) && !self.show_details {
            ui.horizontal(|ui| {
                widgets::status(ui, Status::Ok, "Ready to play");
                if widgets::text_button(ui, "Show details").clicked() {
                    self.show_details = true;
                }
            });
            return;
        }
        if kjk_report::all_ready(report, ffmpeg)
            && widgets::text_button(ui, "Hide details").clicked()
        {
            self.show_details = false;
        }
        for game in Game::ALL {
            widgets::hairline(ui);
            let status = report.status(game).cloned().unwrap_or(GameStatus::Missing);
            self.game_row(ui, current, game, &status);
        }
        widgets::hairline(ui);
        self.ffmpeg_row(ui, ffmpeg);
        widgets::hairline(ui);
    }

    /// The ffmpeg row: its status, why it matters when it is missing, and a
    /// button that shows how to install it.
    pub(super) fn ffmpeg_row(&mut self, ui: &mut egui::Ui, installed: bool) {
        ui.add_space(theme::SPACE_3);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(widgets::text("ffmpeg", Text::Heading, theme::INK));
                if !installed {
                    ui.label(widgets::text(
                        "Movies are skipped until ffmpeg is installed. The game runs without it.",
                        Text::Body,
                        theme::INK_MUTED,
                    ));
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::secondary_button(ui, "How to install").clicked() {
                    self.ffmpeg_dialog = true;
                }
                ui.add_space(theme::SPACE_4);
                if installed {
                    widgets::status(ui, Status::Ok, "Installed");
                } else {
                    widgets::status(ui, Status::Warn, "Not installed");
                }
            });
        });
        ui.add_space(theme::SPACE_3);
    }

    /// One game's row: its name, its folder or why kjk needs it, its status
    /// and a button to choose the folder.
    pub(super) fn game_row(
        &mut self,
        ui: &mut egui::Ui,
        current: &Installed,
        game: Game,
        status: &GameStatus,
    ) {
        ui.add_space(theme::SPACE_3);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(widgets::text(game.name(), Text::Heading, theme::INK));
                match status {
                    GameStatus::Found { path, .. } | GameStatus::Refused { path, .. } => {
                        ui.label(widgets::text(
                            path.display().to_string(),
                            Text::Mono,
                            theme::INK_MUTED,
                        ));
                    }
                    GameStatus::Missing => {
                        ui.label(widgets::text(
                            game.needed_for(),
                            Text::Body,
                            theme::INK_MUTED,
                        ));
                    }
                }
                let why = match status {
                    GameStatus::Refused { why, .. } => Some(why.clone()),
                    _ => None,
                };
                if let Some(why) = self.folder_problems.get(game.key()).cloned().or(why) {
                    ui.label(widgets::text(why, Text::Body, theme::INK_MUTED));
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let found = matches!(status, GameStatus::Found { .. });
                let label = if found {
                    "Change folder"
                } else {
                    "Choose folder"
                };
                if widgets::secondary_button(ui, label).clicked() {
                    self.choose_game_folder(current, game);
                }
                ui.add_space(theme::SPACE_4);
                let (dot, word) = kjk_report::status_word(status);
                widgets::status(ui, dot, word);
            });
        });
        ui.add_space(theme::SPACE_3);
    }

    /// Asks for a folder, has kjk check it, and saves it in kjk's settings
    /// only when kjk finds the game there.
    pub(super) fn choose_game_folder(&mut self, current: &Installed, game: Game) {
        let Some(folder) = rfd::FileDialog::new()
            .set_title(format!("Choose the {} folder", game.name()))
            .pick_folder()
        else {
            return;
        };
        let kjk = install::game_binary(&current.dir);
        match kjk_report::check(&kjk, game, &folder) {
            Some(GameStatus::Found { .. }) => {
                self.folder_problems.remove(game.key());
                let saved = self
                    .paths
                    .settings()
                    .ok_or_else(|| "The launcher could not find kjk's settings file.".to_string())
                    .and_then(|file| kjk_settings::set_install(&file, game, &folder));
                self.notice = Some(match saved {
                    Ok(()) => Notice::Info(format!("The {} folder is saved.", game.name())),
                    Err(e) => Notice::Error(e),
                });
                self.games = Games::Stale;
            }
            Some(GameStatus::Refused { why, .. }) => {
                self.folder_problems.insert(game.key(), why);
            }
            _ => {
                self.notice = Some(Notice::Error(
                    "This version of kjk cannot check a folder.".to_string(),
                ));
            }
        }
    }

    /// The Play page before any version is installed.
    pub(super) fn nothing_installed(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label(widgets::text(
            "kjk is not installed yet",
            Text::Display,
            theme::INK,
        ));
        ui.label(widgets::text(
            "kjk needs your own copies of KOTOR, KOTOR II and Jedi Academy. It finds them the first time it starts.",
            Text::Body,
            theme::INK_MUTED,
        ));
        ui.add_space(theme::SPACE_4);
        let newest = github::newest(&github::installable(
            &self.releases,
            self.config.prereleases,
        ))
        .cloned();
        match newest {
            Some(release) => {
                ui.horizontal(|ui| {
                    let label = format!("Install {}", release_menu::title(&release.tag_name));
                    if widgets::primary_button(ui, &label).clicked() && self.job.is_none() {
                        self.start_install(ctx, release, true);
                    }
                    if widgets::secondary_button(ui, "Show all versions").clicked() {
                        self.tab = Tab::Versions;
                    }
                });
            }
            None => {
                let words = if self.checking.is_some() {
                    "The launcher is looking for releases."
                } else {
                    "No release was found for this computer."
                };
                ui.label(widgets::text(words, Text::Body, theme::INK_MUTED));
            }
        }
    }

    /// The selected version as a title that opens the menu of releases and
    /// pre-releases.
    pub(super) fn version_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, current: &str) {
        let button = widgets::version_title(ui, &release_menu::title(current));
        let menu = release_menu::menu(&self.releases, &self.installed, Some(current));
        let mut picked = None;
        let mut show_all = false;
        let open = std::mem::take(&mut self.open_menu).then_some(egui::SetOpenCommand::Bool(true));
        egui::Popup::from_toggle_button_response(&button)
            .open_memory(open)
            .frame(widgets::raised_frame())
            .width(theme::MENU_WIDTH)
            .align(egui::RectAlign::BOTTOM_START)
            .show(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                egui::ScrollArea::vertical()
                    .max_height(theme::MENU_HEIGHT)
                    .show(ui, |ui| {
                        for (label, rows) in [
                            ("Releases", &menu.releases),
                            ("Pre-releases", &menu.prereleases),
                        ] {
                            if rows.is_empty() {
                                continue;
                            }
                            widgets::section_label(ui, label);
                            for row in rows {
                                let state = match (row.state, row.newest) {
                                    (release_menu::State::Selected, _) => "Selected",
                                    (release_menu::State::Installed, _) => "Installed",
                                    (release_menu::State::NotInstalled, true) => {
                                        "Newest. Not installed"
                                    }
                                    (release_menu::State::NotInstalled, false) => "Not installed",
                                };
                                if widgets::menu_row(ui, &row.tag, Text::Mono, state).clicked() {
                                    picked = Some(row.clone());
                                }
                            }
                            ui.add_space(theme::SPACE_2);
                        }
                    });
                widgets::hairline(ui);
                if widgets::menu_row(ui, "Show all versions", Text::Body, "").clicked() {
                    show_all = true;
                }
            });
        if show_all {
            self.tab = Tab::Versions;
        }
        match picked {
            Some(row) if row.state == release_menu::State::NotInstalled => {
                if let Some(release) = self.releases.iter().find(|r| r.tag_name == row.tag) {
                    self.start_install(ctx, release.clone(), true);
                }
            }
            Some(row) if row.state == release_menu::State::Installed => self.choose(row.tag),
            _ => {}
        }
    }

    /// The notice of a newer version.
    pub(super) fn offer_box(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, offer: Release) {
        let version = release_menu::title(&offer.tag_name);
        widgets::panel(ui, |ui| {
            ui.label(widgets::text(
                format!("{version} is available."),
                Text::Heading,
                theme::INK,
            ));
            ui.label(widgets::text(
                "Your saves are backed up before the new version starts for the first time.",
                Text::Body,
                theme::INK_MUTED,
            ));
            ui.add_space(theme::SPACE_3);
            ui.horizontal(|ui| {
                if !offer.html_url.is_empty()
                    && widgets::secondary_button(ui, "Release notes").clicked()
                {
                    ctx.open_url(egui::OpenUrl::new_tab(&offer.html_url));
                }
                if widgets::secondary_button(ui, "Skip this version").clicked() {
                    self.config.skipped = Some(offer.tag_name.clone());
                    self.save();
                }
                if widgets::secondary_button(ui, "Update").clicked() && self.job.is_none() {
                    self.start_install(ctx, offer.clone(), true);
                }
            });
        });
    }
}
