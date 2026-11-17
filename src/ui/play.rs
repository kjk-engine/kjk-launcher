//! The Play page: the selected version, Play, the update notice, the
//! "Setup" rows for the games and ffmpeg, and the selected version's
//! release notes.

use super::*;

impl Ready {
    /// When all is ready and the Setup list is closed, the release notes
    /// fill the rest of the page and scroll inside their panel. Otherwise
    /// the whole page scrolls.
    pub(super) fn play_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(current) = self.current().cloned() else {
            scrolling(ui, |ui| {
                self.progress(ui);
                self.nothing_installed(ui, ctx);
            });
            return;
        };
        self.ask_kjk(ctx, &current);
        let (report, ffmpeg) = match &self.games {
            Games::Known(_, report, ffmpeg) => (report.clone(), *ffmpeg),
            _ => (None, true),
        };
        let ready = report
            .as_ref()
            .is_some_and(|report| kjk_report::all_ready(report, ffmpeg));
        if ready && !self.show_details {
            self.play_top(ui, ctx, &current, report.as_ref(), ffmpeg);
            self.release_notes(ui, &current, true);
        } else {
            scrolling(ui, |ui| {
                self.play_top(ui, ctx, &current, report.as_ref(), ffmpeg);
                self.release_notes(ui, &current, false);
            });
        }
        self.ffmpeg_window(ui.ctx());
    }

    /// The version, Play, the update notice and the Setup section.
    fn play_top(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        current: &Installed,
        report: Option<&Report>,
        ffmpeg: bool,
    ) {
        self.progress(ui);
        let blocked = kjk_report::blocked_by(report);

        ui.horizontal(|ui| {
            self.version_menu(ui, ctx, &current.tag);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Play closes the launcher, which would stop a download or
                // a move half way, so it waits for the job to end.
                let can_play = blocked.is_none() && self.job.is_none();
                let play = ui
                    .add_enabled_ui(can_play, |ui| widgets::primary_button(ui, "Play"))
                    .inner;
                if self.focus_start && can_play {
                    play.request_focus();
                }
                if play.clicked() {
                    match launch::play(&self.paths, &mut self.config, &self.args) {
                        Ok(started) if self.from_steam => {
                            // Steam counts the time while the launcher runs:
                            // it stays, hidden, until kjk exits.
                            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                            let mut child = started.child;
                            std::thread::spawn(move || {
                                let _ = child.wait();
                                std::process::exit(0);
                            });
                        }
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

        self.before_you_play(ui, report, ffmpeg);
    }

    /// "What is new in 0.19.0" and the selected version's release notes on
    /// a panel. With `fill` the panel takes the height that is left and
    /// scrolls inside itself.
    fn release_notes(&mut self, ui: &mut egui::Ui, current: &Installed, fill: bool) {
        ui.add_space(theme::SPACE_6);
        widgets::section_label(
            ui,
            &format!("What is new in {}", release_menu::number(&current.tag)),
        );
        ui.add_space(theme::SPACE_2);
        let notes = self
            .releases
            .iter()
            .find(|r| r.tag_name == current.tag)
            .and_then(|r| r.body.as_deref())
            .map(crate::notes::lines)
            .filter(|lines| !lines.is_empty());
        let Some(lines) = notes else {
            widgets::muted(ui, "The release notes for this version are not available.");
            return;
        };
        let height = ui.available_height() - 2.0 * theme::SPACE_4;
        widgets::panel(ui, |ui| {
            if fill {
                egui::ScrollArea::vertical()
                    .max_height(height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| note_lines(ui, &lines));
            } else {
                note_lines(ui, &lines);
            }
        });
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

    /// The "Setup" section: a row for each game.
    pub(super) fn before_you_play(
        &mut self,
        ui: &mut egui::Ui,
        report: Option<&Report>,
        ffmpeg: bool,
    ) {
        widgets::section_label(ui, "Setup");
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
            self.game_row(ui, game, &status);
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
                if !installed {
                    if widgets::compact_button(ui, "How to install").clicked() {
                        self.ffmpeg_dialog = true;
                    }
                    ui.add_space(theme::SPACE_4);
                }
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
    pub(super) fn game_row(&mut self, ui: &mut egui::Ui, game: Game, status: &GameStatus) {
        let found = matches!(status, GameStatus::Found { .. });
        let label = if found {
            "Change folder"
        } else {
            "Choose folder"
        };
        let checking = self.folder_check.as_ref().map(|check| check.game);
        let (mut pick, mut typed) = (false, false);
        widgets::row(
            ui,
            |ui| {
                ui.label(widgets::text(game.name(), Text::Heading, theme::INK));
                match status {
                    GameStatus::Found { path, .. } | GameStatus::Refused { path, .. } => {
                        widgets::wrapped_mono(ui, &path.display().to_string(), theme::INK_MUTED);
                    }
                    GameStatus::Missing => widgets::muted(ui, game.needed_for()),
                }
                let why = match status {
                    GameStatus::Refused { why, .. } => Some(why.clone()),
                    _ => None,
                };
                if let Some(why) = self.folder_problems.get(game.key()).cloned().or(why) {
                    widgets::muted(ui, why);
                }
            },
            |ui| {
                (pick, typed) = folder_buttons(ui, label, checking.is_none());
                ui.add_space(theme::SPACE_4);
                if checking == Some(game) {
                    ui.label(widgets::text("Checking", Text::Label, theme::INK_MUTED));
                } else {
                    let (dot, word) = kjk_report::status_word(status);
                    widgets::status(ui, dot, word);
                }
            },
        );
        if pick {
            self.choose_game_folder(ui.ctx(), game);
        }
        if typed {
            self.type_folder(FolderFor::Game(game));
        }
    }

    /// Asks for a game's folder.
    pub(super) fn choose_game_folder(&mut self, ctx: &egui::Context, game: Game) {
        if let Some(folder) = self.ask_folder(FolderFor::Game(game)) {
            self.game_folder_chosen(ctx, game, folder);
        }
    }

    /// kjk checks `folder` on a background thread, and the folder is saved
    /// in kjk's settings only when kjk finds the game there.
    pub(super) fn game_folder_chosen(&mut self, ctx: &egui::Context, game: Game, folder: PathBuf) {
        let Some(current) = self.current().cloned() else {
            return;
        };
        self.clear_error();
        let kjk = install::game_binary(&current.dir);
        let settings = self.paths.settings();
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let checked = match kjk_report::check(&kjk, game, &folder) {
                Some(GameStatus::Found { .. }) => FolderChecked::Found(
                    settings
                        .ok_or_else(|| {
                            "The launcher could not find kjk's settings file.".to_string()
                        })
                        .and_then(|file| kjk_settings::set_install(&file, game, &folder)),
                ),
                Some(GameStatus::Refused { why, .. }) => FolderChecked::Refused(why),
                _ => FolderChecked::CannotCheck,
            };
            let _ = tx.send(checked);
            ctx.request_repaint();
        });
        self.folder_check = Some(FolderCheck { game, done: rx });
    }

    /// Takes the answer of a folder check, when it has come.
    pub(super) fn poll_folder_check(&mut self) {
        let Some(check) = &self.folder_check else {
            return;
        };
        let Ok(checked) = check.done.try_recv() else {
            return;
        };
        let game = check.game;
        self.folder_check = None;
        match checked {
            FolderChecked::Found(saved) => {
                self.folder_problems.remove(game.key());
                self.notice = Some(match saved {
                    Ok(()) => Notice::Info(format!("The {} folder is saved.", game.name())),
                    Err(e) => Notice::Error(e),
                });
                self.games = Games::Stale;
            }
            FolderChecked::Refused(why) => {
                self.folder_problems.insert(game.key(), why);
            }
            FolderChecked::CannotCheck => {
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
            // While a version installs, the progress line above says so.
            Some(_) if self.job.is_some() => {}
            Some(release) => {
                ui.horizontal(|ui| {
                    let label = format!("Install {}", release_menu::title(&release.tag_name));
                    let install = widgets::primary_button(ui, &label);
                    if self.focus_start {
                        install.request_focus();
                    }
                    if install.clicked() {
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
        let menu = release_menu::menu(
            &self.releases,
            &self.installed,
            Some(current),
            self.config.prereleases,
        );
        let mut picked = None;
        let mut show_all = false;
        let busy = self.job.is_some();
        // A click, or Enter or Space while the title has the focus, opens
        // and closes the menu.
        let open = std::mem::take(&mut self.open_menu)
            .then_some(egui::SetOpenCommand::Bool(true))
            .or(button.clicked().then_some(egui::SetOpenCommand::Toggle));
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
                                // A second install cannot start during a job.
                                let enabled =
                                    !busy || row.state != release_menu::State::NotInstalled;
                                let clicked = ui
                                    .add_enabled_ui(enabled, |ui| {
                                        widgets::menu_row(ui, &row.tag, Text::Mono, state)
                                    })
                                    .inner
                                    .clicked();
                                if clicked {
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
                let update = ui
                    .add_enabled_ui(self.job.is_none(), |ui| {
                        widgets::secondary_button(ui, "Update")
                    })
                    .inner;
                if update.clicked() {
                    self.start_install(ctx, offer.clone(), true);
                }
            });
        });
    }
}

/// The whole page in a scroll area.
fn scrolling(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, add);
}

/// Release notes: headings in the heading style, the rest in the body style.
fn note_lines(ui: &mut egui::Ui, lines: &[crate::notes::Line]) {
    use crate::notes::Line;
    for line in lines {
        match line {
            Line::Heading(words) => {
                ui.label(widgets::text(words, Text::Heading, theme::INK));
            }
            Line::Bullet(words) => {
                ui.label(widgets::text(
                    format!("•  {words}"),
                    Text::Body,
                    theme::INK_MUTED,
                ));
            }
            Line::Text(words) => {
                ui.label(widgets::text(words, Text::Body, theme::INK_MUTED));
            }
            Line::Gap => ui.add_space(theme::SPACE_2),
        }
    }
}
