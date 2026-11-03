//! What Enji keeps on this computer: the account's cookie, readable only by
//! the user, and the QR code shown while signing in.

use std::{fs, path::PathBuf};

pub use enji_kit::store::{cookie, dir};

fn cookie_path() -> PathBuf {
    dir().join("cookie.txt")
}

pub fn save_cookie(cookie: &str) -> std::io::Result<()> {
    fs::create_dir_all(dir())?;
    fs::write(cookie_path(), cookie)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(cookie_path(), fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn settings_path() -> PathBuf {
    dir().join("settings.json")
}

/// The settings saved last, if any.
pub fn settings() -> serde_json::Value {
    fs::read_to_string(settings_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(serde_json::Value::Null)
}

pub fn save_settings(settings: &serde_json::Value) {
    let _ = fs::create_dir_all(dir());
    let _ = fs::write(settings_path(), settings.to_string());
}

fn history_path() -> PathBuf {
    dir().join("history.json")
}

/// What was played, as it was kept.
pub fn history() -> Vec<serde_json::Value> {
    fs::read_to_string(history_path())
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
}

pub fn save_history(entries: &[serde_json::Value]) {
    let _ = fs::create_dir_all(dir());
    let _ = fs::write(history_path(), serde_json::Value::Array(entries.to_vec()).to_string());
}

/// Where a phone looks for music: the app's own Documents folder, which
/// the Files app shows.
pub fn phone_music_dir() -> Option<PathBuf> {
    #[cfg(target_os = "ios")]
    {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Documents"))
    }
    #[cfg(not(target_os = "ios"))]
    {
        None
    }
}

pub fn forget_cookie() {
    let _ = fs::remove_file(cookie_path());
}

/// `text` as a QR code in a PNG, eight pixels a module with a quiet zone of
/// four; a new file each time, so the picture is never an old one.
pub fn qr_png(text: &str, serial: u64) -> Result<PathBuf, String> {
    let code = qrcode::QrCode::new(text.as_bytes()).map_err(|e| e.to_string())?;
    let modules = code.width();
    let colors = code.to_colors();
    let (scale, quiet) = (8, 4);
    let side = (modules + quiet * 2) * scale;
    let mut pixels = vec![255u8; side * side];
    for y in 0..modules {
        for x in 0..modules {
            if colors[y * modules + x] == qrcode::Color::Dark {
                for dy in 0..scale {
                    let row = ((y + quiet) * scale + dy) * side;
                    let from = row + (x + quiet) * scale;
                    pixels[from..from + scale].fill(0);
                }
            }
        }
    }
    fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
    for old in fs::read_dir(dir()).into_iter().flatten().flatten() {
        if old.file_name().to_string_lossy().starts_with("qr-") {
            let _ = fs::remove_file(old.path());
        }
    }
    let path = dir().join(format!("qr-{serial}.png"));
    let file = fs::File::create(&path).map_err(|e| e.to_string())?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), side as u32, side as u32);
    encoder.set_color(png::ColorType::Grayscale);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&pixels).map_err(|e| e.to_string())?;
    Ok(path)
}

/// The sign-in QR code made last, while there is one.
pub fn qr_file() -> Option<PathBuf> {
    fs::read_dir(dir())
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.file_name().is_some_and(|name| name.to_string_lossy().starts_with("qr-")))
}
