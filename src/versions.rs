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

/// Both groups newest first. Installed versions that GitHub no longer lists
/// (or that were unzipped by hand) still appear, in the group their tag fits.
pub fn menu(list: &[Release], installed: &[Installed], current: Option<&str>) -> Menu {
    // (tag, publish date, pre-release)
    let mut all: Vec<(String, Option<String>, bool)> = github::installable(list, true)
        .into_iter()
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
        } else if installed.iter().any(|v| v.tag == tag) {
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
    let bare = tag
        .strip_prefix('v')
        .filter(|rest| rest.starts_with(|c: char| c.is_ascii_digit()));
    format!("kjk {}", bare.unwrap_or(tag))
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
        let m = menu(&list(), &on_disk(), Some("v0.19.0"));
        let tags: Vec<_> = m.releases.iter().map(|r| r.tag.as_str()).collect();
        assert_eq!(tags, ["v0.19.1", "v0.19.0", "v0.18.5"]);
        assert!(m.releases[0].newest && matches!(m.releases[0].state, State::NotInstalled));
        assert!(matches!(m.releases[1].state, State::Selected));
        assert!(matches!(m.releases[2].state, State::Installed));
        assert_eq!(m.prereleases.len(), 2);
    }

    #[test]
    fn offline_menu_still_lists_installed_versions() {
        let m = menu(&[], &on_disk(), Some("v0.19.0"));
        assert_eq!(m.releases.len(), 2);
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
