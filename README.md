# kjk launcher

The kjk launcher is an optional program that installs and starts
[kjk](https://github.com/kjk-engine/kjk). It checks GitHub for a new version
of kjk each time it opens. It never installs or updates anything until you
click a button.

Made by a community contributor. The kjk project maintains it from version
0.2.0 on. It is released under the MIT licence (see `LICENSE`).

Version 0.1.0 cannot update itself. Replace it with 0.2.0 by hand once.

## What the launcher does

- The Play page shows the selected version of kjk and a Play button. A click
  on the version opens a menu of releases and pre-releases. A click on a
  version that is not installed downloads it and selects it.
- When a new version of kjk is out, the Play page says so. You can read the
  release notes, skip the version, or update.
- The "Before you play" list shows whether kjk finds KOTOR, KOTOR II and
  Jedi Academy, and whether ffmpeg is installed. Play stays disabled until
  kjk finds all three games. kjk plays without ffmpeg, but it skips the
  movies.
- "Choose folder" lets you pick a game's folder. kjk checks the folder
  first. The launcher saves it in kjk's settings only when kjk finds the game
  there.
- Each version of kjk is installed in its own folder. The launcher keeps the
  3 newest versions by default, so you can go back to an older one on the
  Versions page.
- Before a version starts for the first time, the launcher makes a backup of
  kjk's saves and settings. A newer version can change a save so that an
  older version cannot read it. The Backups page restores a backup.
- Each download is checked against the release's `SHA256SUMS` before it is
  unpacked.
- When a new version of the launcher is out, the bottom of the window says
  so. "Update launcher" downloads it, puts it in place of the old one, and
  starts it.
- The launcher works offline. It then says that it could not reach GitHub,
  and Play still works.

## Steam play time

Steam can count the time you play kjk under KOTOR or KOTOR II. Open the
Settings page and copy the line under "Steam play time". In Steam, open that
game's Properties and paste the line into Launch options. The line looks
like this:

```text
"C:\Games\kjk launcher\kjk-launcher.exe" --play %command%
```

When you press Play on that game in Steam, Steam starts the launcher. The
launcher starts kjk at once and opens no window. It waits until kjk exits, so
Steam counts the whole session. The launcher opens its window instead when
no version is installed, when a new version is offered, or when a game
folder is missing.

This has only been checked on Windows.

## Where things are

| What | Where |
|---|---|
| Installed versions | The install folder, one folder per version, such as `kjk-engine\0.19.0\`. The default is `%LOCALAPPDATA%\kjk-launcher\kjk-engine`. Settings has a button to change it. |
| Backups and `launcher.json` | `%LOCALAPPDATA%\kjk-launcher`, or the folder `KJK_LAUNCHER_HOME` names. |
| kjk's saves and settings | `%APPDATA%\kjk`. The launcher never moves them. `KJK_SAVES` and `KJK_SETTINGS` are honoured, as kjk honours them. |

A kjk package that you unzipped by hand into the install folder (as
`0.18.4\` or `kjk-v0.18.4-windows-x86_64\`) is listed too. The launcher never
removes such a folder by itself.

## Build

```bash
cargo build --release
```

The build makes `kjk-launcher` (the window) and `kjk-launcher-cli` (the same
actions for scripts and tests). `kjk-launcher-cli status` shows the
installed versions and whether an update is offered. `kjk-launcher -- <args>`
passes `<args>` to kjk when you press Play.

## Testing without touching your own install

Point these variables at scratch folders before a test run:

| Variable | What it changes |
|---|---|
| `KJK_LAUNCHER_HOME` | The launcher's versions, backups and `launcher.json`. |
| `KJK_SETTINGS` | kjk's settings file. |
| `KJK_SAVES` | kjk's saves folder. |
| `KJK_LAUNCHER_OFFLINE=1` | The launcher acts as if GitHub could not be reached. |
| `KJK_LAUNCHER_SHOT=<file.png>` | The launcher saves a picture of its window and closes. |
| `KJK_LAUNCHER_TAB=versions` | The window opens on that page (play, versions, backups or settings). |
| `KJK_LAUNCHER_MENU=1` | The version menu is open. `ffmpeg` opens the "Install ffmpeg" dialog instead. |
| `KJK_LAUNCHER_FRAME=windows` | The window has the Windows frame on any system. |
| `KJK_LAUNCHER_SELF_PRERELEASE=1` | The launcher offers its own pre-releases as updates. |

`tests/stub_kjk.sh` stands in for kjk's `installs` command in the tests.

Set `KJK_LAUNCHER_GITHUB_TOKEN` if you reach GitHub's limit for anonymous
checks (60 an hour). An unchanged list does not count against the limit,
because the launcher keeps the list's ETag.

## Fonts

The window uses Jost, Geist and Geist Mono. The font files are built into
the program. Their licence texts, `OFL-Jost.txt` and `OFL-Geist.txt`, are in
`assets/fonts` and in each package.
