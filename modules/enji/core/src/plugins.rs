//! Plugins: scripts the user installs from the plugin market or imports
//! as `.enj` files, which react to what plays and do what they declared
//! they may.
//!
//! A plugin is a folder of `pack.json`, its script and its icon; handed
//! around on its own it is a package, a zip archive of that folder with a
//! `.enj` extension. The market is an index, `plugins.json`, listing each
//! plugin's declaration and where its package is.
//!
//! Each enabled plugin runs in a JavaScript context of its own (Boa). It
//! cannot call the engine back: the engine hands it each event with what it
//! may read, and takes back the calls it queued (`plugin_host.js`).

use crate::store;
use serde_json::{json, Value};
use std::collections::VecDeque;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The plugin market's index.
pub const INDEX: &str = "https://raw.githubusercontent.com/MCDFsteve/enji-plugins/main/plugins.json";
/// More indexes, separated by `;`: addresses or local `plugins.json` files,
/// for trying a market before it is published.
pub const INDEX_VARIABLE: &str = "ENJI_PLUGIN_INDEX";
/// The most a package may hold unpacked, against archives that inflate
/// without end.
const PACKAGE_LIMIT: u64 = 16 << 20;

const HOST: &str = include_str!("plugin_host.js");

/// What a plugin says it is: its `pack.json`.
#[derive(Clone, Debug, PartialEq)]
pub struct Pack {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    /// `lyrics`, `audio`, `overlay`, `cover`.
    pub permissions: Vec<String>,
    /// The systems it works on (`windows`, `macos`, …); empty for all.
    pub platforms: Vec<String>,
    /// The script's file in the plugin's folder.
    pub main: String,
    /// The icon's file in the plugin's folder.
    pub icon: Option<String>,
}

fn text(value: &Value, key: &str) -> String {
    value[key].as_str().map(str::trim).unwrap_or("").to_string()
}

fn texts(value: &Value, key: &str) -> Vec<String> {
    value[key]
        .as_array()
        .map(|list| list.iter().filter_map(Value::as_str).map(|item| item.trim().to_string()).filter(|item| !item.is_empty()).collect())
        .unwrap_or_default()
}

impl Pack {
    pub fn from_json(value: &Value) -> Result<Pack, String> {
        let (id, name, version) = (text(value, "id"), text(value, "name"), text(value, "version"));
        if id.is_empty() || name.is_empty() || version.is_empty() {
            return Err("插件声明缺少 id、name 或 version".into());
        }
        if !id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-')) {
            return Err(format!("插件 id「{id}」只能有字母、数字、点、下划线和连字符"));
        }
        let main = Some(text(value, "main")).filter(|main| !main.is_empty()).unwrap_or_else(|| "main.js".into());
        Ok(Pack {
            id,
            name,
            version,
            description: text(value, "description"),
            author: text(value, "author"),
            permissions: texts(value, "permissions"),
            platforms: texts(value, "platforms"),
            main,
            icon: Some(text(value, "icon")).filter(|icon| !icon.is_empty()),
        })
    }

    /// Whether it works on the system Enji runs on.
    pub fn fits_here(&self) -> bool {
        self.platforms.is_empty() || self.platforms.iter().any(|platform| platform == platform_name())
    }

    /// The systems it works on as people name them, such as `Windows /
    /// macOS`; empty when it works on all.
    pub fn platforms_label(&self) -> String {
        self.platforms
            .iter()
            .map(|platform| match platform.as_str() {
                "windows" => "Windows",
                "macos" => "macOS",
                "linux" => "Linux",
                "android" => "Android",
                "ios" => "iOS",
                other => other,
            })
            .collect::<Vec<_>>()
            .join(" / ")
    }
}

/// This system as plugins name it.
pub fn platform_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "ios") {
        "ios"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "android") {
        "android"
    } else {
        "linux"
    }
}

