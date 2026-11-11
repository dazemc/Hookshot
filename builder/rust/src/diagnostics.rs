//! One report for an issue: versions, paths, config, profiles and the last
//! lines of the game log. This is what the builder's Diagnostics button copies.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::profiles;
use crate::version_policy;

pub fn json(
    folder: &str,
    tools: Option<&str>,
    builder: Option<&str>,
    game: Option<&str>,
) -> Result<Value, String> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    let log = Path::new(folder).join("hookshot_log.txt");
    let log_text = fs::read_to_string(&log).unwrap_or_default();
    let tail: Vec<&str> = log_text
        .lines()
        .rev()
        .take(40)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let list = profiles::load(&profiles::user_dir(folder));
    let note = Path::new(folder).join("hookshot_damaged_files.txt");
    let env: Vec<(String, String)> = std::env::vars()
        .filter(|(key, _)| {
            key.starts_with("HOOKSHOT_") || key.starts_with("RX_VK_") || key.starts_with("HOS_")
        })
        .collect();
    Ok(json!({
        "builder": builder.unwrap_or(""),
        "game": game.unwrap_or(""),
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "home": home,
        "cwd": std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
        "tools": tools.unwrap_or(""),
        "tools_exists": tools.map(|p| Path::new(p).is_dir()).unwrap_or(false),
        "game_folder": folder,
        "game_ready": Path::new(folder).join("image.bin").is_file(),
        "version_file": Path::new(folder).join("hookshot_version.txt").to_string_lossy(),
        "version": read_version(folder),
        "pinned": version_policy::pinned(folder),
        "profiles": list.iter().map(|(id, name)| json!({"id": id, "name": name})).collect::<Vec<_>>(),
        "profiles_supported": profiles::supported(folder),
        "user_folder": profiles::user_dir(folder).to_string_lossy(),
        "save_folder": profiles::folder_to_show(&profiles::user_dir(folder), &profiles::DEFAULT.to_owned()).map(|p| p.to_string_lossy().into_owned()),
        "damaged_note": note.is_file(),
        "damaged_note_path": note.to_string_lossy(),
        "log_path": log.to_string_lossy(),
        "log_tail": tail,
        "config": config(),
        "environment": env,
    }))
}

fn read_version(folder: &str) -> String {
    fs::read_to_string(Path::new(folder).join("hookshot_version.txt"))
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix("version=").map(str::to_owned))
        })
        .unwrap_or_default()
}

fn config() -> Value {
    let path = config_path();
    let mut map = serde_json::Map::new();
    if let Ok(text) = fs::read_to_string(&path) {
        for line in text.lines() {
            if let Some((key, value)) = line.split_once('=') {
                map.insert(key.to_owned(), json!(value));
            }
        }
    }
    json!({"path": path.to_string_lossy(), "values": map})
}

fn config_path() -> PathBuf {
    if cfg!(windows) {
        return PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default())
            .join("HookshotBuilder/builder.ini");
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_default();
    base.join("hookshot/builder.ini")
}
