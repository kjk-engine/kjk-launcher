//! Backups of kjk's saves and settings, as zips in the launcher's
//! `backups/` folder. A new version may migrate saves so that an older
//! version cannot read them again; a backup taken before it first starts
//! is the way back.

use crate::config::{file_safe, write_atomic, Paths};
use std::fs::{self, File};
use std::io::{self, BufWriter, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use zip::write::SimpleFileOptions;

#[derive(Clone, Debug)]
pub struct Backup {
    pub path: PathBuf,
    pub name: String,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

fn stamp() -> String {
    chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string()
}

/// Zips the saves folder and `settings.json`. `None` when kjk has neither yet.
pub fn create(paths: &Paths, label: &str) -> Result<Option<PathBuf>, String> {
    let saves = paths.saves().filter(|s| s.is_dir());
    let settings = paths.settings().filter(|s| s.is_file());
    if saves.is_none() && settings.is_none() {
        return Ok(None);
    }
    let out = paths.backups.join(format!("{}_{}.zip", stamp(), file_safe(label)));
    let part = out.with_extension("zip.part");
    let written = (|| -> Result<(), String> {
        let file = File::create(&part).map_err(|e| format!("{}: {e}", part.display()))?;
        let mut zip = zip::ZipWriter::new(BufWriter::new(file));
        let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        if let Some(settings) = &settings {
            zip.start_file("settings.json", options).map_err(|e| e.to_string())?;
            let mut source = File::open(settings).map_err(|e| format!("{}: {e}", settings.display()))?;
            io::copy(&mut source, &mut zip).map_err(|e| e.to_string())?;
        }
        if let Some(saves) = &saves {
            zip.add_directory("saves/", options).map_err(|e| e.to_string())?;
            add_folder(&mut zip, saves, "saves", options)?;
        }
        zip.finish().map_err(|e| e.to_string())?;
        Ok(())
    })();
    if let Err(e) = written {
        let _ = fs::remove_file(&part);
        return Err(format!("could not back up the saves: {e}"));
    }
    fs::rename(&part, &out).map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(Some(out))
}

fn add_folder<W: io::Write + io::Seek>(
    zip: &mut zip::ZipWriter<W>,
    dir: &Path,
    prefix: &str,
    options: SimpleFileOptions,
) -> Result<(), String> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = format!("{prefix}/{}", entry.file_name().to_string_lossy());
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| format!("{}: {e}", path.display()))?;
        if kind.is_dir() {
            zip.add_directory(format!("{name}/"), options).map_err(|e| e.to_string())?;
            add_folder(zip, &path, &name, options)?;
        } else if kind.is_file() {
            zip.start_file(name, options).map_err(|e| e.to_string())?;
            let mut source = File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            io::copy(&mut source, zip).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Backups, newest first.
pub fn list(paths: &Paths) -> Vec<Backup> {
    let mut found: Vec<Backup> = fs::read_dir(&paths.backups)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".zip"))
        .map(|e| {
            let meta = e.metadata().ok();
            Backup {
                path: e.path(),
                name: e.file_name().to_string_lossy().into_owned(),
                size: meta.as_ref().map_or(0, |m| m.len()),
                modified: meta.and_then(|m| m.modified().ok()),
            }
        })
        .collect();
    found.sort_by(|a, b| (b.modified, &b.name).cmp(&(a.modified, &a.name)));
    found
}

/// Deletes all but the newest `keep` backups.
pub fn prune(paths: &Paths, keep: usize) {
    for old in list(paths).into_iter().skip(keep.max(1)) {
        let _ = fs::remove_file(old.path);
    }
}

/// Puts a backup's saves and settings back. The present ones are backed
/// up first (returned), so a restore can itself be undone.
pub fn restore(paths: &Paths, backup: &Path) -> Result<Option<PathBuf>, String> {
    if crate::launch::kjk_running() {
        return Err("close kjk (and the save editor) before restoring".into());
    }
    let saves = paths.saves().ok_or("could not tell where kjk's saves are")?;
    let file = File::open(backup).map_err(|e| format!("{}: {e}", backup.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", backup.display()))?;

    let safety = create(paths, "before-restore")?;

    let staging = saves.with_file_name(format!("{}.restoring", file_name(&saves)));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|e| format!("{}: {e}", staging.display()))?;
    }
    let mut settings: Option<Vec<u8>> = None;
    let mut has_saves = false;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let Some(name) = entry.enclosed_name() else { continue };
        if name == Path::new("settings.json") {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
            settings = Some(bytes);
            continue;
        }
        let Ok(relative) = name.strip_prefix("saves") else { continue };
        has_saves = true;
        let out = staging.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
            }
            let mut target = File::create(&out).map_err(|e| format!("{}: {e}", out.display()))?;
            io::copy(&mut entry, &mut target).map_err(|e| format!("{}: {e}", out.display()))?;
        }
    }

    if has_saves {
        if saves.exists() {
            let old = saves.with_file_name(format!("{}.old-{}", file_name(&saves), stamp()));
            fs::rename(&saves, &old).map_err(|e| format!("could not move the present saves aside: {e}"))?;
            let _ = fs::remove_dir_all(&old);
        }
        fs::rename(&staging, &saves).map_err(|e| format!("{}: {e}", saves.display()))?;
    }
    if let (Some(bytes), Some(path)) = (settings, paths.settings()) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        write_atomic(&path, &bytes)?;
    }
    Ok(safety)
}

fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(|| "saves".into(), |n| n.to_string_lossy().into_owned())
}
