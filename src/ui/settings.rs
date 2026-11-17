//! The Settings page.

use super::*;

/// The backups listed before "Show all".
const BACKUPS_SHOWN: usize = 5;

impl Ready {
    pub(super) fn settings_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        widgets::page_title(ui, "Settings");
        self.progress(ui);
        let mut changed = false;

        widgets::section_label(ui, "Updates");
        let checks = [
            (
                &mut self.config.check_on_start,
                "Check for updates when the launcher opens",
            ),
            (&mut self.config.prereleases, "Show pre-releases"),
            (&mut self.config.pinned, "Never show update notices"),
        ];
        for (value, label) in checks {
            changed |= widgets::checkbox(ui, value, label).changed();
        }
        if let Some(skipped) = self.config.skipped.clone() {
            widgets::line(ui, |ui| {
                widgets::muted(
                    ui,
                    format!("You skipped version {}.", release_menu::number(&skipped)),
                );
                if widgets::compact_button(ui, "Undo").clicked() {
                    self.config.skipped = None;
                    changed = true;
                }
            });
        }
        ui.add_space(theme::SPACE_1);
        let check = ui.add_enabled_ui(self.checking.is_none(), |ui| {
            widgets::compact_button(ui, "Check for updates now")
        });
        if check.inner.clicked() {
            self.not_now = false;
            self.check(ctx);
        }
        ui.add_space(theme::SPACE_6);

        widgets::section_label(ui, "Storage");
        changed |= widgets::checkbox(
            ui,
            &mut self.config.backup_before_new_version,
            "Back up the saves before a version starts for the first time",
        )
        .changed();
        let older = !install::older(&self.paths, &self.config).is_empty();
        let delete = ui
            .add_enabled_ui(older && self.job.is_none(), |ui| {
                widgets::compact_button(ui, "Delete older versions")
            })
            .inner;
        if delete.clicked() {
            self.confirm = Some(Confirm::DeleteOlder);
        }
        if changed {
            self.not_now = false;
            self.save();
        }
        ui.add_space(theme::SPACE_6);

