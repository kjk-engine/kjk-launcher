//! The release list from GitHub, cached with its ETag so that checking
//! again costs nothing against the API's rate limit when nothing changed.

use crate::config::{write_atomic, Config, Paths, REPO, USER_AGENT};
use semver::Version;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fs;
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub html_url: String,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

/// The end of this computer's package name. A pre-release's package is
/// named by `git describe`, not by its tag, so the suffix is what matches.
pub fn package_suffix() -> &'static str {
    if cfg!(windows) {
        "-windows-x86_64.zip"
    } else if cfg!(target_os = "macos") {
        "-macos-aarch64.tar.gz"
    } else {
        "-linux-x86_64.tar.gz"
    }
}

impl Release {
    pub fn package(&self) -> Option<&Asset> {
        self.assets
            .iter()
            .find(|a| a.name.ends_with(package_suffix()))
    }

    pub fn sums(&self) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == "SHA256SUMS")
    }

    pub fn date(&self) -> &str {
        date(self.published_at.as_deref())
    }

    pub fn notes(&self) -> &str {
        self.body
            .as_deref()
            .map(str::trim)
            .filter(|b| !b.is_empty())
            .unwrap_or("(no release notes)")
    }
}

pub fn date(published_at: Option<&str>) -> &str {
    published_at.and_then(|p| p.get(..10)).unwrap_or("")
}

pub fn semver(tag: &str) -> Option<Version> {
    Version::parse(tag.strip_prefix('v').unwrap_or(tag)).ok()
}

/// Orders two versions by their numbers when both tags are version
/// numbers, else by when they were published (`pre-<branch>-…` tags).
pub fn compare(
    a_tag: &str,
    a_published: Option<&str>,
    b_tag: &str,
    b_published: Option<&str>,
) -> Ordering {
    match (semver(a_tag), semver(b_tag)) {
        (Some(a), Some(b)) => a.cmp(&b),
        _ => a_published.unwrap_or("").cmp(b_published.unwrap_or("")),
    }
}

pub struct Releases {
    pub list: Vec<Release>,
    /// GitHub answered (fresh or "not modified").
    pub online: bool,
    /// Why GitHub could not be asked.
    pub problem: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
struct Cache {
    etag: Option<String>,
    releases: Vec<Release>,
}

fn read_cache(paths: &Paths) -> Cache {
    fs::read_to_string(&paths.cache_file)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// The releases seen at the last check, without asking GitHub.
pub fn cached(paths: &Paths) -> Vec<Release> {
    read_cache(paths).releases
}

pub fn agent(timeout: Option<Duration>) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(8)))
        .timeout_global(timeout)
        .http_status_as_error(false)
        .user_agent(USER_AGENT)
        .build()
        .into()
}

/// Asks GitHub for the releases; falls back to the cache when offline.
pub fn fetch(paths: &Paths) -> Releases {
    let cache = read_cache(paths);
    match fetch_network(&cache) {
        Ok(Some((etag, list))) => {
            let fresh = Cache {
                etag,
                releases: list,
            };
            if let Ok(json) = serde_json::to_vec(&fresh) {
                let _ = write_atomic(&paths.cache_file, &json);
            }
            Releases {
                list: fresh.releases,
                online: true,
                problem: None,
            }
        }
        Ok(None) => Releases {
            list: cache.releases,
            online: true,
            problem: None,
        },
        Err(problem) => Releases {
            list: cache.releases,
            online: false,
            problem: Some(problem),
        },
    }
}

/// A fresh list and its ETag; `None` when GitHub says nothing changed.
type Fresh = Option<(Option<String>, Vec<Release>)>;

fn fetch_network(cache: &Cache) -> Result<Fresh, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=100");
    let mut request = agent(Some(Duration::from_secs(10)))
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if !cache.releases.is_empty() {
        if let Some(etag) = &cache.etag {
            request = request.header("If-None-Match", etag);
        }
    }
    if let Ok(token) = std::env::var("KJK_LAUNCHER_GITHUB_TOKEN") {
        request = request.header("Authorization", &format!("Bearer {token}"));
    }
    let mut response = request
        .call()
        .map_err(|e| format!("could not reach GitHub ({e})"))?;
    match response.status().as_u16() {
        304 => Ok(None),
        200 => {
            let etag = response
                .headers()
                .get("etag")
                .and_then(|v| v.to_str().ok())
                .map(String::from);
            let text = response
                .body_mut()
                .with_config()
                .limit(64 * 1024 * 1024)
                .read_to_string()
                .map_err(|e| format!("could not read GitHub's answer ({e})"))?;
            let list: Vec<Release> = serde_json::from_str(&text)
                .map_err(|e| format!("could not read GitHub's answer ({e})"))?;
            Ok(Some((etag, list)))
        }
        403 | 429 => Err("GitHub's rate limit was reached; try again in a while".into()),
        status => Err(format!("GitHub answered {status}")),
    }
}

