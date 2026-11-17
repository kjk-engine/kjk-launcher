//! The launcher's window: Play, the update offer, versions, backups, settings.

use crate::backup::{self, Backup};
use crate::config::{Config, Paths};
use crate::github::{self, Release, Releases};
use crate::install::{self, Installed, Moved, Progress};
use crate::launch;
use eframe::egui::{self, Color32, RichText};
use std::cmp::Ordering as CmpOrdering;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc};
use std::time::Duration;

const GREEN: Color32 = Color32::from_rgb(90, 170, 90);
const RED: Color32 = Color32::from_rgb(220, 90, 80);
const AMBER: Color32 = Color32::from_rgb(220, 160, 60);
const BLUE: Color32 = Color32::from_rgb(120, 160, 220);

pub fn run(args: Vec<String>) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("kjk launcher")
            .with_inner_size([900.0, 760.0])
            .with_min_inner_size([560.0, 440.0]),
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
    problem: Option<String>,
    job: Option<Job>,
    not_now: bool,
    confirm: Option<Confirm>,
    notice: Option<Notice>,
    tab: Tab,
    /// Passed to kjk when Play starts it.
    args: Vec<String>,
}

enum App {
    Ready(Box<Ready>),
    Broken(String),
}

impl App {
    fn new(ctx: &egui::Context, args: Vec<String>) -> App {
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
                    problem: None,
                    job: None,
                    not_now: false,
                    confirm: None,
                    notice: None,
                    tab: Tab::Play,
                    args,
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
                self.problem = releases.problem;
                if !releases.list.is_empty() {
                    self.releases = releases.list;
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
        if self.job.is_some() || self.checking.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        egui::Panel::top("tabs").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading("kjk launcher");
                ui.separator();
                ui.selectable_value(&mut self.tab, Tab::Play, "Play");
                ui.selectable_value(&mut self.tab, Tab::Versions, "Versions");
                ui.selectable_value(&mut self.tab, Tab::Backups, "Backups");
                ui.selectable_value(&mut self.tab, Tab::Settings, "Settings");
            });
            ui.add_space(4.0);
        });

        egui::Panel::bottom("status").show(ui, |ui| {
            ui.add_space(2.0);
            ui.horizontal_wrapped(|ui| self.status_line(ui));
            ui.add_space(2.0);
        });

        egui::CentralPanel::default().show(ui, |ui| {
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
    }

    fn status_line(&mut self, ui: &mut egui::Ui) {
        match &self.notice {
            Some(Notice::Info(text)) => {
                ui.label(RichText::new(text).color(GREEN));
            }
            Some(Notice::Error(text)) => {
                ui.label(RichText::new(text).color(RED));
            }
            None => {
                if self.checking.is_some() {
                    ui.spinner();
                    ui.label("Checking GitHub for updates…");
                } else if let Some(problem) = &self.problem {
                    ui.label(format!("Could not check for updates: {problem}."));
                } else if self.online == Some(true) {
                    ui.label("Checked GitHub for updates.");
                } else {
                    ui.label("Not checked for updates yet.");
                }
            }
        }
        if self.notice.is_some() && ui.small_button("✕").clicked() {
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
            ui.label(RichText::new(self.paths.versions.display().to_string()).monospace());
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
        ui.add_space(8.0);
        self.progress(ui);

        let Some(current) = self.current().cloned() else {
            ui.heading("kjk is not installed yet");
            ui.label("kjk needs your own installs of KOTOR, Jedi Academy and KOTOR II; it finds them the first time it starts.");
            ui.add_space(8.0);
            self.install_folder(ui);
            ui.add_space(8.0);
            let newest = github::newest(&github::installable(
                &self.releases,
                self.config.prereleases,
            ))
            .cloned();
            match newest {
                Some(release) => {
                    let button = egui::Button::new(
                        RichText::new(format!("Install {}", release.tag_name)).size(18.0),
                    );
                    if ui.add_enabled(self.job.is_none(), button).clicked() {
                        self.start_install(ctx, release, true);
                    }
                    if ui.link("Pick another version…").clicked() {
                        self.tab = Tab::Versions;
                    }
                }
                None if self.checking.is_some() => {
                    ui.label("Looking for releases…");
                }
                None => {
                    ui.label("No release was found for this computer.");
                }
            }
            return;
        };

        if let Some(offer) = self.offer().filter(|_| !self.not_now) {
            self.offer_box(ui, ctx, &current, offer);
            ui.add_space(12.0);
        }

        ui.horizontal(|ui| {
            self.version_picker(ui, &current);
            if current.prerelease {
                ui.label(RichText::new("pre-release").color(AMBER));
            }
            if self.config.pinned {
                ui.label(RichText::new("📌 pinned").color(BLUE));
            }
        });
        let released = if current.date().is_empty() {
            "Installed".to_string()
        } else {
            format!("Released {}", current.date())
        };
        ui.label(RichText::new(released).weak())
            .on_hover_text(current.dir.display().to_string());
        ui.add_space(10.0);

        ui.horizontal(|ui| {
            let play = egui::Button::new(RichText::new("▶  Play").size(24.0))
                .min_size(egui::vec2(180.0, 52.0));
            if ui.add(play).clicked() {
                match launch::play(&self.paths, &mut self.config, &self.args) {
                    Ok(_) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                    Err(e) => self.notice = Some(Notice::Error(e)),
                }
            }
            if install::editor_exe(&current.dir).is_some() && ui.button("Save editor").clicked() {
                if let Err(e) = launch::save_editor(&self.paths, &self.config) {
                    self.notice = Some(Notice::Error(e));
                }
            }
        });
        if !self.config.launched.contains(&current.tag) && self.config.backup_before_new_version {
            ui.label(
                RichText::new(
                    "This version has not started before; your saves are backed up first.",
                )
                .small()
                .weak(),
            );
        }
        ui.add_space(16.0);
        self.install_folder(ui);
    }

    /// The drop-down of installed versions, each with an Uninstall button.
    fn version_picker(&mut self, ui: &mut egui::Ui, current: &Installed) {
        let mut chosen = None;
        let mut uninstall = None;
        let mut more = false;
        egui::ComboBox::from_id_salt("version")
            .selected_text(
                RichText::new(format!("kjk {}", current.tag))
                    .size(18.0)
                    .strong(),
            )
            .width(260.0)
            .show_ui(ui, |ui| {
                ui.set_min_width(360.0);
                for version in &self.installed {
                    ui.horizontal(|ui| {
                        let selected = version.tag == current.tag;
                        let label = format!("{}   {}", version.tag, version.date());
                        if ui.selectable_label(selected, label).clicked() && !selected {
                            chosen = Some(version.tag.clone());
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Uninstall").clicked() {
                                uninstall = Some(version.tag.clone());
                            }
                            let size = self.sizes.get(&version.dir).copied().unwrap_or(0);
                            ui.label(RichText::new(megabytes(size)).weak());
                        });
                    });
                }
                ui.separator();
                if ui.button("Install another version…").clicked() {
                    more = true;
                }
            });
        if let Some(tag) = chosen {
            self.choose(tag);
        }
        if let Some(tag) = uninstall {
            self.confirm = Some(Confirm::Uninstall(tag));
        }
        if more {
            self.tab = Tab::Versions;
        }
    }

    fn offer_box(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        current: &Installed,
        offer: Release,
    ) {
        let on_disk = self.installed.iter().any(|i| i.tag == offer.tag_name);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if on_disk {
                ui.label(
                    RichText::new(format!("kjk {} is already installed (you play {})", offer.tag_name, current.tag))
                        .size(17.0)
                        .strong(),
                );
                ui.label("Switch to it here or in the version list below; nothing needs to be downloaded.");
            } else {
                ui.label(
                    RichText::new(format!("kjk {} is available (you have {})", offer.tag_name, current.tag))
                        .size(17.0)
                        .strong(),
                );
                ui.label(format!(
                    "Released {}. Nothing changes unless you update; {} stays installed so you can go back.",
                    offer.date(),
                    current.tag
                ));
            }
            egui::CollapsingHeader::new("What's new")
                .id_salt(("notes", &offer.tag_name))
                .default_open(!on_disk)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("offer-notes")
                        .max_height(150.0)
                        .show(ui, |ui| ui.label(offer.notes()));
                });
            ui.horizontal(|ui| {
                if on_disk {
                    let switch = egui::Button::new(RichText::new(format!("Switch to {}", offer.tag_name)).strong());
                    if ui.add(switch).clicked() {
                        self.choose(offer.tag_name.clone());
                    }
                } else {
                    let busy = self.job.is_some();
                    if ui.add_enabled(!busy, egui::Button::new(RichText::new("Update").strong())).clicked() {
                        self.start_install(ctx, offer.clone(), true);
                    }
                }
                if ui.button("Skip this version").clicked() {
                    self.config.skipped = Some(offer.tag_name.clone());
                    self.save();
                }
                if ui.button("Not now").clicked() {
                    self.not_now = true;
                }
                if !offer.html_url.is_empty() {
                    ui.hyperlink_to("On GitHub", &offer.html_url);
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
                        ui.label(RichText::new("plays now").color(GREEN));
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
                    ui.label(RichText::new("pre-release").color(AMBER));
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
