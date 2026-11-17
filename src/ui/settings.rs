//! The Settings page.

use super::*;

impl Ready {
    pub(super) fn settings_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        widgets::page_title(ui, "Settings");
        let mut changed = false;

        widgets::section_label(ui, "Updates");
        let checks = [
            (
                &mut self.config.check_on_start,
                "Check for updates when the launcher opens",
            ),
            (
                &mut self.config.prereleases,
                "Offer pre-releases as updates",
            ),
            (
                &mut self.config.pinned,
                "Stay on the selected version and offer no updates",
            ),
        ];
        for (value, label) in checks {
            changed |= ui.checkbox(value, label).changed();
        }
        if let Some(skipped) = self.config.skipped.clone() {
            ui.horizontal(|ui| {
                widgets::muted(
                    ui,
                    format!("You skipped {skipped}. The launcher does not offer it."),
                );
                if widgets::secondary_button(ui, "Offer it again").clicked() {
                    self.config.skipped = None;
                    changed = true;
                }
            });
        }
        let check = ui.add_enabled_ui(self.checking.is_none(), |ui| {
            widgets::secondary_button(ui, "Check for updates now")
        });
        if check.inner.clicked() {
            self.not_now = false;
            self.check(ctx);
        }
        ui.add_space(theme::SPACE_6);

        widgets::section_label(ui, "Versions and backups");
        changed |= ui
            .checkbox(
                &mut self.config.backup_before_new_version,
                "Back up the saves before a version starts for the first time",
            )
            .changed();
        ui.horizontal(|ui| {
            changed |= ui
                .add(egui::DragValue::new(&mut self.config.keep_versions).range(0..=50))
                .changed();
            widgets::muted(
                ui,
                "Installed versions to keep after an update. 0 keeps all of them.",
            );
        });
        ui.horizontal(|ui| {
            changed |= ui
                .add(egui::DragValue::new(&mut self.config.keep_backups).range(1..=500))
                .changed();
            widgets::muted(ui, "Backups to keep.");
        });
        if changed {
            self.not_now = false;
            self.save();
        }
        ui.add_space(theme::SPACE_6);

        widgets::section_label(ui, "Folders");
        widgets::hairline(ui);
        self.install_folder(ui);
        widgets::hairline(ui);
        ui.add_space(theme::SPACE_3);
        ui.horizontal(|ui| {
            if widgets::secondary_button(ui, "Open the launcher's folder").clicked() {
                launch::open_folder(&self.paths.root);
            }
            if let Some(dir) = self.paths.kjk_data.clone() {
                if widgets::secondary_button(ui, "Open kjk's folder").clicked() {
                    launch::open_folder(&dir);
                }
            }
        });
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
}
