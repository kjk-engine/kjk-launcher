//! A picture of the window, to check its look without a screen grab.
//! `KJK_LAUNCHER_SHOT=<file.png>` saves the window once it has drawn a few
//! frames and nothing is loading, then closes it.
//! `KJK_LAUNCHER_TAB=play|versions|settings` picks the tab the
//! window opens on. `KJK_LAUNCHER_MENU=1` opens the version menu, and
//! `KJK_LAUNCHER_MENU=ffmpeg` opens the "Install ffmpeg" dialog.
//! `KJK_LAUNCHER_MENU=move:<folder>` opens the dialog that asks to move
//! kjk's folder there (nothing moves unless that dialog is answered).
//! `KJK_LAUNCHER_MENU=type:<text>` opens the dialog for a typed folder path
//! with that text in it.
//! `KJK_LAUNCHER_SCROLL=<points>` scrolls the Versions and Settings pages
//! down that far. `KJK_LAUNCHER_KEYS="tab tab shift+tab enter"` replays
//! those keys one by one and saves a numbered picture after each.

use eframe::egui;
use std::path::PathBuf;
use std::sync::Arc;

/// Frames drawn before the picture is taken.
const SETTLE: u32 = 20;
/// Frames between two replayed keys, and how many frames after a key its
/// picture is taken.
const KEY_STEP: u32 = 8;
const KEY_SHOT: u32 = 5;

#[derive(Default)]
pub struct Shot {
    to: Option<PathBuf>,
    frames: u32,
    asked: bool,
    /// Keys `KJK_LAUNCHER_KEYS` replays, one every `KEY_STEP` frames.
    keys: Vec<(egui::Key, egui::Modifiers)>,
    /// The key to hand to the next frame's input.
    pending: Option<(egui::Key, egui::Modifiers)>,
    /// The frame the keys started on, once the window settled.
    start: Option<u32>,
}

impl Shot {
    /// The picture the environment asks for, if any.
    pub fn from_env() -> Shot {
        let keys = std::env::var("KJK_LAUNCHER_KEYS").unwrap_or_default();
        Shot {
            to: std::env::var_os("KJK_LAUNCHER_SHOT").map(PathBuf::from),
            keys: keys.split_whitespace().filter_map(key).collect(),
            ..Shot::default()
        }
    }

    /// Adds the key that is due to this frame's input, as a press and a
    /// release.
    pub fn input(&mut self, raw: &mut egui::RawInput) {
        let Some((key, modifiers)) = self.pending.take() else {
            return;
        };
        for pressed in [true, false] {
            raw.events.push(egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers,
            });
        }
    }

    /// Counts a frame. Once enough frames are drawn and nothing is loading
    /// (`busy`), it replays the keys of `KJK_LAUNCHER_KEYS` and saves a
    /// picture after each one (`<name>-01.png` and on), even while a key
    /// has started something that loads. When nothing loads any more, it
    /// saves the picture and closes the window.
    pub fn frame(&mut self, ctx: &egui::Context, busy: bool) {
        let Some(to) = self.to.clone() else {
            return;
        };
        self.frames += 1;
        ctx.request_repaint();
        if self.asked {
            return;
        }
        if self.start.is_none() && self.frames >= SETTLE && !busy {
            self.start = Some(self.frames);
        }
        let Some(start) = self.start else {
            return;
        };
        let since = self.frames - start;
        let step = (since / KEY_STEP) as usize;
        if step < self.keys.len() {
            match since % KEY_STEP {
                0 => self.pending = Some(self.keys[step]),
                KEY_SHOT => save(ctx, numbered(&to, step + 1), false),
                _ => {}
            }
            return;
        }
        if busy {
            return;
        }
        self.asked = true;
        save(ctx, to, true);
    }
}

/// `shot-03.png` for `shot.png` and 3.
fn numbered(to: &std::path::Path, n: usize) -> PathBuf {
    let stem = to.file_stem().unwrap_or_default().to_string_lossy();
    to.with_file_name(format!("{stem}-{n:02}.png"))
}

/// Saves the window's picture at `to`, and closes the window after with
/// `close`.
fn save(ctx: &egui::Context, to: PathBuf, close: bool) {
    let closing = ctx.clone();
    ctx.request_screenshot(move |image: Arc<egui::ColorImage>| {
        let [w, h] = image.size;
        let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
        match image::RgbaImage::from_raw(w as u32, h as u32, bytes).map(|img| img.save(&to)) {
            Some(Ok(())) => eprintln!("saved {}", to.display()),
            other => eprintln!("could not save {}: {other:?}", to.display()),
        }
        if close {
            closing.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        closing.request_repaint();
    });
}

/// A key of `KJK_LAUNCHER_KEYS`: tab, shift+tab, up, down, left, right,
/// enter, space or escape.
fn key(name: &str) -> Option<(egui::Key, egui::Modifiers)> {
    use egui::Key;
    let (shift, name) = match name.strip_prefix("shift+") {
        Some(rest) => (true, rest),
        None => (false, name),
    };
    let key = match name {
        "tab" => Key::Tab,
        "up" => Key::ArrowUp,
        "down" => Key::ArrowDown,
        "left" => Key::ArrowLeft,
        "right" => Key::ArrowRight,
        "enter" => Key::Enter,
        "space" => Key::Space,
        "escape" => Key::Escape,
        _ => return None,
    };
    let modifiers = if shift {
        egui::Modifiers::SHIFT
    } else {
        egui::Modifiers::NONE
    };
    Some((key, modifiers))
}

/// The tab named by `KJK_LAUNCHER_TAB`, lowercased.
pub fn tab() -> Option<String> {
    std::env::var("KJK_LAUNCHER_TAB")
        .ok()
        .map(|tab| tab.trim().to_ascii_lowercase())
}

/// Whether `KJK_LAUNCHER_MENU=1` asks for the version menu to be open.
pub fn menu_open() -> bool {
    std::env::var("KJK_LAUNCHER_MENU").is_ok_and(|v| v == "1")
}

/// Whether `KJK_LAUNCHER_MENU=ffmpeg` asks for the ffmpeg dialog to be open.
pub fn ffmpeg_open() -> bool {
    std::env::var("KJK_LAUNCHER_MENU").is_ok_and(|v| v == "ffmpeg")
}

/// How far `KJK_LAUNCHER_SCROLL` scrolls the page down, in points.
pub fn scroll() -> Option<f32> {
    std::env::var("KJK_LAUNCHER_SCROLL").ok()?.parse().ok()
}

/// The text in the dialog for a typed path that `KJK_LAUNCHER_MENU=type`
/// or `type:<text>` opens.
pub fn typed_open() -> Option<String> {
    let menu = std::env::var("KJK_LAUNCHER_MENU").ok()?;
    if menu == "type" {
        return Some(String::new());
    }
    menu.strip_prefix("type:").map(str::to_string)
}

/// The folder `KJK_LAUNCHER_MENU=move:<folder>` names.
pub fn move_to() -> Option<PathBuf> {
    let menu = std::env::var("KJK_LAUNCHER_MENU").ok()?;
    menu.strip_prefix("move:").map(PathBuf::from)
}
