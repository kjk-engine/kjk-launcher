//! The Versions page: installed versions and the releases on GitHub.

use super::*;

impl Ready {
    pub(super) fn versions_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.add_space(8.0);
        self.progress(ui);
        self.install_folder(ui);
        ui.horizontal(|ui| {
            if ui
                .checkbox(&mut self.config.prereleases, "Show pre-releases")
                .changed()
            {
                self.save();
            }
            if ui
                .add_enabled(self.checking.is_none(), egui::Button::new("Check now"))
                .clicked()
            {
                self.check(ctx);
            }
        });
        ui.add_space(6.0);

        let total: u64 = self.sizes.values().sum();
        ui.heading(format!("Installed ({})", megabytes(total)));
        let installed = self.installed.clone();
        if installed.is_empty() {
            ui.label("Nothing yet.");
        }
        let current = self.config.current.clone();
        egui::Grid::new("installed")
            .num_columns(5)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                for version in &installed {
                    ui.label(RichText::new(&version.tag).strong());
                    ui.label(version.date());
                    ui.label(
                        RichText::new(megabytes(
                            self.sizes.get(&version.dir).copied().unwrap_or(0),
                        ))
                        .weak(),
                    );
                    if current.as_deref() == Some(version.tag.as_str()) {
                        ui.label(widgets::text("Selected", Text::Label, theme::INK));
                    } else if ui.button("Use").clicked() {
                        self.choose(version.tag.clone());
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Uninstall").clicked() {
                            self.confirm = Some(Confirm::Uninstall(version.tag.clone()));
                        }
                        if version.adopted {
                            ui.label(RichText::new("found in the folder").weak())
                                .on_hover_text(
                                    "Unzipped by hand; the launcher never removes it by itself",
                                );
                        }
                    });
                    ui.end_row();
                }
            });

        ui.add_space(10.0);
        ui.heading("On GitHub");
        let releases: Vec<Release> = github::installable(&self.releases, self.config.prereleases)
            .into_iter()
            .filter(|r| !installed.iter().any(|i| i.tag == r.tag_name))
            .cloned()
            .collect();
        if releases.is_empty() {
            ui.label("No other releases for this computer.");
        }
        for release in releases {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&release.tag_name).strong());
                ui.label(release.date());
                if release.prerelease {
                    ui.label(widgets::text("Pre-release", Text::Label, theme::INK_MUTED));
                }
                if self.config.skipped.as_deref() == Some(release.tag_name.as_str()) {
                    ui.label(RichText::new("skipped").weak());
                }
                if ui
                    .add_enabled(self.job.is_none(), egui::Button::new("Install"))
                    .clicked()
                {
                    let first = self.config.current.is_none();
                    self.start_install(ctx, release.clone(), first);
                }
            });
            egui::CollapsingHeader::new("Release notes")
                .id_salt(("notes", &release.tag_name))
                .show(ui, |ui| ui.label(release.notes()));
        }
    }
}
