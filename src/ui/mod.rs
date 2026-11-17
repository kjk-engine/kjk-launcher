//! The launcher's window: the frame, the shared state and the footer. Each
//! page is in its own file.

mod dialogs;
mod frame;
mod play;
mod settings;
mod versions;

use crate::backup::{self, Backup};
use crate::config::{self, Config, Paths};
use crate::ffmpeg;
use crate::folders;
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

/// Opens the window. `steam` is the sentence to show when `--play` could
/// not start kjk at once. The window then hides after Play and the
/// launcher exits when kjk does, so Steam counts the whole session.
pub fn run(args: Vec<String>, steam: Option<String>) -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("kjk launcher")
        .with_inner_size(theme::WINDOW)
        .with_min_inner_size(theme::WINDOW_MIN);
    if let Some(icon) = theme::app_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport: frame::viewport(viewport),
        ..Default::default()
    };
    eframe::run_native(
        "kjk launcher",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(&cc.egui_ctx, args, steam)))),
    )
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Play,
    Versions,
    Settings,
}

impl Tab {
    const ALL: [Tab; 3] = [Tab::Play, Tab::Versions, Tab::Settings];

    fn name(self) -> &'static str {
        match self {
            Tab::Play => "Play",
            Tab::Versions => "Versions",
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
    /// kjk's folder moved to `to`, or why it did not.
    KjkMoved {
        to: PathBuf,
        result: Result<(), String>,
    },
    /// The launcher's update failed (on success the new launcher runs and
    /// this one has closed).
    SelfUpdateFailed {
        link: String,
    },
}

struct Job {
    title: String,
    progress: Arc<Progress>,
    cancellable: bool,
    /// The progress counts files and folders, not bytes.
    items: bool,
    done: mpsc::Receiver<Outcome>,
}

enum Confirm {
    Restore(PathBuf),
    Uninstall(String),
    UseOlder(String),
    ChangeFolder(PathBuf),
    /// Move kjk's folder to this folder.
    MoveKjk(PathBuf),
    /// Use this folder, which already has kjk's files, as kjk's folder.
    UseKjk(PathBuf),
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
    Warn(String),
    Info(String),
    Error(String),
}

/// A game folder kjk is checking on a background thread.
struct FolderCheck {
    game: Game,
    done: mpsc::Receiver<FolderChecked>,
}

/// What kjk said about a folder the player chose.
enum FolderChecked {
    /// kjk found the game there; whether the folder could be saved.
    Found(Result<(), String>),
    /// kjk did not find the game there, and why.
    Refused(String),
    /// This version of kjk cannot check a folder.
    CannotCheck,
}

struct Ready {
    paths: Paths,
    config: Config,
    installed: Vec<Installed>,
    sizes: HashMap<PathBuf, u64>,
    releases: Vec<Release>,
    backups: Vec<Backup>,
    /// The check in progress: kjk's releases and the launcher's own.
    checking: Option<mpsc::Receiver<(Releases, Releases)>>,
    /// The launcher's own releases.
    launcher_releases: Vec<Release>,
    /// Why the launcher could not update itself, with the release page.
    self_update_failed: Option<(String, String)>,
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
    /// Opened by `--play`: after Play the window hides, and the launcher
    /// exits when kjk does.
    from_steam: bool,
    mark: Option<egui::TextureHandle>,
    /// What the selected version reports about its game folders and ffmpeg.
    games: Games,
    /// The report being asked for, on a background thread.
    asking: Option<mpsc::Receiver<(Option<Report>, bool)>>,
    /// Whether the "Install ffmpeg" dialog is open.
    ffmpeg_dialog: bool,
    /// Whether the full Setup list is open while all is ready.
    show_details: bool,
    /// Why a folder the player chose is not usable, by game.
    folder_problems: HashMap<&'static str, String>,
    /// The folder check in progress.
    folder_check: Option<FolderCheck>,
    /// Whether Settings lists every backup, not only the newest.
    show_all_backups: bool,
    /// Opens the version menu on the first frame (`KJK_LAUNCHER_MENU`).
    open_menu: bool,
    shot: Shot,
}

enum App {
    Ready(Box<Ready>),
    Broken(String),
}

impl App {
    fn new(ctx: &egui::Context, args: Vec<String>, steam: Option<String>) -> App {
        theme::install(ctx);
        frame::start(ctx);
        match config::open_noting() {
            Ok((paths, config, moved)) => {
                let paths_launcher_cache = paths.launcher_cache_file.clone();
                install::clean_leftovers(&paths);
                let mut ready = Ready {
                    releases: github::cached(&paths),
                    installed: Vec::new(),
                    sizes: HashMap::new(),
                    backups: Vec::new(),
                    paths,
                    config,
                    checking: None,
                    launcher_releases: github::cached_in(&paths_launcher_cache),
                    self_update_failed: None,
                    online: None,
                    checked_at: None,
                    problem: None,
                    job: None,
                    not_now: false,
                    confirm: None,
                    notice: None,
                    tab: Tab::from_env(),
                    args,
                    from_steam: steam.is_some(),
                    mark: theme::mark(ctx),
                    games: Games::Stale,
                    asking: None,
                    ffmpeg_dialog: shot::ffmpeg_open(),
                    show_details: false,
                    folder_problems: HashMap::new(),
                    folder_check: None,
                    show_all_backups: false,
                    open_menu: shot::menu_open(),
                    shot: Shot::from_env(),
                };
                ready.refresh();
                ready.notice = steam.map(Notice::Warn).or(moved.map(Notice::Error));
                ready.confirm = shot::move_to().map(Confirm::MoveKjk);
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
            let launcher =
                github::fetch_from(crate::config::LAUNCHER_REPO, &paths.launcher_cache_file);
            let _ = tx.send((github::fetch(&paths), launcher));
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
            items: false,
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
            items: false,
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

    /// Asks for a new place for kjk's folder, and asks to move it there.
    fn pick_kjk_folder(&mut self) {
        if launch::kjk_running() {
            self.notice = Some(Notice::Error(
                "Close kjk before you move its folder.".into(),
            ));
            return;
        }
        let Some(from) = self.paths.kjk_data.clone() else {
            return;
        };
        let Some(to) = rfd::FileDialog::new()
            .set_title("Choose the new place for the kjk folder")
            .pick_folder()
        else {
            return;
        };
        if to == from {
            return;
        }
        if let Some(why) = folders::refuse_move(&from, &to) {
            self.notice = Some(Notice::Error(why));
        } else if folders::is_empty(&to) {
            self.confirm = Some(Confirm::MoveKjk(to));
        } else if folders::is_kjk_folder(&to) {
            self.confirm = Some(Confirm::UseKjk(to));
        } else {
            self.notice = Some(Notice::Error(
                "Choose an empty folder for the kjk folder.".into(),
            ));
        }
    }

    /// Moves kjk's folder to `to` on a background thread.
    fn start_kjk_move(&mut self, ctx: &egui::Context, to: PathBuf) {
        if launch::kjk_running() {
            self.notice = Some(Notice::Error(
                "Close kjk before you move its folder.".into(),
            ));
            return;
        }
        let (Some(from), Some(default)) =
            (self.paths.kjk_data.clone(), config::default_kjk_folder())
        else {
            return;
        };
        let progress = Arc::new(Progress::default());
        let (tx, rx) = mpsc::channel();
        let worker = progress.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = folders::move_kjk_folder(&from, &to, &default, &worker);
            let _ = tx.send(Outcome::KjkMoved { to, result });
            ctx.request_repaint();
        });
        self.job = Some(Job {
            title: "Moving the kjk folder".into(),
            progress,
            cancellable: false,
            items: true,
            done: rx,
        });
    }

    /// Makes `to`, which already has kjk's files, kjk's folder.
    fn use_kjk_folder(&mut self, to: PathBuf) {
        let Some(default) = config::default_kjk_folder() else {
            return;
        };
        self.notice = Some(match folders::point_to(&default, &to) {
            Ok(()) => Notice::Info(format!("The kjk folder is now {}.", to.display())),
            Err(e) => Notice::Error(e),
        });
        self.reload_paths();
    }

    /// Finds the launcher's folders again after kjk's folder changed, and
    /// saves the settings at the new place.
    fn reload_paths(&mut self) {
        match Paths::discover().and_then(|(mut paths, _)| {
            paths.use_install_dir(&self.config)?;
            Ok(paths)
        }) {
            Ok(paths) => {
                self.paths = paths;
                self.save();
                self.games = Games::Stale;
            }
            Err(e) => self.notice = Some(Notice::Error(e)),
        }
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
        self.poll_folder_check();
        if let Some(rx) = &self.checking {
            if let Ok((releases, launcher)) = rx.try_recv() {
                self.checking = None;
                if !launcher.list.is_empty() {
                    self.launcher_releases = launcher.list;
                }
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
            Outcome::KjkMoved { to, result } => {
                self.notice = Some(match result {
                    Ok(()) => Notice::Info(format!("The kjk folder is now {}.", to.display())),
                    Err(e) => Notice::Error(format!("The kjk folder could not be moved. {e}")),
                });
                self.reload_paths();
            }
            Outcome::SelfUpdateFailed { link } => {
                self.self_update_failed = Some((
                    "The launcher could not update itself. Download the new version from the release page.".to_string(),
                    link,
                ));
            }
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

    /// Whether anything runs on a background thread.
    fn busy(&self) -> bool {
        self.job.is_some()
            || self.checking.is_some()
            || self.asking.is_some()
            || self.folder_check.is_some()
    }

    fn update(&mut self, ui: &mut egui::Ui) {
        let ctx = &ui.ctx().clone();
        self.poll();
        if self.busy() {
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
            .frame(egui::Frame::new().fill(theme::GROUND))
            .exact_size(theme::TOP_BAR)
            .show_separator_line(false)
            .resizable(false)
            .show(ui, |ui| self.top_bar(ui, &top));

        egui::Panel::bottom("status")
            .frame(bar)
            .resizable(false)
            .show(ui, |ui| {
                let size = egui::vec2(ui.available_width(), theme::FOOTER_ROW);
                let layout = egui::Layout::left_to_right(egui::Align::Center);
                ui.allocate_ui_with_layout(size, layout, |ui| {
                    self.status_line(ui);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        self.launcher_line(ui, ctx);
                    });
                });
            });

        egui::CentralPanel::default().frame(page).show(ui, |ui| {
            // The Play page scrolls by itself: its release notes can scroll
            // inside their panel.
            if self.tab == Tab::Play {
                self.play_tab(ui, ctx);
                return;
            }
            let mut page = egui::ScrollArea::vertical().auto_shrink([false, false]);
            if let Some(offset) = shot::scroll() {
                page = page.vertical_scroll_offset(offset);
            }
            page.show(ui, |ui| match self.tab {
                Tab::Versions => self.versions_tab(ui, ctx),
                _ => self.settings_tab(ui, ctx),
            });
        });

        self.confirm_window(ctx);
        frame::edges(ctx);
        let busy = self.busy();
        self.shot.frame(ctx, busy);
    }

    /// The mark's size in the top bar.
    fn mark_size(&self) -> Option<egui::Vec2> {
        self.mark
            .as_ref()
            .map(|mark| egui::vec2(theme::MARK * mark.aspect_ratio(), theme::MARK))
    }

    /// How wide the mark and the word "kjk-engine" are together.
    fn name_width(&self, ui: &egui::Ui) -> f32 {
        let mark = self.mark_size().map_or(0.0, |size| size.x + theme::SPACE_2);
        mark + widgets::width(ui, "kjk-engine", Text::Wordmark)
    }

    /// Paints the mark and the word "kjk-engine" from `left`, centred on
    /// `middle`, and returns where they end.
    fn name(&self, ui: &egui::Ui, left: f32, middle: f32) -> f32 {
        let mut x = left;
        if let (Some(mark), Some(size)) = (&self.mark, self.mark_size()) {
            let rect = egui::Rect::from_min_size(egui::pos2(x, middle - size.y / 2.0), size);
            egui::Image::new(mark).paint_at(ui, rect);
            x = rect.right() + theme::SPACE_2;
        }
        widgets::paint_centred(ui, "kjk-engine", Text::Wordmark, theme::INK, x, middle).right()
    }

    /// The mark, the word "kjk-engine" and the tab names, all centred on
    /// the middle of the bar, which ends in a hairline. The open tab is
    /// `INK` with a 1px `INK` line on that hairline under its name, the
    /// others `INK_FAINT`.
    fn top_bar(&mut self, ui: &mut egui::Ui, bar: &frame::Bar) {
        let whole = ui.max_rect();
        frame::bar_background(ui, whole);
        let middle = whole.center().y;
        let mut x = whole.left() + bar.left;
        if bar.name_at_right {
            let right = whole.right() - bar.right;
            self.name(ui, right - self.name_width(ui), middle);
        } else {
            x = self.name(ui, x, middle) + theme::SPACE_8;
        }
        let mut underline = None;
        for tab in Tab::ALL {
            let open = self.tab == tab;
            let color = if open { theme::INK } else { theme::INK_FAINT };
            let rect = widgets::paint_centred(ui, tab.name(), Text::Label, color, x, middle);
            x = rect.right() + theme::SPACE_6;
            let response = ui
                .interact(rect, ui.scope_id().with(tab.name()), egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand);
            widgets::focus_outline(ui, &response);
            if open {
                underline = Some(rect.x_range());
            }
            if response.clicked() {
                if tab == Tab::Play && self.tab != Tab::Play {
                    self.games = Games::Stale;
                }
                self.tab = tab;
            }
        }
        let buttons = egui::Rect::from_min_max(
            egui::pos2(x, whole.top()),
            egui::pos2(whole.right() - bar.right, whole.bottom()),
        );
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(buttons)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
            frame::window_buttons,
        );
        let painter = ui.painter();
        let line = whole.bottom() - theme::HAIRLINE / 2.0;
        painter.hline(
            whole.x_range(),
            line,
            egui::Stroke::new(theme::HAIRLINE, theme::LINE),
        );
        if let Some(under) = underline {
            painter.hline(under, line, egui::Stroke::new(theme::HAIRLINE, theme::INK));
        }
    }

    /// The footer's right side: whether the launcher is up to date, or the
    /// offer of a newer launcher.
    fn launcher_line(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let version = env!("CARGO_PKG_VERSION");
        if let Some((sentence, link)) = &self.self_update_failed {
            if !link.is_empty() && widgets::text_button(ui, "Release page").clicked() {
                ctx.open_url(egui::OpenUrl::new_tab(link));
            }
            widgets::status(ui, Status::Error, sentence);
            return;
        }
        let offer = crate::selfupdate::offer(
            &self.launcher_releases,
            crate::selfupdate::prereleases_allowed(),
        )
        .cloned();
        let Some(offer) = offer else {
            ui.label(widgets::text(
                format!("Launcher {version} is up to date."),
                Text::Label,
                theme::INK_MUTED,
            ));
            return;
        };
        let update = ui.add_enabled_ui(self.job.is_none(), |ui| {
            widgets::compact_button(ui, "Update launcher")
        });
        if update.inner.clicked() {
            self.start_self_update(ctx, offer.clone());
        }
        let tag = offer.tag_name.trim_start_matches('v');
        ui.label(widgets::text(
            format!("Launcher {tag} is available."),
            Text::Label,
            theme::INK,
        ));
    }

    /// Downloads the new launcher, swaps it in and starts it, then closes
    /// this window. A failure leaves this launcher running.
    fn start_self_update(&mut self, ctx: &egui::Context, release: Release) {
        let progress = Arc::new(Progress::default());
        let (tx, rx) = mpsc::channel();
        let downloads = self.paths.downloads.clone();
        let worker = progress.clone();
        let ctx = ctx.clone();
        let title = format!("Updating the launcher to {}", release.tag_name);
        std::thread::spawn(move || {
            match crate::selfupdate::update(&release, &downloads, &worker) {
                Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Err(e) => {
                    eprintln!("launcher update: {e}");
                    let link = release.html_url.clone();
                    let _ = tx.send(Outcome::SelfUpdateFailed { link });
                }
            }
            ctx.request_repaint();
        });
        self.job = Some(Job {
            title,
            progress,
            cancellable: true,
            items: false,
            done: rx,
        });
    }

    /// The footer's left side: a notice, or when the launcher last checked
    /// for updates.
    fn status_line(&mut self, ui: &mut egui::Ui) {
        match &self.notice {
            Some(Notice::Info(text)) => widgets::status(ui, Status::Ok, text),
            Some(Notice::Warn(text)) => widgets::status(ui, Status::Warn, text),
            Some(Notice::Error(text)) => widgets::status(ui, Status::Error, text),
            None => {
                let words = if self.checking.is_some() {
                    "Checking for updates.".to_string()
                } else if self.problem.is_some() {
                    "The launcher could not check GitHub for updates.".to_string()
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
                let amount = if job.items {
                    format!("{stage}. {done} of {total}.")
                } else {
                    format!(
                        "{stage}. {:.1} of {:.1} MB.",
                        done as f64 / 1e6,
                        total as f64 / 1e6
                    )
                };
                widgets::muted(ui, amount);
            },
            |ui| {
                if job.cancellable {
                    cancel = widgets::compact_button(ui, "Cancel").clicked();
                }
            },
        );
        widgets::progress(ui, fraction);
        if cancel {
            job.progress.cancel.store(true, Ordering::Relaxed);
        }
        ui.add_space(theme::SPACE_6);
    }

    /// The install folder, with a button to change it.
    fn install_folder(&mut self, ui: &mut egui::Ui) {
        let folder = self.paths.versions.display().to_string();
        widgets::row(
            ui,
            |ui| {
                ui.label(widgets::text("Install folder", Text::Heading, theme::INK));
                ui.label(widgets::text(folder, Text::Mono, theme::INK_MUTED));
            },
            |ui| {
                let change = ui.add_enabled_ui(self.job.is_none(), |ui| {
                    widgets::compact_button(ui, "Change")
                });
                if change.inner.clicked() {
                    self.pick_install_dir();
                }
            },
        );
    }
}