/// Where plugins are installed, each in a folder named by its id.
pub fn dir() -> PathBuf {
    store::dir().join("plugins")
}

/// The installed plugins and their folders, by name.
pub fn installed() -> Vec<(Pack, PathBuf)> {
    let mut found: Vec<(Pack, PathBuf)> = std::fs::read_dir(dir())
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let folder = entry.path();
            let value: Value = serde_json::from_slice(&std::fs::read(folder.join("pack.json")).ok()?).ok()?;
            Some((Pack::from_json(&value).ok()?, folder))
        })
        .collect();
    found.sort_by(|a, b| a.0.name.cmp(&b.0.name));
    found
}

/// Unpack a `.enj` package into the plugins folder, in place of an earlier
/// version of the same plugin; its declaration.
pub fn unpack(package: &[u8]) -> Result<Pack, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(package)).map_err(|_| "这不是 Enji 插件包（.enj）".to_string())?;
    let pack = {
        let mut file = archive.by_name("pack.json").map_err(|_| "插件包里没有 pack.json".to_string())?;
        let mut text = String::new();
        file.read_to_string(&mut text).map_err(|e| e.to_string())?;
        Pack::from_json(&serde_json::from_str(&text).map_err(|e| format!("pack.json 不是有效的 JSON：{e}"))?)?
    };
    let staging = dir().join(format!(".{}.unpacking", pack.id));
    let _ = std::fs::remove_dir_all(&staging);
    let mut total = 0u64;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        // Paths that would leave the plugin's folder are refused.
        let Some(name) = file.enclosed_name() else {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(format!("插件包里有越出插件文件夹的路径：{}", file.name()));
        };
        let path = staging.join(name);
        if file.is_dir() {
            std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
            continue;
        }
        total += file.size();
        if total > PACKAGE_LIMIT {
            let _ = std::fs::remove_dir_all(&staging);
            return Err("插件包解开后太大".into());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    }
    if !staging.join(&pack.main).is_file() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!("插件包里没有脚本 {}", pack.main));
    }
    let folder = dir().join(&pack.id);
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::rename(&staging, &folder).map_err(|e| e.to_string())?;
    Ok(pack)
}

/// Remove an installed plugin's folder.
pub fn remove(id: &str) {
    if !id.is_empty() && !id.contains(['/', '\\']) && id != "." && id != ".." {
        let _ = std::fs::remove_dir_all(dir().join(id));
    }
}

/// A plugin the market lists, and where its package is.
#[derive(Clone, Debug, PartialEq)]
pub struct Listing {
    pub pack: Pack,
    pub package: String,
}

/// The indexes to read.
pub fn indexes() -> Vec<String> {
    let mut indexes = vec![INDEX.to_string()];
    if let Ok(more) = std::env::var(INDEX_VARIABLE) {
        indexes.extend(more.split(';').map(str::trim).filter(|index| !index.is_empty()).map(String::from));
    }
    indexes
}

/// An address beside `index`: relative paths are taken from the index's
/// folder.
pub fn beside(index: &str, path: &str) -> String {
    if path.contains("://") || path.starts_with('/') {
        return path.to_string();
    }
    match index.rfind('/') {
        Some(at) => format!("{}/{}", &index[..at], path),
        None => path.to_string(),
    }
}

/// What an index lists.
pub fn listings(index: &str, text: &str) -> Vec<Listing> {
    let value: Value = serde_json::from_str(text).unwrap_or_default();
    value["plugins"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let pack = Pack::from_json(entry).ok()?;
            let package = Some(text_of(entry, "package")).filter(|package| !package.is_empty())?;
            Some(Listing { pack, package: beside(index, &package) })
        })
        .collect()
}

fn text_of(value: &Value, key: &str) -> String {
    text(value, key)
}

/// A call a plugin queued.
#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    OverlayShow(String, Value),
    OverlayHide(String),
    CoverPulse,
    Toast(String),
    StorageSet(String, String),
    Log(String),
}

