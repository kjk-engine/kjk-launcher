//! The launcher updating itself from the releases of `LAUNCHER_REPO`. The
//! new package is downloaded and checked like a kjk version, unpacked next
//! to the running program, and swapped in: each old file is renamed to
//! `<name>.old` first, and the new launcher deletes those when it starts.

use crate::github::{self, Release};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The newer launcher release, if any. Pre-releases are offered only when
/// `prereleases` is true, which the caller sets from the environment
/// variable KJK_LAUNCHER_SELF_PRERELEASE=1 (used for the Windows check).
pub fn offer(list: &[Release], prereleases: bool) -> Option<&Release> {
    let running = github::semver(env!("CARGO_PKG_VERSION"))?;
    github::installable(list, prereleases)
        .into_iter()
        .filter(|r| github::semver(&r.tag_name).is_some_and(|v| v > running))
        .max_by_key(|r| github::semver(&r.tag_name))
}

/// Whether KJK_LAUNCHER_SELF_PRERELEASE=1 allows pre-releases of the
/// launcher.
pub fn prereleases_allowed() -> bool {
    std::env::var("KJK_LAUNCHER_SELF_PRERELEASE").is_ok_and(|v| v == "1")
}

/// The folder the launcher's files are in: the running program's folder,
/// or on macOS the folder that holds the `.app` bundle.
pub fn home() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.to_path_buf();
    let bundle = dir
        .ancestors()
        .find(|a| a.extension().is_some_and(|e| e == "app"))
        .map(Path::to_path_buf);
    match bundle {
        Some(app) => app.parent().map(Path::to_path_buf),
        None => Some(dir),
    }
}

fn old_name(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".old");
    PathBuf::from(name)
}

/// Replaces the files in `home` (the folder of the running program, or the
/// .app bundle on macOS) with those in `staged`. Each old file is renamed to
/// `<name>.old` first. On any error every rename is undone and `home` is as before.
pub fn swap(home: &Path, staged: &Path) -> Result<(), String> {
    let entries: Vec<PathBuf> = fs::read_dir(staged)
        .map_err(|e| format!("{}: {e}", staged.display()))?
        .flatten()
        .map(|e| e.path())
        .collect();
    // Each step done so far, to undo in reverse: (from, to) of a rename.
    let mut done: Vec<(PathBuf, PathBuf)> = Vec::new();
    let step = |from: &Path, to: &Path, done: &mut Vec<(PathBuf, PathBuf)>| {
        fs::rename(from, to).map_err(|e| format!("{}: {e}", to.display()))?;
        done.push((from.to_path_buf(), to.to_path_buf()));
        Ok::<(), String>(())
    };
    let result = (|| {
        for new in &entries {
            let name = new.file_name().ok_or("a file without a name")?;
            let target = home.join(name);
            if target.exists() {
                let old = old_name(&target);
                if old.exists() {
                    remove(&old).map_err(|e| format!("{}: {e}", old.display()))?;
                }
                step(&target, &old, &mut done)?;
            }
            step(new, &target, &mut done)?;
        }
        Ok(())
    })();
    if result.is_err() {
        for (from, to) in done.into_iter().rev() {
            let _ = fs::rename(&to, &from);
        }
    }
    result
}

fn remove(path: &Path) -> std::io::Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Deletes `*.old` left in `home` by an earlier update.
pub fn clean(home: &Path) {
    for entry in fs::read_dir(home).into_iter().flatten().flatten() {
        if entry.file_name().to_string_lossy().ends_with(".old") {
            let _ = remove(&entry.path());
        }
    }
}

/// Starts the new launcher and returns; the caller then exits.
pub fn restart(home: &Path) -> Result<(), String> {
    let mut command = if cfg!(target_os = "macos") {
        let mut open = Command::new("open");
        open.arg("-n").arg(home.join("kjk launcher.app"));
        open
    } else if cfg!(windows) {
        Command::new(home.join("kjk-launcher.exe"))
    } else {
        Command::new(home.join("kjk-launcher"))
    };
    command
        .spawn()
        .map(drop)
        .map_err(|e| format!("could not start the new launcher: {e}"))
}

