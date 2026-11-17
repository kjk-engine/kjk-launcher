//! Starting kjk (and its save editor), and telling whether it runs.

use crate::backup;
use crate::config::{Config, Paths};
use crate::install::{self, Installed};
use std::path::{Path, PathBuf};
use std::process::Command;

/// What Play did besides starting kjk.
pub struct Started {
    pub tag: String,
    /// The backup taken because this version had never started before.
    pub backup: Option<PathBuf>,
}

/// Starts the current version with `args`. Before a version's first start
/// the saves are backed up; if that fails, kjk is not started.
pub fn play(paths: &Paths, config: &mut Config, args: &[String]) -> Result<Started, String> {
    let version = current(paths, config)?;
    let tag = version.tag.clone();
    let mut taken = None;
    if !config.launched.contains(&tag) {
        if config.backup_before_new_version {
            taken = backup::create(paths, &format!("before-{tag}"))?;
            backup::prune(paths, config.keep_backups);
        }
        config.launched.push(tag.clone());
        config.save(paths)?;
    }
    start(&version.dir, &install::game_exe(&version.dir), args)?;
    Ok(Started { tag, backup: taken })
}

pub fn save_editor(paths: &Paths, config: &Config) -> Result<(), String> {
    let version = current(paths, config)?;
    let editor = install::editor_exe(&version.dir).ok_or("this version has no save editor")?;
    start(&version.dir, &editor, &[])
}

fn current(paths: &Paths, config: &Config) -> Result<Installed, String> {
    let tag = config.current.as_deref().ok_or("no version of kjk is installed yet")?;
    install::find(paths, tag).ok_or_else(|| format!("{tag} is not installed any more"))
}

fn start(dir: &Path, program: &Path, args: &[String]) -> Result<(), String> {
    let mut command = if cfg!(target_os = "macos") {
        let mut open = Command::new("open");
        open.arg("-n").arg(program).arg("--args").args(args);
        open
    } else {
        let mut direct = Command::new(program);
        direct.args(args);
        direct
    };
    command
        .current_dir(dir)
        .spawn()
        .map(drop)
        .map_err(|e| format!("could not start {}: {e}", program.display()))
}

/// Whether kjk or its save editor is running (from any folder).
pub fn kjk_running() -> bool {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let Ok(mut system) = System::new() else { return false };
    system.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing());
    system.processes().values().any(|process| {
        let name = process.name().to_string_lossy().to_ascii_lowercase();
        let name = name.strip_suffix(".exe").unwrap_or(&name);
        name == "kjk" || name == "kjk-save-editor"
    })
}

/// Opens a folder in the system's file manager.
pub fn open_folder(path: &Path) {
    let _ = std::fs::create_dir_all(path);
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let _ = Command::new(program).arg(path).spawn();
}
