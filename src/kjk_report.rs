//! What the selected kjk version finds: its game folders and ffmpeg, as
//! `kjk installs --json` reports them (a command of kjk 0.19.2 and later).

use crate::run;
use crate::widgets::Status;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum GameStatus {
    Found { path: PathBuf, source: Source },
    Refused { path: PathBuf, why: String },
    Missing,
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Steam,
    Gog,
    Settings,
    Env,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Game {
    K1,
    Tsl,
    Ja,
}

impl Game {
    pub const ALL: [Game; 3] = [Game::K1, Game::Tsl, Game::Ja];

    /// The key kjk uses: `k1`, `tsl` or `ja`.
    pub fn key(self) -> &'static str {
        match self {
            Game::K1 => "k1",
            Game::Tsl => "tsl",
            Game::Ja => "ja",
        }
    }

    pub fn from_key(key: &str) -> Option<Game> {
        Game::ALL.into_iter().find(|game| game.key() == key)
    }

    /// The name a player reads.
    pub fn name(self) -> &'static str {
        match self {
            Game::K1 => "KOTOR",
            Game::Tsl => "KOTOR II",
            Game::Ja => "Jedi Academy",
        }
    }

    /// Why kjk needs the game, shown when it is not found.
    pub fn needed_for(self) -> &'static str {
        match self {
            Game::K1 => "kjk needs KOTOR for the campaign.",
            Game::Tsl => "kjk needs KOTOR II for the look of its screens.",
            Game::Ja => "kjk needs Jedi Academy for movement, lightsabers and the Force.",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Report {
    pub games: Vec<(Game, GameStatus)>,
    /// Whether kjk finds ffmpeg and ffprobe. `None` when the report has no
    /// `ffmpeg` field.
    pub ffmpeg: Option<bool>,
}

impl Report {
    pub fn status(&self, game: Game) -> Option<&GameStatus> {
        self.games.iter().find(|(g, _)| *g == game).map(|(_, s)| s)
    }
}

/// The report in kjk's answer, or `None` when the answer is anything else.
pub fn parse(stdout: &str) -> Option<Report> {
    let value: serde_json::Value = serde_json::from_str(stdout.trim())
        .ok()
        .or_else(|| serde_json::from_str(stdout.trim().lines().last()?).ok())?;
    let mut games = Vec::new();
    for entry in value.get("games")?.as_array()? {
        let game = Game::from_key(entry.get("game")?.as_str()?)?;
        let status = GameStatus::deserialize(entry).ok()?;
        games.push((game, status));
    }
    let ffmpeg = match value.get("ffmpeg") {
        None => None,
        Some(ffmpeg) => Some(ffmpeg.get("status")?.as_str()? == "found"),
    };
    Some(Report { games, ffmpeg })
}

/// Runs `<kjk> installs --json`. None when the program is older than the
/// command, prints anything else, fails, or takes longer than 10 seconds.
pub fn ask(kjk: &Path) -> Option<Report> {
    parse(&run::output(kjk, &["installs", "--json"])?)
}

/// Runs `<kjk> installs --json --check <game> <path>`.
pub fn check(kjk: &Path, game: Game, folder: &Path) -> Option<GameStatus> {
    let folder = folder.to_str()?;
    let answer = run::output(kjk, &["installs", "--json", "--check", game.key(), folder])?;
    parse(&answer)?.status(game).cloned()
}

/// The status dot and word for a game's row.
pub fn status_word(status: &GameStatus) -> (Status, &'static str) {
    match status {
        GameStatus::Found {
            source: Source::Steam,
            ..
        } => (Status::Ok, "Found on Steam"),
        GameStatus::Found {
            source: Source::Gog,
            ..
        } => (Status::Ok, "Found on GOG"),
        GameStatus::Found { .. } => (Status::Ok, "Found"),
        GameStatus::Missing => (Status::Warn, "Not found"),
        GameStatus::Refused { .. } => (Status::Error, "Not usable"),
    }
}

/// The first game that stops Play: one that is missing or refused. `None`
/// when every game is found, and when there is no report, because the
/// launcher then does not know.
pub fn blocked_by(report: Option<&Report>) -> Option<Game> {
    let report = report?;
    Game::ALL
        .into_iter()
        .find(|game| !matches!(report.status(*game), Some(GameStatus::Found { .. })))
}

/// Whether every game is found and ffmpeg is installed, so the "Before you
/// play" section can be one line.
pub fn all_ready(report: &Report, ffmpeg: bool) -> bool {
    ffmpeg && blocked_by(Some(report)).is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{"games": [
  {"game": "k1",  "status": "found",   "path": "C:\\Games\\swkotor", "source": "steam"},
  {"game": "tsl", "status": "refused", "path": "D:\\wrong", "source": "settings", "why": "no swkotor2.exe"},
  {"game": "ja",  "status": "missing"}
 ],
 "ffmpeg": {"status": "found"}}"#;

    fn found(game: Game) -> (Game, GameStatus) {
        let status = GameStatus::Found {
            path: PathBuf::from("/x"),
            source: Source::Steam,
        };
        (game, status)
    }

    fn report(games: Vec<(Game, GameStatus)>) -> Report {
        Report {
            games,
            ffmpeg: Some(true),
        }
    }

    #[test]
    fn parses_the_report() {
        let r = parse(SAMPLE).unwrap();
        assert_eq!(r.games.len(), 3);
        assert!(matches!(
            r.games[0],
            (
                Game::K1,
                GameStatus::Found {
                    source: Source::Steam,
                    ..
                }
            )
        ));
        assert!(matches!(r.games[2], (Game::Ja, GameStatus::Missing)));
        assert_eq!(r.ffmpeg, Some(true));
    }

    #[test]
    fn anything_else_is_none() {
        for text in [
            "",
            "error: unrecognized subcommand 'installs'",
            "{\"games\": 3}",
        ] {
            assert!(parse(text).is_none(), "{text}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn asks_the_stub_and_gives_up_on_a_hang() {
        assert!(ask(Path::new("tests/stub_kjk.sh")).is_some());
        // `cat` waits on its input forever: it is stopped, and there is no report.
        assert!(ask(Path::new("/bin/cat")).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn checks_a_folder_with_the_stub() {
        let stub = Path::new("tests/stub_kjk.sh");
        let good = check(stub, Game::K1, Path::new("/good"));
        assert!(matches!(good, Some(GameStatus::Found { .. })));
        let bad = check(stub, Game::Tsl, Path::new("/bad"));
        assert!(matches!(bad, Some(GameStatus::Refused { .. })));
    }

    #[test]
    fn blocked_by_the_first_game_that_is_not_found() {
        let refused = GameStatus::Refused {
            path: PathBuf::from("/x"),
            why: String::new(),
        };
        let all = report(Game::ALL.map(found).to_vec());
        let missing = report(vec![
            found(Game::K1),
            (Game::Tsl, GameStatus::Missing),
            found(Game::Ja),
        ]);
        let not_usable = report(vec![found(Game::K1), found(Game::Tsl), (Game::Ja, refused)]);
        let cases = [
            (Some(&all), None),
            (Some(&missing), Some(Game::Tsl)),
            (Some(&not_usable), Some(Game::Ja)),
            (None, None),
        ];
        for (report, expected) in cases {
            assert_eq!(blocked_by(report), expected);
        }
    }

    #[test]
    fn ready_needs_every_game_and_ffmpeg() {
        let all = report(Game::ALL.map(found).to_vec());
        let missing = report(vec![
            found(Game::K1),
            (Game::Tsl, GameStatus::Missing),
            found(Game::Ja),
        ]);
        assert!(all_ready(&all, true));
        assert!(!all_ready(&all, false));
        assert!(!all_ready(&missing, true));
    }
}
