//! The new lighting: a download for KOTOR and KOTOR II that kjk installs.
//! kjk itself downloads nothing. The launcher finds the download, fetches
//! its manifest, and asks kjk whether it fits the player's game files
//! (`kjk lighting check`). Only then does it download the zip, check it
//! against `SHA256SUMS`, and hand both files to `kjk lighting install`,
//! which checks the zip's BLAKE3 again.
//!
//! The downloads are on GitHub releases of kjk whose tag starts with
//! `lighting-`. Each holds, for each game, `kjk-lighting-<k1|tsl>-<label>.zip`,
//! its manifest `kjk-lighting-<k1|tsl>-<label>.txt`, and `SHA256SUMS`.
//! `KJK_LIGHTING_SOURCE=<folder>` takes the same files from a folder instead,
//! for tests and for installs without a network. After the files are
//! fetched, both take the same path.
//!
//! A failed or cancelled download deletes what it fetched. It is never
//! resumed: the next try starts again from the beginning.

use crate::github::{Asset, Release};
use crate::install::{self, Progress};
use crate::kjk_report::Game;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::Duration;

/// The start of a lighting release's tag: `lighting-1`.
pub const TAG_PREFIX: &str = "lighting-";

/// The variable that names a folder to take the download from.
pub const SOURCE_VAR: &str = "KJK_LIGHTING_SOURCE";

/// The games with a lighting download.
pub const GAMES: [Game; 2] = [Game::K1, Game::Tsl];

/// The patch a game's lighting is made for, as the player reads it.
pub fn patch(game: Game) -> &'static str {
    match game {
        Game::K1 => "the K1 Community Patch",
        _ => "TSLRCM",
    }
}

/// The size of a game's zip when the release does not say it.
pub fn usual_size(game: Game) -> u64 {
    match game {
        Game::K1 => 300_000_000,
        _ => 185_000_000,
    }
}

/// One game's download in one release (or in the source folder).
#[derive(Clone, Debug)]
pub struct Offer {
    /// The release's tag; `local` for the source folder.
    pub tag: String,
    pub game: Game,
    /// The part of the file names after the game: `1` in `kjk-lighting-k1-1.zip`.
    pub label: String,
    pub manifest: Asset,
    pub zip: Asset,
    pub sums: Option<Asset>,
    /// From the source folder: each asset's `browser_download_url` is a path.
    pub local: bool,
}

/// The label of a game's manifest named `name`: `1` for
/// `kjk-lighting-k1-1.txt` and game K1.
fn manifest_label(name: &str, game: Game) -> Option<&str> {
    let prefix = format!("kjk-lighting-{}-", game.key());
    name.strip_prefix(&prefix)?
        .strip_suffix(".txt")
        .filter(|label| !label.is_empty())
}

fn zip_name(game: Game, label: &str) -> String {
    format!("kjk-lighting-{}-{label}.zip", game.key())
}

/// The newest manifest and its zip among `assets`, with `SHA256SUMS`.
fn pick(assets: &[Asset], game: Game) -> Option<(String, Asset, Asset, Option<Asset>)> {
    let mut found: Vec<(String, Asset, Asset)> = assets
        .iter()
        .filter_map(|manifest| {
            let label = manifest_label(&manifest.name, game)?;
            let zip = assets.iter().find(|a| a.name == zip_name(game, label))?;
            Some((label.to_string(), manifest.clone(), zip.clone()))
        })
        .collect();
    found.sort_by(|a, b| label_order(&b.0, &a.0));
    let (label, manifest, zip) = found.into_iter().next()?;
    let sums = assets.iter().find(|a| a.name == "SHA256SUMS").cloned();
    Some((label, manifest, zip, sums))
}

/// Labels by their numbers when both are numbers, else by their text.
fn label_order(a: &str, b: &str) -> std::cmp::Ordering {
    match (a.parse::<u64>(), b.parse::<u64>()) {
        (Ok(a), Ok(b)) => a.cmp(&b),
        _ => a.cmp(b),
    }
}

/// The download for `game` in `release`, when the release is a lighting
/// release and holds one.
pub fn in_release(release: &Release, game: Game) -> Option<Offer> {
    if release.draft || !release.tag_name.starts_with(TAG_PREFIX) {
        return None;
    }
    let (label, manifest, zip, sums) = pick(&release.assets, game)?;
    Some(Offer {
        tag: release.tag_name.clone(),
        game,
        label,
        manifest,
        zip,
        sums,
        local: false,
    })
}

/// The downloads for `game` in GitHub's list of releases, newest first: by
/// the number after `lighting-` when both tags have one, else by date.
/// Pre-releases count too, so a lighting release can be kept off GitHub's
/// "Latest" mark.
pub fn offers(list: &[Release], game: Game) -> Vec<Offer> {
    let mut found: Vec<(&Release, Offer)> = list
        .iter()
        .filter_map(|release| Some((release, in_release(release, game)?)))
        .collect();
    found.sort_by(|(a, _), (b, _)| {
        let number = |r: &Release| r.tag_name[TAG_PREFIX.len()..].parse::<u64>().ok();
        match (number(a), number(b)) {
            (Some(a), Some(b)) => b.cmp(&a),
            _ => b.published_at.cmp(&a.published_at),
        }
    });
    found.into_iter().map(|(_, offer)| offer).collect()
}

/// The download for `game` in the folder `dir`: the same files as on a
/// release, with the newest label when there are several.
pub fn local_offers(dir: &Path, game: Game) -> Vec<Offer> {
    let listed = fs::read_dir(dir);
    if let Err(e) = &listed {
        crate::log::line(format!(
            "lighting: {SOURCE_VAR} names {}, which cannot be read: {e}",
            dir.display()
        ));
    }
    let assets: Vec<Asset> = listed
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|t| t.is_file()))
        .map(|entry| Asset {
            name: entry.file_name().to_string_lossy().into_owned(),
            browser_download_url: entry.path().to_string_lossy().into_owned(),
            size: entry.metadata().map_or(0, |m| m.len()),
        })
        .collect();
    pick(&assets, game)
        .map(|(label, manifest, zip, sums)| Offer {
            tag: "local".into(),
            game,
            label,
            manifest,
            zip,
            sums,
            local: true,
        })
        .into_iter()
        .collect()
}

/// The folder `KJK_LIGHTING_SOURCE` names, when it is set.
pub fn source() -> Option<PathBuf> {
    std::env::var_os(SOURCE_VAR)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// The downloads for `game`, newest first: from the source folder when
/// `KJK_LIGHTING_SOURCE` is set, else from GitHub's list.
pub fn available(list: &[Release], game: Game) -> Vec<Offer> {
    match source() {
        Some(dir) => local_offers(&dir, game),
        None => offers(list, game),
    }
}

/// The `format` line of a manifest.
pub fn manifest_format(text: &str) -> Option<u32> {
    text.lines().find_map(|line| {
        let mut words = line.split_whitespace();
        (words.next() == Some("format")).then(|| words.next()?.parse().ok())?
    })
}

/// One game's line in kjk's answer.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct GameLine {
    #[serde(default)]
    pub game: Option<String>,
    pub status: String,
    #[serde(default)]
    pub areas: Option<usize>,
    #[serde(default)]
    pub archive: Option<String>,
    #[serde(default)]
    pub why: Option<String>,
    /// What went wrong, for a line that failed or was refused: one of
    /// kjk's closed set of kinds (`kind_words`). An older kjk gives none.
    #[serde(default)]
    pub kind: Option<String>,
    /// kjk's store folder, for a failure to write into it. An older kjk
    /// gives none.
    #[serde(default)]
    pub store: Option<String>,
}

