//! The launcher's window: the frame, the shared state and the footer. Each
//! page is in its own file.

mod dialogs;
mod frame;
mod lighting;
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
use crate::size::size;
use crate::theme::{self, Text};
use crate::versions as release_menu;
use crate::widgets::{self, Status};
use eframe::egui;
use std::cmp::Ordering as CmpOrdering;
use std::collections::{HashMap, HashSet};
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
    /// kjk's folder moved from `from` to `to`, or why it did not.
    KjkMoved {
        from: PathBuf,
        to: PathBuf,
        result: Result<(), String>,
    },
    /// The launcher's update failed (on success the new launcher runs and
    /// this one has closed).
    SelfUpdateFailed {
        link: String,
    },
    /// A download of the new lighting for a game ended.
    Lighting {
        game: Game,
        result: Result<crate::lighting::Outcome, String>,
    },
    /// The new lighting of a game was removed, or why it was not.
    LightingRemoved {
        game: Game,
        result: Result<bool, String>,
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
    /// Uninstall every version but the selected one.
    DeleteOlder,
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

/// What a folder is being chosen for.
#[derive(Clone, Copy, PartialEq)]
enum FolderFor {
    Game(Game),
    InstallDir,
    KjkFolder,
}

impl FolderFor {
    /// The title of the folder dialog and of the dialog for a typed path.
    fn title(self) -> String {
        match self {
            FolderFor::Game(game) => format!("Choose the {} folder", game.name()),
            FolderFor::InstallDir => "Choose the folder for kjk's versions".into(),
            FolderFor::KjkFolder => "Choose the new place for the kjk folder".into(),
        }
    }
}

/// The open dialog for a typed folder path.
struct TypedPath {
    folder_for: FolderFor,
    text: String,
    /// Why the typed path cannot be used.
    problem: Option<String>,
}

/// A system folder dialog that closes faster than this opened nothing: no
/// dialog program is there, as in the Steam Deck's Gaming Mode. Nobody
/// presses Cancel this fast.
const NO_DIALOG: Duration = Duration::from_millis(300);

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
    mark: Option<theme::Mark>,
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
    /// The dialog for a typed folder path, while it is open.
    typed: Option<TypedPath>,
    /// Why the answer to the open question could not be carried out, shown
    /// in the dialog so the player can try again.
    confirm_problem: Option<String>,
    /// Puts the focus on Play (or on the install button) until the player
    /// first presses a key or clicks, so Enter alone starts the game.
    focus_start: bool,
    shot: Shot,
    /// What the selected version says about the new lighting.
    lighting: lighting::Lighting,
    /// The answer about the new lighting, asked on a background thread.
    lighting_asking: Option<mpsc::Receiver<crate::lighting::Status>>,
    /// What the last download of the new lighting found, by game.
    lighting_done: HashMap<&'static str, crate::lighting::Outcome>,
    /// The games whose list of areas with the game's own lighting is open.
    lighting_open: HashSet<&'static str>,
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
                    mark: theme::Mark::new(),
                    games: Games::Stale,
                    asking: None,
                    ffmpeg_dialog: shot::ffmpeg_open(),
                    show_details: false,
                    folder_problems: HashMap::new(),
                    folder_check: None,
                    show_all_backups: false,
                    open_menu: shot::menu_open(),
                    typed: None,
                    confirm_problem: None,
                    focus_start: true,
                    shot: Shot::from_env(),
                    lighting: lighting::Lighting::Stale,
                    lighting_asking: None,
                    lighting_done: HashMap::new(),
                    lighting_open: HashSet::new(),
                };
                ready.typed = shot::typed_open().map(|text| TypedPath {
                    folder_for: FolderFor::KjkFolder,
                    text,
                    problem: None,
                });
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
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        if let App::Ready(ready) = self {
            ready.shot.input(raw);
        }
    }

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

