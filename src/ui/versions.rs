//! The Versions page: the installed versions and the releases on GitHub,
//! one line each.

use super::*;

impl Ready {
    pub(super) fn versions_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.label(widgets::text("Versions", Text::Title, theme::INK));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let check = ui.add_enabled_ui(self.checking.is_none(), |ui| {
                    widgets::compact_button(ui, "Check for updates")
                });
                if check.inner.clicked() {
                    self.check(ctx);
                }
            });
        });
        ui.add_space(theme::SPACE_4);
        self.progress(ui);

        let installed = self.installed.clone();
        let releases: Vec<Release> = github::installable(&self.releases, self.config.prereleases)
            .into_iter()
            .filter(|r| !installed.iter().any(|i| i.tag == r.tag_name))
            .cloned()
            .collect();
        // The words of each line: tag, date and, when installed, size.
        let words: Vec<Vec<String>> = installed
            .iter()
            .map(|i| {
                let size = megabytes(self.sizes.get(&i.dir).copied().unwrap_or(0));
                vec![
                    release_menu::shown_tag(&i.tag).to_string(),
                    github::long_date(i.published_at.as_deref()),
                    size,
                ]
            })
            .chain(releases.iter().map(|r| {
                vec![
                    release_menu::shown_tag(&r.tag_name).to_string(),
                    github::long_date(r.published_at.as_deref()),
                ]
            }))
            .collect();
        let cells: Vec<Vec<widgets::Cell>> = words
            .iter()
            .map(|line| {
                line.iter()
                    .enumerate()
                    .map(|(column, words)| match column {
                        0 => widgets::Cell::new(words, Text::Mono, theme::INK),
                        _ => widgets::Cell::new(words, Text::Body, theme::INK_MUTED),
                    })
                    .collect()
            })
            .collect();
        let starts = widgets::columns(ui, &cells);

        widgets::section_label(ui, "Installed");
        if installed.is_empty() {
            widgets::muted(ui, "No version is installed yet.");
        }
        let current = self.config.current.clone();
        for (version, line) in installed.iter().zip(&cells) {
            widgets::hairline(ui);
            let selected = current.as_deref() == Some(version.tag.as_str());
            let line = widgets::list_line(ui, &starts, line, |ui| {
                if widgets::compact_button(ui, "Uninstall").clicked() {
                    self.confirm = Some(Confirm::Uninstall(version.tag.clone()));
                }
                if selected {
                    widgets::state_word(ui, "Selected", theme::INK);
                } else if widgets::compact_button(ui, "Select").clicked() {
                    self.choose(version.tag.clone());
                }
            });
            if version.adopted {
                line.on_hover_text(
                    "This version was unzipped by hand. The launcher never removes it by itself.",
                );
            }
        }
        widgets::hairline(ui);
        ui.add_space(theme::SPACE_6);

        widgets::section_label(ui, "Available");
        if releases.is_empty() {
            widgets::muted(ui, "GitHub has no other release for this computer.");
        }
        for (release, line) in releases.iter().zip(&cells[installed.len()..]) {
            widgets::hairline(ui);
            let skipped = self.config.skipped.as_deref() == Some(release.tag_name.as_str());
            widgets::list_line(ui, &starts, line, |ui| {
                let install = ui.add_enabled_ui(self.job.is_none(), |ui| {
                    widgets::compact_button(ui, "Install")
                });
                if install.inner.clicked() {
                    let first = self.config.current.is_none();
                    self.start_install(ctx, release.clone(), first);
                }
                if !release.html_url.is_empty()
                    && widgets::compact_button(ui, "Release notes").clicked()
                {
                    ctx.open_url(egui::OpenUrl::new_tab(&release.html_url));
                }
                if skipped {
                    widgets::state_word(ui, "Skipped", theme::INK_MUTED);
                } else if release.prerelease {
                    widgets::state_word(ui, "Pre-release", theme::INK_MUTED);
                }
            });
        }
        widgets::hairline(ui);
    }
}