/// Releases this computer can install under the chosen channel, newest first.
pub fn installable(list: &[Release], prereleases: bool) -> Vec<&Release> {
    let mut found: Vec<&Release> = list
        .iter()
        .filter(|r| !r.draft && r.package().is_some() && (prereleases || !r.prerelease))
        .collect();
    found.sort_by(|a, b| b.published_at.cmp(&a.published_at));
    found
}

pub fn newest<'a>(list: &[&'a Release]) -> Option<&'a Release> {
    list.iter().copied().max_by(|a, b| {
        compare(
            &a.tag_name,
            a.published_at.as_deref(),
            &b.tag_name,
            b.published_at.as_deref(),
        )
    })
}

/// The release to offer: the newest under the channel, when it is newer
/// than `current` (tag, publish date), not skipped, and not pinned.
pub fn offer<'a>(
    list: &'a [Release],
    config: &Config,
    current: Option<(&str, Option<&str>)>,
) -> Option<&'a Release> {
    if config.pinned {
        return None;
    }
    let newest = newest(&installable(list, config.prereleases))?;
    if config.skipped.as_deref() == Some(newest.tag_name.as_str()) {
        return None;
    }
    match current {
        None => Some(newest),
        Some((tag, published)) => {
            let newer = compare(
                &newest.tag_name,
                newest.published_at.as_deref(),
                tag,
                published,
            ) == Ordering::Greater;
            newer.then_some(newest)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, published: &str, prerelease: bool) -> Release {
        Release {
            tag_name: tag.into(),
            name: None,
            body: None,
            prerelease,
            draft: false,
            published_at: Some(published.into()),
            html_url: String::new(),
            assets: vec![Asset {
                name: format!("kjk-{tag}{}", package_suffix()),
                browser_download_url: String::new(),
                size: 0,
            }],
        }
    }

    #[test]
    fn offers_only_newer_stable() {
        let list = vec![
            release("pre-feat-x-v0.19.0-3-gabc", "2026-11-17T10:00:00Z", true),
            release("v0.19.0", "2026-11-16T19:46:12Z", false),
            release("v0.18.5", "2026-11-15T18:55:16Z", false),
        ];
        let mut config = Config::default();
        let offered = offer(
            &list,
            &config,
            Some(("v0.18.5", Some("2026-11-15T18:55:16Z"))),
        );
        assert_eq!(offered.map(|r| r.tag_name.as_str()), Some("v0.19.0"));
        assert!(offer(&list, &config, Some(("v0.19.0", None))).is_none());

        config.prereleases = true;
        let offered = offer(
            &list,
            &config,
            Some(("v0.19.0", Some("2026-11-16T19:46:12Z"))),
        );
        assert_eq!(
            offered.map(|r| r.tag_name.as_str()),
            Some("pre-feat-x-v0.19.0-3-gabc")
        );

        config.skipped = Some("pre-feat-x-v0.19.0-3-gabc".into());
        assert!(offer(
            &list,
            &config,
            Some(("v0.19.0", Some("2026-11-16T19:46:12Z")))
        )
        .is_none());

        config.skipped = None;
        config.pinned = true;
        assert!(offer(&list, &config, Some(("v0.18.5", None))).is_none());
    }

    #[test]
    fn semver_beats_dates_for_backports() {
        // A v0.18.6 published after v0.19.0 is still older.
        assert_eq!(
            compare("v0.18.6", Some("2026-11-18"), "v0.19.0", Some("2026-11-16")),
            Ordering::Less
        );
        assert_eq!(
            compare("v0.19.0", None, "v0.19.0-rc.1", None),
            Ordering::Greater
        );
    }
}
