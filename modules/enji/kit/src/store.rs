//! The files Enji keeps on this device, in the app's own folder: what the
//! core and the modules share of them.

use std::{fs, path::PathBuf};

/// Where Enji keeps its files: the app's own storage on phones and TVs, the
/// system's place for application data elsewhere (Application Support,
/// AppData, ~/.local/share).
pub fn dir() -> PathBuf {
    #[cfg(any(target_os = "android", target_os = "ios", target_os = "tvos"))]
    {
        bern::storage::data_dir().unwrap_or_else(std::env::temp_dir)
    }
    #[cfg(not(any(target_os = "android", target_os = "ios", target_os = "tvos")))]
    {
        dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("Enji")
    }
}

pub fn cookie_path() -> PathBuf {
    dir().join("cookie.txt")
}

pub fn cookie() -> Option<String> {
    let text = fs::read_to_string(cookie_path()).ok()?;
    let text = text.trim();
    text.contains("MUSIC_U=").then(|| text.to_string())
}

/// The JSON kept in `name`; `Null` when there is none.
pub fn read(name: &str) -> serde_json::Value {
    fs::read_to_string(dir().join(name))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(serde_json::Value::Null)
}

/// Keep `value` as JSON in `name`.
pub fn write(name: &str, value: &serde_json::Value) {
    let _ = fs::create_dir_all(dir());
    let _ = fs::write(dir().join(name), value.to_string());
}

fn plays_path() -> PathBuf {
    dir().join("plays.json")
}

/// How many times each song was played here, by song id.
pub fn plays() -> std::collections::HashMap<i64, i64> {
    fs::read_to_string(plays_path())
        .ok()
        .and_then(|text| serde_json::from_str::<std::collections::HashMap<String, i64>>(&text).ok())
        .map(|kept| kept.into_iter().filter_map(|(id, count)| Some((id.parse().ok()?, count))).collect())
        .unwrap_or_default()
}

pub fn save_plays(plays: &std::collections::HashMap<i64, i64>) {
    let _ = fs::create_dir_all(dir());
    let kept: std::collections::HashMap<String, i64> = plays.iter().map(|(id, count)| (id.to_string(), *count)).collect();
    let _ = fs::write(plays_path(), serde_json::to_string(&kept).unwrap_or_default());
}