impl Ready {
    /// Drops an error notice once an action goes ahead, so an old error
    /// does not stay on screen after the player tried again.
    fn clear_error(&mut self) {
        if matches!(self.notice, Some(Notice::Error(_))) {
            self.notice = None;
        }
    }

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
        self.clear_error();
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
        let name = release_menu::title(&release.tag_name);
        if self.installed.iter().any(|i| i.tag == release.tag_name) {
            let mut text = format!("{name} is already installed.");
            if make_current {
                self.choose(release.tag_name.clone());
                text.push_str(" Play now starts it.");
            }
            self.notice = Some(Notice::Info(text));
            return;
        }
        self.clear_error();
        let progress = Arc::new(Progress::default());
        let (tx, rx) = mpsc::channel();
        let paths = self.paths.clone();
        let worker = progress.clone();
        let ctx = ctx.clone();
        let title = format!("Installing {name}");
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
        self.clear_error();
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

    /// Asks for a folder in the system's folder dialog. When that dialog
    /// cannot open, or `KJK_LAUNCHER_TYPE_PATHS=1` is set, it opens the
    /// launcher's dialog for a typed path instead, and returns `None`; the
    /// typed folder comes later through `folder_chosen`.
    fn ask_folder(&mut self, folder_for: FolderFor) -> Option<PathBuf> {
        if !folders::type_paths() {
            let start = match folder_for {
                FolderFor::InstallDir => Some(self.paths.versions.clone()),
                FolderFor::KjkFolder => self
                    .paths
                    .kjk_data
                    .as_deref()
                    .and_then(std::path::Path::parent)
                    .map(std::path::Path::to_path_buf),
                FolderFor::Game(_) => None,
            };
            let mut dialog = rfd::FileDialog::new().set_title(folder_for.title());
            if let Some(start) = start {
                dialog = dialog.set_directory(start);
            }
            let opened = std::time::Instant::now();
            let picked = dialog.pick_folder();
            if picked.is_some() || opened.elapsed() >= NO_DIALOG {
                return picked;
            }
        }
        self.type_folder(folder_for);
        None
    }

    /// Opens the launcher's dialog for a typed folder path.
    fn type_folder(&mut self, folder_for: FolderFor) {
        self.typed = Some(TypedPath {
            folder_for,
            text: String::new(),
            problem: None,
        });
    }

    /// Goes on with a folder the player picked or typed, with the same
    /// checks for both.
    fn folder_chosen(&mut self, ctx: &egui::Context, folder_for: FolderFor, folder: PathBuf) {
        match folder_for {
            FolderFor::Game(game) => self.game_folder_chosen(ctx, game, folder),
            FolderFor::InstallDir => self.install_dir_chosen(folder),
            FolderFor::KjkFolder => self.kjk_folder_chosen(folder),
        }
    }

    /// Asks for a new place for kjk's folder, and asks to move it there.
    fn pick_kjk_folder(&mut self) {
        if let Some(to) = self.ask_folder(FolderFor::KjkFolder) {
            self.kjk_folder_chosen(to);
        }
    }

    /// Asks to move kjk's folder to `to`, or to use `to` when it already
    /// has kjk's files.
    fn kjk_folder_chosen(&mut self, to: PathBuf) {
        let Some(from) = self.paths.kjk_data.clone() else {
            return;
        };
        if to == from {
            return;
        }
        if let Some(why) = folders::refuse_move(&from, &to) {
            self.notice = Some(Notice::Error(why));
        } else if folders::is_empty(&to) {
            self.clear_error();
            self.confirm = Some(Confirm::MoveKjk(to));
        } else if folders::is_kjk_folder(&to) {
            self.clear_error();
            self.confirm = Some(Confirm::UseKjk(to));
        } else {
            self.notice = Some(Notice::Error(
                "Choose an empty folder for the kjk folder.".into(),
            ));
        }
    }

