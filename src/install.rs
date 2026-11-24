//! Installed versions, each in its own folder under `versions/`, and
//! installing one: download, check against `SHA256SUMS`, unpack into a
//! `.partial` folder, then rename it into place.

use crate::config::{file_safe, write_atomic, Config, Paths};
use crate::github::{self, compare, Release};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

const META: &str = "kjk-launcher.json";
const PARTIAL: &str = ".partial";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Installed {
    pub tag: String,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub installed_at: String,
    #[serde(skip)]
    pub dir: PathBuf,
    /// Found in the install folder without the launcher's file (unzipped
    /// by hand); never removed by itself.
    #[serde(skip)]
    pub adopted: bool,
}

impl Installed {
    pub fn date(&self) -> &str {
        github::date(self.published_at.as_deref())
    }

    pub fn key(&self) -> (&str, Option<&str>) {
        (&self.tag, self.published_at.as_deref())
    }
}

const REMOVING: &str = ".removing";

/// A version's folder name: `0.19.0` for `v0.19.0`; other tags as they are.
pub fn folder_for(tag: &str) -> String {
    let bare = tag
        .strip_prefix('v')
        .filter(|rest| rest.starts_with(|c: char| c.is_ascii_digit()));
    file_safe(bare.unwrap_or(tag))
}

/// The tag a hand-made folder stands for: `0.18.0`, `v0.18.0` and
/// `kjk-v0.18.0-windows-x86_64` are all `v0.18.0`.
fn tag_from_folder(name: &str) -> String {
    let name = name.strip_prefix("kjk-").unwrap_or(name);
    let platform = github::package_suffix().split('.').next().unwrap_or("");
    let name = name.strip_suffix(platform).unwrap_or(name);
    if name.starts_with(|c: char| c.is_ascii_digit()) {
        format!("v{name}")
    } else {
        name.to_string()
    }
}

/// Installed versions in the install folder, newest first.
pub fn installed(paths: &Paths) -> Vec<Installed> {
    let mut found: Vec<Installed> = fs::read_dir(&paths.versions)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            !name.ends_with(PARTIAL) && !name.ends_with(REMOVING)
        })
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|entry| {
            let dir = entry.path();
            match fs::read_to_string(dir.join(META)) {
                Ok(text) => {
                    let mut meta: Installed = serde_json::from_str(&text).ok()?;
                    meta.dir = dir;
                    Some(meta)
                }
                Err(_) if game_exe(&dir).exists() => Some(Installed {
                    tag: tag_from_folder(&entry.file_name().to_string_lossy()),
                    published_at: None,
                    prerelease: false,
                    installed_at: String::new(),
                    dir,
                    adopted: true,
                }),
                Err(_) => None,
            }
        })
        .collect();
    found.sort_by(|a, b| {
        compare(
            &b.tag,
            b.published_at.as_deref(),
            &a.tag,
            a.published_at.as_deref(),
        )
    });
    found.dedup_by(|a, b| a.tag == b.tag);
    found
}

/// Bytes in a folder and everything under it.
pub fn dir_size(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(t) if t.is_dir() => dir_size(&entry.path()),
            _ => entry.metadata().map_or(0, |m| m.len()),
        })
        .sum()
}

