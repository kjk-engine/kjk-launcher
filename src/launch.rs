//! Starting kjk (and its save editor), and telling whether it runs.

use crate::backup;
use crate::config::{Config, Paths};
use crate::install::{self, Installed};
use crate::kjk_report;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};

/// What Play did besides starting kjk.
pub struct Started {
    pub tag: String,
    /// The backup taken because this version had never started before.
    pub backup: Option<PathBuf>,
    /// The running game. On macOS it is `open -W`, which lives as long as
    /// kjk does.
    pub child: Child,
}

/// `kjk-launcher --play`: starts the selected version and waits until kjk
/// exits. `Err` carries the sentence the window opens with when the player
/// is needed first.
pub fn from_steam() -> Result<(), String> {
    let (paths, mut config) = crate::config::open()?;
    let releases = crate::github::fetch(&paths);
    let current = config
        .current
        .as_deref()
        .and_then(|tag| install::find(&paths, tag));
    let offered = current.as_ref().is_some_and(|version| {
        crate::github::offer(&releases.list, &config, Some(version.key())).is_some()
    });
    match play_now(&paths, &mut config, offered) {
        PlayNow::Started(mut child) => {
            let _ = child.wait();
            Ok(())
        }
        PlayNow::NeedsWindow(sentence) => Err(sentence),
    }
}

/// What `--play` does.
pub enum PlayNow {
    Started(Child),
    /// The window opens with this sentence.
    NeedsWindow(String),
}

/// For --play: starts the selected version when nothing needs the player.
/// NeedsWindow carries the sentence to show: no version installed, the
/// selected version's folder is gone, an update is offered, a game folder
/// is missing, or the backup failed.
pub fn play_now(paths: &Paths, config: &mut Config, offered: bool) -> PlayNow {
    let Some(tag) = config.current.clone() else {
        return PlayNow::NeedsWindow(
            "No version of kjk is installed yet. Install one, then press Play.".into(),
        );
    };
    let Some(version) = install::find(paths, &tag) else {
        return PlayNow::NeedsWindow(format!(
            "The folder of kjk {tag} is gone. Choose another version, then press Play."
        ));
    };
    if offered {
        return PlayNow::NeedsWindow(
            "A new version of kjk is available. Update or skip it, then press Play.".into(),
        );
    }
    let report = kjk_report::ask(&install::game_binary(&version.dir));
    if let Some(game) = kjk_report::blocked_by(report.as_ref()) {
        return PlayNow::NeedsWindow(format!("Choose the {} folder to play.", game.name()));
    }
    match play(paths, config, &[]) {
        Ok(started) => PlayNow::Started(started.child),
        Err(e) => PlayNow::NeedsWindow(e),
    }
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
    let child = start(&version.dir, &install::game_exe(&version.dir), args)?;
    Ok(Started {
        tag,
        backup: taken,
        child,
    })
}

pub fn save_editor(paths: &Paths, config: &Config) -> Result<(), String> {
    let version = current(paths, config)?;
    let editor = install::editor_exe(&version.dir).ok_or("this version has no save editor")?;
    start(&version.dir, &editor, &[]).map(drop)
}

fn current(paths: &Paths, config: &Config) -> Result<Installed, String> {
    let tag = config
        .current
        .as_deref()
        .ok_or("no version of kjk is installed yet")?;
    install::find(paths, tag).ok_or_else(|| format!("{tag} is not installed any more"))
}

/// Starts kjk or its save editor, with the variables of
/// `config::kjk_env` while kjk's folder is moved.
fn start(dir: &Path, program: &Path, args: &[String]) -> Result<Child, String> {
    let env = crate::config::kjk_env();
    let mut command = if cfg!(target_os = "macos") {
        // -W waits until the app quits, so the child lives as long as kjk.
        // An app started by `open` gets its variables only through --env.
        let mut open = Command::new("open");
        open.arg("-W").arg("-n");
        for (name, value) in &env {
            open.arg("--env").arg(format!("{name}={}", value.display()));
        }
        open.arg(program).arg("--args").args(args);
        open
    } else {
        let mut direct = Command::new(program);
        direct
            .args(args)
            .envs(env.iter().map(|(name, value)| (name, value)));
        direct
    };
    command
        .current_dir(dir)
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", program.display()))
}

/// Whether kjk or its save editor is running (from any folder).
pub fn kjk_running() -> bool {
    use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};
    let Ok(mut system) = System::new() else {
        return false;
    };
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Launcher folders in a fresh temporary folder.
    fn scratch(name: &str) -> Paths {
        let root =
            std::env::temp_dir().join(format!("kjk-launcher-play-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("kjk-engine")).unwrap();
        Paths::at(root)
    }

    #[test]
    fn play_now_needs_the_window_without_a_version() {
        let paths = scratch("none");
        let mut config = Config::default();
        assert!(matches!(
            play_now(&paths, &mut config, false),
            PlayNow::NeedsWindow(_)
        ));
    }

    #[test]
    fn play_now_needs_the_window_when_the_folder_is_gone() {
        let paths = scratch("gone");
        let mut config = Config {
            current: Some("v0.19.0".into()),
            ..Config::default()
        };
        assert!(matches!(
            play_now(&paths, &mut config, false),
            PlayNow::NeedsWindow(_)
        ));
    }
}
