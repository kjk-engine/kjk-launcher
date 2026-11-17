//! Where things live, and the launcher's settings (`launcher.json`).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// The GitHub repository whose releases are installed.
pub const REPO: &str = "kjk-engine/kjk";

/// The GitHub repository of the launcher's own releases.
pub const LAUNCHER_REPO: &str = "kjk-engine/kjk-launcher";

pub const USER_AGENT: &str = concat!("kjk-launcher/", env!("CARGO_PKG_VERSION"));

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// The installed version that Play starts.
    pub current: Option<String>,
    /// The top folder that holds one subfolder per version (`0.19.0/`).
    /// `None`: `kjk-engine` in the launcher's folder.
    pub install_dir: Option<PathBuf>,
    /// Offer and list pre-releases as well as releases.
    pub prereleases: bool,
    /// Stay on `current`: never offer an update.
    pub pinned: bool,
    /// A release the player chose to skip; it is not offered again.
    pub skipped: Option<String>,
    pub check_on_start: bool,
    /// Zip kjk's saves and settings before a version's first start.
    pub backup_before_new_version: bool,
    /// Backups kept; older ones are deleted.
    pub keep_backups: usize,
    /// Versions that have been started at least once.
    pub launched: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            current: None,
            install_dir: None,
            prereleases: false,
            pinned: false,
            skipped: None,
            check_on_start: true,
            backup_before_new_version: true,
            keep_backups: 10,
            launched: Vec::new(),
        }
    }
}

impl Config {
    pub fn load(paths: &Paths) -> Config {
        fs::read_to_string(&paths.config_file)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, paths: &Paths) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        write_atomic(&paths.config_file, &json)
    }

    /// The settings to use after the launcher switched to another kjk
    /// folder whose files are at `paths`. That folder's own `launcher.json`
    /// is kept and loaded when it has one: `(its settings, false)`.
    /// Otherwise `(self, true)`: these settings, to be saved there.
    pub fn for_folder(&self, paths: &Paths) -> (Config, bool) {
        if paths.config_file.is_file() {
            (Config::load(paths), false)
        } else {
            (self.clone(), true)
        }
    }
}

/// The launcher's folders and its settings, ready to use. A failed move
/// from the 0.1.0 layout is printed, and the old place is used.
pub fn open() -> Result<(Paths, Config), String> {
    let (paths, config, problem) = open_noting()?;
    if let Some(problem) = problem {
        eprintln!("{problem}");
    }
    Ok((paths, config))
}

/// As `open`, with the sentence to show when the move from the 0.1.0
/// layout failed.
pub fn open_noting() -> Result<(Paths, Config, Option<String>), String> {
    let (mut paths, problem) = Paths::discover()?;
    let config = Config::load(&paths);
    paths.use_install_dir(&config)?;
    Ok((paths, config, problem))
}

/// The file in kjk's default folder that names the folder kjk was moved to.
pub const MOVED_TO: &str = "moved-to.txt";

#[derive(Clone, Debug)]
pub struct Paths {
    /// The launcher's own files: `launcher` in kjk's folder (or
    /// `KJK_LAUNCHER_HOME`, or the 0.1.0 folder `%LOCALAPPDATA%\kjk-launcher`).
    pub root: PathBuf,
    /// The install folder: one subfolder per version (`Config::install_dir`).
    pub versions: PathBuf,
    /// The install folder when the settings name none.
    pub default_versions: PathBuf,
    pub backups: PathBuf,
    pub downloads: PathBuf,
    pub config_file: PathBuf,
    pub cache_file: PathBuf,
    /// The launcher's own release list, cached like kjk's.
    pub launcher_cache_file: PathBuf,
    /// kjk's own folder: settings, saves, logs. Found the way kjk finds it
    /// (`resolve_kjk_folder`).
    pub kjk_data: Option<PathBuf>,
}