/// Downloads `release` into `<home>/.update`, swaps it in, and starts it.
pub fn update(
    release: &Release,
    downloads: &Path,
    progress: &crate::install::Progress,
) -> Result<(), String> {
    let home = home().ok_or("could not tell where the launcher is")?;
    let staged = home.join(".update");
    if staged.exists() {
        fs::remove_dir_all(&staged).map_err(|e| format!("{}: {e}", staged.display()))?;
    }
    let done = crate::install::fetch_package(release, downloads, &staged, progress)
        .and_then(|()| swap(&home, &staged))
        .and_then(|()| restart(&home));
    let _ = fs::remove_dir_all(&staged);
    done
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::{package_suffix, Asset};
    use std::path::{Path, PathBuf};

    const NAMES: [&str; 2] = ["kjk-launcher", "kjk-launcher-cli"];

    /// `home` and `staged` folders in a fresh temporary folder, each with
    /// the two programs holding "old" and "new".
    fn folders(name: &str) -> (PathBuf, PathBuf) {
        let top =
            std::env::temp_dir().join(format!("kjk-launcher-swap-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&top);
        let (home, staged) = (top.join("home"), top.join("staged"));
        for (dir, text) in [(&home, "old"), (&staged, "new")] {
            std::fs::create_dir_all(dir).unwrap();
            for file in NAMES {
                std::fs::write(dir.join(file), text).unwrap();
            }
        }
        (home, staged)
    }

    fn read(path: &Path) -> String {
        std::fs::read_to_string(path).unwrap()
    }

    fn old_copies(home: &Path) -> usize {
        std::fs::read_dir(home)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".old"))
            .count()
    }

    fn release(tag: &str, prerelease: bool) -> Release {
        Release {
            tag_name: tag.into(),
            name: None,
            body: None,
            prerelease,
            draft: false,
            published_at: None,
            html_url: String::new(),
            assets: vec![Asset {
                name: format!("kjk-launcher-{tag}{}", package_suffix()),
                browser_download_url: String::new(),
                size: 0,
            }],
        }
    }

    #[test]
    fn swap_replaces_files_and_keeps_old_copies() {
        let (home, staged) = folders("replace");
        swap(&home, &staged).unwrap();
        for file in NAMES {
            assert_eq!(read(&home.join(file)), "new");
            assert_eq!(read(&home.join(format!("{file}.old"))), "old");
        }
    }

    #[cfg(unix)]
    #[test]
    fn swap_into_a_read_only_folder_changes_nothing() {
        use std::os::unix::fs::PermissionsExt;
        let (home, staged) = folders("readonly");
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o555)).unwrap();
        let swapped = swap(&home, &staged);
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(swapped.is_err());
        for file in NAMES {
            assert_eq!(read(&home.join(file)), "old");
        }
        assert_eq!(old_copies(&home), 0);
    }

    #[test]
    fn clean_removes_old_copies() {
        let (home, staged) = folders("clean");
        swap(&home, &staged).unwrap();
        clean(&home);
        assert_eq!(old_copies(&home), 0);
        assert_eq!(read(&home.join(NAMES[0])), "new");
    }

    #[test]
    fn offers_only_a_newer_release() {
        let tag = |r: Option<&Release>| r.map(|r| r.tag_name.clone());
        let newer = [release("v0.1.0", false), release("v99.0.0", false)];
        assert_eq!(tag(offer(&newer, false)), Some("v99.0.0".into()));
        assert_eq!(tag(offer(&[release("v0.1.0", false)], false)), None);
        let rc = [release("v99.1.0-rc.1", true)];
        assert_eq!(tag(offer(&rc, false)), None);
    }
}
