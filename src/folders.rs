//! Moving files between folders: the move of the launcher's files from the
//! 0.1.0 layout into kjk's folder, and the move of kjk's whole folder to a
//! place the player picks.

use crate::config::{Config, MOVED_TO};
use crate::install::Progress;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

/// Moves `from` to `to`: a rename, or a copy and then a delete when the
/// two are on different drives. A file or folder that already exists at
/// `to` is never overwritten: `Ok(false)` then, and `from` stays.
pub fn move_entry(from: &Path, to: &Path) -> Result<bool, String> {
    if to.exists() {
        return Ok(false);
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    if fs::rename(from, to).is_ok() {
        return Ok(true);
    }
    let copied = copy_all(from, to);
    if let Err(e) = copied {
        let _ = remove_all(to);
        return Err(e);
    }
    remove_all(from)?;
    Ok(true)
}

fn copy_all(from: &Path, to: &Path) -> Result<(), String> {
    let error = |path: &Path, e: std::io::Error| format!("{}: {e}", path.display());
    if from.is_dir() {
        fs::create_dir_all(to).map_err(|e| error(to, e))?;
        for entry in fs::read_dir(from).map_err(|e| error(from, e))? {
            let entry = entry.map_err(|e| error(from, e))?;
            copy_all(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(from, to).map(drop).map_err(|e| error(from, e))
    }
}

fn remove_all(path: &Path) -> Result<(), String> {
    let removed = if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    removed.map_err(|e| format!("{}: {e}", path.display()))
}

/// Moves every entry of the folder `from` into the folder `to`, skipping
/// names that already exist there. Stops at the first failure.
fn move_contents(from: &Path, to: &Path) -> Result<(), String> {
    let Ok(entries) = fs::read_dir(from) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        move_entry(&entry.path(), &to.join(entry.file_name()))?;
    }
    let _ = fs::remove_dir(from);
    Ok(())
}

/// Whether the launcher's files are still in the 0.1.0 layout at
/// `old_root` and not yet in kjk's folder `new_root`.
pub fn needs_migration(old_root: &Path, new_root: &Path) -> bool {
    old_root.join("launcher.json").is_file()
        && !new_root.join("launcher").join("launcher.json").exists()
}

/// Moves the launcher's files from the 0.1.0 layout at `old_root` into
/// kjk's folder `new_root`: the release lists into `launcher/`, the
/// backups into `backups/`, the default install folder `kjk-engine` into
/// `versions/` (only when the settings name no other install folder), and
/// `launcher.json` last. A file that already exists at the new place is
/// never overwritten. On a failure the rest stays at the old place, and
/// since `launcher.json` moves last, the next start tries again.
pub fn migrate(old_root: &Path, new_root: &Path) -> Result<(), String> {
    if !needs_migration(old_root, new_root) {
        return Ok(());
    }
    let launcher = new_root.join("launcher");
    let old_config: Config = fs::read_to_string(old_root.join("launcher.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    for name in ["releases-cache.json", "launcher-releases-cache.json"] {
        let from = old_root.join(name);
        if from.exists() {
            move_entry(&from, &launcher.join(name))?;
        }
    }
    move_contents(&old_root.join("backups"), &new_root.join("backups"))?;
    if old_config.install_dir.is_none() {
        move_contents(&old_root.join("kjk-engine"), &new_root.join("versions"))?;
    }
    move_entry(
        &old_root.join("launcher.json"),
        &launcher.join("launcher.json"),
    )?;
    Ok(())
}

/// Why kjk's folder cannot be moved to `to`, if it cannot.
pub fn refuse_move(from: &Path, to: &Path) -> Option<String> {
    if to.starts_with(from) {
        return Some("Choose a folder outside the kjk folder.".into());
    }
    if from.starts_with(to) {
        return Some("Choose a folder that is not above the kjk folder.".into());
    }
    None
}

/// Whether `KJK_LAUNCHER_TYPE_PATHS=1` asks for folders to be typed in the
/// launcher instead of chosen in the system's folder dialog.
pub fn type_paths() -> bool {
    std::env::var("KJK_LAUNCHER_TYPE_PATHS").is_ok_and(|v| v == "1")
}

/// The folder a player typed or pasted: surrounding spaces and quotes are
/// dropped, and a leading `~` stands for `home`. It must be a full path to
/// a folder that exists, as a folder picked in the system's dialog is.
pub fn typed_folder(typed: &str, home: Option<&Path>) -> Result<PathBuf, String> {
    let typed = typed.trim().trim_matches(|c| c == '"' || c == '\'');
    let path = match (typed.strip_prefix('~'), home) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with(['/', '\\']) => {
            home.join(rest.trim_start_matches(['/', '\\']))
        }
        _ => PathBuf::from(typed),
    };
    if typed.is_empty() || !path.is_absolute() {
        return Err("Type the folder's full path.".into());
    }
    if !path.is_dir() {
        return Err(format!("There is no folder at {}.", path.display()));
    }
    Ok(path)
}

/// Whether `dir` already is a kjk folder: it has kjk's settings or saves.
pub fn is_kjk_folder(dir: &Path) -> bool {
    dir.join("settings.json").is_file() || dir.join("saves").is_dir()
}

/// Whether `dir` has nothing in it, or does not exist.
pub fn is_empty(dir: &Path) -> bool {
    fs::read_dir(dir).map_or(true, |mut entries| entries.next().is_none())
}

/// Moves every entry of kjk's folder `from` into `to`, then records the
/// new place in `moved-to.txt` in the default folder `default`, or deletes
/// that file when `to` is the default folder. If an entry fails to move,
/// the entries moved so far go back, the pointer is not changed, and the
/// error is returned. The pointer itself never moves.
pub fn move_kjk_folder(
    from: &Path,
    to: &Path,
    default: &Path,
    progress: &Progress,
) -> Result<(), String> {
    if let Some(why) = refuse_move(from, to) {
        return Err(why);
    }
    fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let entries: Vec<PathBuf> = fs::read_dir(from)
        .map_err(|e| format!("{}: {e}", from.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| !(from == default && path.file_name() == Some(MOVED_TO.as_ref())))
        .collect();
    progress
        .total
        .store(entries.len() as u64, Ordering::Relaxed);
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    for entry in &entries {
        let name = entry.file_name().unwrap_or_default().to_string_lossy();
        progress.stage(format!("Moving {name}"));
        let target = to.join(entry.file_name().unwrap_or_default());
        let result = if target.exists() {
            Err(format!("{} already exists.", target.display()))
        } else {
            move_entry(entry, &target)
        };
        match result {
            Ok(_) => {
                moved.push((entry.clone(), target));
                progress.done.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                for (back, at) in moved.iter().rev() {
                    let _ = move_entry(at, back);
                }
                return Err(e);
            }
        }
    }
    point_to(default, to)
}

/// Where `path` is after kjk's folder moved from `from` to `to`: the same
/// place inside `to` when `path` was inside `from`, else `None` (the path
/// did not move).
pub fn follow_move(path: &Path, from: &Path, to: &Path) -> Option<PathBuf> {
    path.strip_prefix(from).ok().map(|inside| to.join(inside))
}

/// Records `to` as kjk's folder in `moved-to.txt` in the default folder
/// `default`, or deletes that file when `to` is the default folder.
pub fn point_to(default: &Path, to: &Path) -> Result<(), String> {
    let pointer = default.join(MOVED_TO);
    if to == default {
        return match fs::remove_file(&pointer) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                Err(format!("{}: {e}", pointer.display()))
            }
            _ => Ok(()),
        };
    }
    fs::create_dir_all(default).map_err(|e| format!("{}: {e}", default.display()))?;
    crate::config::write_atomic(&pointer, format!("{}\n", to.display()).as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh temporary folder for one test.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kjk-launcher-folders-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    /// The 0.1.0 layout, with `config` as launcher.json.
    fn old_layout(root: &Path, config: &str) {
        write(&root.join("launcher.json"), config);
        write(&root.join("releases-cache.json"), "cache");
        write(&root.join("backups/a.zip"), "backup");
        write(&root.join("kjk-engine/0.19.0/kjk"), "game");
    }

    #[test]
    fn migration_moves_everything() {
        let dir = scratch("migrate-full");
        let (old, new) = (dir.join("old"), dir.join("kjk"));
        old_layout(&old, "{}");
        migrate(&old, &new).unwrap();
        assert_eq!(read(&new.join("launcher/launcher.json")), "{}");
        assert_eq!(read(&new.join("launcher/releases-cache.json")), "cache");
        assert_eq!(read(&new.join("backups/a.zip")), "backup");
        assert_eq!(read(&new.join("versions/0.19.0/kjk")), "game");
        assert!(!old.join("launcher.json").exists());
        assert!(!old.join("kjk-engine").exists());
    }

    #[test]
    fn migration_leaves_a_custom_install_folder() {
        let dir = scratch("migrate-custom");
        let (old, new) = (dir.join("old"), dir.join("kjk"));
        old_layout(&old, r#"{"install_dir":"D:\\kjk"}"#);
        migrate(&old, &new).unwrap();
        assert!(new.join("launcher/launcher.json").is_file());
        assert!(old.join("kjk-engine/0.19.0/kjk").is_file());
        assert!(!new.join("versions").exists());
    }

    #[test]
    fn migration_never_overwrites() {
        let dir = scratch("migrate-existing");
        let (old, new) = (dir.join("old"), dir.join("kjk"));
        old_layout(&old, "{}");
        write(&new.join("backups/a.zip"), "newer");
        migrate(&old, &new).unwrap();
        assert_eq!(read(&new.join("backups/a.zip")), "newer");
        assert_eq!(read(&old.join("backups/a.zip")), "backup");
        assert!(new.join("launcher/launcher.json").is_file());
    }

    #[test]
    fn migration_with_nothing_to_move() {
        let dir = scratch("migrate-nothing");
        let (old, new) = (dir.join("old"), dir.join("kjk"));
        migrate(&old, &new).unwrap();
        assert!(!new.exists());
        // A launcher.json at the new place means the move was done.
        old_layout(&old, "{}");
        write(&new.join("launcher/launcher.json"), "new");
        migrate(&old, &new).unwrap();
        assert_eq!(read(&new.join("launcher/launcher.json")), "new");
        assert!(old.join("launcher.json").is_file());
    }

    /// kjk's folder with saves, settings and the launcher's files.
    fn kjk_folder(dir: &Path) {
        write(&dir.join("settings.json"), "settings");
        write(&dir.join("saves/slot1/save.sav"), "save");
        write(&dir.join("launcher/launcher.json"), "{}");
    }

    #[test]
    fn move_writes_the_pointer() {
        let dir = scratch("move-full");
        let (default, to) = (dir.join("default"), dir.join("elsewhere"));
        kjk_folder(&default);
        move_kjk_folder(&default, &to, &default, &Progress::default()).unwrap();
        assert_eq!(read(&to.join("saves/slot1/save.sav")), "save");
        assert_eq!(read(&to.join("launcher/launcher.json")), "{}");
        assert!(!default.join("settings.json").exists());
        assert_eq!(
            read(&default.join(MOVED_TO)).trim(),
            to.display().to_string()
        );
        let resolved = crate::config::resolve_kjk_folder(None, Some(default.clone()));
        assert_eq!(resolved, Some(to));
    }

    #[test]
    fn failed_move_goes_back() {
        let dir = scratch("move-fail");
        let (default, to) = (dir.join("default"), dir.join("elsewhere"));
        kjk_folder(&default);
        // The target already has a "saves" entry, so that entry fails.
        write(&to.join("saves"), "in the way");
        let result = move_kjk_folder(&default, &to, &default, &Progress::default());
        assert!(result.is_err());
        assert_eq!(read(&default.join("settings.json")), "settings");
        assert_eq!(read(&default.join("saves/slot1/save.sav")), "save");
        assert_eq!(read(&default.join("launcher/launcher.json")), "{}");
        assert!(!to.join("settings.json").exists());
        assert!(!default.join(MOVED_TO).exists());
    }

    #[test]
    fn move_into_itself_is_refused() {
        let dir = scratch("move-inside");
        let default = dir.join("default");
        kjk_folder(&default);
        let inside = default.join("saves").join("new");
        let result = move_kjk_folder(&default, &inside, &default, &Progress::default());
        assert!(result.is_err());
        assert!(default.join("settings.json").is_file());
        assert!(!inside.exists());
    }

    #[test]
    fn install_folder_follows_the_move() {
        let dir = scratch("move-install-dir");
        let (default, to) = (dir.join("default"), dir.join("elsewhere"));
        kjk_folder(&default);
        let installs = default.join("my-installs");
        write(&installs.join("0.19.0/kjk"), "game");
        move_kjk_folder(&default, &to, &default, &Progress::default()).unwrap();
        let followed = follow_move(&installs, &default, &to).unwrap();
        assert_eq!(followed, to.join("my-installs"));
        assert_eq!(read(&followed.join("0.19.0/kjk")), "game");
        // A folder outside the kjk folder stays where it is.
        assert_eq!(follow_move(&dir.join("other"), &default, &to), None);
        move_kjk_folder(&to, &default, &default, &Progress::default()).unwrap();
        assert_eq!(
            follow_move(&followed, &to, &default),
            Some(installs.clone())
        );
        assert_eq!(read(&installs.join("0.19.0/kjk")), "game");
    }

    #[test]
    fn typed_folders() {
        let dir = scratch("typed");
        let games = dir.join("Games");
        fs::create_dir_all(&games).unwrap();
        let shown = games.display().to_string();
        let home = Some(dir.as_path());
        let ok = [
            shown.clone(),
            format!("  {shown}  "),
            format!("\"{shown}\""),
            "~/Games".to_string(),
        ];
        for typed in &ok {
            assert_eq!(typed_folder(typed, home), Ok(games.clone()), "{typed}");
        }
        assert_eq!(typed_folder("~", home), Ok(dir.clone()));
        for typed in ["", "   ", "Games", "~/Games"] {
            let home = if typed.starts_with('~') { None } else { home };
            assert_eq!(
                typed_folder(typed, home),
                Err("Type the folder's full path.".to_string()),
                "{typed}"
            );
        }
        let missing = dir.join("Missing");
        assert_eq!(
            typed_folder(&missing.display().to_string(), home),
            Err(format!("There is no folder at {}.", missing.display()))
        );
    }

    #[test]
    fn moving_back_removes_the_pointer() {
        let dir = scratch("move-back");
        let (default, to) = (dir.join("default"), dir.join("elsewhere"));
        kjk_folder(&default);
        move_kjk_folder(&default, &to, &default, &Progress::default()).unwrap();
        move_kjk_folder(&to, &default, &default, &Progress::default()).unwrap();
        assert_eq!(read(&default.join("settings.json")), "settings");
        assert!(!default.join(MOVED_TO).exists());
        assert!(fs::read_dir(&to).unwrap().next().is_none());
    }
}
