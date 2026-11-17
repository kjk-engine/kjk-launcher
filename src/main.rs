// No console window behind the launcher on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

/// `kjk-launcher [-- args]`: opens the window; args after `--` (or all of
/// them) are passed to kjk when it is started.
fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let passed = match args.iter().position(|a| a == "--") {
        Some(at) => args[at + 1..].to_vec(),
        None => args,
    };
    kjk_launcher::ui::run(passed)
}
