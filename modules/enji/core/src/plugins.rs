//! Plugins: scripts the user installs from the plugin market or imports
//! as `.enj` files, which react to what plays and do what they declared
//! they may.
//!
//! A plugin is a folder of `pack.json`, its script and its icon; handed
//! around on its own it is a package, a zip archive of that folder with a
//! `.enj` extension. The market is an index, `plugins.json`, listing each
//! plugin's declaration and where its package is.
//!
//! Each enabled plugin runs in a JavaScript context of its own (Boa). In
//! addition to the snapshot/outbox SDK (`plugin_host.js`), scripts can invoke
//! Enji's in-process command catalog directly through `await bern.cli(...)`.

use crate::store;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

mod bridge;
pub mod download;
mod install;
mod manifest;
mod settings;
#[cfg(test)]
mod sdk_tests;

pub use bridge::Call;
pub use install::{installed, remove, unpack, unpack_for};
pub use manifest::{Action, Pack, Setting, SettingKind};
pub use settings::setting_values;

/// The plugin market's index.
pub const INDEX: &str = "https://raw.githubusercontent.com/MCDFsteve/enji-plugins/main/plugins.json";
/// More indexes, separated by `;`: addresses or local `plugins.json` files,
/// for trying a market before it is published.
pub const INDEX_VARIABLE: &str = "ENJI_PLUGIN_INDEX";
const HOST: &str = include_str!("plugin_host.js");

/// Whether the version `offered` is later than `installed`, number by number.
pub fn is_newer(offered: &str, installed: &str) -> bool {
    let numbers = |version: &str| -> Vec<u64> { version.split('.').map(|part| part.parse().unwrap_or(0)).collect() };
    numbers(offered) > numbers(installed)
}

/// This system as plugins name it.
pub fn platform_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "ios") {
        "ios"
    } else if cfg!(target_os = "tvos") {
        "tvos"
    } else if cfg!(target_os = "watchos") {
        "watchos"
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
            let package = Some(manifest::text(entry, "package")).filter(|package| !package.is_empty())?;
            Some(Listing { pack, package: beside(index, &package) })
        })
        .collect()
}

struct Pending {
    evaluation: bern::script::Evaluation,
    /// The playback visible to this particular handler, not a later event's token.
    token: Option<String>,
    /// A late suggestion belongs to the settings the handler actually saw.
    settings_revision: u64,
}

struct Running {
    pack: Pack,
    context: bern::script::Context,
    pending: VecDeque<Pending>,
    settings_revision: u64,
    last_token: Option<String>,
    request_prefix: String,
    pages: HashMap<String, bridge::PageRequest>,
}

impl Running {
    fn dispatch(&mut self, event: &str, data: Value, snapshot: Option<&Value>) {
        let mut message = json!({ "type": event, "data": data });
        if let Some(snapshot) = snapshot {
            let snapshot = bridge::snapshot(&self.pack, snapshot);
            self.last_token = snapshot["playbackToken"].as_str().filter(|s| !s.is_empty()).map(str::to_owned);
            message["snapshot"] = snapshot;
        }
        let input = bridge::json_string(&message);
        let evaluation = self.context.eval(format!("__enji.dispatch(JSON.parse({input}))"));
        self.pending.push_back(Pending { evaluation, token: self.last_token.clone(), settings_revision: self.settings_revision });
    }

    fn setting_changed(&mut self, key: &str, value: Value) {
        self.settings_revision = self.settings_revision.wrapping_add(1);
        // The next handler sees the new value but retains this instance's last playback snapshot.
        self.dispatch("settingChanged", json!({ "id": key, "value": value }), None);
    }