/// An area that keeps the game's own lighting, and why.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct FallBack {
    pub area: String,
    pub why: String,
}

/// What `kjk lighting check` found.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Check {
    pub game: String,
    /// `allowed` or `refused`.
    pub status: String,
    #[serde(default)]
    pub needs: String,
    pub areas: usize,
    pub matching: usize,
    #[serde(default)]
    pub matched: Vec<String>,
    #[serde(default)]
    pub placeables_differ: Vec<String>,
    #[serde(default)]
    pub fall_back: Vec<FallBack>,
    #[serde(default)]
    pub cached: bool,
    /// kjk's sentence for the player when the install is refused.
    #[serde(default)]
    pub why: Option<String>,
    /// Why the install is refused: `files_differ` or `bake_format`. An
    /// older kjk gives none.
    #[serde(default)]
    pub kind: Option<String>,
    /// The format of the download's bakes. An older kjk gives none.
    #[serde(default)]
    pub bake_format: Option<u32>,
    /// The format of the bakes this kjk reads. An older kjk gives none.
    #[serde(default)]
    pub reads_format: Option<u32>,
}

impl Check {
    pub fn allowed(&self) -> bool {
        self.status == "allowed"
    }

    /// The download's format and the format kjk reads, when they differ.
    /// kjk gives both. An older kjk says so only in the sentence of each
    /// area that falls back: "the bake's format is 3, and this game reads
    /// format 2".
    pub fn format_mismatch(&self) -> Option<(u32, u32)> {
        if let (Some(bake), Some(reads)) = (self.bake_format, self.reads_format) {
            return (bake != reads).then_some((bake, reads));
        }
        self.fall_back.iter().find_map(|fall| {
            let rest = fall.why.split("the bake's format is ").nth(1)?;
            let (bake, rest) = rest.split_once(',')?;
            let reads = rest.split("reads format ").nth(1)?;
            let reads: String = reads.chars().take_while(char::is_ascii_digit).collect();
            Some((bake.trim().parse().ok()?, reads.parse().ok()?))
        })
    }
}

/// A progress line: `done` of `total` areas checked or unpacked.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Step {
    pub step: String,
    pub done: u64,
    pub total: u64,
    #[serde(default)]
    pub area: String,
}

/// The last line of a command: the games and, for a check or an install,
/// the check.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Answer {
    pub games: Vec<GameLine>,
    pub check: Option<Check>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Progress(Step),
    Answer(Answer),
}

/// One line of kjk's output, or `None` when it is not one of its JSON lines.
pub fn parse_line(text: &str) -> Option<Line> {
    let value: serde_json::Value = serde_json::from_str(text.trim()).ok()?;
    if let Some(step) = value.get("progress") {
        return Some(Line::Progress(Step::deserialize(step).ok()?));
    }
    let games = match value.get("games") {
        Some(games) => Vec::<GameLine>::deserialize(games).ok()?,
        None => Vec::new(),
    };
    let check = match value.get("check") {
        Some(check) => Some(Check::deserialize(check).ok()?),
        None => None,
    };
    if games.is_empty() && check.is_none() {
        return None;
    }
    Some(Line::Answer(Answer { games, check }))
}

/// What is installed for one game.
#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Missing,
    /// Installed with this many areas, from the zip of this name.
    Installed {
        areas: usize,
        archive: String,
    },
    /// Installed, but kjk cannot read it, and why.
    Broken(String),
    /// kjk could not tell, and why.
    Failed(String),
}

/// What kjk says about the lighting.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// This kjk has no `lighting` command.
    TooOld,
    Games(Vec<(Game, State)>),
}

impl Status {
    pub fn state(&self, game: Game) -> Option<&State> {
        match self {
            Status::TooOld => None,
            Status::Games(games) => games.iter().find(|(g, _)| *g == game).map(|(_, s)| s),
        }
    }
}

/// The status in the output of `kjk lighting status --json`. A line
/// without a game counts for both games.
pub fn parse_status(stdout: &str) -> Status {
    let answer = stdout
        .lines()
        .rev()
        .find_map(|line| match parse_line(line) {
            Some(Line::Answer(answer)) => Some(answer),
            _ => None,
        });
    answer.map_or(Status::TooOld, |answer| from_answer(&answer))
}

/// The status in kjk's answer to `lighting status`.
fn from_answer(answer: &Answer) -> Status {
    let mut games = Vec::new();
    for line in &answer.games {
        let state = match line.status.as_str() {
            "missing" => State::Missing,
            "installed" => State::Installed {
                areas: line.areas.unwrap_or(0),
                archive: line.archive.clone().unwrap_or_default(),
            },
            "broken" => State::Broken(line.why.clone().unwrap_or_default()),
            _ => State::Failed(match line.kind.as_deref() {
                Some("no_store") => NO_STORE.to_string(),
                _ => line.why.clone().unwrap_or_default(),
            }),
        };
        match line.game.as_deref().and_then(Game::from_key) {
            Some(game) => games.push((game, state)),
            None => games.extend(GAMES.map(|game| (game, state.clone()))),
        }
    }
    Status::Games(games)
}

/// Whether `offer` is newer than what is installed: the installed zip has
/// another name.
pub fn newer(state: &State, offer: &Offer) -> bool {
    matches!(state, State::Installed { archive, .. } if *archive != offer.zip.name)
}

/// What a cancelled download returns as its error.
pub const CANCELLED: &str = "cancelled";

/// The sentence that sends the player to the log for the details.
pub fn log_hint() -> String {
    match crate::log::path() {
        Some(path) => format!("The launcher's log has the details: {}", path.display()),
        None => "The launcher's log has the details.".to_string(),
    }
}

/// Writes `detail` to the log and returns `player`, the sentence the
/// player reads.
fn failed(player: impl Into<String>, detail: impl AsRef<str>) -> String {
    let player = player.into();
    crate::log::line(format!(
        "lighting: failed: {}\nthe player reads: {player}",
        detail.as_ref()
    ));
    player
}

/// The sentence for a download that could not be read from the source.
fn unreachable(offer: &Offer) -> String {
    if offer.local {
        format!(
            "The launcher could not read the new lighting from the folder that {SOURCE_VAR} names."
        )
    } else {
        "The launcher could not reach GitHub to download the new lighting. Check your internet connection, then try again.".into()
    }
}

