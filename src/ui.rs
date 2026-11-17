//! The launcher's window: Play, the update offer, versions, backups, settings.

use crate::backup::{self, Backup};
use crate::config::{Config, Paths};
use crate::ffmpeg;
use crate::github::{self, Release, Releases};
use crate::install::{self, Installed, Moved, Progress};
use crate::kjk_report::{self, Game, GameStatus, Report};
use crate::kjk_settings;
use crate::launch;
use crate::shot::{self, Shot};
use crate::theme::{self, Text};
use crate::versions;
use crate::widgets::{self, Status};
use eframe::egui::{self, RichText};
use std::cmp::Ordering as CmpOrdering;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc};
use std::time::Duration;

pub fn run(args: Vec<String>) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("kjk launcher")
            .with_inner_size(theme::WINDOW)
            .with_min_inner_size(theme::WINDOW_MIN),
        ..Default::default()
    };
    eframe::run_native(
        "kjk launcher",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(&cc.egui_ctx, args)))),
    )
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Play,
    Versions,
    Backups,
    Settings,
}

impl Tab {
    const ALL: [Tab; 4] = [Tab::Play, Tab::Versions, Tab::Backups, Tab::Settings];

    fn name(self) -> &'static str {
        match self {
            Tab::Play => "Play",
            Tab::Versions => "Versions",
            Tab::Backups => "Backups",
            Tab::Settings => "Settings",
        }
    }

    /// The tab `KJK_LAUNCHER_TAB` names, or Play.
    fn from_env() -> Tab {
        let named = shot::tab();
        Tab::ALL
            .into_iter()
            .find(|tab| named.as_deref() == Some(&tab.name().to_ascii_lowercase()))
            .unwrap_or(Tab::Play)
    }
}

enum Outcome {
    Installed {
        make_current: bool,
        result: Result<Installed, String>,
    },
    Moved {
        to: PathBuf,
        moved: Moved,
    },
}

struct Job {
    title: String,
    progress: Arc<Progress>,
    cancellable: bool,
    done: mpsc::Receiver<Outcome>,
}

enum Confirm {
    Restore(PathBuf),
    Uninstall(String),
    UseOlder(String),
    ChangeFolder(PathBuf),
}

/// The game folders as the selected version reports them.
enum Games {
    /// Not asked for the selected version yet, or asked again.
    Stale,
    /// Asked, and waiting for the answer for this version.
    Asking(String),
    /// The answer for this version (`None` when it cannot report), and
    /// whether ffmpeg is installed.
    Known(String, Option<Report>, bool),
}

enum Notice {
    Info(String),
    Error(String),
}

struct Ready {
    paths: Paths,
    config: Config,
    installed: Vec<Installed>,
    sizes: HashMap<PathBuf, u64>,
    releases: Vec<Release>,
    backups: Vec<Backup>,
    checking: Option<mpsc::Receiver<Releases>>,
    online: Option<bool>,
    /// When GitHub last answered a check.
    checked_at: Option<std::time::Instant>,
    problem: Option<String>,
    job: Option<Job>,
    not_now: bool,
    confirm: Option<Confirm>,
    notice: Option<Notice>,
    tab: Tab,
    /// Passed to kjk when Play starts it.
    args: Vec<String>,
    mark: Option<egui::TextureHandle>,
    /// What the selected version reports about its game folders and ffmpeg.
    games: Games,
    /// The report being asked for, on a background thread.
    asking: Option<mpsc::Receiver<(Option<Report>, bool)>>,
    /// Whether the "Install ffmpeg" dialog is open.
    ffmpeg_dialog: bool,
    /// Whether the full "Before you play" list is open while all is ready.
    show_details: bool,
    /// Why a folder the player chose is not usable, by game.
    folder_problems: HashMap<&'static str, String>,
    /// Opens the version menu on the first frame (`KJK_LAUNCHER_MENU`).
    open_menu: bool,
    shot: Shot,
}

enum App {
    Ready(Box<Ready>),
    Broken(String),
}

