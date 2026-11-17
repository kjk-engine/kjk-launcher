//! The Versions page: the installed versions and the releases on GitHub,
//! one line each.

use super::*;

/// Where the date and the size start on each line, so they line up down
/// the page.
struct Columns {
    date: f32,
    size: f32,
}

impl Columns {
    /// Columns after the widest tag and the widest date of the page.
    fn fit(ui: &egui::Ui, tags: &[&str], dates: &[String]) -> Columns {
        let widest = |words: &mut dyn Iterator<Item = &str>, style: Text| {
            words
                .map(|w| {
                    ui.painter()
                        .layout_no_wrap(w.to_string(), theme::font(style), theme::INK)
                        .size()
                        .x
                })
                .fold(0.0, f32::max)
        };
        let date = widest(&mut tags.iter().copied(), Text::Mono) + theme::SPACE_8;
        let size =
            date + widest(&mut dates.iter().map(String::as_str), Text::Body) + theme::SPACE_8;
        Columns { date, size }
    }
}

/// One version's line: the tag, the date and the size at their columns,
/// and `right` adding its controls from the right end.
fn version_line(
    ui: &mut egui::Ui,
    columns: &Columns,
    tag: &str,
    date: &str,
    size: Option<&str>,
    right: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, theme::VERSION_ROW), egui::Sense::hover());
    let middle = rect.center().y;
    let left = rect.left();
    widgets::paint_centred(ui, tag, Text::Mono, theme::INK, left, middle);
    widgets::paint_centred(
        ui,
        date,
        Text::Body,
        theme::INK_MUTED,
        left + columns.date,
        middle,
    );
    if let Some(size) = size {
        widgets::paint_centred(
            ui,
            size,
            Text::Body,
            theme::INK_MUTED,
            left + columns.size,
            middle,
        );
    }
    let layout = egui::Layout::right_to_left(egui::Align::Center);
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(layout), right);
    response
}

/// A state word at the right of a line, such as "Selected".
fn state_word(ui: &mut egui::Ui, word: &str, color: egui::Color32) {
    ui.label(widgets::text(word, Text::Label, color));
}

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
        let tags: Vec<&str> = installed
            .iter()
            .map(|i| release_menu::shown_tag(&i.tag))
            .chain(
                releases
                    .iter()
                    .map(|r| release_menu::shown_tag(&r.tag_name)),
            )
            .collect();
        let dates: Vec<String> = installed
            .iter()
            .map(|i| github::long_date(i.published_at.as_deref()))
            .chain(
                releases
                    .iter()
                    .map(|r| github::long_date(r.published_at.as_deref())),
            )
            .collect();
        let columns = Columns::fit(ui, &tags, &dates);

        widgets::section_label(ui, "Installed");
        if installed.is_empty() {
            widgets::muted(ui, "No version is installed yet.");
        }
        let current = self.config.current.clone();
        for (version, date) in installed.iter().zip(&dates) {
            widgets::hairline(ui);
            let size = megabytes(self.sizes.get(&version.dir).copied().unwrap_or(0));
            let selected = current.as_deref() == Some(version.tag.as_str());
            let tag = release_menu::shown_tag(&version.tag);
            let line = version_line(ui, &columns, tag, date, Some(&size), |ui| {
                if widgets::compact_button(ui, "Uninstall").clicked() {
                    self.confirm = Some(Confirm::Uninstall(version.tag.clone()));
                }
                if selected {
                    state_word(ui, "Selected", theme::INK);
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
        for (release, date) in releases.iter().zip(&dates[installed.len()..]) {
            widgets::hairline(ui);
            let skipped = self.config.skipped.as_deref() == Some(release.tag_name.as_str());
            let tag = release_menu::shown_tag(&release.tag_name);
            version_line(ui, &columns, tag, date, None, |ui| {
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
                    state_word(ui, "Skipped", theme::INK_MUTED);
                } else if release.prerelease {
                    state_word(ui, "Pre-release", theme::INK_MUTED);
                }
            });
        }
        widgets::hairline(ui);
    }
}
