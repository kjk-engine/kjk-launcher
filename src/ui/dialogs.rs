//! The dialogs: questions before a change, and how to install ffmpeg.

use super::*;

impl Ready {
    /// The "Install ffmpeg" dialog.
    pub(super) fn ffmpeg_window(&mut self, ctx: &egui::Context) {
        if !self.ffmpeg_dialog {
            return;
        }
        let mut open = true;
        let mut check = false;
        dialog(ctx, "Install ffmpeg", |ui| {
            ui.label(widgets::text(
                "Run this command in a terminal to install ffmpeg.",
                Text::Body,
                theme::INK,
            ));
            ui.horizontal(|ui| {
                ui.label(widgets::text(
                    ffmpeg::install_command(),
                    Text::Mono,
                    theme::INK,
                ));
                if widgets::secondary_button(ui, "Copy").clicked() {
                    ui.ctx().copy_text(ffmpeg::install_command().to_string());
                }
            });
            if cfg!(target_os = "linux") {
                ui.label(widgets::text(
                    "Use your own package manager if this computer does not have apt.",
                    Text::Body,
                    theme::INK_MUTED,
                ));
            }
            ui.label(widgets::text(
                "The ffmpeg website lists other ways to install it.",
                Text::Body,
                theme::INK_MUTED,
            ));
            ui.hyperlink_to(
                widgets::text(ffmpeg::DOWNLOAD_PAGE, Text::Mono, theme::INK),
                ffmpeg::DOWNLOAD_PAGE,
            );
            ui.add_space(theme::SPACE_3);
            ui.horizontal(|ui| {
                if widgets::primary_button(ui, "Check again").clicked() {
                    check = true;
                }
                if widgets::secondary_button(ui, "Close").clicked() {
                    open = false;
                }
            });
        });
        if check {
            self.games = Games::Stale;
            open = false;
        }
        self.ffmpeg_dialog = open;
    }

    pub(super) fn confirm_window(&mut self, ctx: &egui::Context) {
        let Some(confirm) = &self.confirm else { return };
        let (title, text, yes, alternative) = match confirm {
            Confirm::Restore(path) => (
                "Restore a backup",
                vec![
                    format!(
                        "The saves and settings of kjk are replaced with those in {}.",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ),
                    "The launcher backs up the present ones first.".to_string(),
                    "Close kjk before you restore.".to_string(),
                ],
                "Restore",
                None,
            ),
            Confirm::Uninstall(tag) => {
                let size = self
                    .installed
                    .iter()
                    .find(|i| &i.tag == tag)
                    .and_then(|i| self.sizes.get(&i.dir))
                    .copied()
                    .unwrap_or(0);
                let mut text = vec![
                    format!("kjk {tag} is deleted from this computer. It frees {}.", megabytes(size)),
                    "Your saves are not touched.".to_string(),
                ];
                if self.config.current.as_deref() == Some(tag.as_str()) {
                    text.push(match self.installed.iter().find(|i| &i.tag != tag) {
                        Some(next) => format!("Play then starts {}.", next.tag),
                        None => "No other version is installed.".to_string(),
                    });
                }
                ("Uninstall a version", text, "Uninstall", None)
            }
            Confirm::UseOlder(tag) => (
                "Go back to an older version",
                vec![
                    format!("A save made or opened by a newer version may not load in {tag}."),
                    "If a save does not load, restore the backup from before the update on the Backups page.".to_string(),
                ],
                "Select it",
                None,
            ),
            Confirm::ChangeFolder(dir) => (
                "Change the install folder",
                vec![
                    format!("New versions are installed in {}.", dir.display()),
                    format!(
                        "The {} installed versions can move there too. If they stay in {}, the launcher no longer lists them.",
                        self.installed.len(),
                        self.paths.versions.display()
                    ),
                ],
                "Move them",
                Some("Leave them"),
            ),
        };
        let mut answer = None;
        dialog(ctx, title, |ui| {
            for sentence in text {
                ui.label(widgets::text(sentence, Text::Body, theme::INK));
            }
            ui.add_space(theme::SPACE_3);
            ui.horizontal(|ui| {
                if widgets::primary_button(ui, yes).clicked() {
                    answer = Some(Some(true));
                }
                if let Some(alternative) = alternative {
                    if widgets::secondary_button(ui, alternative).clicked() {
                        answer = Some(Some(false));
                    }
                }
                if widgets::secondary_button(ui, "Cancel").clicked() {
                    answer = Some(None);
                }
            });
        });
        let Some(answer) = answer else { return };
        let confirm = self.confirm.take().unwrap();
        let Some(yes) = answer else { return };
        match confirm {
            Confirm::Restore(path) => {
                self.notice = Some(match backup::restore(&self.paths, &path) {
                    Ok(_) => {
                        Notice::Info("Restored. The saves from before are in a new backup.".into())
                    }
                    Err(e) => Notice::Error(e),
                });
            }
            Confirm::Uninstall(tag) => {
                match install::remove(&self.paths, &mut self.config, &tag) {
                    Ok(()) => self.notice = Some(Notice::Info(format!("Uninstalled {tag}."))),
                    Err(e) => self.notice = Some(Notice::Error(e)),
                }
                self.save();
            }
            Confirm::UseOlder(tag) => {
                self.config.current = Some(tag);
                self.save();
            }
            Confirm::ChangeFolder(dir) => {
                if yes {
                    self.start_move(ctx, dir);
                } else {
                    self.set_install_dir(dir);
                }
            }
        }
        self.refresh();
    }
}

/// A dialog in the middle of the window: `SURFACE_RAISED`, `RADIUS_LG`
/// corners and a title in the title style.
pub(super) fn dialog(ctx: &egui::Context, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Window::new(title)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .frame(widgets::raised_frame().inner_margin(egui::Margin::same(theme::SPACE_6 as i8)))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_max_width(theme::DIALOG_WIDTH);
            ui.label(widgets::text(title, Text::Title, theme::INK));
            ui.add_space(theme::SPACE_3);
            add(ui);
        });
}