impl App {
    fn new(ctx: &egui::Context, args: Vec<String>) -> App {
        theme::install(ctx);
        match crate::config::open() {
            Ok((paths, config)) => {
                install::clean_leftovers(&paths);
                let mut ready = Ready {
                    releases: github::cached(&paths),
                    installed: Vec::new(),
                    sizes: HashMap::new(),
                    backups: Vec::new(),
                    paths,
                    config,
                    checking: None,
                    online: None,
                    checked_at: None,
                    problem: None,
                    job: None,
                    not_now: false,
                    confirm: None,
                    notice: None,
                    tab: Tab::from_env(),
                    args,
                    mark: theme::mark(ctx),
                    games: Games::Stale,
                    asking: None,
                    ffmpeg_dialog: shot::ffmpeg_open(),
                    show_details: false,
                    folder_problems: HashMap::new(),
                    open_menu: shot::menu_open(),
                    shot: Shot::from_env(),
                };
                ready.refresh();
                if ready.config.check_on_start {
                    ready.check(ctx);
                }
                App::Ready(Box::new(ready))
            }
            Err(error) => App::Broken(error),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        match self {
            App::Broken(error) => {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.heading("The launcher cannot start");
                    ui.label(error.as_str());
                });
            }
            App::Ready(ready) => ready.update(ui),
        }
    }
}

/// "Checked for updates 3 minutes ago."
fn checked_ago(minutes: u64) -> String {
    match minutes {
        0 => "Checked for updates less than a minute ago.".to_string(),
        1 => "Checked for updates 1 minute ago.".to_string(),
        n => format!("Checked for updates {n} minutes ago."),
    }
}

/// A dialog in the middle of the window: `SURFACE_RAISED`, `RADIUS_LG`
/// corners and a title in the title style.
fn dialog(ctx: &egui::Context, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Window::new(title)
        .title_bar(false)
        .collapsible(false)
        .resizable(false)
        .frame(widgets::raised_frame().inner_margin(egui::Margin::same(theme::SPACE_6 as i8)))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_max_width(theme::DIALOG_WIDTH);
            ui.label(widgets::text(title, Text::Title, theme::INK));
            ui.add_space(theme::SPACE_3);
            add(ui);
        });
}

fn megabytes(bytes: u64) -> String {
    format!("{:.0} MB", bytes as f64 / 1e6)
}

