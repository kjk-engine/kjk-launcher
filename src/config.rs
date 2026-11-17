//! Where things live, and the launcher's settings (`launcher.json`).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// The GitHub repository whose releases are installed.
pub const REPO: &str = "kjk-engine/kjk";

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
    /// Installed versions kept after an update (the current one always
    /// is); 0 keeps them all.
    pub keep_versions: usize,
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
            keep_versions: 3,
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
}

/// The launcher's folders and its settings, ready to use.
pub fn open() -> Result<(Paths, Config), String> {
    let mut paths = Paths::discover()?;
    let config = Config::load(&paths);
    paths.use_install_dir(&config)?;
    Ok((paths, config))
}

#[derive(Clone, Debug)]
pub struct Paths {
    /// `%LOCALAPPDATA%\kjk-launcher` (or `KJK_LAUNCHER_HOME`).
    pub root: PathBuf,
    /// The install folder: one subfolder per version (`Config::install_dir`).
    pub versions: PathBuf,
    pub backups: PathBuf,
    pub downloads: PathBuf,
    pub config_file: PathBuf,
    pub cache_file: PathBuf,
    /// kjk's own folder: settings, saves, logs. Found the way kjk finds it
    /// (`store::kjk_folder`): `%APPDATA%\kjk`, `~/Library/Application
    /// Support/kjk`, or `$XDG_CONFIG_HOME/kjk`.
    pub kjk_data: Option<PathBuf>,
}

impl Paths {
    pub fn discover() -> Result<Paths, String> {
        let root = var("KJK_LAUNCHER_HOME")
            .map(PathBuf::from)
            .or_else(|| dirs::data_local_dir().map(|dir| dir.join("kjk-launcher")))
            .ok_or("could not tell where to keep the launcher's files (no home folder)")?;
        let paths = Paths {
            versions: root.join("kjk-engine"),
            backups: root.join("backups"),
            downloads: root.join("downloads"),
            config_file: root.join("launcher.json"),
            cache_file: root.join("releases-cache.json"),
            kjk_data: kjk_folder(),
            root,
        };
        for dir in [&paths.root, &paths.backups, &paths.downloads] {
            fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        Ok(paths)
    }

    pub fn default_install_dir(&self) -> PathBuf {
        self.root.join("kjk-engine")
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

    pub fn settings(&self) -> Option<PathBuf> {
        self.kjk_data.as_ref().map(|dir| dir.join("settings.json"))
    }
}

fn var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

fn kjk_folder() -> Option<PathBuf> {
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

    #[test]
    fn reads_a_0_1_0_config_with_background_fields() {
        let old = r#"{"current":"v0.19.0","keep_versions":3,"background":"C:\\x\\background.png","background_credit":"someone"}"#;
        let config: Config = serde_json::from_str(old).unwrap();
        assert_eq!(config.current.as_deref(), Some("v0.19.0"));
        let again = serde_json::to_string(&config).unwrap();
        assert!(!again.contains("background"));
    }
}
