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

/// How the "Install ffmpeg" dialog tells the player to install ffmpeg.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hint {
    /// A command to run in a terminal.
    Command(&'static str),
    /// SteamOS: no command, a sentence and the download page.
    SteamOs,
    /// A system the launcher does not know: only the download page.
    LinkOnly,
}

/// The hint for this system. On Linux it depends on `/etc/os-release`.
pub fn install_hint() -> Hint {
    if cfg!(windows) {
        Hint::Command("winget install Gyan.FFmpeg")
    } else if cfg!(target_os = "macos") {
        Hint::Command("brew install ffmpeg")
    } else {
        linux_hint(&std::fs::read_to_string("/etc/os-release").unwrap_or_default())
    }
}

/// The hint for the Linux system that `os_release` (the text of
/// `/etc/os-release`) describes. Its `ID` is looked at first, then each
/// name in `ID_LIKE`. SteamOS says it is like Arch, but it has no command.
pub fn linux_hint(os_release: &str) -> Hint {
    let field = |name: &str| {
        os_release
            .lines()
            .find_map(|line| line.trim().strip_prefix(name)?.strip_prefix('='))
            .map(|value| {
                value
                    .trim()
                    .trim_matches(|c| c == '"' || c == '\'')
                    .to_ascii_lowercase()
            })
            .unwrap_or_default()
    };
    let id = field("ID");
    if id == "steamos" {
        return Hint::SteamOs;
    }
    let like = field("ID_LIKE");
    let names = std::iter::once(id.as_str()).chain(like.split_whitespace());
    for name in names {
        let command = match name {
            "debian" | "ubuntu" => "sudo apt install ffmpeg",
            "fedora" => "sudo dnf install ffmpeg",
            "arch" => "sudo pacman -S ffmpeg",
            _ if name.contains("suse") || name == "sles" => "sudo zypper install ffmpeg",
            _ => continue,
        };
        return Hint::Command(command);
    }
    Hint::LinkOnly
}

/// Where to read about other ways to install ffmpeg.
pub const DOWNLOAD_PAGE: &str = "https://ffmpeg.org/download.html";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hint_for_each_linux_system() {
        let cases = [
            ("ID=debian", Hint::Command("sudo apt install ffmpeg")),
            (
                "ID=ubuntu\nID_LIKE=debian",
                Hint::Command("sudo apt install ffmpeg"),
            ),
            (
                "NAME=\"Linux Mint\"\nID=linuxmint\nID_LIKE=\"ubuntu debian\"",
                Hint::Command("sudo apt install ffmpeg"),
            ),
            ("ID=fedora", Hint::Command("sudo dnf install ffmpeg")),
            (
                "ID=nobara\nID_LIKE=\"rhel centos fedora\"",
                Hint::Command("sudo dnf install ffmpeg"),
            ),
            ("ID=arch", Hint::Command("sudo pacman -S ffmpeg")),
            (
                "ID=endeavouros\nID_LIKE=arch",
                Hint::Command("sudo pacman -S ffmpeg"),
            ),
            ("ID=steamos\nID_LIKE=arch", Hint::SteamOs),
            ("ID=\"steamos\"", Hint::SteamOs),
            (
                "ID=\"opensuse-tumbleweed\"\nID_LIKE=\"opensuse suse\"",
                Hint::Command("sudo zypper install ffmpeg"),
            ),
            ("ID=nixos", Hint::LinkOnly),
            ("", Hint::LinkOnly),
        ];
        for (os_release, expected) in cases {
            assert_eq!(linux_hint(os_release), expected, "{os_release}");
        }
    }

    #[test]
    fn hint_for_this_system() {
        if cfg!(windows) {
            assert_eq!(install_hint(), Hint::Command("winget install Gyan.FFmpeg"));
        } else if cfg!(target_os = "macos") {
            assert_eq!(install_hint(), Hint::Command("brew install ffmpeg"));
        }
    }
}
