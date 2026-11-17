# kjk-launcher

A launcher for [kjk](https://github.com/kjk-engine/kjk). It checks GitHub
for a newer release each time it opens and **offers** it. Nothing is
updated until you click Update, so a run in progress or a mod that needs a
certain version is never broken behind your back.

Made by a community contributor. The kjk project maintains it from
version 0.2.0 on. It is released under the MIT licence (see `LICENSE`).

- **Update / Skip this version / Not now**, with the release notes shown.
- **Versions side by side.** Each version gets its own folder; older ones
  stay installed (3 by default), so you can go back from the Versions tab.
- **Pin** (Settings) stays on the current version and stops the offers.
- **Saves are backed up** (saves folder + `settings.json`, as a zip) before
  a version starts for the first time. A newer kjk may migrate saves so
  that older versions can't read them; restoring that backup is the way
  back. Restoring backs up the present saves first.
- **Downloads are checked** against the release's `SHA256SUMS`, unpacked
  into a `.partial` folder and only then renamed into place.
- Offline or rate-limited: the launcher says so and Play still works.
- Pre-releases: off by default (Settings or Versions tab).

## Where things are

| what | where |
|---|---|
| installed versions | the install folder you choose (Play tab → Change…), one subfolder per version: `kjk-engine\0.19.0\`, `kjk-engine\0.18.5\`. Default `%LOCALAPPDATA%\kjk-launcher\kjk-engine` |
| backups, `launcher.json` | `%LOCALAPPDATA%\kjk-launcher` (or `KJK_LAUNCHER_HOME`) |

Changing the install folder offers to move the installed versions along.
A kjk zip unzipped by hand into the install folder (as `0.18.4\` or
`kjk-v0.18.4-windows-x86_64\`) is found and listed too; the launcher never
removes such a folder by itself. Pick a version from the drop-down above
Play; each entry there has an Uninstall button.
| kjk's own saves and settings (never moved) | `%APPDATA%\kjk` (`KJK_SAVES` is honoured, as kjk does) |

## Build

```bash
cargo build --release
```

Gives `target/release/kjk-launcher.exe` (the window) and
`kjk-launcher-cli.exe` (the same actions for scripts and testing):

```bash
kjk-launcher-cli status
```

`kjk-launcher -- <args>` passes `<args>` to kjk when you press Play.

## Testing without touching your real install

```powershell
$env:KJK_LAUNCHER_HOME = "C:\temp\kl\home"   # launcher's versions/backups
$env:APPDATA = "C:\temp\kl\appdata"          # where it looks for kjk's saves
.\target\release\kjk-launcher-cli.exe install v0.18.5
.\target\release\kjk-launcher.exe            # offers the newest release
```

Set `KJK_LAUNCHER_GITHUB_TOKEN` if you hit GitHub's anonymous rate limit
(60 checks an hour; unchanged lists are answered from the ETag cache).
