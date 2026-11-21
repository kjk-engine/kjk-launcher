// No console window behind the launcher on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

use kjk_launcher::args::{self, Mode};

/// `kjk-launcher [-- args]` opens the window, and the args after `--` (or
/// all of them) go to kjk when it starts. `kjk-launcher --play ...` starts
/// kjk at once for Steam's launch options, and opens the window only when
/// the player is needed.
fn main() -> eframe::Result {
    kjk_launcher::launch::vulkan_alone_under_reshade();
    if let Some(home) = kjk_launcher::selfupdate::home() {
        kjk_launcher::selfupdate::clean(&home);
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args::parse(&args) {
        Mode::Window { pass } => kjk_launcher::ui::run(pass, None),
        Mode::Play => match kjk_launcher::launch::from_steam() {
            Ok(()) => Ok(()),
            Err(sentence) => kjk_launcher::ui::run(Vec::new(), Some(sentence)),
        },
    }
}