/// Deletes what is left of earlier uninstalls that could not finish, and
/// of downloads of the new lighting that never ended.
pub fn clean_leftovers(paths: &Paths) {
    crate::lighting::clean_downloads(&paths.downloads);
    for entry in fs::read_dir(&paths.versions)
        .into_iter()
        .flatten()
        .flatten()
    {
        if entry.file_name().to_string_lossy().ends_with(REMOVING) {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

pub fn find(paths: &Paths, tag: &str) -> Option<Installed> {
    installed(paths).into_iter().find(|i| i.tag == tag)
}

/// The game in a version's folder (on macOS, the app bundle).
pub fn game_exe(dir: &Path) -> PathBuf {
    if cfg!(windows) {
        dir.join("kjk.exe")
    } else if cfg!(target_os = "macos") {
        dir.join("kjk.app")
    } else {
        dir.join("kjk")
    }
}

/// The program in a version's folder that can be run with arguments:
/// `kjk.exe`, the program inside `kjk.app` on macOS, or `kjk`.
pub fn game_binary(dir: &Path) -> PathBuf {
    if cfg!(windows) {
        dir.join("kjk.exe")
    } else if cfg!(target_os = "macos") {
        dir.join("kjk.app")
            .join("Contents")
            .join("MacOS")
            .join("kjk")
    } else {
        dir.join("kjk")
    }
}

/// The save editor packed beside the game, when there is one.
pub fn editor_exe(dir: &Path) -> Option<PathBuf> {
    let path = if cfg!(windows) {
        dir.join("kjk-save-editor.exe")
    } else if cfg!(target_os = "macos") {
        dir.join("kjk Save Editor.app")
    } else {
        dir.join("kjk-save-editor")
    };
    path.exists().then_some(path)
}

#[derive(Default)]
pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub cancel: AtomicBool,
    /// While set, the progress counts areas or files, not bytes.
    pub items: AtomicBool,
    /// While set, the step cannot be stopped, so Cancel is hidden.
    pub locked: AtomicBool,
    stage: Mutex<String>,
}

impl Progress {
    pub fn stage(&self, text: impl Into<String>) {
        *self.stage.lock().unwrap() = text.into();
    }

    pub fn snapshot(&self) -> (String, u64, u64) {
        (
            self.stage.lock().unwrap().clone(),
            self.done.load(Ordering::Relaxed),
            self.total.load(Ordering::Relaxed),
        )
    }
}

/// Downloads the package of `release` for this computer into `downloads`,
/// checks it against the release's `SHA256SUMS`, and unpacks it into
/// `into`, a fresh folder. The download is deleted afterwards.
pub fn fetch_package(
    release: &Release,
    downloads: &Path,
    into: &Path,
    progress: &Progress,
) -> Result<(), String> {
    let package = release
        .package()
        .ok_or("this release has no package for this computer")?;
    let sums = release
        .sums()
        .ok_or("this release has no SHA256SUMS, so its download cannot be checked")?;

    progress.stage("Reading checksums");
    let sums_text = get_text(&sums.browser_download_url)?;
    let expected = expected_hash(&sums_text, &package.name)
        .ok_or_else(|| format!("SHA256SUMS does not list {}", package.name))?;

    progress.stage(format!("Downloading {}", package.name));
    fs::create_dir_all(downloads).map_err(|e| format!("{}: {e}", downloads.display()))?;
    let archive = downloads.join(&package.name);
    let part = downloads.join(format!("{}.part", package.name));
    let actual = download(&package.browser_download_url, &part, package.size, progress);
    let actual = match actual {
        Ok(hash) => hash,
        Err(e) => {
            let _ = fs::remove_file(&part);
            return Err(e.to_string());
        }
    };
    if actual != expected {
        let _ = fs::remove_file(&part);
        return Err(format!(
            "the download does not match SHA256SUMS (expected {expected}, got {actual}); nothing was installed"
        ));
    }
    fs::rename(&part, &archive).map_err(|e| format!("{}: {e}", archive.display()))?;

    progress.stage("Unpacking");
    let unpacked = (|| {
        if into.exists() {
            fs::remove_dir_all(into).map_err(|e| format!("{}: {e}", into.display()))?;
        }
        fs::create_dir_all(into).map_err(|e| format!("{}: {e}", into.display()))?;
        if package.name.ends_with(".zip") {
            unzip(&archive, into)
        } else {
            untar_gz(&archive, into)
        }
    })();
    let _ = fs::remove_file(&archive);
    unpacked
}

pub fn install(paths: &Paths, release: &Release, progress: &Progress) -> Result<Installed, String> {
    let name = folder_for(&release.tag_name);
    let staging = paths.versions.join(format!("{name}{PARTIAL}"));
    let target = paths.versions.join(&name);
    let unpacked = fetch_package(release, &paths.downloads, &staging, progress).and_then(|()| {
        if !game_exe(&staging).exists() {
            return Err("the package has no kjk program in it".to_string());
        }
        let meta = Installed {
            tag: release.tag_name.clone(),
            published_at: release.published_at.clone(),
            prerelease: release.prerelease,
            installed_at: chrono::Local::now().to_rfc3339(),
            dir: target.clone(),
            adopted: false,
        };
        let json = serde_json::to_vec_pretty(&meta).map_err(|e| e.to_string())?;
        write_atomic(&staging.join(META), &json)?;
        if target.exists() {
            fs::remove_dir_all(&target).map_err(|e| {
                format!(
                    "could not replace {} (is it running?): {e}",
                    target.display()
                )
            })?;
        }
        fs::rename(&staging, &target).map_err(|e| format!("{}: {e}", target.display()))?;
        Ok(meta)
    });
    if unpacked.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    progress.stage("Installed");
    unpacked
}

pub(crate) fn expected_hash(sums: &str, file: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        let hash = words.next()?;
        let name = words.next()?.trim_start_matches('*');
        (name == file).then(|| hash.to_ascii_lowercase())
    })
}

