//! The game folders in kjk's own settings file (`install.k1`, `install.tsl`
//! and `install.ja`), the keys the game and the save editor read.

use crate::config::write_atomic;
use crate::kjk_report::Game;
use std::collections::BTreeMap;
use std::path::Path;

/// Sets `install.<game>` in kjk's settings file and keeps every other key.
/// A missing or empty file is created. A file that is not a JSON object of
/// strings is left untouched and the error says so.
pub fn set_install(file: &Path, game: Game, folder: &Path) -> Result<(), String> {
    let text = match std::fs::read_to_string(file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            return Err(format!(
                "The launcher could not read {}. {e}",
                file.display()
            ))
        }
    };
    let mut values: BTreeMap<String, String> = if text.trim().is_empty() {
        BTreeMap::new()
    } else {
        serde_json::from_str(&text).map_err(|_| {
            format!(
                "The launcher could not read {}. It did not change the file.",
                file.display()
            )
        })?
    };
    let folder = folder
        .to_str()
        .ok_or("The folder's name cannot be saved.")?;
    values.insert(format!("install.{}", game.key()), folder.to_string());
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(&values).map_err(|e| e.to_string())?;
    write_atomic(file, json.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A settings file in a fresh temporary folder, with `text` in it unless
    /// it is `None`.
    fn settings_file(name: &str, text: Option<&str>) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("kjk-launcher-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.json");
        if let Some(text) = text {
            std::fs::write(&file, text).unwrap();
        }
        file
    }

    fn read(file: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
    }

    #[test]
    fn sets_one_key_and_keeps_the_rest() {
        let file = settings_file("keep", Some(r#"{"mouse.speed":"1.5","install.k1":"old"}"#));
        set_install(&file, Game::K1, Path::new("/games/kotor")).unwrap();
        let json = read(&file);
        assert_eq!(json["mouse.speed"], "1.5");
        assert_eq!(json["install.k1"], "/games/kotor");
    }

    #[test]
    fn creates_a_missing_file() {
        let file = settings_file("missing", None);
        set_install(&file, Game::Ja, Path::new("/games/ja")).unwrap();
        assert_eq!(read(&file), serde_json::json!({"install.ja": "/games/ja"}));
    }

    #[test]
    fn refuses_a_file_it_cannot_read() {
        for text in ["not json", "[1, 2]", r#"{"volume": 3}"#] {
            let file = settings_file("refuse", Some(text));
            assert!(
                set_install(&file, Game::Tsl, Path::new("/x")).is_err(),
                "{text}"
            );
            assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
        }
    }
}
