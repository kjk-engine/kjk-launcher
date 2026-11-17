//! The Backups page: the backups of kjk's saves and settings.

use super::*;

impl Ready {
    pub(super) fn backups_tab(&mut self, ui: &mut egui::Ui) {
        widgets::page_title(ui, "Backups");
        widgets::muted(
            ui,
            "A backup is a copy of the saves and the settings of kjk. The launcher makes one before a version starts for the first time. A newer version can change a save so that an older version cannot read it.",
        );
        if let Some(saves) = self.paths.saves() {
            ui.add_space(theme::SPACE_2);
            widgets::muted(ui, "kjk keeps its saves in this folder.");
            ui.label(widgets::text(
                saves.display().to_string(),
                Text::Mono,
                theme::INK_MUTED,
            ));
        }
        ui.add_space(theme::SPACE_4);
        ui.horizontal(|ui| {
            if widgets::primary_button(ui, "Back up now").clicked() {
                self.notice = Some(match backup::create(&self.paths, "manual") {
                    Ok(Some(path)) => Notice::Info(format!(
                        "The backup is saved as {}.",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    )),
                    Ok(None) => Notice::Info("kjk has no saves or settings yet.".into()),
                    Err(e) => Notice::Error(e),
                });
                self.backups = backup::list(&self.paths);
            }
            if widgets::secondary_button(ui, "Open the backups folder").clicked() {
                launch::open_folder(&self.paths.backups);
            }
        });
        ui.add_space(theme::SPACE_6);
        widgets::section_label(ui, "Backups on this computer");
        if self.backups.is_empty() {
            widgets::muted(ui, "There are no backups yet.");
        }
        for b in self.backups.clone() {
            widgets::hairline(ui);
            widgets::row(
                ui,
                |ui| {
                    ui.label(widgets::text(&b.name, Text::Mono, theme::INK));
                    widgets::muted(ui, format!("It takes {:.1} MB.", b.size as f64 / 1e6));
                },
                |ui| {
                    if widgets::secondary_button(ui, "Restore").clicked() {
                        self.confirm = Some(Confirm::Restore(b.path.clone()));
                    }
                },
            );
        }
        if !self.backups.is_empty() {
            widgets::hairline(ui);
        }
    }
}