pub(crate) fn get_text(url: &str) -> Result<String, String> {
    let mut response = github::agent(Some(std::time::Duration::from_secs(30)))
        .get(url)
        .call()
        .map_err(|e| format!("could not download {url} ({e})"))?;
    if response.status().as_u16() != 200 {
        return Err(format!("{url} answered {}", response.status()));
    }
    response
        .body_mut()
        .read_to_string()
        .map_err(|e| format!("{url}: {e}"))
}

/// Why a download failed.
#[derive(Debug)]
pub enum Failure {
    Cancelled,
    /// Nothing came: the server could not be reached, or it answered with
    /// an error.
    Unreachable(String),
    /// The download stopped part way.
    Interrupted(String),
    /// The file at this path could not be written.
    Write(PathBuf, io::Error),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Cancelled => write!(f, "cancelled"),
            Failure::Unreachable(why) | Failure::Interrupted(why) => write!(f, "{why}"),
            Failure::Write(path, e) => write!(f, "{}: {e}", path.display()),
        }
    }
}

/// Downloads `url` to `to` and returns the file's SHA-256 in hex.
pub(crate) fn download(
    url: &str,
    to: &Path,
    size: u64,
    progress: &Progress,
) -> Result<String, Failure> {
    let mut response = github::agent(None)
        .get(url)
        .call()
        .map_err(|e| Failure::Unreachable(format!("could not download {url} ({e})")))?;
    if response.status().as_u16() != 200 {
        return Err(Failure::Unreachable(format!(
            "{url} answered {}",
            response.status()
        )));
    }
    let total = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok()?.parse().ok())
        .unwrap_or(size);
    progress.total.store(total, Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);

    let mut reader = response.body_mut().as_reader();
    let file = File::create(to).map_err(|e| Failure::Write(to.to_path_buf(), e))?;
    let mut file = BufWriter::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        if progress.cancel.load(Ordering::Relaxed) {
            return Err(Failure::Cancelled);
        }
        let n = reader
            .read(&mut buffer)
            .map_err(|e| Failure::Interrupted(format!("download interrupted ({e})")))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        file.write_all(&buffer[..n])
            .map_err(|e| Failure::Write(to.to_path_buf(), e))?;
        progress.done.fetch_add(n as u64, Ordering::Relaxed);
    }
    file.flush()
        .map_err(|e| Failure::Write(to.to_path_buf(), e))?;
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Whether every path sits in one top folder (kjk's packages do:
/// `kjk-v0.19.0-windows-x86_64/kjk.exe`), which is then left out.
fn single_top(paths: &[PathBuf]) -> bool {
    let mut tops = paths.iter().filter_map(|p| p.components().next());
    let Some(first) = tops.next() else {
        return false;
    };
    tops.all(|top| top == first) && paths.iter().any(|p| p.components().count() > 1)
}

fn strip_top(path: &Path) -> Option<PathBuf> {
    let mut components = path.components();
    components.next()?;
    let rest: PathBuf = components.collect();
    (!rest.as_os_str().is_empty()).then_some(rest)
}

/// A relative path with no `..`, root or drive in it.
fn safe(path: &Path) -> bool {
    path.components()
        .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
}

fn unzip(archive: &Path, dest: &Path) -> Result<(), String> {
    let open = || -> Result<zip::ZipArchive<File>, String> {
        let file = File::open(archive).map_err(|e| format!("{}: {e}", archive.display()))?;
        zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", archive.display()))
    };
    let mut zip = open()?;
    let names: Vec<PathBuf> = (0..zip.len())
        .filter_map(|i| zip.by_index(i).ok()?.enclosed_name())
        .collect();
    let strip = single_top(&names);
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.enclosed_name().ok_or_else(|| {
            format!(
                "the package has an unsafe path: {}",
                entry.name().unwrap_or_default()
            )
        })?;
        let relative = if strip { strip_top(&name) } else { Some(name) };
        let Some(relative) = relative else { continue };
        let out = dest.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let mut file = File::create(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        io::copy(&mut entry, &mut file).map_err(|e| format!("{}: {e}", out.display()))?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&out, fs::Permissions::from_mode(mode));
        }
    }
    Ok(())
}