/// The player's sentence for a download that failed with `failure`, into
/// the folder `dir`, from `offer`.
fn download_failed(failure: install::Failure, offer: &Offer, dir: &Path) -> String {
    use install::Failure;
    let player = match &failure {
        Failure::Cancelled => {
            crate::log::line("lighting: the download was cancelled");
            return CANCELLED.into();
        }
        Failure::Unreachable(_) => unreachable(offer),
        Failure::Interrupted(_) => "The download stopped before it was complete. Check your internet connection, then try again.".into(),
        Failure::Write(_, error) => write_failed(error, dir, offer.zip.size),
    };
    failed(player, failure.to_string())
}

/// The player's sentence for a file that could not be written into `dir`,
/// for a download of about `bytes`.
fn write_failed(error: &std::io::Error, dir: &Path, bytes: u64) -> String {
    // ERROR_HANDLE_DISK_FULL and ERROR_DISK_FULL on Windows, ENOSPC elsewhere.
    let code = error.raw_os_error();
    let full = if cfg!(windows) {
        matches!(code, Some(39 | 112))
    } else {
        code == Some(28)
    };
    if full || error.kind() == std::io::ErrorKind::StorageFull {
        format!(
            "There is not enough free space to download the new lighting into {}. It needs about {}. Free some space, then try again.",
            dir.display(),
            crate::size::size(bytes)
        )
    } else if error.kind() == std::io::ErrorKind::PermissionDenied {
        format!(
            "The launcher is not allowed to write into {}, so the new lighting was not downloaded. Check that the folder is not read-only, then try again.",
            dir.display()
        )
    } else {
        format!(
            "The launcher could not write the new lighting into {}. {}",
            dir.display(),
            log_hint()
        )
    }
}

/// kjk's sentence and kind for a line that failed, for the log.
fn detail(line: &GameLine) -> String {
    let why = line.why.clone().unwrap_or_else(|| line.status.clone());
    match &line.kind {
        Some(kind) => format!("{why} (kind {kind})"),
        None => why,
    }
}

/// The sentence for a kjk that found no folder for its files.
const NO_STORE: &str = "kjk could not find the folder where it keeps its files.";

/// What the launcher asked kjk to do, for the player's sentence when it
/// failed.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Asked {
    Check,
    Install,
    Remove,
}

/// The player's sentence for `line`, a game's line of kjk that failed or
/// was refused while kjk did `asked` for `game`, from the `kind` kjk gives.
/// `bytes` is about the size the download takes on the disk. `None` when
/// kjk gives no kind, as an older kjk does, or one the launcher does not
/// know: the caller then says what it said before kjk gave kinds.
fn kind_words(line: &GameLine, asked: Asked, game: Game, bytes: u64) -> Option<String> {
    let folder = match &line.store {
        Some(store) => format!("the folder {store}"),
        None => "the folder where kjk keeps its files".to_string(),
    };
    let words = match line.kind.as_deref()? {
        "no_store" => format!("{NO_STORE} {}", log_hint()),
        "store_not_writable" if asked == Asked::Remove => format!(
            "kjk is not allowed to change {folder}. Check that the folder is not read-only, then try again."
        ),
        "store_not_writable" => format!(
            "kjk is not allowed to write into {folder}. Check that the folder is not read-only, then try again."
        ),
        "disk_full" => format!(
            "There is not enough free space on the disk of {folder}. The new lighting needs about {}. Free some space, then try again.",
            crate::size::size(bytes)
        ),
        "in_use" => "Another program is using the files of the new lighting. Close kjk if it is running, then try again.".to_string(),
        "game_not_found" => format!(
            "kjk did not find {}. Choose the game's folder on the Play page, then try again.",
            game.name()
        ),
        "game_unreadable" => format!(
            "kjk could not read the files of {}. Check the game's folder on the Play page, then try again.",
            game.name()
        ),
        "list_unreadable" => {
            "kjk could not read the list of areas of this download. Try the download again."
                .to_string()
        }
        "list_invalid" => "This version of kjk cannot read the list of areas of this download. Update kjk, then try again.".to_string(),
        "wrong_game" => format!(
            "This download is not the new lighting for {}.",
            game.name()
        ),
        "zip_unreadable" => {
            "kjk could not open the downloaded file. Try the download again.".to_string()
        }
        "zip_mismatch" | "zip_damaged" => DAMAGED.to_string(),
        "files_differ" | "bake_format" => game_names(line.why.as_deref()?),
        _ => return None,
    };
    Some(words)
}

/// kjk's sentence with the games named as the launcher names them. An
/// older kjk says "KOTOR 1" and "KOTOR 2".
pub fn game_names(text: &str) -> String {
    text.replace("KOTOR 1", Game::K1.name())
        .replace("KOTOR 2", Game::Tsl.name())
}

/// What a manifest says about its download.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Facts {
    pub game: Option<String>,
    pub format: Option<u32>,
    pub needs: Option<String>,
    /// The zip's file name and size in bytes.
    pub archive: Option<(String, u64)>,
    pub areas: usize,
}

/// The facts in the text of a manifest.
pub fn facts(text: &str) -> Facts {
    let mut facts = Facts {
        format: manifest_format(text),
        ..Facts::default()
    };
    for line in text.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["game", game] => facts.game = Some((*game).to_string()),
            ["needs", ..] => facts.needs = Some(words[1..].join(" ")),
            ["archive", file, "blake3", _, "bytes", bytes] => {
                facts.archive = bytes.parse().ok().map(|b| ((*file).to_string(), b));
            }
            ["area", ..] => facts.areas += 1,
            _ => {}
        }
    }
    facts
}

