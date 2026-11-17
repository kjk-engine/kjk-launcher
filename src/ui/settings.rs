//! The Settings page.

use super::*;

impl Ready {
    pub(super) fn settings_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_space(8.0);
        self.install_folder(ui);
        ui.add_space(6.0);
        let mut changed = false;
        changed |= ui
            .checkbox(
                &mut self.config.check_on_start,
                "Check for updates when the launcher opens",
            )
            .changed();
        changed |= ui
            .checkbox(&mut self.config.prereleases, "Offer pre-releases too")
            .changed();
        changed |= ui
            .checkbox(
                &mut self.config.pinned,
                "Stay on the current version (never offer updates)",
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut self.config.backup_before_new_version,
                "Back up saves before a version's first start",
            )
            .changed();
        ui.horizontal(|ui| {
            changed |= ui
                .add(egui::DragValue::new(&mut self.config.keep_versions).range(0..=50))
                .changed();
            ui.label("installed versions to keep after an update (0 keeps all)");
        });
        ui.horizontal(|ui| {
            changed |= ui
                .add(egui::DragValue::new(&mut self.config.keep_backups).range(1..=500))
                .changed();
            ui.label("backups to keep");
        });
        if let Some(skipped) = self.config.skipped.clone() {
            ui.horizontal(|ui| {
                ui.label(format!("Skipped release: {skipped}"));
                if ui.button("Offer it again").clicked() {
                    self.config.skipped = None;
                    changed = true;
                }
            });
        }
        if changed {
            self.not_now = false;
            self.save();
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.checking.is_none(),
                    egui::Button::new("Check for updates now"),
                )
                .clicked()
            {
                self.not_now = false;
                self.check(ctx);
            }
            if ui.button("Open launcher folder").clicked() {
                launch::open_folder(&self.paths.root);
            }
            if let Some(dir) = self.paths.kjk_data.clone() {
                if ui.button("Open kjk's folder").clicked() {
                    launch::open_folder(&dir);
                }
            }
        });
        ui.add_space(6.0);
        ui.label(
            RichText::new(format!(
                "Launcher {} · releases from github.com/{}",
                env!("CARGO_PKG_VERSION"),
                crate::config::REPO
            ))
            .weak(),
        );
    }
}