fn untar_gz(archive: &Path, dest: &Path) -> Result<(), String> {
    let open = || -> Result<tar::Archive<flate2::read::GzDecoder<File>>, String> {
        let file = File::open(archive).map_err(|e| format!("{}: {e}", archive.display()))?;
        Ok(tar::Archive::new(flate2::read::GzDecoder::new(file)))
    };
    let names: Vec<PathBuf> = open()?
        .entries()
        .map_err(|e| e.to_string())?
        .filter_map(|entry| Some(entry.ok()?.path().ok()?.into_owned()))
        .collect();
    let strip = single_top(&names);
    let mut tar = open()?;
    for entry in tar.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let name = entry.path().map_err(|e| e.to_string())?.into_owned();
        if !safe(&name) {
            return Err(format!(
                "the package has an unsafe path: {}",
                name.display()
            ));
        }
        let relative = if strip { strip_top(&name) } else { Some(name) };
        let Some(relative) = relative else { continue };
        let out = dest.join(relative);
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        entry
            .unpack(&out)
            .map_err(|e| format!("{}: {e}", out.display()))?;
    }
    Ok(())
}

/// Uninstalls a version. The folder is renamed first, which Windows
/// refuses while the version runs, so a running kjk is never half deleted.
/// When it was the current version, the newest one left becomes current.
pub fn remove(paths: &Paths, config: &mut Config, tag: &str) -> Result<(), String> {
    let version = find(paths, tag).ok_or_else(|| format!("{tag} is not installed"))?;
    delete_folder(&version.dir)
        .map_err(|e| format!("could not uninstall {tag} (is it running?): {e}"))?;
    if config.current.as_deref() == Some(tag) {
        config.current = installed(paths).first().map(|i| i.tag.clone());
    }
    Ok(())
}

fn delete_folder(dir: &Path) -> io::Result<()> {
    let mut doomed = dir.as_os_str().to_owned();
    doomed.push(REMOVING);
    let doomed = PathBuf::from(doomed);
    fs::rename(dir, &doomed)?;
    let _ = fs::remove_dir_all(&doomed);
    Ok(())
}

/// The installed versions other than the selected one, which "Delete older
/// versions" removes. A version unzipped by hand is never one of them.
pub fn older(paths: &Paths, config: &Config) -> Vec<Installed> {
    installed(paths)
        .into_iter()
        .filter(|version| {
            !version.adopted && config.current.as_deref() != Some(version.tag.as_str())
        })
        .collect()
}

/// Uninstalls every version `older` lists and returns the tags it removed.
/// A version that is running stays, because its folder cannot be renamed.
pub fn delete_older(paths: &Paths, config: &Config) -> Vec<String> {
    older(paths, config)
        .into_iter()
        .filter(|version| delete_folder(&version.dir).is_ok())
        .map(|version| version.tag)
        .collect()
}

