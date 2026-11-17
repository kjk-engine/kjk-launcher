//! `kjk-launcher-cli`: the same actions as the window, for scripts and tests.

use crate::backup;
use crate::config::{Config, Paths};
use crate::github::{self, Release};
use crate::install::{self, Progress};
use crate::launch;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

const HELP: &str = "\
kjk-launcher-cli <command>

  status              installed versions, and whether an update is offered
  list                releases this computer can install
  install [tag]       install a release (the newest by default) and play it from now on
  use <tag>           play an installed version from now on
  uninstall <tag>     delete an installed version
  folder [path]       show the install folder, or move the installs to another one
  play [-- args]      start the current version, passing args to kjk
  backup              back up kjk's saves and settings now
  backups             list backups
  restore <file>      put a backup's saves and settings back
  skip <tag>          do not offer this release again
  unskip              offer the skipped release again
  pin | unpin         stay on the current version, or offer updates again
  prereleases on|off  offer and list pre-releases too
  background <file> [credit] | none
                      a picture of your own for the Play tab, and who made it
";

pub fn run(args: Vec<String>) -> Result<(), String> {
    let (mut paths, mut config) = crate::config::open()?;
    install::clean_leftovers(&paths);
    let command = args.first().map(String::as_str).unwrap_or("help");
    let arg = args.get(1).map(String::as_str);
    match command {
        "status" | "check" => status(&paths, &config),
        "list" => list(&paths, &config),
        "install" => {
            let releases = github::fetch(&paths);
            if let Some(problem) = &releases.problem {
                eprintln!("note: {problem}; using the last list seen");
            }
            let installable = github::installable(&releases.list, true);
            let release = match arg {
                Some(tag) => installable.into_iter().find(|r| r.tag_name == tag),
                None => github::newest(&github::installable(&releases.list, config.prereleases)),
            }
            .ok_or("no such release for this computer")?
            .clone();
            if install::find(&paths, &release.tag_name).is_some() {
                config.current = Some(release.tag_name.clone());
                config.save(&paths)?;
                println!(
                    "{} is already installed; Play starts it from now on",
                    release.tag_name
                );
                return Ok(());
            }
            let installed = install_with_progress(&paths, &release)?;
            config.current = Some(installed.tag.clone());
            config.save(&paths)?;
            println!("installed {} in {}", installed.tag, installed.dir.display());
            for tag in install::prune(&paths, &config) {
                println!("removed old version {tag}");
            }
            Ok(())
        }
        "use" => {
            let tag = arg.ok_or("which version?")?;
            install::find(&paths, tag).ok_or_else(|| format!("{tag} is not installed"))?;
            config.current = Some(tag.into());
            config.save(&paths)?;
            println!("Play starts {tag} from now on");
            Ok(())
        }
        "remove" | "uninstall" => {
            let tag = arg.ok_or("which version?")?;
            install::remove(&paths, &mut config, tag)?;
            config.save(&paths)?;
            println!(
                "uninstalled {tag}; Play starts {}",
                config.current.as_deref().unwrap_or("nothing")
            );
            Ok(())
        }
        "folder" => {
            let Some(to) = arg else {
                println!("{}", paths.versions.display());
                return Ok(());
            };
            let to = std::path::PathBuf::from(to);
            let versions = install::installed(&paths);
            if let Some(inside) = versions.iter().find(|v| to.starts_with(&v.dir)) {
                return Err(format!(
                    "that folder is inside the install of {}",
                    inside.tag
                ));
            }
            let moved = install::move_installs(&versions, &to, &Progress::default());
            for problem in &moved.problems {
                eprintln!("note: {problem}");
            }
            config.install_dir = Some(to);
            config.save(&paths)?;
            paths.use_install_dir(&config)?;
            println!(
                "moved {} version(s); installing into {} from now on",
                moved.moved,
                paths.versions.display()
            );
            Ok(())
        }
        "play" => {
            let passed: Vec<String> = match args.iter().position(|a| a == "--") {
                Some(at) => args[at + 1..].to_vec(),
                None => args[1..].to_vec(),
            };
            let started = launch::play(&paths, &mut config, &passed)?;
            if let Some(backup) = started.backup {
                println!("backed up the saves to {}", backup.display());
            }
            println!("started kjk {}", started.tag);
            Ok(())
        }
        "backup" => {
            match backup::create(&paths, "manual")? {
                Some(path) => println!("backed up to {}", path.display()),
                None => println!("kjk has no saves or settings yet"),
            }
            Ok(())
        }
        "backups" => {
            for b in backup::list(&paths) {
                println!("{:>8.1} MB  {}", b.size as f64 / 1e6, b.name);
            }
            Ok(())
        }
        "restore" => {
            let name = arg.ok_or("which backup?")?;
            let path = backup::list(&paths)
                .into_iter()
                .find(|b| b.name == name || b.path.to_string_lossy() == name)
                .ok_or("no such backup")?
                .path;
            let safety = backup::restore(&paths, &path)?;
            println!("restored {name}");
            if let Some(safety) = safety {
                println!("the saves from before are in {}", safety.display());
            }
            Ok(())
        }
        "skip" => {
            config.skipped = Some(arg.ok_or("which release?")?.into());
            config.save(&paths)
        }
        "background" => {
            match arg {
                Some("none") | None => {
                    config.background = None;
                    config.background_credit = None;
                }
                Some(file) => {
                    config.background = Some(crate::config::import_background(
                        &paths,
                        std::path::Path::new(file),
                    )?);
                    config.background_credit =
                        args.get(2).cloned().filter(|c| !c.trim().is_empty());
                }
            }
            config.save(&paths)
        }
        "unskip" => {
            config.skipped = None;
            config.save(&paths)
        }
        "pin" | "unpin" => {
            config.pinned = command == "pin";
            config.save(&paths)
        }
        "prereleases" => {
            config.prereleases = match arg {
                Some("on") => true,
                Some("off") => false,
                _ => return Err("prereleases on|off".into()),
            };
            config.save(&paths)
        }
        _ => {
            print!("{HELP}");
            Ok(())
        }
    }
}

