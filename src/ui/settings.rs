//! The Settings page.

use super::*;

/// The most installed versions the player can choose to keep.
const KEEP_VERSIONS_MOST: usize = 20;

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
            (&mut self.config.prereleases, "Show pre-releases"),
            (&mut self.config.pinned, "Never show update notices"),
        ];
        for (value, label) in checks {
            changed |= ui.checkbox(value, label).changed();
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
        changed |= ui
            .checkbox(
                &mut self.config.backup_before_new_version,
                "Back up the saves before a version starts for the first time",
            )
            .changed();
        widgets::line(ui, |ui| {
            // An old launcher.json can hold 0, which keeps every version.
            let mut keep = match self.config.keep_versions {
                0 => KEEP_VERSIONS_MOST,
                n => n.min(KEEP_VERSIONS_MOST),
            };
            if widgets::number_field(ui, &mut keep, 1..=KEEP_VERSIONS_MOST).changed() {
                self.config.keep_versions = keep;
                changed = true;
            }
            widgets::muted(ui, "Installed versions to keep");
        });
        if changed {
            self.not_now = false;
            self.save();
        }
        ui.add_space(theme::SPACE_6);

        self.steam_section(ui);
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

    /// The "Steam play time" section: the line for Steam's launch options.
    fn steam_section(&mut self, ui: &mut egui::Ui) {
        widgets::section_label(ui, "Steam play time");
        widgets::muted(
            ui,
            "Steam can count the time you play kjk under KOTOR or KOTOR II. Open that game's Properties in Steam and paste this line into Launch options.",
        );
        let launcher = std::env::current_exe().unwrap_or_default();
        let line = crate::args::steam_line(&launcher);
        widgets::line(ui, |ui| {
            ui.label(widgets::text(&line, Text::Mono, theme::INK));
            if widgets::compact_button(ui, "Copy").clicked() {
                ui.ctx().copy_text(line.clone());
            }
        });
        widgets::muted(
            ui,
            "Steam then starts kjk through this launcher when you press Play on that game.",
        );
        if !cfg!(windows) {
            widgets::muted(ui, "This has only been checked on Windows.");
        }
    }
}