impl Call {
    fn parse(value: &Value) -> Option<Call> {
        let name = value[0].as_str()?;
        let args = &value[1];
        let arg = |i: usize| args[i].as_str().unwrap_or("").to_string();
        Some(match name {
            "overlayShow" => Call::OverlayShow(arg(0), args[1].clone()),
            "overlayHide" => Call::OverlayHide(arg(0)),
            "coverPulse" => Call::CoverPulse,
            "toast" => Call::Toast(arg(0)),
            "storageSet" => Call::StorageSet(arg(0), arg(1)),
            "log" => Call::Log(arg(0)),
            _ => return None,
        })
    }

    /// The permission it needs, if any.
    fn permission(&self) -> Option<&'static str> {
        match self {
            Call::OverlayShow(..) | Call::OverlayHide(_) => Some("overlay"),
            Call::CoverPulse => Some("cover"),
            _ => None,
        }
    }
}

/// An enabled plugin: its context, and the events handed to it whose calls
/// are not taken yet.
struct Running {
    pack: Pack,
    context: bern::script::Context,
    pending: VecDeque<bern::script::Evaluation>,
}

/// The enabled plugins.
#[derive(Default)]
pub struct Plugins {
    running: Vec<Running>,
}

/// What the plugins keep between runs, by plugin and key.
fn storage_path() -> PathBuf {
    store::dir().join("plugin-storage.json")
}

fn storage() -> Value {
    std::fs::read(storage_path()).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_else(|| json!({}))
}

fn keep(id: &str, key: &str, value: &str) {
    let mut all = storage();
    if !all[id].is_object() {
        all[id] = json!({});
    }
    all[id][key] = Value::String(value.to_string());
    let _ = std::fs::create_dir_all(store::dir());
    let _ = std::fs::write(storage_path(), all.to_string());
}

impl Plugins {
    /// Start the installed plugins of `enabled` that work here.
    pub fn start(enabled: &[String]) -> (Plugins, Vec<String>) {
        let mut plugins = Plugins::default();
        let mut errors = Vec::new();
        for (pack, folder) in installed() {
            if enabled.contains(&pack.id)
                && pack.fits_here()
                && let Err(message) = plugins.run(pack, &folder)
            {
                errors.push(message);
            }
        }
        (plugins, errors)
    }

    fn run(&mut self, pack: Pack, folder: &Path) -> Result<(), String> {
        let script = std::fs::read_to_string(folder.join(&pack.main)).map_err(|e| format!("读不了插件「{}」的脚本：{e}", pack.name))?;
        let context = bern::script::Context::new().map_err(|e| format!("插件「{}」启动失败：{e}", pack.name))?;
        let config = json!({
            "id": pack.id,
            "name": pack.name,
            "version": pack.version,
            "permissions": pack.permissions,
            "platform": platform_name(),
            "hostVersion": env!("CARGO_PKG_VERSION"),
            "storage": storage()[&pack.id],
        });
        // Its answer is no call list, but an error in the script still shows in the log.
        let started = context.eval(format!("var __enjiConfig = {config};\n{HOST}\n{script}\n;\"[]\""));
        self.running.push(Running { pack, context, pending: VecDeque::from([started]) });
        Ok(())
    }

    /// Start or stop one installed plugin.
    pub fn set_running(&mut self, id: &str, on: bool) -> Result<(), String> {
        if !on {
            self.running.retain(|running| running.pack.id != id);
            return Ok(());
        }
        if self.running.iter().any(|running| running.pack.id == id) {
            return Ok(());
        }
        let (pack, folder) = installed().into_iter().find(|(pack, _)| pack.id == id).ok_or_else(|| format!("没有安装插件 {id}"))?;
        if !pack.fits_here() {
            return Err(format!("插件「{}」只能用在 {}", pack.name, pack.platforms_label()));
        }
        self.run(pack, &folder)
    }

