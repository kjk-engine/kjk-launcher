//! The launcher's log: `logs/launcher.log` in the launcher's folder. It
//! holds what the launcher did in detail, for a bug report: what it
//! downloaded from where, the sizes and hashes it found, and each command
//! it gave kjk with kjk's whole answer. The player reads the plain
//! sentences in the window; the details are written here.
//!
//! When the log is over `KEEP` bytes as the launcher starts, it is renamed
//! to `launcher.old.log`, so the log of the run before is kept.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// The size at which the log is started again.
const KEEP: u64 = 4_000_000;

/// The log's file, once the launcher's folder is known.
static FILE: Mutex<Option<PathBuf>> = Mutex::new(None);

/// The log's file in the launcher's folder `root`.
pub fn file_in(root: &Path) -> PathBuf {
    root.join("logs").join("launcher.log")
}

/// Writes the log into the launcher's folder `root` from now on.
pub fn open(root: &Path) {
    let file = file_in(root);
    if let Some(dir) = file.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if fs::metadata(&file).is_ok_and(|m| m.len() > KEEP) {
        let _ = fs::rename(&file, file.with_file_name("launcher.old.log"));
    }
    let mut slot = FILE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if slot.as_ref() != Some(&file) {
        *slot = Some(file);
        drop(slot);
        line(format!(
            "kjk launcher {} on {} {}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH
        ));
    }
}

/// Where the log is written, once it is open.
pub fn path() -> Option<PathBuf> {
    FILE.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Adds `text` to the log with the time. Each further line of `text` is
/// indented under the first. Nothing is written before `open`, and a log
/// that cannot be written is skipped.
pub fn line(text: impl AsRef<str>) {
    let slot = FILE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(file) = slot.as_ref() else { return };
    let time = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    let mut entry = String::new();
    for (i, part) in text.as_ref().lines().enumerate() {
        if i == 0 {
            entry.push_str(&format!("{time} {part}\n"));
        } else {
            entry.push_str(&format!("    {part}\n"));
        }
    }
    if entry.is_empty() {
        return;
    }
    if let Ok(mut out) = OpenOptions::new().create(true).append(true).open(file) {
        let _ = out.write_all(entry.as_bytes());
    }
}