impl Ready {
    fn check(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let paths = self.paths.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(github::fetch(&paths));
            ctx.request_repaint();
        });
        self.checking = Some(rx);
    }

    fn save(&mut self) {
        if let Err(e) = self.config.save(&self.paths) {
            self.notice = Some(Notice::Error(e));
        }
    }

    /// Reads the install folder and backups again, and keeps `current`
    /// pointing at an installed version.
    fn refresh(&mut self) {
        self.installed = install::installed(&self.paths);
        // A folder unzipped by hand has no date; GitHub's list knows it.
        for version in self
            .installed
            .iter_mut()
            .filter(|v| v.published_at.is_none())
        {
            if let Some(release) = self.releases.iter().find(|r| r.tag_name == version.tag) {
                version.published_at = release.published_at.clone();
                version.prerelease = release.prerelease;
            }
        }
        self.sizes = self
            .installed
            .iter()
            .map(|i| (i.dir.clone(), install::dir_size(&i.dir)))
            .collect();
        self.backups = backup::list(&self.paths);
        let known = self
            .config
            .current
            .as_deref()
            .is_some_and(|tag| self.installed.iter().any(|i| i.tag == tag));
        if !known {
            let newest = self.installed.first().map(|i| i.tag.clone());
            if newest != self.config.current {
                self.config.current = newest;
                self.save();
            }
        }
    }

    fn current(&self) -> Option<&Installed> {
        let tag = self.config.current.as_deref()?;
        self.installed.iter().find(|i| i.tag == tag)
    }

    fn offer(&self) -> Option<Release> {
        let current = self.current().map(Installed::key);
        current?;
        github::offer(&self.releases, &self.config, current).cloned()
    }

    /// Makes `tag` the version Play starts, asking first when it is older.
    fn choose(&mut self, tag: String) {
        let older = match (self.current(), self.installed.iter().find(|i| i.tag == tag)) {
            (Some(now), Some(picked)) => {
                github::compare(
                    &picked.tag,
                    picked.published_at.as_deref(),
                    &now.tag,
                    now.published_at.as_deref(),
                ) == CmpOrdering::Less
            }
            _ => false,
        };
        if older {
            self.confirm = Some(Confirm::UseOlder(tag));
        } else {
            self.config.current = Some(tag);
            self.games = Games::Stale;
            self.save();
        }
    }

    fn start_install(&mut self, ctx: &egui::Context, release: Release, make_current: bool) {
        if self.job.is_some() {
            return;
        }
        if self.installed.iter().any(|i| i.tag == release.tag_name) {
            let mut text = format!("{} is already installed.", release.tag_name);
            if make_current {
                self.choose(release.tag_name.clone());
                text.push_str(" Play now starts it.");
            }
            self.notice = Some(Notice::Info(text));
            return;
        }
        let progress = Arc::new(Progress::default());
        let (tx, rx) = mpsc::channel();
        let paths = self.paths.clone();
        let worker = progress.clone();
        let ctx = ctx.clone();
        let title = format!("Installing {}", release.tag_name);
        std::thread::spawn(move || {
            let result = install::install(&paths, &release, &worker);
            let _ = tx.send(Outcome::Installed {
                make_current,
                result,
            });
            ctx.request_repaint();
        });
        self.job = Some(Job {
            title,
            progress,
            cancellable: true,
            done: rx,
        });
    }

    fn start_move(&mut self, ctx: &egui::Context, to: PathBuf) {
        let progress = Arc::new(Progress::default());
        let (tx, rx) = mpsc::channel();
        let versions = self.installed.clone();
        let worker = progress.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let moved = install::move_installs(&versions, &to, &worker);
            let _ = tx.send(Outcome::Moved { to, moved });
            ctx.request_repaint();
        });
        self.job = Some(Job {
            title: "Moving installed versions".into(),
            progress,
            cancellable: false,
            done: rx,
        });
    }

    fn set_install_dir(&mut self, dir: PathBuf) {
        self.config.install_dir = (dir != self.paths.default_install_dir()).then_some(dir);
        if let Err(e) = self.paths.use_install_dir(&self.config) {
            self.notice = Some(Notice::Error(e));
        }
        self.save();
        self.refresh();
    }

    fn pick_install_dir(&mut self) {
        let Some(dir) = rfd::FileDialog::new()
            .set_title("Choose the folder for kjk's versions")
            .set_directory(&self.paths.versions)
            .pick_folder()
        else {
            return;
        };
        if dir == self.paths.versions {
            return;
        }
        if let Some(inside) = self.installed.iter().find(|v| dir.starts_with(&v.dir)) {
            self.notice = Some(Notice::Error(format!(
                "That folder is inside the install of {}; choose the folder above the versions.",
                inside.tag
            )));
            return;
        }
        if self.installed.is_empty() {
            self.set_install_dir(dir);
        } else {
            self.confirm = Some(Confirm::ChangeFolder(dir));
        }
    }

    fn poll(&mut self) {
        if let Some(rx) = &self.checking {
            if let Ok(releases) = rx.try_recv() {
                self.checking = None;
                self.online = Some(releases.online);
                if releases.online {
                    self.checked_at = Some(std::time::Instant::now());
                }
                self.problem = releases.problem;
                if !releases.list.is_empty() {
                    self.releases = releases.list;
                }
            }
        }
        if let Some(rx) = &self.asking {
            if let Ok(answer) = rx.try_recv() {
                self.asking = None;
                if let Games::Asking(tag) = &self.games {
                    let (report, ffmpeg) = answer;
                    self.games = Games::Known(tag.clone(), report, ffmpeg);
                }
            }
        }
        let finished = self.job.as_ref().and_then(|job| job.done.try_recv().ok());
        let Some(outcome) = finished else { return };
        self.job = None;
        match outcome {
            Outcome::Installed {
                make_current,
                result: Ok(installed),
            } => {
                let mut text = format!("Installed {}.", installed.tag);
                if make_current {
                    self.config.current = Some(installed.tag.clone());
                    self.not_now = false;
                    self.save();
                    text.push_str(" Play now starts it.");
                }
                let removed = install::prune(&self.paths, &self.config);
                if !removed.is_empty() {
                    text.push_str(&format!(
                        " Uninstalled old versions: {}.",
                        removed.join(", ")
                    ));
                }
                self.notice = Some(Notice::Info(text));
            }
            Outcome::Installed { result: Err(e), .. } => {
                self.notice = Some(Notice::Error(format!("Installing failed: {e}")));
            }
            Outcome::Moved { to, moved } => {
                let mut text = format!("Moved {} version(s) to {}.", moved.moved, to.display());
                for problem in &moved.problems {
                    text.push_str(&format!(" {problem}."));
                }
                self.notice = Some(if moved.problems.is_empty() {
                    Notice::Info(text)
                } else {
                    Notice::Error(text)
                });
                self.set_install_dir(to);
            }
        }
        self.refresh();
    }

    fn update(&mut self, ui: &mut egui::Ui) {
        let ctx = &ui.ctx().clone();
        self.poll();
        if self.job.is_some() || self.checking.is_some() || self.asking.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        } else {
            // The footer counts the minutes since the last check.
            ctx.request_repaint_after(Duration::from_secs(30));
        }

        let page = egui::Frame::new()
            .fill(theme::GROUND)
            .inner_margin(egui::Margin::same(theme::SPACE_8 as i8));
        let bar = egui::Frame::new()
            .fill(theme::GROUND)
            .inner_margin(egui::Margin::symmetric(
                theme::SPACE_8 as i8,
                theme::SPACE_3 as i8,
            ));

        egui::Panel::top("tabs")
            .frame(bar)
            .resizable(false)
            .show(ui, |ui| self.top_bar(ui));

        egui::Panel::bottom("status")
            .frame(bar)
            .resizable(false)
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| self.status_line(ui));
            });

        egui::CentralPanel::default().frame(page).show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| match self.tab {
                    Tab::Play => self.play_tab(ui, ctx),
                    Tab::Versions => self.versions_tab(ui, ctx),
                    Tab::Backups => self.backups_tab(ui),
                    Tab::Settings => self.settings_tab(ui, ctx),
                });
        });

        self.confirm_window(ctx);
        let busy = self.job.is_some() || self.checking.is_some() || self.asking.is_some();
        self.shot.frame(ctx, busy);
    }

    /// The mark, the word "kjk-engine" and the four tab names. The open tab
    /// is `INK` with a 1px `INK` underline, the others `INK_FAINT`.
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(mark) = &self.mark {
                ui.add(
                    egui::Image::new(mark).fit_to_exact_size(egui::vec2(theme::MARK, theme::MARK)),
                );
            }
            ui.label(widgets::text("kjk-engine", Text::Wordmark, theme::INK));
            ui.add_space(theme::SPACE_6);
            ui.spacing_mut().item_spacing.x = theme::SPACE_6;
            for tab in Tab::ALL {
                let open = self.tab == tab;
                let color = if open { theme::INK } else { theme::INK_FAINT };
                let label = egui::Label::new(widgets::text(tab.name(), Text::Label, color))
                    .sense(egui::Sense::click());
                let response = widgets::add(ui, label);
                if open {
                    let under = response.rect.bottom() + theme::SPACE_1;
                    ui.painter().hline(
                        response.rect.x_range(),
                        under,
                        egui::Stroke::new(theme::HAIRLINE, theme::INK),
                    );
                }
                if response.clicked() {
                    if tab == Tab::Play && self.tab != Tab::Play {
                        self.games = Games::Stale;
                    }
                    self.tab = tab;
                }
            }
        });
    }

    /// The footer's left side: a notice, or when the launcher last checked
    /// for updates.
    fn status_line(&mut self, ui: &mut egui::Ui) {
        match &self.notice {
            Some(Notice::Info(text)) => widgets::status(ui, Status::Ok, text),
            Some(Notice::Error(text)) => widgets::status(ui, Status::Error, text),
            None => {
                let words = if self.checking.is_some() {
                    "Checking for updates.".to_string()
                } else if self.problem.is_some() {
                    "The launcher could not reach GitHub. Play still works.".to_string()
                } else if let Some(at) = self.checked_at {
                    checked_ago(at.elapsed().as_secs() / 60)
                } else {
                    "The launcher has not checked for updates yet.".to_string()
                };
                ui.label(widgets::text(words, Text::Label, theme::INK_MUTED));
            }
        }
        if self.notice.is_some() && widgets::text_button(ui, "Dismiss").clicked() {
            self.notice = None;
        }
    }

    fn progress(&mut self, ui: &mut egui::Ui) {
        let Some(job) = &self.job else { return };
        let (stage, done, total) = job.progress.snapshot();
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.label(format!("{}: {stage}", job.title));
            let fraction = if total > 0 {
                done as f32 / total as f32
            } else {
                0.0
            };
            ui.add(egui::ProgressBar::new(fraction).text(format!(
                "{:.1} / {:.1} MB",
                done as f64 / 1e6,
                total as f64 / 1e6
            )));
            if job.cancellable && ui.button("Cancel").clicked() {
                job.progress.cancel.store(true, Ordering::Relaxed);
            }
        });
        ui.add_space(8.0);
    }

    /// The install folder, with buttons to change and open it.
    fn install_folder(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label("Install folder:");
            ui.label(widgets::text(
                self.paths.versions.display().to_string(),
                Text::Mono,
                theme::INK_MUTED,
            ));
            if ui
                .add_enabled(self.job.is_none(), egui::Button::new("Change…"))
                .on_hover_text("Each version goes in its own subfolder, such as 0.19.0")
                .clicked()
            {
                self.pick_install_dir();
            }
            if ui.button("Open").clicked() {
                launch::open_folder(&self.paths.versions);
            }
        });
    }

    fn play_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.progress(ui);

        let Some(current) = self.current().cloned() else {
            self.nothing_installed(ui, ctx);
            return;
        };

        self.ask_kjk(ctx, &current);
        let (report, ffmpeg) = match &self.games {
            Games::Known(_, report, ffmpeg) => (report.clone(), *ffmpeg),
            _ => (None, true),
        };
        let blocked = kjk_report::blocked_by(report.as_ref());

        ui.horizontal(|ui| {
            self.version_menu(ui, ctx, &current.tag);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let play = ui
                    .add_enabled_ui(blocked.is_none(), |ui| widgets::primary_button(ui, "Play"))
                    .inner;
                if play.clicked() {
                    match launch::play(&self.paths, &mut self.config, &self.args) {
                        Ok(_) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                        Err(e) => self.notice = Some(Notice::Error(e)),
                    }
                }
                if let Some(game) = blocked {
                    ui.label(widgets::text(
                        format!("Choose the {} folder to play.", game.name()),
                        Text::Body,
                        theme::INK_MUTED,
                    ));
                }
                if install::editor_exe(&current.dir).is_some()
                    && widgets::secondary_button(ui, "Open the save editor").clicked()
                {
                    if let Err(e) = launch::save_editor(&self.paths, &self.config) {
                        self.notice = Some(Notice::Error(e));
                    }
                }
            });
        });
        if !self.config.launched.contains(&current.tag) && self.config.backup_before_new_version {
            ui.label(widgets::text(
                "This version has not started before. Your saves are backed up when it starts.",
                Text::Body,
                theme::INK_MUTED,
            ));
        }
        ui.add_space(theme::SPACE_6);

        if let Some(offer) = self.offer().filter(|_| !self.not_now) {
            self.offer_box(ui, ctx, offer);
            ui.add_space(theme::SPACE_6);
        }

        self.before_you_play(ui, &current, report.as_ref(), ffmpeg);
        self.ffmpeg_window(ui.ctx());
    }

    /// Asks the selected version for its report on a background thread,
    /// unless it was asked already.
    fn ask_kjk(&mut self, ctx: &egui::Context, current: &Installed) {
        let fresh = match &self.games {
            Games::Stale => false,
            Games::Asking(tag) | Games::Known(tag, ..) => *tag == current.tag,
        };
        if fresh {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let kjk = install::game_binary(&current.dir);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let report = kjk_report::ask(&kjk);
            // A kjk without the command, or a report without ffmpeg, leaves
            // the launcher to look for ffmpeg itself.
            let ffmpeg = report
                .as_ref()
                .and_then(|r| r.ffmpeg)
                .unwrap_or_else(ffmpeg::installed);
            let _ = tx.send((report, ffmpeg));
            ctx.request_repaint();
        });
        self.asking = Some(rx);
        self.games = Games::Asking(current.tag.clone());
    }

    /// The "Before you play" section: a row for each game.
    fn before_you_play(
        &mut self,
        ui: &mut egui::Ui,
        current: &Installed,
        report: Option<&Report>,
        ffmpeg: bool,
    ) {
        widgets::section_label(ui, "Before you play");
        ui.add_space(theme::SPACE_2);
        if matches!(self.games, Games::Asking(_) | Games::Stale) {
            ui.label(widgets::text(
                "The launcher is asking kjk where the games are.",
                Text::Body,
                theme::INK_MUTED,
            ));
            return;
        }
        let Some(report) = report else {
            ui.label(widgets::text(
                "This version of kjk cannot report its game folders. It finds them itself when it starts.",
                Text::Body,
                theme::INK_MUTED,
            ));
            ui.add_space(theme::SPACE_2);
            widgets::hairline(ui);
            self.ffmpeg_row(ui, ffmpeg);
            widgets::hairline(ui);
            return;
        };
        if kjk_report::all_ready(report, ffmpeg) && !self.show_details {
            ui.horizontal(|ui| {
                widgets::status(ui, Status::Ok, "Ready to play");
                if widgets::text_button(ui, "Show details").clicked() {
                    self.show_details = true;
                }
            });
            return;
        }
        if kjk_report::all_ready(report, ffmpeg)
            && widgets::text_button(ui, "Hide details").clicked()
        {
            self.show_details = false;
        }
        for game in Game::ALL {
            widgets::hairline(ui);
            let status = report.status(game).cloned().unwrap_or(GameStatus::Missing);
            self.game_row(ui, current, game, &status);
        }
        widgets::hairline(ui);
        self.ffmpeg_row(ui, ffmpeg);
        widgets::hairline(ui);
    }

    /// The ffmpeg row: its status, why it matters when it is missing, and a
    /// button that shows how to install it.
    fn ffmpeg_row(&mut self, ui: &mut egui::Ui, installed: bool) {
        ui.add_space(theme::SPACE_3);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(widgets::text("ffmpeg", Text::Heading, theme::INK));
                if !installed {
                    ui.label(widgets::text(
                        "Movies are skipped until ffmpeg is installed. The game runs without it.",
                        Text::Body,
                        theme::INK_MUTED,
                    ));
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::secondary_button(ui, "How to install").clicked() {
                    self.ffmpeg_dialog = true;
                }
                ui.add_space(theme::SPACE_4);
                if installed {
                    widgets::status(ui, Status::Ok, "Installed");
                } else {
                    widgets::status(ui, Status::Warn, "Not installed");
                }
            });
        });
        ui.add_space(theme::SPACE_3);
    }

    /// The "Install ffmpeg" dialog.
    fn ffmpeg_window(&mut self, ctx: &egui::Context) {
        if !self.ffmpeg_dialog {
            return;
        }
        let mut open = true;
        let mut check = false;
        dialog(ctx, "Install ffmpeg", |ui| {
            ui.label(widgets::text(
                "Run this command in a terminal to install ffmpeg.",
                Text::Body,
                theme::INK,
            ));
            ui.horizontal(|ui| {
                ui.label(widgets::text(
                    ffmpeg::install_command(),
                    Text::Mono,
                    theme::INK,
                ));
                if widgets::secondary_button(ui, "Copy").clicked() {
                    ui.ctx().copy_text(ffmpeg::install_command().to_string());
                }
            });
            if cfg!(target_os = "linux") {
                ui.label(widgets::text(
                    "Use your own package manager if this computer does not have apt.",
                    Text::Body,
                    theme::INK_MUTED,
                ));
            }
            ui.label(widgets::text(
                "The ffmpeg website lists other ways to install it.",
                Text::Body,
                theme::INK_MUTED,
            ));
            ui.hyperlink_to(
                widgets::text(ffmpeg::DOWNLOAD_PAGE, Text::Mono, theme::INK),
                ffmpeg::DOWNLOAD_PAGE,
            );
            ui.add_space(theme::SPACE_3);
            ui.horizontal(|ui| {
                if widgets::primary_button(ui, "Check again").clicked() {
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
        self.ffmpeg_dialog = open;
    }

    /// One game's row: its name, its folder or why kjk needs it, its status
    /// and a button to choose the folder.
    fn game_row(
        &mut self,
        ui: &mut egui::Ui,
        current: &Installed,
        game: Game,
        status: &GameStatus,
    ) {
        ui.add_space(theme::SPACE_3);
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(widgets::text(game.name(), Text::Heading, theme::INK));
                match status {
                    GameStatus::Found { path, .. } | GameStatus::Refused { path, .. } => {
                        ui.label(widgets::text(
                            path.display().to_string(),
                            Text::Mono,
                            theme::INK_MUTED,
                        ));
                    }
                    GameStatus::Missing => {
                        ui.label(widgets::text(
                            game.needed_for(),
                            Text::Body,
                            theme::INK_MUTED,
                        ));
                    }
                }
                let why = match status {
                    GameStatus::Refused { why, .. } => Some(why.clone()),
                    _ => None,
                };
                if let Some(why) = self.folder_problems.get(game.key()).cloned().or(why) {
                    ui.label(widgets::text(why, Text::Body, theme::INK_MUTED));
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let found = matches!(status, GameStatus::Found { .. });
                let label = if found {
                    "Change folder"
                } else {
                    "Choose folder"
                };
                if widgets::secondary_button(ui, label).clicked() {
                    self.choose_game_folder(current, game);
                }
                ui.add_space(theme::SPACE_4);
                let (dot, word) = kjk_report::status_word(status);
                widgets::status(ui, dot, word);
            });
        });
        ui.add_space(theme::SPACE_3);
    }

    /// Asks for a folder, has kjk check it, and saves it in kjk's settings
    /// only when kjk finds the game there.
    fn choose_game_folder(&mut self, current: &Installed, game: Game) {
        let Some(folder) = rfd::FileDialog::new()
            .set_title(format!("Choose the {} folder", game.name()))
            .pick_folder()
        else {
            return;
        };
        let kjk = install::game_binary(&current.dir);
        match kjk_report::check(&kjk, game, &folder) {
            Some(GameStatus::Found { .. }) => {
                self.folder_problems.remove(game.key());
                let saved = self
                    .paths
                    .settings()
                    .ok_or_else(|| "The launcher could not find kjk's settings file.".to_string())
                    .and_then(|file| kjk_settings::set_install(&file, game, &folder));
                self.notice = Some(match saved {
                    Ok(()) => Notice::Info(format!("The {} folder is saved.", game.name())),
                    Err(e) => Notice::Error(e),
                });
                self.games = Games::Stale;
            }
            Some(GameStatus::Refused { why, .. }) => {
                self.folder_problems.insert(game.key(), why);
            }
            _ => {
                self.notice = Some(Notice::Error(
                    "This version of kjk cannot check a folder.".to_string(),
                ));
            }
        }
    }

    /// The Play page before any version is installed.
    fn nothing_installed(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label(widgets::text(
            "kjk is not installed yet",
            Text::Display,
            theme::INK,
        ));
        ui.label(widgets::text(
            "kjk needs your own copies of KOTOR, KOTOR II and Jedi Academy. It finds them the first time it starts.",
            Text::Body,
            theme::INK_MUTED,
        ));
        ui.add_space(theme::SPACE_4);
        let newest = github::newest(&github::installable(
            &self.releases,
            self.config.prereleases,
        ))
        .cloned();
        match newest {
            Some(release) => {
                ui.horizontal(|ui| {
                    let label = format!("Install {}", versions::title(&release.tag_name));
                    if widgets::primary_button(ui, &label).clicked() && self.job.is_none() {
                        self.start_install(ctx, release, true);
                    }
                    if widgets::secondary_button(ui, "Show all versions").clicked() {
                        self.tab = Tab::Versions;
                    }
                });
            }
            None => {
                let words = if self.checking.is_some() {
                    "The launcher is looking for releases."
                } else {
                    "No release was found for this computer."
                };
                ui.label(widgets::text(words, Text::Body, theme::INK_MUTED));
            }
        }
    }

    /// The selected version as a title that opens the menu of releases and
    /// pre-releases.
    fn version_menu(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, current: &str) {
        let button = widgets::version_title(ui, &versions::title(current));
        let menu = versions::menu(&self.releases, &self.installed, Some(current));
        let mut picked = None;
        let mut show_all = false;
        let open = std::mem::take(&mut self.open_menu).then_some(egui::SetOpenCommand::Bool(true));
        egui::Popup::from_toggle_button_response(&button)
            .open_memory(open)
            .frame(widgets::raised_frame())
            .width(theme::MENU_WIDTH)
            .align(egui::RectAlign::BOTTOM_START)
            .show(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                egui::ScrollArea::vertical()
                    .max_height(theme::MENU_HEIGHT)
                    .show(ui, |ui| {
                        for (label, rows) in [
                            ("Releases", &menu.releases),
                            ("Pre-releases", &menu.prereleases),
                        ] {
                            if rows.is_empty() {
                                continue;
                            }
                            widgets::section_label(ui, label);
                            for row in rows {
                                let state = match (row.state, row.newest) {
                                    (versions::State::Selected, _) => "Selected",
                                    (versions::State::Installed, _) => "Installed",
                                    (versions::State::NotInstalled, true) => {
                                        "Newest. Not installed"
                                    }
                                    (versions::State::NotInstalled, false) => "Not installed",
                                };
                                if widgets::menu_row(ui, &row.tag, Text::Mono, state).clicked() {
                                    picked = Some(row.clone());
                                }
                            }
                            ui.add_space(theme::SPACE_2);
                        }
                    });
                widgets::hairline(ui);
                if widgets::menu_row(ui, "Show all versions", Text::Body, "").clicked() {
                    show_all = true;
                }
            });
        if show_all {
            self.tab = Tab::Versions;
        }
        match picked {
            Some(row) if row.state == versions::State::NotInstalled => {
                if let Some(release) = self.releases.iter().find(|r| r.tag_name == row.tag) {
                    self.start_install(ctx, release.clone(), true);
                }
            }
            Some(row) if row.state == versions::State::Installed => self.choose(row.tag),
            _ => {}
        }
    }

    /// The notice of a newer version.
    fn offer_box(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, offer: Release) {
        let version = versions::title(&offer.tag_name);
        widgets::panel(ui, |ui| {
            ui.label(widgets::text(
                format!("{version} is available."),
                Text::Heading,
                theme::INK,
            ));
            ui.label(widgets::text(
                "Your saves are backed up before the new version starts for the first time.",
                Text::Body,
                theme::INK_MUTED,
            ));
            ui.add_space(theme::SPACE_3);
            ui.horizontal(|ui| {
                if !offer.html_url.is_empty()
                    && widgets::secondary_button(ui, "Release notes").clicked()
                {
                    ctx.open_url(egui::OpenUrl::new_tab(&offer.html_url));
                }
                if widgets::secondary_button(ui, "Skip this version").clicked() {
                    self.config.skipped = Some(offer.tag_name.clone());
                    self.save();
                }
                if widgets::secondary_button(ui, "Update").clicked() && self.job.is_none() {
                    self.start_install(ctx, offer.clone(), true);
                }
            });
        });
    }

    fn versions_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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

    fn backups_tab(&mut self, ui: &mut egui::Ui) {
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

    fn settings_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
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

    fn confirm_window(&mut self, ctx: &egui::Context) {
        let Some(confirm) = &self.confirm else { return };
        let (title, text, yes, alternative) = match confirm {
            Confirm::Restore(path) => (
                "Restore backup",
                format!(
                    "Replace kjk's saves and settings with {}?\n\nThe present ones are backed up first. Close kjk before you continue.",
                    path.file_name().unwrap_or_default().to_string_lossy()
                ),
                "Restore",
                None,
            ),
            Confirm::Uninstall(tag) => {
                let size = self
                    .installed
                    .iter()
                    .find(|i| &i.tag == tag)
                    .and_then(|i| self.sizes.get(&i.dir))
                    .copied()
                    .unwrap_or(0);
                let mut text = format!("Delete kjk {tag} ({}) from this computer? Saves are not touched.", megabytes(size));
                if self.config.current.as_deref() == Some(tag.as_str()) {
                    match self.installed.iter().find(|i| &i.tag != tag) {
                        Some(next) => text.push_str(&format!("\n\nPlay will start {} instead.", next.tag)),
                        None => text.push_str("\n\nNo other version is installed."),
                    }
                }
                ("Uninstall version", text, "Uninstall", None)
            }
            Confirm::UseOlder(tag) => (
                "Go back to an older version",
                format!(
                    "Saves made or opened by a newer version may not load in {tag}.\n\nIf they don't, restore the backup taken before you updated (Backups tab)."
                ),
                "Use it",
                None,
            ),
            Confirm::ChangeFolder(dir) => (
                "Change install folder",
                format!(
                    "Install new versions in {}?\n\nThe {} installed version(s) can move there too, or stay in {} (the launcher then no longer lists them).",
                    dir.display(),
                    self.installed.len(),
                    self.paths.versions.display()
                ),
                "Move them",
                Some("Leave them"),
            ),
        };
        let mut answer = None;
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_max_width(440.0);
                ui.label(text);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button(RichText::new(yes).strong()).clicked() {
                        answer = Some(Some(true));
                    }
                    if let Some(alternative) = alternative {
                        if ui.button(alternative).clicked() {
                            answer = Some(Some(false));
                        }
                    }
                    if ui.button("Cancel").clicked() {
                        answer = Some(None);
                    }
                });
            });
        let Some(answer) = answer else { return };
        let confirm = self.confirm.take().unwrap();
        let Some(yes) = answer else { return };
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
                match install::remove(&self.paths, &mut self.config, &tag) {
                    Ok(()) => self.notice = Some(Notice::Info(format!("Uninstalled {tag}."))),
                    Err(e) => self.notice = Some(Notice::Error(e)),
                }
                self.save();
            }
            Confirm::UseOlder(tag) => {
                self.config.current = Some(tag);
                self.save();
            }
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
}