/// Runs kjk with `args`, hands each progress line to `step`, and returns
/// its last answer. With `progress`, a cancel stops kjk unless the step is
/// locked. `Ok(None)` when kjk printed no answer: it is older than the
/// command. The command, kjk's whole output and its exit code go to the
/// log; the progress lines are counted, with the last one.
fn run_kjk(
    kjk: &Path,
    args: &[&std::ffi::OsStr],
    progress: Option<&Progress>,
    step: &mut dyn FnMut(&Step),
) -> Result<Option<Answer>, String> {
    let shown: Vec<String> = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let command_line = format!("{} {}", kjk.display(), shown.join(" "));
    crate::log::line(format!("lighting: runs {command_line}"));
    let started = std::time::Instant::now();
    let mut command = Command::new(kjk);
    command
        .args(args)
        .envs(crate::config::kjk_env())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::run::no_window(&mut command);
    let mut child = command.spawn().map_err(|e| {
        failed(
            "kjk could not be started. Install this version of kjk again on the Versions page, then try again.",
            format!("{command_line}: {e}"),
        )
    })?;
    let stdout = child.stdout.take().ok_or("kjk could not start")?;
    let stderr = child.stderr.take();
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut stderr) = stderr {
            let _ = stderr.read_to_string(&mut text);
        }
        text
    });
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    let mut answer = None;
    let mut output = Vec::new();
    let (mut steps, mut last_step) = (0usize, String::new());
    let mut stopped = false;
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(text) => match parse_line(&text) {
                Some(Line::Progress(found)) => {
                    steps += 1;
                    last_step = text;
                    step(&found);
                }
                Some(Line::Answer(found)) => {
                    output.push(text);
                    answer = Some(found);
                }
                None => output.push(text),
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {
                stopped = progress.is_some_and(|p| {
                    p.cancel.load(Ordering::Relaxed) && !p.locked.load(Ordering::Relaxed)
                });
                if stopped {
                    let _ = child.kill();
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = child.wait();
    let errors = errors.join().unwrap_or_default();
    let ended = match (&status, stopped) {
        (_, true) => "was stopped by a cancel".to_string(),
        (Ok(status), _) => format!("ended with {status}"),
        (Err(e), _) => format!("could not be waited for ({e})"),
    };
    let mut entry = format!(
        "lighting: kjk {ended} after {:.1} s, with {steps} progress lines",
        started.elapsed().as_secs_f64()
    );
    if !last_step.is_empty() {
        entry.push_str(&format!("\nlast progress: {last_step}"));
    }
    for line in &output {
        entry.push_str(&format!("\nout: {line}"));
    }
    for line in errors.lines() {
        entry.push_str(&format!("\nerr: {line}"));
    }
    crate::log::line(entry);
    if stopped {
        return Err(CANCELLED.into());
    }
    Ok(answer)
}

/// `kjk lighting status --json`.
pub fn status(kjk: &Path) -> Status {
    let args = ["lighting", "status", "--json"].map(std::ffi::OsStr::new);
    match run_kjk(kjk, &args, None, &mut |_| {}) {
        Ok(Some(answer)) => from_answer(&answer),
        _ => Status::TooOld,
    }
}

/// `kjk lighting remove <game> --json`. `Ok(true)` when it removed the
/// lighting, `Ok(false)` when none was installed.
pub fn remove(kjk: &Path, game: Game) -> Result<bool, String> {
    let args = ["lighting", "remove", game.key(), "--json"].map(std::ffi::OsStr::new);
    let answer = run_kjk(kjk, &args, None, &mut |_| {})?.ok_or_else(|| {
        failed(
            "This version of kjk cannot remove the new lighting. Update kjk, then try again.",
            "kjk printed no answer to lighting remove",
        )
    })?;
    match answer.games.first() {
        Some(line) if line.status == "removed" => Ok(true),
        Some(line) if line.status == "missing" => Ok(false),
        Some(line) => Err(failed(
            kind_words(line, Asked::Remove, game, 0).unwrap_or_else(|| {
                format!(
                    "kjk could not remove the new lighting. Close kjk if it is running, then try again. {}",
                    log_hint()
                )
            }),
            detail(line),
        )),
        None => Err(failed(
            format!("kjk did not answer. {}", log_hint()),
            "kjk printed no game line to lighting remove",
        )),
    }
}

/// How a download ended, when nothing failed.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// Installed, with this many areas, and what the check found.
    Installed { areas: usize, check: Box<Check> },
    /// kjk turned the download away, with its sentence for the player.
    /// Nothing but the manifest was downloaded.
    Refused { why: String },
    /// The download is newer than this kjk reads.
    NeedsNewerKjk,
    /// This kjk reads a newer download than any there is.
    NeedsNewerDownload,
}

/// "All 75 areas match your game files." or "73 of the 75 areas match
/// your game files."
pub fn matching_words(check: &Check) -> String {
    if check.matching == check.areas {
        format!("All {} areas match your game files.", check.areas)
    } else {
        format!(
            "{} of the {} areas match your game files.",
            check.matching, check.areas
        )
    }
}

/// The text of `SHA256SUMS` beside `offer`.
fn sums_text(offer: &Offer) -> Result<String, String> {
    let sums = offer
        .sums
        .as_ref()
        .ok_or_else(|| failed(UNCHECKED, format!("{} has no SHA256SUMS", offer.tag)))?;
    crate::log::line(format!("lighting: reads {}", sums.browser_download_url));
    let text = if offer.local {
        fs::read_to_string(&sums.browser_download_url)
            .map_err(|e| format!("{}: {e}", sums.browser_download_url))
    } else {
        install::get_text(&sums.browser_download_url)
    };
    text.map_err(|e| failed(unreachable(offer), e))
}

/// Copies the file at `from` to `to` and returns its SHA-256 in hex, as
/// `install::download` does for a URL.
fn copy_hashing(from: &Path, to: &Path, progress: &Progress) -> Result<String, install::Failure> {
    use install::Failure;
    let mut reader =
        File::open(from).map_err(|e| Failure::Unreachable(format!("{}: {e}", from.display())))?;
    let total = reader.metadata().map_or(0, |m| m.len());
    progress.total.store(total, Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);
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
            .map_err(|e| Failure::Interrupted(format!("{}: {e}", from.display())))?;
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

/// The sentence for a downloaded file that is not the one the release
/// lists.
const DAMAGED: &str = "The downloaded file is damaged. Try the download again.";

/// The sentence for a download without checksums.
const UNCHECKED: &str =
    "This download of the new lighting has no checksums, so the launcher cannot check it.";

/// Fetches `asset` of `offer` into `dir` and checks it against `sums`. The
/// file is deleted when it fails or does not match. An error is the
/// player's sentence, or `CANCELLED`; the details go to the log.
pub fn fetch(
    offer: &Offer,
    asset: &Asset,
    sums: &str,
    dir: &Path,
    progress: &Progress,
) -> Result<PathBuf, String> {
    let expected = install::expected_hash(sums, &asset.name).ok_or_else(|| {
        failed(
            UNCHECKED,
            format!("SHA256SUMS does not list {}", asset.name),
        )
    })?;
    let to = dir.join(&asset.name);
    let started = std::time::Instant::now();
    crate::log::line(format!(
        "lighting: fetches {} ({} bytes listed) into {}",
        asset.browser_download_url,
        asset.size,
        to.display()
    ));
    let actual = if offer.local {
        copy_hashing(Path::new(&asset.browser_download_url), &to, progress)
    } else {
        install::download(&asset.browser_download_url, &to, asset.size, progress)
    };
    let actual = match actual {
        Ok(actual) => actual,
        Err(failure) => {
            let _ = fs::remove_file(&to);
            crate::log::line(format!(
                "lighting: {} stopped after {} bytes and {:.1} s; the part was deleted",
                asset.name,
                progress.done.load(Ordering::Relaxed),
                started.elapsed().as_secs_f64()
            ));
            return Err(download_failed(failure, offer, dir));
        }
    };
    let bytes = fs::metadata(&to).map_or(0, |m| m.len());
    crate::log::line(format!(
        "lighting: {} has {bytes} bytes, fetched in {:.1} s; SHA-256 expected {expected}, found {actual}",
        asset.name,
        started.elapsed().as_secs_f64()
    ));
    if actual != expected {
        let _ = fs::remove_file(&to);
        return Err(failed(
            DAMAGED,
            format!("{} does not match SHA256SUMS", asset.name),
        ));
    }
    Ok(to)
}

/// The folder a game's files are downloaded into.
pub fn download_dir(downloads: &Path, game: Game) -> PathBuf {
    downloads.join(format!("lighting-{}", game.key()))
}

/// Deletes the folders of downloads that never ended, as when the launcher
/// was closed during one. A folder whose files kjk still reads cannot be
/// deleted and stays until the next download.
pub fn clean_downloads(downloads: &Path) {
    for game in GAMES {
        let dir = download_dir(downloads, game);
        if dir.exists() {
            let removed = fs::remove_dir_all(&dir);
            crate::log::line(format!(
                "lighting: an earlier download was left in {}; removing it: {removed:?}",
                dir.display()
            ));
        }
    }
}

/// Downloads the new lighting for one game from the first of `offers`
/// (newest first) and has `kjk` install it. The manifest comes first, and
/// kjk checks it against the player's files. The zip is downloaded only
/// when kjk allows the install. Whatever happens, the downloaded files are
/// deleted at the end. An error is the player's sentence, or `CANCELLED`;
/// the details are in the log.
pub fn download(
    kjk: &Path,
    offers: &[Offer],
    downloads: &Path,
    progress: &Progress,
) -> Result<Outcome, String> {
    progress.stage("Reading the list of areas");
    let first = offers.first().ok_or_else(|| {
        failed(
            "The new lighting is not available yet.",
            match source() {
                Some(dir) => format!("no download for this game in {}", dir.display()),
                None => "no lighting release offers this game".to_string(),
            },
        )
    })?;
    let started = std::time::Instant::now();
    crate::log::line(format!(
        "lighting: download for {} starts: {} from {}, with {}",
        first.game.key(),
        first.zip.name,
        if first.local {
            format!("the folder {}", source().unwrap_or_default().display())
        } else {
            format!("the release {}", first.tag)
        },
        kjk.display()
    ));
    let dir = download_dir(downloads, first.game);
    let made = (|| {
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        fs::create_dir_all(&dir)
    })();
    if let Err(error) = made {
        return Err(failed(
            write_failed(&error, downloads, first.zip.size),
            format!("{}: {error}", dir.display()),
        ));
    }
    let result = download_into(kjk, offers, &dir, progress);
    let _ = fs::remove_dir_all(&dir);
    crate::log::line(format!(
        "lighting: download for {} ended after {:.1} s: {}",
        first.game.key(),
        started.elapsed().as_secs_f64(),
        match &result {
            Ok(Outcome::Installed { areas, .. }) => format!("installed, {areas} areas"),
            Ok(other) => format!("{other:?}"),
            Err(why) => format!("failed: {why}"),
        }
    ));
    result
}

fn download_into(
    kjk: &Path,
    offers: &[Offer],
    dir: &Path,
    progress: &Progress,
) -> Result<Outcome, String> {
    let mut chosen = 0;
    let (offer, sums, manifest, facts, check) = loop {
        let offer = &offers[chosen];
        progress.items.store(false, Ordering::Relaxed);
        progress.stage("Reading the list of areas");
        let sums = sums_text(offer)?;
        let manifest = fetch(offer, &offer.manifest, &sums, dir, progress)?;
        let facts = facts(&fs::read_to_string(&manifest).unwrap_or_default());
        crate::log::line(format!("lighting: the manifest says {facts:?}"));
        if facts.game.as_deref() != Some(offer.game.key()) {
            return Err(failed(
                format!(
                    "This download is not the new lighting for {}.",
                    offer.game.name()
                ),
                format!("{} is for the game {:?}", offer.manifest.name, facts.game),
            ));
        }
        let check = run_check(kjk, &manifest, offer.game, progress)?;
        let mismatch = check.format_mismatch().filter(|_| !check.allowed());
        let Some((bake, reads)) = mismatch else {
            break (offer, sums, manifest, facts, check);
        };
        // A newer download than this kjk reads: an older release may have
        // the format it reads.
        if bake < reads {
            return Ok(Outcome::NeedsNewerDownload);
        }
        let older = (chosen + 1..offers.len())
            .find(|&i| format_of(&offers[i], dir, progress) == Some(reads));
        match older {
            Some(i) => chosen = i,
            None => return Ok(Outcome::NeedsNewerKjk),
        }
    };
    if !check.allowed() {
        let why = check.why.as_deref().map(game_names).unwrap_or_else(|| {
            format!(
                "The new lighting needs {}. Only {} of the {} areas match.",
                check.needs, check.matching, check.areas
            )
        });
        let why = format!("{why} Nothing was downloaded or installed.");
        return Ok(Outcome::Refused { why });
    }

    progress.items.store(false, Ordering::Relaxed);
    progress.stage("Downloading");
    let zip = fetch(offer, &offer.zip, &sums, dir, progress)?;
    // The manifest gives the zip's name and size too: a zip that is not the
    // one the manifest names is turned away before kjk reads it.
    let bytes = fs::metadata(&zip).map_or(0, |m| m.len());
    if let Some((name, size)) = &facts.archive {
        if *name != offer.zip.name || *size != bytes {
            let _ = fs::remove_file(&zip);
            return Err(failed(
                DAMAGED,
                format!(
                    "the manifest names {name} with {size} bytes; the download is {} with {bytes} bytes",
                    offer.zip.name
                ),
            ));
        }
    }

    progress.locked.store(true, Ordering::Relaxed);
    progress.items.store(true, Ordering::Relaxed);
    progress.total.store(0, Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);
    progress.stage("Installing");
    let args = [
        "lighting".as_ref(),
        "install".as_ref(),
        manifest.as_os_str(),
        zip.as_os_str(),
        "--json".as_ref(),
    ];
    let answer = run_kjk(kjk, &args, Some(progress), &mut |step| show(progress, step))?
        .ok_or_else(|| {
            failed(
                "This version of kjk cannot install the new lighting. Update kjk, then try again.",
                "kjk printed no answer to lighting install",
            )
        })?;
    let line = answer.games.first().ok_or_else(|| {
        failed(
            format!("kjk did not answer. {}", log_hint()),
            "kjk printed no game line to lighting install",
        )
    })?;
    match line.status.as_str() {
        "installed" => Ok(Outcome::Installed {
            areas: line.areas.unwrap_or(check.areas),
            check: Box::new(answer.check.unwrap_or(check)),
        }),
        "refused" => Ok(Outcome::Refused {
            why: game_names(line.why.as_deref().unwrap_or_default()),
        }),
        _ => Err(failed(
            match kind_words(line, Asked::Install, offer.game, bytes) {
                // Without a store there was no lighting before.
                Some(words) if line.kind.as_deref() == Some("no_store") => words,
                Some(words) => format!("{words} The lighting you had before is unchanged."),
                None => format!(
                    "kjk could not install it, and the lighting you had before is unchanged. {}",
                    log_hint()
                ),
            },
            detail(line),
        )),
    }
}

/// The format of `offer`'s manifest, fetched into `dir` and checked.
fn format_of(offer: &Offer, dir: &Path, progress: &Progress) -> Option<u32> {
    let sums = sums_text(offer).ok()?;
    let sub = dir.join(crate::config::file_safe(&offer.tag));
    fs::create_dir_all(&sub).ok()?;
    let path = fetch(offer, &offer.manifest, &sums, &sub, progress).ok()?;
    manifest_format(&fs::read_to_string(path).ok()?)
}

/// Shows kjk's progress line: the areas checked, then the areas unpacked.
fn show(progress: &Progress, step: &Step) {
    let stage = match step.step.as_str() {
        "unpack" => "Unpacking the areas",
        _ => "Checking your game files",
    };
    if progress.snapshot().0 != stage {
        progress.stage(stage);
    }
    progress.total.store(step.total, Ordering::Relaxed);
    progress.done.store(step.done, Ordering::Relaxed);
}

/// `kjk lighting check <manifest> --game <game> --json`, with its progress.
fn run_check(
    kjk: &Path,
    manifest: &Path,
    game: Game,
    progress: &Progress,
) -> Result<Check, String> {
    progress.items.store(true, Ordering::Relaxed);
    progress.total.store(0, Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);
    progress.stage("Checking your game files");
    let args = [
        "lighting".as_ref(),
        "check".as_ref(),
        manifest.as_os_str(),
        "--game".as_ref(),
        game.key().as_ref(),
        "--json".as_ref(),
    ];
    let answer = run_kjk(kjk, &args, Some(progress), &mut |step| show(progress, step))?
        .ok_or_else(|| {
            failed(
                "This version of kjk cannot install the new lighting. Update kjk, then try again.",
                "kjk printed no answer to lighting check",
            )
        })?;
    let Some(check) = answer.check else {
        let line = answer.games.first();
        return Err(failed(
            line.and_then(|line| kind_words(line, Asked::Check, game, 0))
                .unwrap_or_else(|| format!("kjk could not check your game files. {}", log_hint())),
            line.map_or_else(|| "kjk printed no check".into(), detail),
        ));
    };
    let fall_back: Vec<String> = check
        .fall_back
        .iter()
        .map(|f| format!("{}: {}", f.area, f.why))
        .collect();
    crate::log::line(format!(
        "lighting: the check says {}{}: {} of {} areas match, objects differ in {}, {} keep the game's lighting, needs {}, bake format {}, kjk reads format {}{}{}",
        check.status,
        check
            .kind
            .as_ref()
            .map_or(String::new(), |kind| format!(" ({kind})")),
        check.matching,
        check.areas,
        check.placeables_differ.len(),
        check.fall_back.len(),
        check.needs,
        check
            .bake_format
            .map_or("not given".to_string(), |f| f.to_string()),
        check
            .reads_format
            .map_or("not given".to_string(), |f| f.to_string()),
        if check.cached { ", from an earlier check" } else { "" },
        if fall_back.is_empty() {
            String::new()
        } else {
            format!("\n{}", fall_back.join("\n"))
        }
    ));
    Ok(check)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(name: &str) -> Asset {
        Asset {
            name: name.into(),
            browser_download_url: format!("https://example.invalid/{name}"),
            size: 7,
        }
    }

    fn release(tag: &str, published: &str, names: &[&str]) -> Release {
        Release {
            tag_name: tag.into(),
            name: None,
            body: None,
            prerelease: false,
            draft: false,
            published_at: Some(published.into()),
            html_url: String::new(),
            assets: names.iter().map(|n| asset(n)).collect(),
        }
    }

    fn full(label: &str) -> Vec<String> {
        vec![
            format!("kjk-lighting-k1-{label}.zip"),
            format!("kjk-lighting-k1-{label}.txt"),
            format!("kjk-lighting-tsl-{label}.zip"),
            format!("kjk-lighting-tsl-{label}.txt"),
            "SHA256SUMS".into(),
        ]
    }

    #[test]
    fn picks_the_newest_lighting_release() {
        let two = full("2");
        let ten = full("10");
        let two: Vec<&str> = two.iter().map(String::as_str).collect();
        let ten: Vec<&str> = ten.iter().map(String::as_str).collect();
        let list = vec![
            release("v0.20.0", "2026-12-01", &["kjk-v0.20.0-windows-x86_64.zip"]),
            release("lighting-2", "2026-11-25", &two),
            // Published earlier than lighting-2, but its number is higher.
            release("lighting-10", "2026-11-24", &ten),
            // Only KOTOR II's files.
            release(
                "lighting-11",
                "2026-11-26",
                &["kjk-lighting-tsl-11.zip", "kjk-lighting-tsl-11.txt"],
            ),
        ];
        let k1 = offers(&list, Game::K1);
        let tags: Vec<&str> = k1.iter().map(|o| o.tag.as_str()).collect();
        assert_eq!(tags, ["lighting-10", "lighting-2"]);
        assert_eq!(k1[0].zip.name, "kjk-lighting-k1-10.zip");
        assert_eq!(k1[0].manifest.name, "kjk-lighting-k1-10.txt");
        assert!(k1[0].sums.is_some());
        assert!(!k1[0].local);
        assert_eq!(offers(&list, Game::Tsl)[0].tag, "lighting-11");
        assert!(offers(&list, Game::Ja).is_empty());
    }

    #[test]
    fn a_draft_or_a_release_without_the_zip_is_skipped() {
        let mut draft = release(
            "lighting-3",
            "2026-11-25",
            &["kjk-lighting-k1-3.zip", "kjk-lighting-k1-3.txt"],
        );
        draft.draft = true;
        let no_zip = release("lighting-4", "2026-11-26", &["kjk-lighting-k1-4.txt"]);
        assert!(offers(&[draft, no_zip], Game::K1).is_empty());
    }

    #[test]
    fn a_newer_download_has_another_zip() {
        let list = vec![release(
            "lighting-2",
            "2026-11-25",
            &["kjk-lighting-k1-2.zip", "kjk-lighting-k1-2.txt"],
        )];
        let offer = &offers(&list, Game::K1)[0];
        let installed = |archive: &str| State::Installed {
            areas: 35,
            archive: archive.into(),
        };
        assert!(newer(&installed("kjk-lighting-k1-1.zip"), offer));
        assert!(!newer(&installed("kjk-lighting-k1-2.zip"), offer));
        assert!(!newer(&State::Missing, offer));
    }

    const PROGRESS: &str = r#"{"progress":{"step":"check","done":3,"total":35,"area":"m12aa"}}"#;

    const ALLOWED: &str = r#"{"check":{"game":"k1","status":"allowed","needs":"the K1 Community Patch","areas":35,"matching":34,"matched":["m01aa"],"placeables_differ":["m01ab"],"fall_back":[{"area":"m12aa","why":"the area's rooms or stored lightmaps in the game's files are not the ones the bake was made from"}],"cached":false,"why":null}}"#;

    const REFUSED: &str = r#"{"games":[{"game":"k1","status":"refused","why":"The new lighting needs KOTOR 1 with the K1 Community Patch installed. Only 2 of the 6 areas in this game's files match the files the lighting was made from, and at least 90% must match. Nothing was installed."}],"check":{"game":"k1","status":"refused","needs":"the K1 Community Patch","areas":6,"matching":2,"matched":["a","b"],"placeables_differ":[],"fall_back":[{"area":"c","why":"x"}],"cached":true,"why":"The new lighting needs KOTOR 1 with the K1 Community Patch installed."}}"#;

    #[test]
    fn reads_progress_and_check_lines() {
        assert_eq!(
            parse_line(PROGRESS),
            Some(Line::Progress(Step {
                step: "check".into(),
                done: 3,
                total: 35,
                area: "m12aa".into(),
            }))
        );
        let Some(Line::Answer(answer)) = parse_line(ALLOWED) else {
            panic!("no answer");
        };
        let check = answer.check.unwrap();
        assert!(check.allowed());
        assert_eq!((check.matching, check.areas), (34, 35));
        assert_eq!(check.fall_back[0].area, "m12aa");
        assert_eq!(check.placeables_differ, ["m01ab"]);
        assert_eq!(check.why, None);
        assert_eq!(check.format_mismatch(), None);

        let Some(Line::Answer(answer)) = parse_line(REFUSED) else {
            panic!("no answer");
        };
        assert_eq!(answer.games[0].status, "refused");
        let check = answer.check.unwrap();
        assert!(!check.allowed());
        assert!(check.cached);
        assert!(check.why.unwrap().contains("K1 Community Patch"));

        for text in ["", "error: unrecognized subcommand 'lighting'", "{}", "[1]"] {
            assert_eq!(parse_line(text), None, "{text}");
        }
    }

    #[test]
    fn tells_a_format_mismatch() {
        let line = r#"{"check":{"game":"tsl","status":"refused","needs":"TSLRCM","areas":2,"matching":0,"fall_back":[{"area":"101per","why":"the bake's format is 3, and this game reads format 2"}]}}"#;
        let Some(Line::Answer(answer)) = parse_line(line) else {
            panic!("no answer");
        };
        assert_eq!(answer.check.unwrap().format_mismatch(), Some((3, 2)));
    }

    #[test]
    fn reads_the_formats_kjk_gives() {
        // A newer kjk gives both formats, and the sentences are not read.
        let line = r#"{"check":{"game":"tsl","status":"refused","needs":"TSLRCM","areas":2,"matching":0,"fall_back":[{"area":"101per","why":"other words"}],"kind":"bake_format","bake_format":3,"reads_format":2}}"#;
        let Some(Line::Answer(answer)) = parse_line(line) else {
            panic!("no answer");
        };
        let check = answer.check.unwrap();
        assert_eq!(check.format_mismatch(), Some((3, 2)));
        assert_eq!(check.kind.as_deref(), Some("bake_format"));
        // The same format is no mismatch, whatever a sentence says.
        let same = Check {
            bake_format: Some(2),
            reads_format: Some(2),
            fall_back: vec![FallBack {
                area: "101per".into(),
                why: "the bake's format is 3, and this game reads format 2".into(),
            }],
            ..check
        };
        assert_eq!(same.format_mismatch(), None);
    }

    fn failed_line(kind: Option<&str>, store: Option<&str>) -> GameLine {
        GameLine {
            game: Some("tsl".into()),
            status: "failed".into(),
            areas: None,
            archive: None,
            why: Some("C:\\s\\lighting\\tsl.partial: Access is denied. (os error 5)".into()),
            kind: kind.map(str::to_string),
            store: store.map(str::to_string),
        }
    }

    #[test]
    fn says_what_to_do_for_each_kind() {
        let words = |kind: &str, asked: Asked| {
            kind_words(
                &failed_line(Some(kind), Some("C:\\s")),
                asked,
                Game::Tsl,
                214_000_000,
            )
            .unwrap()
        };
        assert_eq!(
            words("store_not_writable", Asked::Install),
            "kjk is not allowed to write into the folder C:\\s. Check that the folder is not read-only, then try again."
        );
        assert!(
            words("store_not_writable", Asked::Remove).starts_with("kjk is not allowed to change")
        );
        assert!(words("disk_full", Asked::Install).contains("not enough free space"));
        assert!(words("in_use", Asked::Remove).contains("Close kjk"));
        assert!(words("game_not_found", Asked::Check).contains("kjk did not find KOTOR II."));
        assert!(words("list_invalid", Asked::Check).contains("Update kjk"));
        assert_eq!(words("zip_mismatch", Asked::Install), DAMAGED);
        assert_eq!(words("zip_damaged", Asked::Install), DAMAGED);
        assert!(words("wrong_game", Asked::Check).contains("for KOTOR II"));
        for kind in [
            "no_store",
            "store_not_writable",
            "disk_full",
            "in_use",
            "game_not_found",
            "game_unreadable",
            "list_unreadable",
            "list_invalid",
            "wrong_game",
            "zip_unreadable",
            "zip_mismatch",
            "zip_damaged",
        ] {
            let said = words(kind, Asked::Install);
            assert!(!said.contains("os error"), "{kind}: {said}");
        }
        // Without the store's folder, the folder is named in words.
        let line = failed_line(Some("store_not_writable"), None);
        let said = kind_words(&line, Asked::Install, Game::K1, 0).unwrap();
        assert!(
            said.contains("the folder where kjk keeps its files"),
            "{said}"
        );
        // An older kjk gives no kind, and a kind the launcher does not know
        // is said as before.
        assert_eq!(
            kind_words(&failed_line(None, None), Asked::Install, Game::K1, 0),
            None
        );
        assert_eq!(
            kind_words(
                &failed_line(Some("new_kind"), None),
                Asked::Install,
                Game::K1,
                0
            ),
            None
        );
    }

    #[test]
    fn names_the_games_as_the_launcher_does() {
        let old =
            "The new lighting needs KOTOR 2 with TSLRCM installed. Only 2 of the 6 areas match.";
        assert_eq!(
            game_names(old),
            "The new lighting needs KOTOR II with TSLRCM installed. Only 2 of the 6 areas match."
        );
        assert_eq!(
            game_names("The new lighting needs KOTOR 1 with the K1 Community Patch installed."),
            "The new lighting needs KOTOR with the K1 Community Patch installed."
        );
        let new = "The new lighting needs KOTOR II with TSLRCM installed.";
        assert_eq!(game_names(new), new);
    }

    #[test]
    fn reads_the_status() {
        let text = r#"{"games":[{"game":"k1","status":"installed","path":"C:\\s\\lighting\\k1","archive":"kjk-lighting-k1-1.zip","areas":171,"needs":"the K1 Community Patch"},{"game":"tsl","status":"missing"}]}"#;
        let status = parse_status(text);
        assert_eq!(
            status.state(Game::K1),
            Some(&State::Installed {
                areas: 171,
                archive: "kjk-lighting-k1-1.zip".into()
            })
        );
        assert_eq!(status.state(Game::Tsl), Some(&State::Missing));

        let broken =
            r#"{"games":[{"game":"k1","status":"broken","path":"x","why":"no manifest"}]}"#;
        assert_eq!(
            parse_status(broken).state(Game::K1),
            Some(&State::Broken("no manifest".into()))
        );

        // A failure that names no game counts for both.
        let failed = r#"{"games":[{"game":null,"status":"failed","why":"kjk found no folder for its store."}]}"#;
        let failed = parse_status(failed);
        assert!(matches!(failed.state(Game::Tsl), Some(State::Failed(_))));
        // A newer kjk gives the kind, and the player reads no setting's name.
        let no_store = r#"{"games":[{"game":null,"status":"failed","why":"kjk found no folder for its store. Set KJK_STORE.","kind":"no_store"}]}"#;
        assert_eq!(
            parse_status(no_store).state(Game::K1),
            Some(&State::Failed(NO_STORE.into()))
        );

        let usage = "error: unrecognized subcommand 'lighting'\n\nUsage: kjk.exe <COMMAND>\n";
        assert_eq!(parse_status(usage), Status::TooOld);
        assert_eq!(parse_status(""), Status::TooOld);
    }

    #[test]
    fn reads_the_manifest_format() {
        let text = "# comment\ngame k1\nformat 2\nneeds the K1 Community Patch\n";
        assert_eq!(manifest_format(text), Some(2));
        assert_eq!(manifest_format("game k1\n"), None);
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "kjk-launcher-lighting-{name}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sha256(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    /// A source folder with two labels of K1's download and SHA256SUMS.
    fn source_folder(name: &str) -> PathBuf {
        let dir = scratch(name);
        let mut sums = String::new();
        for (file, bytes) in [
            ("kjk-lighting-k1-1.txt", &b"game k1\nformat 2\n"[..]),
            ("kjk-lighting-k1-1.zip", &b"old zip"[..]),
            ("kjk-lighting-k1-2.txt", &b"game k1\nformat 2\n"[..]),
            ("kjk-lighting-k1-2.zip", &b"new zip"[..]),
        ] {
            fs::write(dir.join(file), bytes).unwrap();
            sums.push_str(&format!("{}  {file}\n", sha256(bytes)));
        }
        fs::write(dir.join("SHA256SUMS"), sums).unwrap();
        dir
    }

    #[test]
    fn takes_the_download_from_a_folder() {
        let dir = source_folder("source");
        let found = local_offers(&dir, Game::K1);
        assert_eq!(found.len(), 1);
        let offer = &found[0];
        assert!(offer.local);
        assert_eq!(offer.label, "2");
        assert_eq!(offer.zip.name, "kjk-lighting-k1-2.zip");
        assert_eq!(offer.zip.size, 7);
        assert!(local_offers(&dir, Game::Tsl).is_empty());

        let into = scratch("source-into");
        let sums = sums_text(offer).unwrap();
        let progress = Progress::default();
        let zip = fetch(offer, &offer.zip, &sums, &into, &progress).unwrap();
        assert_eq!(fs::read(&zip).unwrap(), b"new zip");
        assert_eq!(progress.done.load(Ordering::Relaxed), 7);
    }

    #[test]
    fn a_file_that_does_not_match_or_is_cancelled_is_deleted() {
        let dir = source_folder("bad");
        let offer = &local_offers(&dir, Game::K1)[0];
        let sums = sums_text(offer).unwrap();
        fs::write(dir.join("kjk-lighting-k1-2.zip"), b"changed").unwrap();
        let into = scratch("bad-into");
        let progress = Progress::default();
        let failed = fetch(offer, &offer.zip, &sums, &into, &progress);
        assert_eq!(failed.unwrap_err(), DAMAGED);
        assert!(!into.join("kjk-lighting-k1-2.zip").exists());

        progress.cancel.store(true, Ordering::Relaxed);
        let cancelled = fetch(offer, &offer.manifest, &sums, &into, &progress);
        assert_eq!(cancelled.unwrap_err(), "cancelled");
        assert!(fs::read_dir(&into).unwrap().next().is_none());
    }

    /// The whole flow with a real kjk, cancelled half way through the zip:
    /// `KJK_LIGHTING_TRIAL_KJK=<kjk.exe>` with `KJK_LIGHTING_SOURCE`,
    /// `KJK_STORE` and the game's folder set, then
    /// `cargo test -- --ignored cancelled_mid_download`.
    #[test]
    #[ignore]
    fn cancelled_mid_download_leaves_nothing() {
        let kjk = PathBuf::from(std::env::var_os("KJK_LIGHTING_TRIAL_KJK").unwrap());
        let store = PathBuf::from(std::env::var_os("KJK_STORE").unwrap());
        let offers = available(&[], Game::K1);
        assert!(!offers.is_empty(), "no K1 download in KJK_LIGHTING_SOURCE");
        let downloads = scratch("cancel-downloads");
        let progress = std::sync::Arc::new(Progress::default());
        let watcher = progress.clone();
        let stopper = std::thread::spawn(move || loop {
            let (stage, done, total) = watcher.snapshot();
            if stage == "Downloading" && total > 0 && done > total / 2 {
                watcher.cancel.store(true, Ordering::Relaxed);
                return (done, total);
            }
            std::thread::sleep(Duration::from_millis(1));
        });
        let result = download(&kjk, &offers, &downloads, &progress);
        let (done, total) = stopper.join().unwrap();
        eprintln!("cancelled at {done} of {total} bytes");
        assert_eq!(result.unwrap_err(), "cancelled");
        assert!(!download_dir(&downloads, Game::K1).exists());
        assert!(!store.join("lighting").join("k1").exists());
        assert!(!store.join("lighting").join("k1.partial").exists());
    }

    #[test]
    fn a_full_or_locked_disk_is_said_plainly() {
        let dir = Path::new("downloads");
        let full = std::io::Error::from_raw_os_error(if cfg!(windows) { 112 } else { 28 });
        let words = write_failed(&full, dir, 340_000_000);
        assert!(
            words.starts_with("There is not enough free space"),
            "{words}"
        );
        assert!(!words.contains("os error"), "{words}");
        let locked = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        let words = write_failed(&locked, dir, 1);
        assert!(words.contains("not allowed to write"), "{words}");
    }

    #[test]
    fn reads_the_manifest_facts() {
        let text = "# kjk's lighting download
game tsl
format 2
needs TSLRCM
archive kjk-lighting-tsl-1.zip blake3 25aa bytes 213876600
area 001ebo module 001ebo rooms a placeables b
area 002ebo module 002ebo rooms c placeables d
";
        assert_eq!(
            facts(text),
            Facts {
                game: Some("tsl".into()),
                format: Some(2),
                needs: Some("TSLRCM".into()),
                archive: Some(("kjk-lighting-tsl-1.zip".into(), 213_876_600)),
                areas: 2,
            }
        );
    }

    #[test]
    fn says_how_many_areas_match() {
        let Some(Line::Answer(answer)) = parse_line(ALLOWED) else {
            panic!("no answer");
        };
        let mut check = answer.check.unwrap();
        assert_eq!(
            matching_words(&check),
            "34 of the 35 areas match your game files."
        );
        check.matching = 35;
        assert_eq!(
            matching_words(&check),
            "All 35 areas match your game files."
        );
    }

    #[test]
    fn a_download_without_sha256sums_is_refused() {
        let dir = source_folder("no-sums");
        fs::remove_file(dir.join("SHA256SUMS")).unwrap();
        let offer = &local_offers(&dir, Game::K1)[0];
        assert_eq!(sums_text(offer).unwrap_err(), UNCHECKED);
    }
}
