//! The dialogs: questions before a change, how to install ffmpeg, and a
//! typed folder path.

use super::*;

/// A line of a question's text.
enum Line {
    /// A sentence in the body style.
    Say(String),
    /// A section label, such as "From".
    Label(&'static str),
    /// A path in the mono style, wrapped at its separators.
    Path(String),
}

type Question = (&'static str, Vec<Line>, &'static str, Option<&'static str>);

fn say(words: impl Into<String>) -> Line {
    Line::Say(words.into())
}

fn path(at: &std::path::Path) -> Line {
    Line::Path(at.display().to_string())
}

impl Ready {
    /// The "Install ffmpeg" dialog.
    pub(super) fn ffmpeg_window(&mut self, ctx: &egui::Context) {
        if !self.ffmpeg_dialog {
            return;
        }
        let mut open = true;
        let mut check = false;
        let escape = dialog(ctx, "Install ffmpeg", |ui, first| {
            match ffmpeg::install_hint() {
                ffmpeg::Hint::Command(command) => {
                    widgets::muted(ui, "Run this command in a terminal to install ffmpeg.");
                    let mut copy = false;
                    widgets::line(ui, |ui| {
                        ui.label(widgets::text(command, Text::Mono, theme::INK));
                        copy = widgets::compact_button(ui, "Copy").clicked();
                    });
                    if copy {
                        ui.ctx().copy_text(command.to_string());
                    }
                    widgets::muted(ui, "The ffmpeg website lists other ways to install it.");
                }
                ffmpeg::Hint::SteamOs => widgets::muted(
                    ui,
                    "SteamOS does not let programs be installed with a command. See the ffmpeg download page.",
                ),
                ffmpeg::Hint::LinkOnly => {}
            }
            widgets::link(ui, ffmpeg::DOWNLOAD_PAGE);
            ui.add_space(theme::SPACE_3);
            ui.horizontal(|ui| {
                if main_button(ui, "Check again", first).clicked() {
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
        self.ffmpeg_dialog = open && !escape;
    }

    /// The question before a change: its title, its lines, the words of
    /// its main button and, when there is one, of the other answer.
    fn question(&self, confirm: &Confirm) -> Question {
        match confirm {
            Confirm::Restore(backup) => (
                "Restore a backup",
                vec![
                    say("The saves and settings of kjk are replaced with those in this backup."),
                    Line::Path(backup.file_name().unwrap_or_default().to_string_lossy().into()),
                    say("The launcher backs up the present ones first."),
                    say("Close kjk before you restore."),
                ],
                "Restore",
                None,
            ),
            Confirm::Uninstall(tag) => {
                let freed = self
                    .installed
                    .iter()
                    .find(|i| &i.tag == tag)
                    .and_then(|i| self.sizes.get(&i.dir))
                    .copied()
                    .unwrap_or(0);
                let mut text = vec![
                    say(format!(
                        "{} is deleted from this computer. It frees {}.",
                        release_menu::title(tag),
                        size(freed)
                    )),
                    say("Your saves are not touched."),
                ];
                if self.config.current.as_deref() == Some(tag.as_str()) {
                    text.push(match self.installed.iter().find(|i| &i.tag != tag) {
                        Some(next) => say(format!("Play then starts {}.", release_menu::title(&next.tag))),
                        None => say("No other version is installed."),
                    });
                }
                ("Uninstall a version", text, "Uninstall", None)
            }
            Confirm::DeleteOlder => {
                let older = install::older(&self.paths, &self.config);
                let freed: u64 = older.iter().filter_map(|i| self.sizes.get(&i.dir)).sum();
                let mut text = vec![say(format!(
                    "{} older {} deleted from this computer. This frees {}.",
                    older.len(),
                    if older.len() == 1 { "version is" } else { "versions are" },
                    size(freed)
                ))];
                if let Some(current) = &self.config.current {
                    text.push(say(format!("{} stays installed.", release_menu::title(current))));
                }
                text.push(say("Your saves are not touched."));
                ("Delete older versions", text, "Delete", None)
            }
            Confirm::UseOlder(tag) => (
                "Go back to an older version",
                vec![
                    say(format!(
                        "A save made or opened by a newer version may not load in {}.",
                        release_menu::title(tag)
                    )),
                    say("If a save does not load, restore the backup from before the update in Settings."),
                ],
                "Select it",
                None,
            ),
            Confirm::ChangeFolder(dir) => (
                "Change the install folder",
                vec![
                    say("New versions are installed in this folder."),
                    path(dir),
                    say(format!(
                        "The {} installed versions can move there too. If they stay where they are, the launcher no longer lists them.",
                        self.installed.len()
                    )),
                ],
                "Move them",
                Some("Leave them"),
            ),
            Confirm::MoveKjk(to) => {
                let from = self.paths.kjk_data.clone().unwrap_or_default();
                (
                    "Move the kjk folder",
                    vec![
                        say("Everything in the kjk folder moves to the new folder."),
                        Line::Label("From"),
                        path(&from),
                        Line::Label("To"),
                        path(to),
                        say("Saves, settings, logs, backups and installed versions move with it."),
                        say("Close kjk before you move its folder."),
                    ],
                    "Move",
                    None,
                )
            }
            Confirm::UseKjk(to) => (
                "Use this folder",
                vec![
                    path(to),
                    say("This folder already has kjk files. Use it without moving anything?"),
                ],
                "Use this folder",
                None,
            ),
        }
    }

    pub(super) fn confirm_window(&mut self, ctx: &egui::Context) {
        let Some(confirm) = &self.confirm else { return };
        let (title, text, yes, alternative) = self.question(confirm);
        let problem = self.confirm_problem.clone();
        let mut answer = None;
        let escape = dialog(ctx, title, |ui, first| {
            for line in text {
                match line {
                    Line::Say(words) => {
                        ui.label(widgets::text(words, Text::Body, theme::INK));
                    }
                    Line::Label(words) => widgets::section_label(ui, words),
                    Line::Path(words) => widgets::wrapped_mono(ui, &words, theme::INK),
                }
            }
            if let Some(problem) = problem {
                ui.label(widgets::text(problem, Text::Body, theme::ERROR));
            }
            ui.add_space(theme::SPACE_3);
            ui.horizontal(|ui| {
                if main_button(ui, yes, first).clicked() {
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
        if escape {
            answer = Some(None);
        }
        let Some(answer) = answer else { return };
        let Some(yes) = answer else {
            self.confirm = None;
            self.confirm_problem = None;
            return;
        };
        // While kjk runs, the move waits in its dialog until the player
        // closes kjk and presses Move again.
        if let Some(Confirm::MoveKjk(to)) = &self.confirm {
            if let Err(why) = self.start_kjk_move(ctx, to.clone()) {
                self.confirm_problem = Some(why);
                return;
            }
        }
        let confirm = self.confirm.take().unwrap();
        self.confirm_problem = None;
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
                self.notice = Some(match install::remove(&self.paths, &mut self.config, &tag) {
                    Ok(()) => Notice::Info(format!("Uninstalled {}.", release_menu::title(&tag))),
                    Err(e) => Notice::Error(e),
                });
                self.save();
            }
            Confirm::DeleteOlder => {
                let removed = install::delete_older(&self.paths, &self.config);
                self.notice = Some(Notice::Info(match removed.len() {
                    0 => "No version was deleted. Close kjk and try again.".to_string(),
                    1 => "Deleted 1 older version.".to_string(),
                    n => format!("Deleted {n} older versions."),
                }));
                self.save();
            }
            Confirm::UseOlder(tag) => {
                self.config.current = Some(tag);
                self.save();
            }
            // Started above.
            Confirm::MoveKjk(_) => {}
            Confirm::UseKjk(to) => self.use_kjk_folder(to),
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

    /// The dialog for a typed folder path: for a system without a folder
    /// dialog, such as the Steam Deck's Gaming Mode.
    pub(super) fn typed_window(&mut self, ctx: &egui::Context) {
        let Some(typed) = &mut self.typed else { return };
        let (mut submit, mut cancel) = (false, false);
        let escape = dialog(ctx, &typed.folder_for.title(), |ui, first| {
            let field = egui::TextEdit::singleline(&mut typed.text)
                .font(theme::font(Text::Mono))
                .desired_width(f32::INFINITY);
            let field = widgets::add(ui, field);
            if first {
                field.request_focus();
            }
            if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                submit = true;
            }
            widgets::muted(ui, "Type or paste the folder's full path.");
            if let Some(problem) = &typed.problem {
                ui.label(widgets::text(problem, Text::Body, theme::ERROR));
            }
            ui.add_space(theme::SPACE_3);
            ui.horizontal(|ui| {
                submit |= widgets::primary_button(ui, "Use this folder").clicked();
                cancel = widgets::secondary_button(ui, "Cancel").clicked();
            });
        });
        if cancel || escape {
            self.typed = None;
            return;
        }
        if !submit {
            return;
        }
        match folders::typed_folder(&typed.text, dirs::home_dir().as_deref()) {
            Ok(folder) => {
                let folder_for = typed.folder_for;
                self.typed = None;
                self.folder_chosen(ctx, folder_for, folder);
            }
            Err(why) => typed.problem = Some(why),
        }
    }
}

/// A dialog in the middle of the window: `SURFACE_RAISED`, `RADIUS_LG`
/// corners and a title in the title style, over a `SCRIM`. While it is
/// open, the page behind it takes no clicks, and Tab and the arrow keys
/// move only between the dialog's controls. `add` gets `first`, which is
/// true on the frame the dialog opens, so it can put the focus on its main
/// control. Returns true when Escape closes the dialog.
pub(super) fn dialog(
    ctx: &egui::Context,
    title: &str,
    add: impl FnOnce(&mut egui::Ui, bool),
) -> bool {
    let id = egui::Id::unique("dialog");
    let shown_at = id.with("shown at");
    let now = ctx.cumulative_pass_nr();
    let first = ctx.data_mut(|data| {
        let last = data.get_temp::<u64>(shown_at);
        data.insert_temp(shown_at, now);
        last.is_none_or(|last| last + 1 < now)
    });
    let shown = egui::Modal::new(id)
        .backdrop_color(theme::SCRIM)
        .frame(widgets::raised_frame().inner_margin(egui::Margin::same(theme::SPACE_6 as i8)))
        .show(ctx, |ui| {
            ui.set_width(theme::DIALOG_WIDTH);
            ui.label(widgets::text(title, Text::Title, theme::INK));
            ui.add_space(theme::SPACE_3);
            add(ui, first);
        });
    shown.is_top_modal
        && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
}

/// A dialog's main button, which has the focus when the dialog opens.
pub(super) fn main_button(ui: &mut egui::Ui, label: &str, first: bool) -> egui::Response {
    let button = widgets::primary_button(ui, label);
    if first {
        button.request_focus();
    }
    button
}
