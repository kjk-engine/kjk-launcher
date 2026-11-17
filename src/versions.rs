//! The version menu on the Play page: releases and pre-releases, each
//! newest first, with whether each one is selected, installed or not.

use crate::github::{self, compare, Release};
use crate::install::Installed;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Selected,
    Installed,
    NotInstalled,
}

#[derive(Clone, Debug)]
pub struct Row {
    pub tag: String,
    pub state: State,
    /// The newest release. Only the first row of the releases has it.
    pub newest: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Menu {
    pub releases: Vec<Row>,
    pub prereleases: Vec<Row>,
}

/// Both groups newest first. Pre-releases that are not installed are listed
/// only when `prereleases` is on. Installed versions are always listed,
/// also when GitHub no longer lists them (or they were unzipped by hand),
/// in the group their tag fits.
pub fn menu(
    list: &[Release],
    installed: &[Installed],
    current: Option<&str>,
    prereleases: bool,
) -> Menu {
    let is_installed = |tag: &str| installed.iter().any(|v| v.tag == tag);
    // (tag, publish date, pre-release)
    let mut all: Vec<(String, Option<String>, bool)> = github::installable(list, true)
        .into_iter()
        .filter(|r| prereleases || !r.prerelease || is_installed(&r.tag_name))
        .map(|r| (r.tag_name.clone(), r.published_at.clone(), r.prerelease))
        .collect();
    for version in installed {
        if !all.iter().any(|(tag, ..)| *tag == version.tag) {
            let prerelease = !is_release_tag(&version.tag);
            all.push((
                version.tag.clone(),
                version.published_at.clone(),
                prerelease,
            ));
        }
    }
    all.sort_by(|a, b| compare(&b.0, b.1.as_deref(), &a.0, a.1.as_deref()));
    let row = |tag: &str| Row {
        tag: tag.to_string(),
        state: if current == Some(tag) {
            State::Selected
        } else if is_installed(tag) {
            State::Installed
        } else {
            State::NotInstalled
        },
        newest: false,
    };
    let mut menu = Menu::default();
    for (tag, _, prerelease) in &all {
        let group = if *prerelease {
            &mut menu.prereleases
        } else {
            &mut menu.releases
        };
        group.push(row(tag));
    }
    if let Some(first) = menu.releases.first_mut() {
        first.newest = true;
    }
    menu
}

/// Whether a tag is a plain version number, such as `v0.19.0`.
fn is_release_tag(tag: &str) -> bool {
    github::semver(tag).is_some_and(|version| version.pre.is_empty())
}

/// "kjk 0.19.0" for "v0.19.0"; other tags as they are after "kjk ".
pub fn title(tag: &str) -> String {
    format!("kjk {}", number(tag))
}

/// "0.19.0" for "v0.19.0": a tag without the leading v when a number
/// follows it; other tags as they are.
pub fn number(tag: &str) -> &str {
    tag.strip_prefix('v')
        .filter(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
        .unwrap_or(tag)
}

/// A tag as the Versions page shows it: "0.19.0" for a plain version tag
/// such as "v0.19.0", other tags as they are.
pub fn shown_tag(tag: &str) -> &str {
    if is_release_tag(tag) {
        number(tag)
    } else {
        tag
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::{package_suffix, Asset};
    use std::path::PathBuf;

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

    fn installed(tag: &str) -> Installed {
        Installed {
            tag: tag.into(),
            published_at: None,
            prerelease: false,
            installed_at: String::new(),
            dir: PathBuf::from(tag),
            adopted: true,
        }
    }

    fn list() -> Vec<Release> {
        vec![
            release("v0.19.1", "2026-11-17T09:00:00Z", false),
            release("v0.19.0", "2026-11-16T19:46:12Z", false),
            release("v0.18.5", "2026-11-15T18:55:16Z", false),
            release("v0.20.0-rc.1", "2026-11-17T08:00:00Z", true),
            release("pre-feat-x-v0.19.0-3-gabc", "2026-11-17T10:00:00Z", true),
        ]
    }

    fn on_disk() -> Vec<Installed> {
        ["v0.19.0", "v0.18.5", "pre-feat-x-v0.19.0-3-gabc"]
            .map(installed)
            .to_vec()
    }

    #[test]
    fn groups_releases_and_prereleases_newest_first() {
        let m = menu(&list(), &on_disk(), Some("v0.19.0"), true);
        let tags: Vec<_> = m.releases.iter().map(|r| r.tag.as_str()).collect();
        assert_eq!(tags, ["v0.19.1", "v0.19.0", "v0.18.5"]);
        assert!(m.releases[0].newest && matches!(m.releases[0].state, State::NotInstalled));
        assert!(matches!(m.releases[1].state, State::Selected));
        assert!(matches!(m.releases[2].state, State::Installed));
        assert_eq!(m.prereleases.len(), 2);
    }

    #[test]
    fn offline_menu_still_lists_installed_versions() {
        let m = menu(&[], &on_disk(), Some("v0.19.0"), false);
        assert_eq!(m.releases.len(), 2);
    }

    #[test]
    fn prereleases_off_keeps_only_installed_prereleases() {
        let m = menu(&list(), &on_disk(), Some("v0.19.0"), false);
        let tags: Vec<_> = m.prereleases.iter().map(|r| r.tag.as_str()).collect();
        assert_eq!(tags, ["pre-feat-x-v0.19.0-3-gabc"]);
        assert_eq!(m.releases.len(), 3);
    }

    #[test]
    fn shown_tags() {
        for (tag, shown) in [
            ("v0.19.0", "0.19.0"),
            ("v0.20.0-rc.1", "v0.20.0-rc.1"),
            ("pre-feat-x-v0.19.0-3-gabc", "pre-feat-x-v0.19.0-3-gabc"),
        ] {
            assert_eq!(shown_tag(tag), shown);
        }
    }

    #[test]
    fn titles() {
        assert_eq!(title("v0.19.0"), "kjk 0.19.0");
        assert_eq!(
            title("pre-feat-x-v0.19.0-3-gabc"),
            "kjk pre-feat-x-v0.19.0-3-gabc"
        );
    }
}
