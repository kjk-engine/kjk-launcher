# kjk launcher

The kjk launcher is an optional program that installs and starts
[kjk](https://github.com/kjk-engine/kjk). It checks GitHub for a new version
of kjk each time it opens. It never installs or updates anything until you
click a button.

[dansan96](https://github.com/dansan96) started the launcher and wrote version
0.1.0. The kjk project rebuilt it as version 0.2.0 and maintains it from there.
It is released under the MIT licence (see `LICENSE`).

Version 0.1.0 cannot update itself. Replace it with 0.2.0 by hand once.

## What the launcher does

- The Play page shows the selected version of kjk and a Play button. A click
  on the version opens a menu of releases and pre-releases. A click on a
  version that is not installed downloads it and selects it.
- When a new version of kjk is out, the Play page says so. You can read the
  release notes, skip the version, or update.
- The "Setup" list shows whether kjk finds KOTOR, KOTOR II and
  Jedi Academy. Play stays disabled until kjk finds all three games.
- Under the Setup list, the Play page shows the release notes of the
  selected version.
- "Choose folder" lets you pick a game's folder. kjk checks the folder
  first. The launcher saves it in kjk's settings only when kjk finds the game
  there.
- Each version of kjk is installed in its own folder, so you can go back to
  an older one on the Versions page. The launcher never deletes a version by
  itself. "Delete older versions" in Settings deletes every installed
  version except the selected one.
- Before a version starts for the first time, the launcher makes a backup of
  kjk's saves and settings. A newer version can change a save so that an
  older version cannot read it. Settings lists the backups and restores one.
- Each download is checked against the release's `SHA256SUMS` before it is
  unpacked.
- When a new version of the launcher is out, the bottom of the window says
  so. "Update launcher" downloads it, puts it in place of the old one, and
  starts it.
- The launcher works offline. It then says that it could not check GitHub
  for updates, and Play still works.

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

## ReShade

ReShade works on Windows. Set it up once, and it stays set up when kjk
updates.

1. Run ReShade's setup.
2. When it asks for a game, choose `kjk-launcher.exe`.
3. When it asks for the rendering API, choose Vulkan. kjk draws with
   Vulkan, so the other choices do nothing in kjk.
4. Start kjk with the launcher's Play button.

ReShade's setup puts `ReShade.ini`, your presets and the `reshade-shaders`
folder beside `kjk-launcher.exe`. When that `ReShade.ini` is there, the
launcher tells ReShade to use that folder for kjk, and the launcher and kjk
draw with Vulkan alone. ReShade also draws in the launcher's own window.

ReShade writes two logs in that folder: `ReShade.log` for the launcher, and
one for kjk (`ReShade2.log` with ReShade 5, `ReShade.log1` with ReShade 6).

ReShade 5.8.0 and 6.8.0 work with kjk. If ReShade stops kjk from starting,
send both logs with your report.

## Steam Deck

The launcher can be used with the Steam Deck's controls alone. Add the
launcher to Steam as a non-Steam game. Open its controller settings in Steam
and pick a layout for keyboard and mouse, then set the buttons like this:

- The D-pad sends the arrow keys.
- The A button sends Enter.
- The B button sends Escape.
- The right trackpad moves the mouse.

When the launcher opens, Play has the focus, so pressing A starts the game.
When no version is installed yet, the install button has the focus instead.
The arrow keys move the focus between the buttons, and Tab and Shift+Tab do
the same in a fixed order. A white outline shows which button has the focus.
Enter or Space presses it. Escape closes the version menu or the open dialog.

Gaming Mode has no folder dialog. When the launcher cannot open one, it asks
you to type or paste the folder's full path instead. On Linux, a "Type a
path" button sits next to each button that chooses a folder, because the
launcher cannot always tell a missing folder dialog from a closed one.

## Where things are

Everything is in one folder, the kjk folder. It is `%APPDATA%\kjk` on
Windows, `~/Library/Application Support/kjk` on macOS and
`~/.config/kjk` on Linux.

| What | Where in the kjk folder |
|---|---|
| kjk's saves, settings and logs | `saves\`, `settings.json` and kjk's own files. `KJK_SAVES` and `KJK_SETTINGS` are honoured, as kjk honours them. |
| Backups | `backups\` |
| Installed versions | `versions\`, one folder per version, such as `versions\0.19.0\`. Settings has a button to choose another install folder. |
| The launcher's settings, release lists and downloads | `launcher\` |

Settings has a "Move" button that moves the whole kjk folder to another
place, and the launcher then writes the new place into `moved-to.txt` in the
default folder. "Use the default folder" moves everything back.

The launcher 0.1.0 kept its files in `%LOCALAPPDATA%\kjk-launcher`. The
first time a newer launcher starts, it moves them into the kjk folder. When
`KJK_LAUNCHER_HOME` is set, the launcher keeps its files in that folder as
0.1.0 did. `KJK_HOME` sets the kjk folder for both kjk and the launcher.

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
| `KJK_HOME` | The kjk folder. The "Move" button is off while it is set. |
| `KJK_SETTINGS` | kjk's settings file. |
| `KJK_SAVES` | kjk's saves folder. |
| `KJK_LAUNCHER_OFFLINE=1` | The launcher acts as if GitHub could not be reached. |
| `KJK_LAUNCHER_SHOT=<file.png>` | The launcher saves a picture of its window and closes. |
| `KJK_LAUNCHER_TAB=versions` | The window opens on that page (play, versions or settings). |
| `KJK_LAUNCHER_MENU=1` | The version menu is open. `move:<folder>` opens the dialog that asks to move the kjk folder there instead. `type:<text>` opens the dialog for a typed folder path with that text in it. |
| `KJK_LAUNCHER_KEYS="tab tab enter"` | With `KJK_LAUNCHER_SHOT`, the launcher presses these keys one after another and saves a numbered picture after each. It knows tab, shift+tab, up, down, left, right, enter, space and escape. |
| `KJK_LAUNCHER_TYPE_PATHS=1` | Every button that chooses a folder opens the launcher's dialog for a typed path instead of the system's folder dialog. |
| `KJK_LAUNCHER_SCROLL=400` | The Versions or Settings page is scrolled down that many points. |
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