        self.backups_section(ui);
        ui.add_space(theme::SPACE_6);
        self.folders_section(ui);
        ui.add_space(theme::SPACE_6);
        self.steam_section(ui);
        ui.add_space(theme::SPACE_6);
        widgets::muted(
            ui,
            format!(
                "This is launcher {}. It installs the releases of github.com/{}.",
                env!("CARGO_PKG_VERSION"),
                crate::config::REPO
            ),
        );
    }

    /// The "Backups" section: a line per backup, newest first, the five
    /// newest unless all are shown, and the buttons to back up and to open
    /// the folder.
    fn backups_section(&mut self, ui: &mut egui::Ui) {
        widgets::section_label(ui, "Backups");
        widgets::muted(
            ui,
            "The launcher backs up the saves and the settings before a version starts for the first time.",
        );
        ui.add_space(theme::SPACE_2);
        let shown = if self.show_all_backups {
            self.backups.len()
        } else {
            BACKUPS_SHOWN
        };
        let words: Vec<(PathBuf, [String; 3])> = self
            .backups
            .iter()
            .take(shown)
            .map(|b| {
                let (when, why) = backup::describe(&b.name);
                (b.path.clone(), [when, why, size(b.size)])
            })
            .collect();
        let cells: Vec<Vec<widgets::Cell>> = words
            .iter()
            .map(|(_, line)| {
                line.iter()
                    .enumerate()
                    .map(|(column, words)| match column {
                        0 => widgets::Cell::new(words, Text::Body, theme::INK),
                        _ => widgets::Cell::new(words, Text::Body, theme::INK_MUTED),
                    })
                    .collect()
            })
            .collect();
        let starts = widgets::columns(ui, &cells);
        // Backups and restores wait while a job moves or installs files.
        let idle = self.job.is_none();
        for ((path, _), line) in words.iter().zip(&cells) {
            widgets::hairline(ui);
            widgets::list_line(ui, &starts, line, |ui| {
                let restore = ui.add_enabled_ui(idle, |ui| widgets::compact_button(ui, "Restore"));
                if restore.inner.clicked() {
                    self.confirm = Some(Confirm::Restore(path.clone()));
                }
            });
        }
        if !words.is_empty() {
            widgets::hairline(ui);
        }
        if self.backups.len() > BACKUPS_SHOWN && !self.show_all_backups {
            ui.add_space(theme::SPACE_1);
            if widgets::text_button(ui, "Show all").clicked() {
                self.show_all_backups = true;
            }
        }
        ui.add_space(theme::SPACE_2);
        widgets::line(ui, |ui| {
            let back_up = ui.add_enabled_ui(idle, |ui| widgets::compact_button(ui, "Back up now"));
            if back_up.inner.clicked() {
                self.notice = Some(match backup::create(&self.paths, "manual") {
                    Ok(Some(_)) => Notice::Info("The saves and the settings are backed up.".into()),
                    Ok(None) => Notice::Info("kjk has no saves or settings yet.".into()),
                    Err(e) => Notice::Error(e),
                });
                self.backups = backup::list(&self.paths);
            }
            if widgets::compact_button(ui, "Open folder").clicked() {
                launch::open_folder(&self.paths.backups);
            }
        });
    }

    /// The "Folders" section: kjk's folder and the install folder.
    fn folders_section(&mut self, ui: &mut egui::Ui) {
        widgets::section_label(ui, "Folders");
        if let Some(dir) = self.paths.kjk_data.clone() {
            let idle = self.job.is_none();
            // KJK_HOME is the player's own choice of folder.
            let movable = idle && std::env::var_os("KJK_HOME").is_none();
            let default = config::default_kjk_folder().filter(|_| config::kjk_folder_moved());
            let (mut back, mut pick, mut typed) = (false, false, false);
            widgets::hairline(ui);
            widgets::row(
                ui,
                |ui| {
                    ui.label(widgets::text("kjk folder", Text::Heading, theme::INK));
                    widgets::wrapped_mono(ui, &dir.display().to_string(), theme::INK_MUTED);
                    widgets::muted(
                        ui,
                        "Saves, settings, logs, backups and installed versions are in this folder.",
                    );
                    if default.is_some() {
                        widgets::muted(
                            ui,
                            "kjk versions before 0.20 still write their logs in the default folder.",
                        );
                        ui.add_space(theme::SPACE_1);
                        back = ui
                            .add_enabled_ui(movable, |ui| {
                                widgets::compact_button(ui, "Use the default folder")
                            })
                            .inner
                            .clicked();
                    }
                },
                |ui| {
                    (pick, typed) = folder_buttons(ui, "Move", movable);
                    if widgets::compact_button(ui, "Open").clicked() {
                        launch::open_folder(&dir);
                    }
                },
            );
            if pick {
                self.pick_kjk_folder();
            }
            if typed {
                self.type_folder(FolderFor::KjkFolder);
            }
            if let Some(default) = default.filter(|_| back) {
                self.confirm = Some(Confirm::MoveKjk(default));
            }
        }
        widgets::hairline(ui);
        self.install_folder(ui);
        widgets::hairline(ui);
    }

    /// The "Steam play time" section: the line for Steam's launch options.
    fn steam_section(&mut self, ui: &mut egui::Ui) {
        widgets::section_label(ui, "Steam play time");
        widgets::muted(
            ui,
            "Steam can count the time you play kjk under KOTOR or KOTOR II. Open that game's Properties in Steam and paste this line into Launch options.",
        );
        let launcher = std::env::current_exe().unwrap_or_default();
        let line = crate::args::steam_line(&launcher);
        let mut copy = false;
        widgets::row(
            ui,
            |ui| widgets::wrapped_mono(ui, &line, theme::INK),
            |ui| copy = widgets::compact_button(ui, "Copy").clicked(),
        );
        if copy {
            ui.ctx().copy_text(line.clone());
        }
        widgets::muted(
            ui,
            "Steam then starts kjk through this launcher when you press Play on that game.",
        );
        if !cfg!(windows) {
            widgets::muted(ui, "This has only been checked on Windows.");
        }
    }
}