    /// Whether a running plugin listens to the music's loudness.
    pub fn wants_audio(&self) -> bool {
        self.running.iter().any(|running| running.pack.permissions.iter().any(|p| p == "audio"))
    }

    pub fn is_empty(&self) -> bool {
        self.running.is_empty()
    }

    /// Hand `event` with its `data` and what may be read to every running
    /// plugin allowed to hear it: `audio` only to those with that
    /// permission, `lyric` to those with `lyrics`.
    pub fn send(&mut self, event: &str, data: &Value, snapshot: &Value) {
        let needs = match event {
            "audio" => Some("audio"),
            "lyric" => Some("lyrics"),
            _ => None,
        };
        let message = json!({ "type": event, "data": data, "snapshot": snapshot });
        for running in &mut self.running {
            if needs.is_some_and(|needed| !running.pack.permissions.iter().any(|p| p == needed)) {
                continue;
            }
            // A plugin that falls behind loses its oldest events.
            if running.pending.len() > 32 {
                running.pending.pop_front();
            }
            running.pending.push_back(running.context.eval(format!("__enji.dispatch({message})")));
        }
    }

    /// The calls of the events the plugins finished with, each with the
    /// plugin that made it; those the plugin may not make are dropped.
    pub fn calls(&mut self) -> Vec<(String, Call)> {
        let mut calls = Vec::new();
        for running in &mut self.running {
            while let Some(result) = running.pending.front().and_then(|evaluation| evaluation.try_result()) {
                running.pending.pop_front();
                let text = match result {
                    Ok(Value::String(text)) => text,
                    Ok(_) => continue,
                    Err(error) => {
                        calls.push((running.pack.id.clone(), Call::Log(error.to_string())));
                        continue;
                    }
                };
                let Ok(Value::Array(queued)) = serde_json::from_str::<Value>(&text) else { continue };
                for call in queued.iter().filter_map(Call::parse) {
                    if call.permission().is_some_and(|needed| !running.pack.permissions.iter().any(|p| p == needed)) {
                        continue;
                    }
                    if let Call::StorageSet(key, value) = &call {
                        keep(&running.pack.id, key, value);
                    }
                    calls.push((running.pack.id.clone(), call));
                }
            }
        }
        calls
    }
}

/// An overlay as a plugin described it: `{ lines: [{ text, size, bold,
/// color }], place, locked }`.
pub fn overlay(spec: &Value) -> bern::overlay::Overlay {
    let color = |value: &Value| -> [u8; 4] {
        let text = value.as_str().unwrap_or("#ffffff").trim_start_matches('#');
        let byte = |at: usize| u8::from_str_radix(text.get(at..at + 2).unwrap_or("ff"), 16).unwrap_or(255);
        [byte(0), byte(2), byte(4), if text.len() >= 8 { byte(6) } else { 255 }]
    };
    let lines = spec["lines"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|line| bern::overlay::Line {
            text: line["text"].as_str().unwrap_or("").to_string(),
            size: line["size"].as_f64().unwrap_or(16.0).clamp(6.0, 96.0),
            bold: line["bold"].as_bool().unwrap_or(false),
            color: color(&line["color"]),
        })
        .take(4)
        .collect();
    bern::overlay::Overlay {
        lines,
        place: if spec["place"].as_str() == Some("taskbar") { bern::overlay::Place::Taskbar } else { bern::overlay::Place::Free },
        locked: spec["locked"].as_bool().unwrap_or(true),
        buttons: Vec::new(),
    }
}

