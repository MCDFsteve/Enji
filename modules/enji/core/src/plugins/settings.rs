//! A plugin's user settings survive replacing its installation. Script storage is a separate namespace.

use super::{Pack, store};
use serde_json::{Value, json};
use std::{fs, io, path::{Path, PathBuf}};

fn path(id: &str) -> PathBuf {
    // Preserve case even on case-insensitive filesystems. Never use a manifest string as a path component here.
    use std::fmt::Write;
    let mut key = String::from("p-");
    for b in id.bytes() {
        if b.is_ascii_uppercase() {
            let _ = write!(key, "~{b:02x}");
        } else {
            key.push(char::from(b));
        }
    }
    store::dir().join("plugin-settings").join(format!("{key}.json"))
}

fn saved(pack: &Pack, file: &Path) -> Result<Value, String> {
    match fs::read(file) {
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes).ok().filter(Value::is_object)
            .ok_or_else(|| format!("插件「{}」保存的设置不是有效的 JSON 对象", pack.name)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(format!("读取插件「{}」的设置失败：{e}", pack.name)),
    }
}

fn resolved(pack: &Pack, saved: &Value) -> Value {
    Value::Object(pack.settings.iter().map(|s| {
        let v = saved.get(&s.id).and_then(|v| s.kind.validate(v, &s.id).ok()).unwrap_or_else(|| s.default.clone());
        (s.id.clone(), v)
    }).collect())
}

/// Only declared, valid values ever reach UI or JS; a changed declaration falls back to its own default.
pub fn setting_values(pack: &Pack) -> Value {
    if pack.settings.is_empty() { return json!({}); }
    resolved(pack, &saved(pack, &path(&pack.id)).unwrap_or(Value::Null))
}

/// A new canonical value after a durable write, or None when it was already effective. Called by the single worker.
pub(super) fn set(pack: &Pack, id: &str, value: &Value) -> Result<Option<Value>, String> {
    set_at(pack, id, value, &path(&pack.id))
}

fn set_at(pack: &Pack, id: &str, value: &Value, dest: &Path) -> Result<Option<Value>, String> {
    let setting = pack.settings.iter().find(|s| s.id == id).ok_or_else(|| format!("插件「{}」未声明设置「{id}」", pack.name))?;
    let value = setting.kind.validate(value, id)?;
    let mut values = resolved(pack, &saved(pack, dest)?);
    if values[id] == value { return Ok(None); }
    values[id] = value.clone();
    let tmp = dest.with_extension("tmp");
    fs::create_dir_all(dest.parent().unwrap()).and_then(|()| fs::write(&tmp, values.to_string()))
        .and_then(|()| fs::rename(&tmp, dest)).map_err(|e| format!("保存插件「{}」的设置失败：{e}", pack.name))?;
    Ok(Some(value))
}

pub(super) fn remove(id: &str) {
    let _ = fs::remove_file(path(id));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_persist_changes_to_the_declaring_plugin() {
        let root = std::env::temp_dir().join(format!("enji-plugin-settings-{:032x}", rand::random::<u128>()));
        let make = |id: &str| Pack::from_json(&json!({"id":id,"name":"P","version":"1","settings":[
            {"id":"limit","label":"Limit","type":"number","min":0,"max":10,"default":5}
        ]})).unwrap();
        let (a, b) = (make("p.A"), make("p.a"));
        let (a_file, b_file) = (root.join(path(&a.id).file_name().unwrap()), root.join(path(&b.id).file_name().unwrap()));
        assert_ne!(a_file, b_file);
        assert_eq!(set_at(&a, "limit", &json!(7), &a_file).unwrap(), Some(json!(7.0)));
        assert_eq!(set_at(&a, "limit", &json!(7.0), &a_file).unwrap(), None);
        assert!(set_at(&a, "limit", &json!(11), &a_file).is_err());
        assert!(set_at(&a, "other", &json!(2), &a_file).is_err());
        assert_eq!(resolved(&a, &saved(&a, &a_file).unwrap())["limit"], json!(7.0));
        assert_eq!(resolved(&b, &saved(&b, &b_file).unwrap())["limit"], json!(5.0));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn revalidate_saved_preferences_against_the_installed_version() {
        let pack = Pack::from_json(&json!({"id":"p","name":"P","version":"1","settings":[
            {"id":"switch","label":"Switch","type":"boolean","default":false},
            {"id":"target","label":"Target","type":"number","min":-20,"max":-5,"default":-14},
            {"id":"mode","label":"Mode","type":"enum","options":["manual","auto"],"default":"auto"}
        ]})).unwrap();
        let old = json!({"switch":true,"target":0,"mode":"no longer offered","unknown":3});
        assert_eq!(resolved(&pack, &old), json!({"switch":true,"target":-14.0,"mode":"auto"}));
    }
}