fn status(paths: &Paths, config: &Config) -> Result<(), String> {
    let installed = install::installed(paths);
    println!("launcher folder: {}", paths.root.display());
    println!("install folder:  {}", paths.versions.display());
    match &paths.kjk_data {
        Some(dir) => println!("kjk's folder:    {}", dir.display()),
        None => println!("kjk's folder:    (unknown)"),
    }
    println!(
        "current:         {}",
        config.current.as_deref().unwrap_or("(none)")
    );
    println!("pinned:          {}", config.pinned);
    println!("pre-releases:    {}", config.prereleases);
    for version in &installed {
        let note = if version.adopted {
            "found in the folder"
        } else {
            version.date()
        };
        println!(
            "  installed {} ({note}) in {}",
            version.tag,
            version.dir.display()
        );
    }
    let releases = github::fetch(paths);
    if let Some(problem) = &releases.problem {
        println!("could not check for updates: {problem}");
    }
    let current = config
        .current
        .as_deref()
        .and_then(|tag| installed.iter().find(|i| i.tag == tag))
        .map(|i| i.key());
    match github::offer(&releases.list, config, current) {
        Some(release) => println!("update offered:  {} ({})", release.tag_name, release.date()),
        None => println!("no update offered"),
    }
    Ok(())
}

fn list(paths: &Paths, config: &Config) -> Result<(), String> {
    let releases = github::fetch(paths);
    if let Some(problem) = &releases.problem {
        eprintln!("note: {problem}; using the last list seen");
    }
    let installed = install::installed(paths);
    for release in github::installable(&releases.list, config.prereleases) {
        let mut marks = Vec::new();
        if release.prerelease {
            marks.push("pre-release");
        }
        if installed.iter().any(|i| i.tag == release.tag_name) {
            marks.push("installed");
        }
        if config.current.as_deref() == Some(release.tag_name.as_str()) {
            marks.push("current");
        }
        println!(
            "{:<45} {}  {}",
            release.tag_name,
            release.date(),
            marks.join(", ")
        );
    }
    Ok(())
}

fn install_with_progress(paths: &Paths, release: &Release) -> Result<install::Installed, String> {
    let progress = Arc::new(Progress::default());
    let worker = {
        let (paths, release, progress) = (paths.clone(), release.clone(), progress.clone());
        std::thread::spawn(move || install::install(&paths, &release, &progress))
    };
    while !worker.is_finished() {
        let (stage, done, total) = progress.snapshot();
        let percent = (done * 100).checked_div(total).unwrap_or(0);
        eprint!(
            "\r{stage}: {:.1} / {:.1} MB ({percent}%)   ",
            done as f64 / 1e6,
            total as f64 / 1e6
        );
        let _ = std::io::stderr().flush();
        std::thread::sleep(Duration::from_millis(250));
    }
    eprintln!();
    worker
        .join()
        .map_err(|_| "the installer stopped unexpectedly".to_string())?
}