/// What moving the installs to another folder did.
pub struct Moved {
    pub moved: usize,
    pub problems: Vec<String>,
}

/// Moves each installed version's folder into `to` (renamed when on the
/// same drive, else copied and then deleted). A version already in `to`
/// is left where it is.
pub fn move_installs(versions: &[Installed], to: &Path, progress: &Progress) -> Moved {
    let mut result = Moved {
        moved: 0,
        problems: Vec::new(),
    };
    if let Err(e) = fs::create_dir_all(to) {
        result.problems.push(format!("{}: {e}", to.display()));
        return result;
    }
    let sizes: Vec<u64> = versions.iter().map(|v| dir_size(&v.dir)).collect();
    progress.total.store(sizes.iter().sum(), Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);
    for (i, (version, size)) in versions.iter().zip(&sizes).enumerate() {
        progress.stage(format!(
            "Moving {} ({} of {})",
            crate::versions::title(&version.tag),
            i + 1,
            versions.len()
        ));
        let Some(name) = version.dir.file_name() else {
            continue;
        };
        let target = to.join(name);
        if target.exists() {
            result.problems.push(format!(
                "{} is already in the new folder, so the old copy stays where it was",
                crate::versions::title(&version.tag)
            ));
            progress.done.fetch_add(*size, Ordering::Relaxed);
            continue;
        }
        let moved = fs::rename(&version.dir, &target).or_else(|_| {
            let mut staging = target.as_os_str().to_owned();
            staging.push(PARTIAL);
            let staging = PathBuf::from(staging);
            copy_dir(&version.dir, &staging, progress)
                .and_then(|()| fs::rename(&staging, &target))
                .inspect_err(|_| {
                    let _ = fs::remove_dir_all(&staging);
                })?;
            delete_folder(&version.dir)
        });
        match moved {
            Ok(()) => result.moved += 1,
            Err(e) => result.problems.push(format!(
                "could not move {} (is it running?): {e}",
                version.tag
            )),
        }
        progress
            .done
            .store(sizes[..=i].iter().sum(), Ordering::Relaxed);
    }
    result
}

fn copy_dir(from: &Path, to: &Path, progress: &Progress) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target, progress)?;
        } else {
            let bytes = fs::copy(entry.path(), &target)?;
            progress.done.fetch_add(bytes, Ordering::Relaxed);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_sha256sums() {
        let sums = "10d6  kjk-v0.19.0-linux-x86_64.tar.gz\nA4C5  kjk-v0.19.0-windows-x86_64.zip\n";
        assert_eq!(
            expected_hash(sums, "kjk-v0.19.0-windows-x86_64.zip").as_deref(),
            Some("a4c5")
        );
        assert_eq!(expected_hash(sums, "other.zip"), None);
    }

    #[test]
    fn strips_one_top_folder() {
        let names = vec![PathBuf::from("kjk-v1/"), PathBuf::from("kjk-v1/kjk.exe")];
        assert!(single_top(&names));
        assert!(!single_top(&[
            PathBuf::from("kjk.exe"),
            PathBuf::from("README.txt")
        ]));
        assert_eq!(
            strip_top(Path::new("kjk-v1/kjk.exe")),
            Some(PathBuf::from("kjk.exe"))
        );
        assert!(!safe(Path::new("../evil")));
    }

    #[test]
    fn version_folders() {
        assert_eq!(folder_for("v0.19.0"), "0.19.0");
        assert_eq!(folder_for("v0.15.0-md-rc.1"), "0.15.0-md-rc.1");
        assert_eq!(
            folder_for("pre-feat-x-v0.17.0-40-g6d6b576"),
            "pre-feat-x-v0.17.0-40-g6d6b576"
        );
        assert_eq!(tag_from_folder("0.18.0"), "v0.18.0");
        assert_eq!(tag_from_folder("v0.18.0"), "v0.18.0");
        let platform = github::package_suffix().split('.').next().unwrap();
        assert_eq!(
            tag_from_folder(&format!("kjk-v0.18.0{platform}")),
            "v0.18.0"
        );
    }
}
