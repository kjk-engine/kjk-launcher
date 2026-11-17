//! The launcher's command line. `kjk-launcher [-- args]` opens the window,
//! and the args (after `--`, or all of them) go to kjk when Play starts it.
//! `kjk-launcher --play ...` starts the selected version at once, for
//! Steam's launch options, and ignores what follows.

use std::path::Path;

#[derive(Debug, PartialEq)]
pub enum Mode {
    /// Args for kjk (after "--", or all of them, as in 0.1.0).
    Window { pass: Vec<String> },
    /// "--play" seen; everything after it is ignored.
    Play,
}

pub fn parse(args: &[String]) -> Mode {
    if args.iter().any(|a| a == "--play") {
        return Mode::Play;
    }
    let pass = match args.iter().position(|a| a == "--") {
        Some(at) => args[at + 1..].to_vec(),
        None => args.to_vec(),
    };
    Mode::Window { pass }
}

/// The line for Steam's launch options, for this computer.
pub fn steam_line(launcher: &Path) -> String {
    format!("\"{}\" --play %command%", launcher.display())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modes() {
        let s = |v: &[&str]| v.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        assert_eq!(parse(&s(&[])), Mode::Window { pass: vec![] });
        assert_eq!(
            parse(&s(&["--", "--mute"])),
            Mode::Window {
                pass: s(&["--mute"])
            }
        );
        assert_eq!(
            parse(&s(&["--play", "C:\\Steam\\swkotor.exe", "-x"])),
            Mode::Play
        );
    }

    #[test]
    fn steam_line_quotes_the_path() {
        assert_eq!(
            steam_line(Path::new("C:\\Games\\kjk launcher\\kjk-launcher.exe")),
            "\"C:\\Games\\kjk launcher\\kjk-launcher.exe\" --play %command%"
        );
    }
}