impl Paths {
    /// The launcher's folders, made if missing. `KJK_LAUNCHER_HOME` keeps
    /// the 0.1.0 layout in that folder. Otherwise the launcher's files are
    /// in kjk's folder, moved there from the 0.1.0 folder the first time.
    /// The second value is why that move failed; the 0.1.0 folder is then
    /// used for this run.
    pub fn discover() -> Result<(Paths, Option<String>), String> {
        let old_root = dirs::data_local_dir().map(|dir| dir.join("kjk-launcher"));
        let (paths, problem) = match (var("KJK_LAUNCHER_HOME"), kjk_folder(), old_root) {
            (Some(home), ..) => (Paths::at(PathBuf::from(home)), None),
            (None, Some(kjk), old) => {
                let moved = old
                    .as_ref()
                    .map_or(Ok(()), |old| crate::folders::migrate(old, &kjk));
                match (moved, old) {
                    (Err(e), Some(old)) => (
                        Paths::at(old),
                        Some(format!(
                            "The launcher could not move its files into the kjk folder: {e}"
                        )),
                    ),
                    _ => (Paths::in_kjk_folder(kjk), None),
                }
            }
            (None, None, Some(old)) => (Paths::at(old), None),
            (None, None, None) => {
                return Err(
                    "could not tell where to keep the launcher's files (no home folder)".into(),
                )
            }
        };
        for dir in [&paths.root, &paths.backups, &paths.downloads] {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        Ok((paths, problem))
    }

    /// The 0.1.0 layout in `root`, without making any folder.
    pub fn at(root: PathBuf) -> Paths {
        let versions = root.join("kjk-engine");
        Paths {
            versions: versions.clone(),
            default_versions: versions,
            backups: root.join("backups"),
            downloads: root.join("downloads"),
            config_file: root.join("launcher.json"),
            cache_file: root.join("releases-cache.json"),
            launcher_cache_file: root.join("launcher-releases-cache.json"),
            kjk_data: kjk_folder(),
            root,
        }
    }

    /// The layout in kjk's folder `kjk`: `launcher/`, `backups/` and
    /// `versions/`, without making any folder.
    pub fn in_kjk_folder(kjk: PathBuf) -> Paths {
        let root = kjk.join("launcher");
        let versions = kjk.join("versions");
        Paths {
            versions: versions.clone(),
            default_versions: versions,
            backups: kjk.join("backups"),
            downloads: root.join("downloads"),
            config_file: root.join("launcher.json"),
            cache_file: root.join("releases-cache.json"),
            launcher_cache_file: root.join("launcher-releases-cache.json"),
            kjk_data: Some(kjk),
            root,
        }
    }

    pub fn default_install_dir(&self) -> PathBuf {
        self.default_versions.clone()
    }

    /// Points `versions` at the configured install folder and makes it.
    pub fn use_install_dir(&mut self, config: &Config) -> Result<(), String> {
        let dir = config
            .install_dir
            .clone()
            .unwrap_or_else(|| self.default_install_dir());
        fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        self.versions = dir;
        Ok(())
    }

    /// kjk's saves: `KJK_SAVES`, else `saves` in kjk's folder (as `saves::folder`).
    pub fn saves(&self) -> Option<PathBuf> {
        var("KJK_SAVES")
            .map(PathBuf::from)
            .or_else(|| self.kjk_data.as_ref().map(|dir| dir.join("saves")))
    }

    /// kjk's settings file: `KJK_SETTINGS`, else `settings.json` in kjk's
    /// folder (as kjk finds it).
    pub fn settings(&self) -> Option<PathBuf> {
        var("KJK_SETTINGS")
            .map(PathBuf::from)
            .or_else(|| self.kjk_data.as_ref().map(|dir| dir.join("settings.json")))
    }
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

/// kjk's folder, found as kjk finds it: `KJK_HOME` when set, else the
/// default folder, or the folder its `moved-to.txt` names.
pub fn kjk_folder() -> Option<PathBuf> {
    resolve_kjk_folder(var("KJK_HOME").map(PathBuf::from), default_kjk_folder())
}

/// kjk's folder from `KJK_HOME` (`home`) and the default folder: `home`
/// when it is set; else the folder the first line of the default folder's
/// `moved-to.txt` names, when that folder exists; else the default folder.
pub fn resolve_kjk_folder(home: Option<PathBuf>, default: Option<PathBuf>) -> Option<PathBuf> {
    if home.is_some() {
        return home;
    }
    let default = default?;
    Some(moved_to(&default).unwrap_or(default))
}

/// The existing folder that `moved-to.txt` in the default folder names.
pub fn moved_to(default: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(default.join(MOVED_TO)).ok()?;
    let first = text.lines().next()?.trim();
    let dir = PathBuf::from(first);
    (!first.is_empty() && dir.is_dir()).then_some(dir)
}

/// kjk's folder when it was never moved: `%APPDATA%\kjk`,
/// `~/Library/Application Support/kjk`, or `$XDG_CONFIG_HOME/kjk`.
pub fn default_kjk_folder() -> Option<PathBuf> {
    let base = if cfg!(target_os = "macos") {
        var("HOME").map(|home| {
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
        })
    } else if cfg!(windows) {
        var("APPDATA").map(PathBuf::from)
    } else {
        var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| var("HOME").map(|home| PathBuf::from(home).join(".config")))
    };
    base.map(|base| base.join("kjk"))
}

/// Whether kjk's folder was moved with the launcher: `KJK_HOME` is not set
/// and the default folder has a `moved-to.txt` that names a folder.
pub fn kjk_folder_moved() -> bool {
    var("KJK_HOME").is_none() && default_kjk_folder().is_some_and(|dir| moved_to(&dir).is_some())
}

/// The variables every start of kjk gets while its folder is moved, so a
/// kjk that does not read `moved-to.txt` finds its folder too: `KJK_HOME`,
/// `KJK_SAVES` and `KJK_SETTINGS`, each unless the player set it already.
pub fn kjk_env() -> Vec<(&'static str, PathBuf)> {
    if !kjk_folder_moved() {
        return Vec::new();
    }
    let Some(dir) = kjk_folder() else {
        return Vec::new();
    };
    [
        ("KJK_HOME", dir.clone()),
        ("KJK_SAVES", dir.join("saves")),
        ("KJK_SETTINGS", dir.join("settings.json")),
    ]
    .into_iter()
    .filter(|(name, _)| var(name).is_none())
    .collect()
}

/// Writes through a temporary file, so a crash never leaves half a file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, bytes).map_err(|e| format!("{}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// A tag made safe as a file or folder name.
pub fn file_safe(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-+".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("kjk-launcher-config-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn kjk_folder_in_order() {
        let dir = scratch("resolve");
        let (default, moved, home) = (dir.join("default"), dir.join("moved"), dir.join("home"));
        fs::create_dir_all(&default).unwrap();
        let resolve =
            |home: Option<&PathBuf>| resolve_kjk_folder(home.cloned(), Some(default.clone()));
        assert_eq!(resolve(None), Some(default.clone()));
        // A pointer to a folder that does not exist is ignored.
        fs::write(default.join(MOVED_TO), format!("{}\nmore", moved.display())).unwrap();
        assert_eq!(resolve(None), Some(default.clone()));
        fs::create_dir_all(&moved).unwrap();
        assert_eq!(resolve(None), Some(moved.clone()));
        assert_eq!(resolve(Some(&home)), Some(home.clone()));
        assert_eq!(resolve_kjk_folder(None, None), None);
    }

    #[test]
    fn another_kjk_folder_keeps_its_own_settings() {
        let dir = scratch("for-folder");
        let paths = Paths::in_kjk_folder(dir.join("other"));
        let mine = Config {
            current: Some("v0.19.0".into()),
            ..Config::default()
        };
        // A folder without a launcher.json takes these settings.
        let (config, save) = mine.for_folder(&paths);
        assert_eq!(config.current.as_deref(), Some("v0.19.0"));
        assert!(save);
        // A folder with one keeps it, untouched.
        let theirs = r#"{"current":"v0.18.5","pinned":true}"#;
        fs::create_dir_all(&paths.root).unwrap();
        fs::write(&paths.config_file, theirs).unwrap();
        let (config, save) = mine.for_folder(&paths);
        assert_eq!(config.current.as_deref(), Some("v0.18.5"));
        assert!(config.pinned);
        assert!(!save);
        assert_eq!(fs::read_to_string(&paths.config_file).unwrap(), theirs);
    }

    #[test]
    fn reads_a_0_1_0_config_with_background_fields() {
        let old = r#"{"current":"v0.19.0","keep_versions":3,"background":"C:\\x\\background.png","background_credit":"someone"}"#;
        let config: Config = serde_json::from_str(old).unwrap();
        assert_eq!(config.current.as_deref(), Some("v0.19.0"));
        let again = serde_json::to_string(&config).unwrap();
        assert!(!again.contains("background"));
    }
}
