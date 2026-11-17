//! The launcher's window: the frame, the shared state and the footer. Each
//! page is in its own file.

mod backups;
mod dialogs;
mod frame;
mod play;
mod settings;
mod versions;

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
use crate::versions as release_menu;
use crate::widgets::{self, Status};
use eframe::egui;
use std::cmp::Ordering as CmpOrdering;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc};
use std::time::Duration;

pub fn run(args: Vec<String>) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: frame::viewport(
            egui::ViewportBuilder::default()
                .with_title("kjk launcher")
                .with_inner_size(theme::WINDOW)
                .with_min_inner_size(theme::WINDOW_MIN),
        ),
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
        frame::start(ctx);
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
                    widgets::page_title(ui, "The launcher cannot start");
                    widgets::muted(ui, error.as_str());
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

        let top = frame::bar(ctx);
        egui::Panel::top("tabs")
            .frame(
                egui::Frame::new()
                    .fill(theme::GROUND)
                    .inner_margin(top.margin),
            )
            .resizable(false)
            .show(ui, |ui| self.top_bar(ui, top.height));

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
        frame::edges(ctx);
        let busy = self.job.is_some() || self.checking.is_some() || self.asking.is_some();
        self.shot.frame(ctx, busy);
    }

    /// The mark, the word "kjk-engine" and the four tab names. The open tab
    /// is `INK` with a 1px `INK` underline, the others `INK_FAINT`.
    fn top_bar(&mut self, ui: &mut egui::Ui, height: f32) {
        let whole = ui.clip_rect();
        frame::bar_background(ui, whole);
        let size = egui::vec2(ui.available_width(), height);
        let layout = egui::Layout::left_to_right(egui::Align::Center);
        ui.allocate_ui_with_layout(size, layout, |ui| {
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
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                frame::window_buttons(ui);
            });
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

    /// The download or move in progress: what it is doing, a progress
    /// line and, for a download, a Cancel button.
    fn progress(&mut self, ui: &mut egui::Ui) {
        let Some(job) = &self.job else { return };
        let (stage, done, total) = job.progress.snapshot();
        let fraction = if total > 0 {
            done as f32 / total as f32
        } else {
            0.0
        };
        let mut cancel = false;
        widgets::row(
            ui,
            |ui| {
                ui.label(widgets::text(&job.title, Text::Heading, theme::INK));
                widgets::muted(
                    ui,
                    format!(
                        "{stage}. {:.1} of {:.1} MB.",
                        done as f64 / 1e6,
                        total as f64 / 1e6
                    ),
                );
            },
            |ui| {
                if job.cancellable {
                    cancel = widgets::secondary_button(ui, "Cancel").clicked();
                }
            },
        );
        widgets::progress(ui, fraction);
        if cancel {
            job.progress.cancel.store(true, Ordering::Relaxed);
        }
        ui.add_space(theme::SPACE_6);
    }

    /// The install folder, with buttons to change and open it.
    fn install_folder(&mut self, ui: &mut egui::Ui) {
        let folder = self.paths.versions.display().to_string();
        widgets::row(
            ui,
            |ui| {
                ui.label(widgets::text("Install folder", Text::Heading, theme::INK));
                ui.label(widgets::text(folder, Text::Mono, theme::INK_MUTED));
                widgets::muted(ui, "Each version is in its own folder inside this one.");
            },
            |ui| {
                if widgets::secondary_button(ui, "Open folder").clicked() {
                    launch::open_folder(&self.paths.versions);
                }
                let change = ui.add_enabled_ui(self.job.is_none(), |ui| {
                    widgets::secondary_button(ui, "Change folder")
                });
                if change.inner.clicked() {
                    self.pick_install_dir();
                }
            },
        );
    }
}
