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
    let assets: Vec<Asset> = fs::read_dir(dir)
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
}

impl Check {
    pub fn allowed(&self) -> bool {
        self.status == "allowed"
    }

    /// The download's format and the format kjk reads, when they differ.
    /// kjk says so for each area that falls back: "the bake's format is 3,
    /// and this game reads format 2".
    pub fn format_mismatch(&self) -> Option<(u32, u32)> {
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
            _ => State::Failed(line.why.clone().unwrap_or_default()),
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

/// Runs kjk with `args`, hands each progress line to `step`, and returns
/// its last answer. With `cancellable`, a cancel stops kjk. `Ok(None)` when
/// kjk printed no answer: it is older than the command.
fn run_kjk(
    kjk: &Path,
    args: &[&std::ffi::OsStr],
    progress: Option<&Progress>,
    step: &mut dyn FnMut(&Step),
) -> Result<Option<Answer>, String> {
    let mut command = Command::new(kjk);
    command
        .args(args)
        .envs(crate::config::kjk_env())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    crate::run::no_window(&mut command);
    let mut child = command
        .spawn()
        .map_err(|e| format!("kjk could not start ({e})"))?;
    let stdout = child.stdout.take().ok_or("kjk could not start")?;
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
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(text) => match parse_line(&text) {
                Some(Line::Progress(found)) => step(&found),
                Some(Line::Answer(found)) => answer = Some(found),
                None => {}
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {
                let stop = progress.is_some_and(|p| {
                    p.cancel.load(Ordering::Relaxed) && !p.locked.load(Ordering::Relaxed)
                });
                if stop {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("cancelled".into());
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    let _ = child.wait();
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

/// `kjk lighting remove <game> --json`.
pub fn remove(kjk: &Path, game: Game) -> Result<(), String> {
    let args = ["lighting", "remove", game.key(), "--json"].map(std::ffi::OsStr::new);
    let answer = run_kjk(kjk, &args, None, &mut |_| {})?
        .ok_or("This version of kjk cannot remove the new lighting.")?;
    match answer.games.first() {
        Some(line) if line.status == "removed" || line.status == "missing" => Ok(()),
        Some(line) => Err(line.why.clone().unwrap_or_else(|| line.status.clone())),
        None => Err("kjk did not answer.".into()),
    }
}

/// How a download ended, when nothing failed.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// Installed, with this many areas, and what the check found.
    Installed { areas: usize, check: Check },
    /// kjk turned the download away, with its sentence for the player.
    /// Nothing but the manifest was downloaded.
    Refused { why: String },
    /// The download is newer than this kjk reads.
    NeedsNewerKjk,
    /// This kjk reads a newer download than any there is.
    NeedsNewerDownload,
}

/// The text of `SHA256SUMS` beside `offer`.
fn sums_text(offer: &Offer) -> Result<String, String> {
    let sums = offer
        .sums
        .as_ref()
        .ok_or("this download has no SHA256SUMS, so it cannot be checked")?;
    if offer.local {
        fs::read_to_string(&sums.browser_download_url)
            .map_err(|e| format!("{}: {e}", sums.browser_download_url))
    } else {
        install::get_text(&sums.browser_download_url)
    }
}

/// Copies the file at `from` to `to` and returns its SHA-256 in hex, as
/// `install::download` does for a URL.
fn copy_hashing(from: &Path, to: &Path, progress: &Progress) -> Result<String, String> {
    let mut reader = File::open(from).map_err(|e| format!("{}: {e}", from.display()))?;
    let total = reader.metadata().map_or(0, |m| m.len());
    progress.total.store(total, Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);
    let file = File::create(to).map_err(|e| format!("{}: {e}", to.display()))?;
    let mut file = BufWriter::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 256 * 1024];
    loop {
        if progress.cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let n = reader
            .read(&mut buffer)
            .map_err(|e| format!("{}: {e}", from.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
        file.write_all(&buffer[..n])
            .map_err(|e| format!("{}: {e}", to.display()))?;
        progress.done.fetch_add(n as u64, Ordering::Relaxed);
    }
    file.flush().map_err(|e| format!("{}: {e}", to.display()))?;
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

/// Fetches `asset` of `offer` into `dir` and checks it against `sums`. The
/// file is deleted when it fails or does not match.
pub fn fetch(
    offer: &Offer,
    asset: &Asset,
    sums: &str,
    dir: &Path,
    progress: &Progress,
) -> Result<PathBuf, String> {
    let expected = install::expected_hash(sums, &asset.name)
        .ok_or_else(|| format!("SHA256SUMS does not list {}", asset.name))?;
    let to = dir.join(&asset.name);
    let actual = if offer.local {
        copy_hashing(Path::new(&asset.browser_download_url), &to, progress)
    } else {
        install::download(&asset.browser_download_url, &to, asset.size, progress)
    };
    let actual = actual.inspect_err(|_| {
        let _ = fs::remove_file(&to);
    })?;
    if actual != expected {
        let _ = fs::remove_file(&to);
        return Err(format!(
            "{} does not match SHA256SUMS (expected {expected}, got {actual})",
            asset.name
        ));
    }
    Ok(to)
}

/// The folder a game's files are downloaded into.
pub fn download_dir(downloads: &Path, game: Game) -> PathBuf {
    downloads.join(format!("lighting-{}", game.key()))
}

/// Downloads the new lighting for one game from the first of `offers`
/// (newest first) and has `kjk` install it. The manifest comes first, and
/// kjk checks it against the player's files. The zip is downloaded only
/// when kjk allows the install. Whatever happens, the downloaded files are
/// deleted at the end.
pub fn download(
    kjk: &Path,
    offers: &[Offer],
    downloads: &Path,
    progress: &Progress,
) -> Result<Outcome, String> {
    let first = offers.first().ok_or("The new lighting is not available.")?;
    let dir = download_dir(downloads, first.game);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let result = download_into(kjk, offers, &dir, progress);
    let _ = fs::remove_dir_all(&dir);
    result
}

fn download_into(
    kjk: &Path,
    offers: &[Offer],
    dir: &Path,
    progress: &Progress,
) -> Result<Outcome, String> {
    let mut chosen = 0;
    let (offer, sums, manifest, check) = loop {
        let offer = &offers[chosen];
        progress.items.store(false, Ordering::Relaxed);
        progress.stage("Reading the list of areas");
        let sums = sums_text(offer)?;
        let manifest = fetch(offer, &offer.manifest, &sums, dir, progress)?;
        let check = run_check(kjk, &manifest, progress)?;
        let mismatch = check.format_mismatch().filter(|_| !check.allowed());
        let Some((bake, reads)) = mismatch else {
            break (offer, sums, manifest, check);
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
        let why = check.why.clone().unwrap_or_else(|| {
            format!(
                "The new lighting needs {}. Only {} of the {} areas match.",
                check.needs, check.matching, check.areas
            )
        });
        return Ok(Outcome::Refused { why });
    }

    progress.items.store(false, Ordering::Relaxed);
    progress.stage("Downloading");
    let zip = fetch(offer, &offer.zip, &sums, dir, progress)?;

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
        .ok_or("This version of kjk cannot install the new lighting.")?;
    let line = answer.games.first().ok_or("kjk did not answer.")?;
    match line.status.as_str() {
        "installed" => Ok(Outcome::Installed {
            areas: line.areas.unwrap_or(check.areas),
            check: answer.check.unwrap_or(check),
        }),
        "refused" => Ok(Outcome::Refused {
            why: line.why.clone().unwrap_or_default(),
        }),
        _ => Err(line.why.clone().unwrap_or_else(|| line.status.clone())),
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

fn show(progress: &Progress, step: &Step) {
    progress.total.store(step.total, Ordering::Relaxed);
    progress.done.store(step.done, Ordering::Relaxed);
}

/// `kjk lighting check <manifest> --json`, with its progress.
fn run_check(kjk: &Path, manifest: &Path, progress: &Progress) -> Result<Check, String> {
    progress.items.store(true, Ordering::Relaxed);
    progress.total.store(0, Ordering::Relaxed);
    progress.done.store(0, Ordering::Relaxed);
    progress.stage("Checking your game files");
    let args = [
        "lighting".as_ref(),
        "check".as_ref(),
        manifest.as_os_str(),
        "--json".as_ref(),
    ];
    let answer = run_kjk(kjk, &args, Some(progress), &mut |step| show(progress, step))?
        .ok_or("This version of kjk cannot check the new lighting.")?;
    match answer.check {
        Some(check) => Ok(check),
        None => Err(answer
            .games
            .first()
            .and_then(|line| line.why.clone())
            .unwrap_or_else(|| "kjk did not answer.".into())),
    }
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
        assert!(failed.unwrap_err().contains("does not match SHA256SUMS"));
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
    fn a_download_without_sha256sums_is_refused() {
        let dir = source_folder("no-sums");
        fs::remove_file(dir.join("SHA256SUMS")).unwrap();
        let offer = &local_offers(&dir, Game::K1)[0];
        assert!(sums_text(offer).unwrap_err().contains("SHA256SUMS"));
    }
}
