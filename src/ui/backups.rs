//! The Backups page: the backups of kjk's saves and settings.

use super::*;

impl Ready {
    pub(super) fn backups_tab(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.label("Backups hold kjk's saves and settings.json. One is taken before each version starts for the first time, because a newer version may change saves so that older versions can no longer read them.");
        if let Some(saves) = self.paths.saves() {
            ui.label(RichText::new(format!("Saves: {}", saves.display())).weak());
        }
        ui.horizontal(|ui| {
            if ui.button("Back up now").clicked() {
                self.notice = Some(match backup::create(&self.paths, "manual") {
                    Ok(Some(path)) => Notice::Info(format!("Backed up to {}.", path.display())),
                    Ok(None) => Notice::Info("kjk has no saves or settings yet.".into()),
                    Err(e) => Notice::Error(e),
                });
                self.backups = backup::list(&self.paths);
            }
            if ui.button("Open backups folder").clicked() {
                launch::open_folder(&self.paths.backups);
            }
        });
        ui.add_space(6.0);
        if self.backups.is_empty() {
            ui.label("No backups yet.");
        }
        for b in self.backups.clone() {
            ui.horizontal(|ui| {
                ui.label(&b.name);
                ui.label(RichText::new(format!("{:.1} MB", b.size as f64 / 1e6)).weak());
                if ui.button("Restore").clicked() {
                    self.confirm = Some(Confirm::Restore(b.path.clone()));
                }
            });
        }
    }
}
