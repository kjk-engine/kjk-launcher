//! A picture of the window, to check its look without a screen grab.
//! `KJK_LAUNCHER_SHOT=<file.png>` saves the window once it has drawn a few
//! frames and nothing is loading, then closes it.
//! `KJK_LAUNCHER_TAB=play|versions|settings` picks the tab the
//! window opens on. `KJK_LAUNCHER_MENU=1` opens the version menu, and
//! `KJK_LAUNCHER_MENU=ffmpeg` opens the "Install ffmpeg" dialog.
//! `KJK_LAUNCHER_SCROLL=<points>` scrolls the Versions and Settings pages
//! down that far.

use eframe::egui;
use std::path::PathBuf;
use std::sync::Arc;

/// Frames drawn before the picture is taken.
const SETTLE: u32 = 20;

#[derive(Default)]
pub struct Shot {
    to: Option<PathBuf>,
    frames: u32,
    asked: bool,
}

impl Shot {
    /// The picture the environment asks for, if any.
    pub fn from_env() -> Shot {
        Shot {
            to: std::env::var_os("KJK_LAUNCHER_SHOT").map(PathBuf::from),
            ..Shot::default()
        }
    }

    /// Counts a frame. Once enough frames are drawn and nothing is loading
    /// (`busy`), it asks for the picture, writes it and closes the window.
    pub fn frame(&mut self, ctx: &egui::Context, busy: bool) {
        let Some(to) = self.to.clone() else {
            return;
        };
        self.frames += 1;
        ctx.request_repaint();
        if self.frames < SETTLE || busy || self.asked {
            return;
        }
        self.asked = true;
        let closing = ctx.clone();
        ctx.request_screenshot(move |image: Arc<egui::ColorImage>| {
            let [w, h] = image.size;
            let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
            match image::RgbaImage::from_raw(w as u32, h as u32, bytes).map(|img| img.save(&to)) {
                Some(Ok(())) => eprintln!("saved {}", to.display()),
                other => eprintln!("could not save {}: {other:?}", to.display()),
            }
            closing.send_viewport_cmd(egui::ViewportCommand::Close);
            closing.request_repaint();
        });
    }
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
