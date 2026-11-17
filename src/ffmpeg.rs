//! Whether ffmpeg is installed, for kjk versions that cannot report it, and
//! how to install it. kjk needs both `ffmpeg` and `ffprobe` to convert the
//! games' movies.

use crate::run;
use std::path::{Path, PathBuf};

/// The folders Homebrew installs programs into on macOS. kjk looks there
/// too, because a program started from Finder does not have them on its
/// `PATH`.
const HOMEBREW_BINS: [&str; 2] = ["/opt/homebrew/bin", "/usr/local/bin"];

/// Whether both `ffmpeg -version` and `ffprobe -version` run and succeed.
pub fn installed() -> bool {
    ["ffmpeg", "ffprobe"]
        .into_iter()
        .all(|name| run::output(&program(name), &["-version"]).is_some())
}

/// The program to run: on macOS the first Homebrew copy when no folder on
/// the `PATH` has one, else the bare name.
fn program(name: &str) -> PathBuf {
    let on_path = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).any(|folder| folder.join(name).is_file()))
        .unwrap_or(false);
    if cfg!(target_os = "macos") && !on_path {
        if let Some(found) = HOMEBREW_BINS
            .iter()
            .map(|folder| Path::new(folder).join(name))
            .find(|file| file.is_file())
        {
            return found;
        }
    }
    PathBuf::from(name)
}

/// The command that installs ffmpeg on this system.
pub fn install_command() -> &'static str {
    if cfg!(windows) {
        "winget install Gyan.FFmpeg"
    } else if cfg!(target_os = "macos") {
        "brew install ffmpeg"
    } else {
        "sudo apt install ffmpeg"
    }
}

/// Where to read about other ways to install ffmpeg.
pub const DOWNLOAD_PAGE: &str = "https://ffmpeg.org/download.html";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_command_for_this_system() {
        let expected = if cfg!(windows) {
            "winget install Gyan.FFmpeg"
        } else if cfg!(target_os = "macos") {
            "brew install ffmpeg"
        } else {
            "sudo apt install ffmpeg"
        };
        assert_eq!(install_command(), expected);
    }
}