    /// Moves kjk's folder to `to` on a background thread. While kjk runs it
    /// does not start, and says why.
    fn start_kjk_move(&mut self, ctx: &egui::Context, to: PathBuf) -> Result<(), String> {
        if launch::kjk_running(&self.paths.versions) {
            return Err("kjk is still running. Close it, then press Move again.".into());
        }
        let (Some(from), Some(default)) =
            (self.paths.kjk_data.clone(), config::default_kjk_folder())
        else {
            return Ok(());
        };
        self.clear_error();
        let progress = Arc::new(Progress::default());
        let (tx, rx) = mpsc::channel();
        let worker = progress.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = folders::move_kjk_folder(&from, &to, &default, &worker);
            let _ = tx.send(Outcome::KjkMoved { from, to, result });
            ctx.request_repaint();
        });
        self.job = Some(Job {
            title: "Moving the kjk folder".into(),
            progress,
            cancellable: false,
            items: true,
            done: rx,
        });
        Ok(())
    }

    /// Makes `to`, which already has kjk's files, kjk's folder. The
    /// launcher's settings in that folder are kept and used.
    fn use_kjk_folder(&mut self, to: PathBuf) {
        let Some(default) = config::default_kjk_folder() else {
            return;
        };
        match folders::point_to(&default, &to) {
            Ok(()) => {
                self.notice = Some(Notice::Info(format!(
                    "The kjk folder is now {}.",
                    to.display()
                )));
                self.reload_paths(true);
            }
            Err(e) => self.notice = Some(Notice::Error(e)),
        }
    }

    /// Finds the launcher's folders again after kjk's folder changed. With
    /// `adopt`, the new folder's own settings are loaded when it has them;
    /// otherwise the present settings are saved at the new place.
    fn reload_paths(&mut self, adopt: bool) {
        let found = Paths::discover().and_then(|(mut paths, _)| {
            let (config, save) = if adopt {
                self.config.for_folder(&paths)
            } else {
                (self.config.clone(), true)
            };
            paths.use_install_dir(&config)?;
            Ok((paths, config, save))
        });
        match found {
            Ok((paths, config, save)) => {
                self.paths = paths;
                self.config = config;
                if save {
                    self.save();
                }
                self.games = Games::Stale;
            }
            Err(e) => self.notice = Some(Notice::Error(e)),
        }
    }

    fn pick_install_dir(&mut self) {
        if let Some(dir) = self.ask_folder(FolderFor::InstallDir) {
            self.install_dir_chosen(dir);
        }
    }

    /// Uses `dir` as the install folder, asking first whether the installed
    /// versions move there.
    fn install_dir_chosen(&mut self, dir: PathBuf) {
        if dir == self.paths.versions {
            return;
        }
        if let Some(inside) = self.installed.iter().find(|v| dir.starts_with(&v.dir)) {
            self.notice = Some(Notice::Error(format!(
                "That folder is inside the install of {}. Choose the folder above the versions.",
                release_menu::title(&inside.tag)
            )));
            return;
        }
        self.clear_error();
        if self.installed.is_empty() {
            self.set_install_dir(dir);
        } else {
            self.confirm = Some(Confirm::ChangeFolder(dir));
        }
    }

    fn poll(&mut self) {
        self.poll_folder_check();
        self.poll_lighting();
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
            outcome @ (Outcome::Lighting { .. } | Outcome::LightingRemoved { .. }) => {
                self.lighting_finished(outcome);
            }
            Outcome::KjkMoved { from, to, result } => {
                // An install folder inside kjk's folder moved with it.
                if result.is_ok() {
                    if let Some(followed) = self
                        .config
                        .install_dir
                        .as_deref()
                        .and_then(|dir| folders::follow_move(dir, &from, &to))
                    {
                        self.config.install_dir = Some(followed);
                    }
                }
                self.notice = Some(match result {
                    Ok(()) => Notice::Info(format!("The kjk folder is now {}.", to.display())),
                    Err(e) => Notice::Error(format!("The kjk folder could not be moved. {e}")),
                });
                self.reload_paths(false);
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
                let mut text = format!("Installed {}.", release_menu::title(&installed.tag));
                if make_current {
                    self.config.current = Some(installed.tag.clone());
                    self.not_now = false;
                    self.save();
                    text.push_str(" Play now starts it.");
                }
                self.notice = Some(Notice::Info(text));
            }
            Outcome::Installed { result: Err(e), .. } => {
                self.notice = Some(Notice::Error(format!("Installing failed. {e}")));
            }
            Outcome::Moved { to, moved } => {
                let versions = if moved.moved == 1 {
                    "version"
                } else {
                    "versions"
                };
                let mut text = format!("Moved {} {versions} to {}.", moved.moved, to.display());
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
            || self.lighting_asking.is_some()
    }

    fn update(&mut self, ui: &mut egui::Ui) {
        let ctx = &ui.ctx().clone();
        self.poll();
        if let Some(mark) = &mut self.mark {
            mark.refresh(ctx);
        }
        let pressed = ctx.input(|i| {
            i.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Key { pressed: true, .. }
                        | egui::Event::PointerButton { pressed: true, .. }
                )
            })
        });
        // An open dialog takes the focus instead.
        let dialog = self.confirm.is_some() || self.typed.is_some() || self.ffmpeg_dialog;
        if pressed || dialog {
            self.focus_start = false;
        }
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

        // A notice has its own line above the footer, and wraps there.
        if self.notice.is_some() {
            let notice = egui::Frame::new()
                .fill(theme::GROUND)
                .inner_margin(egui::Margin::symmetric(theme::SPACE_8 as i8, 0));
            egui::Panel::bottom("notice")
                .frame(notice)
                .resizable(false)
                .show_separator_line(false)
                .show(ui, |ui| self.notice_line(ui));
        }

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
        self.typed_window(ctx);
        frame::edges(ctx);
        let busy = self.busy();
        self.shot.frame(ctx, busy);
    }

    /// How wide the mark and the word "kjk-engine" are together.
    fn name_width(&self, ui: &egui::Ui) -> f32 {
        let mark = self
            .mark
            .as_ref()
            .map_or(0.0, |mark| mark.size().x + theme::SPACE_2);
        mark + widgets::width(ui, "kjk-engine", Text::Wordmark)
    }

    /// Paints the mark and the word "kjk-engine" from `left`, centred on
    /// `middle`, and returns where they end.
    fn name(&self, ui: &egui::Ui, left: f32, middle: f32) -> f32 {
        let mut x = left;
        if let Some(mark) = &self.mark {
            x = mark.paint(ui, x, middle) + theme::SPACE_2;
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
        let title = format!(
            "Updating the launcher to {}",
            release_menu::number(&release.tag_name)
        );
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

    /// The footer's left side: when the launcher last checked for updates.
    fn status_line(&mut self, ui: &mut egui::Ui) {
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

    /// The notice on its own line above the footer, with Dismiss.
    fn notice_line(&mut self, ui: &mut egui::Ui) {
        let (status, words) = match &self.notice {
            Some(Notice::Info(text)) => (Status::Ok, text.clone()),
            Some(Notice::Warn(text)) => (Status::Warn, text.clone()),
            Some(Notice::Error(text)) => (Status::Error, text.clone()),
            None => return,
        };
        ui.add_space(theme::SPACE_3);
        if widgets::notice(ui, status, &words) {
            self.notice = None;
        }
        ui.add_space(theme::SPACE_3);
    }

    /// The download or move in progress: what it is doing, a progress
    /// line and, for a download, a Cancel button.
    fn progress(&mut self, ui: &mut egui::Ui) {
        let Some(job) = &self.job else { return };
        let (stage, done, total) = job.progress.snapshot();
        let items = job.items || job.progress.items.load(Ordering::Relaxed);
        let cancellable = job.cancellable && !job.progress.locked.load(Ordering::Relaxed);
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
                let amount = if total == 0 {
                    format!("{stage}.")
                } else if items {
                    format!("{stage}. {done} of {total}.")
                } else {
                    let percent = (done * 100 / total).min(100);
                    format!("{stage}. {} of {} ({percent}%).", size(done), size(total))
                };
                widgets::muted(ui, amount);
            },
            |ui| {
                if cancellable {
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
        let idle = self.job.is_none();
        let (mut pick, mut typed) = (false, false);
        widgets::row(
            ui,
            |ui| {
                ui.label(widgets::text("Install folder", Text::Heading, theme::INK));
                widgets::wrapped_mono(ui, &folder, theme::INK_MUTED);
            },
            |ui| {
                (pick, typed) = folder_buttons(ui, "Change", idle);
            },
        );
        if pick {
            self.pick_install_dir();
        }
        if typed {
            self.type_folder(FolderFor::InstallDir);
        }
    }
}

/// A button that opens the system's folder dialog and, on Linux, a "Type a
/// path" text button beside it, for a system without a folder dialog that
/// the launcher cannot detect. Returns which of the two was pressed. Call it
/// in a right-to-left layout.
fn folder_buttons(ui: &mut egui::Ui, label: &str, enabled: bool) -> (bool, bool) {
    ui.add_enabled_ui(enabled, |ui| {
        let pick = widgets::compact_button(ui, label).clicked();
        let typed = cfg!(target_os = "linux") && widgets::text_button(ui, "Type a path").clicked();
        (pick, typed)
    })
    .inner
}