    fn accept(&mut self, call: &Call, token: Option<&str>) -> bool {
        if call.permission().is_some_and(|p| !self.pack.has(p)) { return false; }
        if let Call::PlaybackControl { playback_token, .. } | Call::LyricsTimeline { playback_token, .. } = call
            && Some(playback_token.as_str()) != token { return false; }
        if let Some((id, page)) = bridge::PageRequest::from_call(call) {
            // New instance, new namespace. A delayed reply cannot fulfill the new instance's first request.
            if !id.strip_prefix(&self.request_prefix).is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                || self.pages.len() >= bridge::PAGES_IN_FLIGHT || self.pages.contains_key(id) { return false; }
            self.pages.insert(id.to_owned(), page);
        }
        true
    }
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
    std::fs::read(storage_path()).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}))
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
            if enabled.contains(&pack.id) && let Err(message) = plugins.run(pack, &folder) {
                errors.push(message);
            }
        }
        (plugins, errors)
    }

    fn run(&mut self, pack: Pack, folder: &Path) -> Result<(), String> {
        pack.check_compatibility()?;
        let script = std::fs::read_to_string(folder.join(&pack.main)).map_err(|e| format!("读不了插件「{}」的脚本：{e}", pack.name))?;
        let context = bern::script::Context::with_policy(
            bern::script::CliPolicy::allow_all().with_label(pack.id.clone()),
        )
        .map_err(|e| format!("插件「{}」启动失败：{e}", pack.name))?;
        // Bind replies to this instance independently of the transport's evaluation id.
        let prefix = format!("{:032x}:", rand::random::<u128>());
        let config = bridge::json_string(&json!({
            "id": pack.id, "name": pack.name, "version": pack.version,
            "permissions": pack.permissions, "platform": platform_name(),
            "hostVersion": env!("CARGO_PKG_VERSION"), "apiVersion": manifest::API_VERSION,
            "storage": storage()[&pack.id], "settings": setting_values(&pack),
            "requestPrefix": prefix, "pageLimit": bridge::PAGE_LIMIT, "pagesInFlight": bridge::PAGES_IN_FLIGHT, "maxOffset": bridge::MAX_OFFSET,
        }));
        // Only the installed script is executable source. Config/event/response values are JSON strings.
        let started = context.eval(format!("var __enjiConfig = JSON.parse({config});\n{HOST}\n{script}\n;__enji.flush()"));
        let pending = VecDeque::from([Pending { evaluation: started, token: None, settings_revision: 0 }]);
        self.running.push(Running { pack, context, pending, settings_revision: 0, request_prefix: prefix, pages: HashMap::new(), last_token: None });
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
        self.run(pack, &folder)
    }

    pub fn is_running(&self, id: &str) -> bool {
        self.running.iter().any(|r| r.pack.id == id)
    }

    /// An installed-but-disabled plugin has no runtime permissions.
    pub fn has_permission(&self, id: &str, permission: &str) -> bool {
        self.running.iter().any(|r| r.pack.id == id && r.pack.has(permission))
    }

    /// `loudness` and `audio_gain` do not opt into the high-rate meter or beat detection.
    pub fn wants_audio(&self) -> bool {
        self.running.iter().any(|r| r.pack.has("audio"))
    }

    pub fn is_empty(&self) -> bool {
        self.running.is_empty()
    }

    /// A broadcast cannot deliver a targeted action, preference or page. No plugin sees another one's packet.
    pub fn send(&mut self, event: &str, data: &Value, snapshot: &Value) {
        for r in &mut self.running {
            // Only drop a *new* meter sample, before executing its handler. A result already submitted may
            // contain a write or page request; never evict it in order to make room for another event.
            if event == "audio" && r.pending.len() >= 4 { continue; }
            if let Some(data) = bridge::broadcast(&r.pack, event, data) { r.dispatch(event, data, Some(snapshot)); }
        }
    }

    /// Editable while disabled; when enabled only this context is notified, after a successful change.
    pub fn set_setting(&mut self, plugin: &str, key: &str, value: Value) -> Result<(), String> {
        if let Some(r) = self.running.iter_mut().find(|r| r.pack.id == plugin) {
            if let Some(value) = settings::set(&r.pack, key, &value)? { r.setting_changed(key, value); }
        } else {
            let (pack, _) = installed().into_iter().find(|(p, _)| p.id == plugin).ok_or_else(|| format!("没有安装插件 {plugin}"))?;
            settings::set(&pack, key, &value)?;
        }
        Ok(())
    }

    /// `track` is resolved by the worker from the clicked row, not looked up from the player's current song.
    pub fn dispatch_action(&mut self, plugin: &str, id: &str, context: &str, track: &Value, snapshot: &Value) -> Result<(), String> {
        let r = self.running.iter_mut().find(|r| r.pack.id == plugin).ok_or_else(|| format!("插件「{plugin}」未启用"))?;
        if !r.pack.actions.iter().any(|a| a.id == id && a.context == context) {
            return Err(format!("插件「{plugin}」未声明用于 {context} 的动作「{id}」"));
        }
        if context == "track" && !track["trackKey"].as_str().is_some_and(|key| !key.is_empty()) { return Err("操作的曲目已不存在".into()); }
        r.dispatch("action", json!({ "id": id, "context": context, "track": bridge::track(track) }), Some(snapshot));
        Ok(())
    }

    /// Answer exactly one outstanding request of this instance. Wrong target/kind, duplicates and replies
    /// arriving after the plugin stopped are ignored; a read failure still completes the request.
    pub fn reply_page(&mut self, plugin: &str, kind: &str, request_id: &str, result: Result<Value, &str>, snapshot: &Value) {
        let Some(r) = self.running.iter_mut().find(|r| r.pack.id == plugin) else { return };
        let Some(page_kind) = bridge::PageKind::from_event(kind) else { return };
        if !r.pack.has(page_kind.permission()) || !r.pages.get(request_id).is_some_and(|p| p.kind == page_kind) { return; }
        let request = r.pages.remove(request_id).unwrap();
        let result = result.and_then(|page| {
            if request.kind == bridge::PageKind::Lyrics && request.token.as_deref() != snapshot["playbackToken"].as_str() {
                return Err("stale_playback");
            }
            request.page(page)
        });
        let data = match result {
            Ok(page) => json!({ "requestId": request_id, "ok": true, "page": page, "error": null }),
            Err(error) => json!({ "requestId": request_id, "ok": false, "page": null, "error": error }),
        };
        r.dispatch(kind, data, Some(snapshot));
    }

    /// The calls of the events the plugins finished with, each with the
    /// plugin that made it; those the plugin may not make are dropped.
    pub fn calls(&mut self) -> Vec<(String, Call)> {
        let mut calls = Vec::new();
        for running in &mut self.running {
            while let Some(result) = running.pending.front().and_then(|p| p.evaluation.try_result()) {
                let pending = running.pending.pop_front().unwrap();
                let text = match result {
                    Ok(Value::String(text)) => text,
                    Ok(_) => continue,
                    Err(error) => {
                        calls.push((running.pack.id.clone(), Call::Log(error.to_string())));
                        continue;
                    }
                };
                let Ok(packet) = serde_json::from_str::<Value>(&text) else { continue };
                if packet["instance"].as_str() != Some(running.request_prefix.as_str()) { continue; }
                let Some(queued) = packet["calls"].as_array() else { continue };
                for call in queued.iter().filter_map(Call::parse) {
                    if matches!(&call, Call::SuggestTrackGain { .. }) && pending.settings_revision != running.settings_revision { continue; }
                    if !running.accept(&call, pending.token.as_deref()) { continue; }
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
            ruby: line["ruby"].as_str().unwrap_or("").to_string(),
            size: line["size"].as_f64().unwrap_or(16.0).clamp(6.0, 96.0),
            bold: line["bold"].as_bool().unwrap_or(false),
            color: color(&line["color"]),
            lit: None,
            route: None,
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

    #[test]
    fn a_later_version_is_newer_and_an_older_one_is_not() {
        assert!(is_newer("1.1.0", "1.0.9"));
        assert!(is_newer("1.10.0", "1.9.0"));
        assert!(!is_newer("1.0.0", "1.1.0"));
        assert!(!is_newer("1.1.0", "1.1.0"));
    }

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
        assert_eq!(Call::parse(&json!(["coverPulse", []])), Some(Call::CoverPulse(0.5)));
        assert_eq!(Call::parse(&json!(["coverPulse", [2.0]])), Some(Call::CoverPulse(1.0)));
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
        assert_eq!(calls, vec![("t.p".into(), Call::CoverPulse(0.5)), ("t.p".into(), Call::Toast(platform_name().into()))]);
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
