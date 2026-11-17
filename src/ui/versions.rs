//! The Versions page: installed versions and the releases on GitHub.

use super::*;

impl Ready {
    pub(super) fn versions_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        widgets::page_title(ui, "Versions");
        self.progress(ui);

        let total: u64 = self.sizes.values().sum();
        widgets::section_label(ui, "Installed");
        widgets::muted(
            ui,
            format!(
                "The installed versions take {} on this computer.",
                megabytes(total)
            ),
        );
        let installed = self.installed.clone();
        if installed.is_empty() {
            widgets::muted(ui, "No version is installed yet.");
        }
        let current = self.config.current.clone();
        for version in &installed {
            widgets::hairline(ui);
            let size = megabytes(self.sizes.get(&version.dir).copied().unwrap_or(0));
            let selected = current.as_deref() == Some(version.tag.as_str());
            widgets::row(
                ui,
                |ui| {
                    ui.label(widgets::text(&version.tag, Text::Mono, theme::INK));
                    let released = match version.date() {
                        "" => format!("It takes {size}."),
                        date => format!("Released on {date}. It takes {size}."),
                    };
                    widgets::muted(ui, released);
                    if version.adopted {
                        widgets::muted(
                            ui,
                            "This version was unzipped by hand. The launcher never removes it by itself.",
                        );
                    }
                },
                |ui| {
                    if widgets::secondary_button(ui, "Uninstall").clicked() {
                        self.confirm = Some(Confirm::Uninstall(version.tag.clone()));
                    }
                    if selected {
                        ui.label(widgets::text("Selected", Text::Label, theme::INK));
                    } else if widgets::secondary_button(ui, "Select").clicked() {
                        self.choose(version.tag.clone());
                    }
                },
            );
        }
        widgets::hairline(ui);
        ui.add_space(theme::SPACE_6);

        widgets::section_label(ui, "On GitHub");
        ui.horizontal(|ui| {
            if ui
                .checkbox(&mut self.config.prereleases, "Show pre-releases")
                .changed()
            {
                self.save();
            }
            let check = ui.add_enabled_ui(self.checking.is_none(), |ui| {
                widgets::secondary_button(ui, "Check for updates")
            });
            if check.inner.clicked() {
                self.check(ctx);
            }
        });
        let releases: Vec<Release> = github::installable(&self.releases, self.config.prereleases)
            .into_iter()
            .filter(|r| !installed.iter().any(|i| i.tag == r.tag_name))
            .cloned()
            .collect();
        if releases.is_empty() {
            widgets::muted(ui, "GitHub has no other release for this computer.");
        }
        for release in releases {
            widgets::hairline(ui);
            let skipped = self.config.skipped.as_deref() == Some(release.tag_name.as_str());
            widgets::row(
                ui,
                |ui| {
                    ui.label(widgets::text(&release.tag_name, Text::Mono, theme::INK));
                    let mut about = format!("Released on {}.", release.date());
                    if release.prerelease {
                        about.push_str(" This is a pre-release.");
                    }
                    if skipped {
                        about.push_str(" You skipped this version.");
                    }
                    widgets::muted(ui, about);
                },
                |ui| {
                    let install = ui.add_enabled_ui(self.job.is_none(), |ui| {
                        widgets::secondary_button(ui, "Install")
                    });
                    if install.inner.clicked() {
                        let first = self.config.current.is_none();
                        self.start_install(ctx, release.clone(), first);
                    }
                    if !release.html_url.is_empty()
                        && widgets::secondary_button(ui, "Release notes").clicked()
                    {
                        ctx.open_url(egui::OpenUrl::new_tab(&release.html_url));
                    }
                },
            );
        }
        widgets::hairline(ui);
    }
}