/// The overlay id of a plugin's overlay `id`, apart from Enji's own.
pub fn overlay_id(plugin: &str, id: &str) -> String {
    format!("plugin.{plugin}.{id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn package(files: &[(&str, &str)]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (name, text) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(text.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn declarations_need_an_id_a_name_and_a_version() {
        let pack = Pack::from_json(&json!({ "id": "a.b", "name": "A", "version": "1.0.0", "permissions": ["overlay", " "], "platforms": ["windows"] })).unwrap();
        assert_eq!(pack.main, "main.js");
        assert_eq!(pack.permissions, vec!["overlay"]);
        assert_eq!(pack.fits_here(), cfg!(target_os = "windows"));
        assert!(Pack::from_json(&json!({ "id": "a b", "name": "A", "version": "1" })).is_err());
        assert!(Pack::from_json(&json!({ "name": "A", "version": "1" })).is_err());
    }

    #[test]
    fn packages_without_a_declaration_or_with_paths_outside_are_refused() {
        assert!(unpack(b"not a zip").is_err());
        assert!(unpack(&package(&[("main.js", "1;")])).is_err());
        let outside = package(&[("pack.json", r#"{"id":"t.p","name":"T","version":"1"}"#), ("main.js", "1;"), ("../x.js", "x")]);
        assert!(unpack(&outside).is_err());
    }

    #[test]
    fn index_entries_find_their_packages_beside_the_index() {
        let found = listings(
            "https://host/repo/plugins.json",
            r#"{"plugins":[{"id":"a.b","name":"A","version":"1","package":"dist/a.b-1.enj"},{"id":"c","name":"C","version":"1"}]}"#,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].package, "https://host/repo/dist/a.b-1.enj");
    }

    #[test]
    fn plugin_overlays_take_their_colours_and_places() {
        let shown = overlay(&json!({ "lines": [{ "text": "x", "size": 200, "bold": true, "color": "#11223380" }], "place": "taskbar" }));
        assert_eq!(shown.lines[0].size, 96.0);
        assert_eq!(shown.lines[0].color, [0x11, 0x22, 0x33, 0x80]);
        assert_eq!(shown.place, bern::overlay::Place::Taskbar);
        assert!(shown.locked);
    }

    #[test]
    fn calls_parse_by_name() {
        assert_eq!(Call::parse(&json!(["coverPulse", []])), Some(Call::CoverPulse));
        assert_eq!(Call::parse(&json!(["toast", ["hi"]])), Some(Call::Toast("hi".into())));
        assert_eq!(Call::parse(&json!(["nope", []])), None);
    }

    #[test]
    fn a_running_plugin_hears_events_and_queues_calls() {
        let mut plugins = Plugins::default();
        let folder = std::env::temp_dir().join(format!("enji-plugin-test-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("main.js"),
            "enji.on('lyric', function (line) { cover.pulse(); overlay.show('a', { lines: [{ text: line.text }] }); ui.toast(enji.platform); });",
        )
        .unwrap();
        let pack = Pack::from_json(&json!({ "id": "t.p", "name": "T", "version": "1", "permissions": ["lyrics", "cover"] })).unwrap();
        plugins.run(pack, &folder).unwrap();
        plugins.send("lyric", &json!({ "text": "hello" }), &json!({}));
        let mut calls = Vec::new();
        for _ in 0..200 {
            calls.extend(plugins.calls());
            if !calls.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        // No `overlay` permission: the overlay is dropped.
        assert_eq!(calls, vec![("t.p".into(), Call::CoverPulse), ("t.p".into(), Call::Toast(platform_name().into()))]);
        let _ = std::fs::remove_dir_all(folder);
    }

    #[test]
    fn a_script_that_fails_to_start_is_logged() {
        let mut plugins = Plugins::default();
        let folder = std::env::temp_dir().join(format!("enji-plugin-broken-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("main.js"), "enji.on('lyric', function (line) {").unwrap();
        let pack = Pack::from_json(&json!({ "id": "t.b", "name": "B", "version": "1" })).unwrap();
        plugins.run(pack, &folder).unwrap();
        let mut calls = Vec::new();
        for _ in 0..200 {
            calls.extend(plugins.calls());
            if !calls.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(matches!(&calls[..], [(id, Call::Log(_))] if id == "t.b"), "{calls:?}");
        let _ = std::fs::remove_dir_all(folder);
    }
}
