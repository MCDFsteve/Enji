//! The same declaration is parsed for a store listing, an archive and an installed plugin.

use super::platform_name;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::{Component, Path};

/// The SDK revision is separate from the application's real release number.
pub(super) const API_VERSION: u32 = 2;

#[derive(Clone, Debug, PartialEq)]
pub enum SettingKind {
    Boolean,
    Number { min: f64, max: f64 },
    Enum { options: Vec<String> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Setting {
    pub id: String,
    pub label: String,
    pub kind: SettingKind,
    pub default: Value,
}

impl SettingKind {
    /// Do not coerce strings or round/clamp values. Canonical numbers make `1` and `1.0` the same setting.
    pub(super) fn validate(&self, value: &Value, id: &str) -> Result<Value, String> {
        match self {
            Self::Boolean => value.as_bool().map(Value::Bool).ok_or_else(|| format!("设置「{id}」需要布尔值")),
            Self::Number { min, max } => value
                .as_f64()
                .filter(|n| n.is_finite() && min <= n && n <= max)
                .map(|n| json!(n))
                .ok_or_else(|| format!("设置「{id}」需要 {min} 到 {max} 之间的数值")),
            Self::Enum { options } => value
                .as_str()
                .filter(|s| options.iter().any(|o| o.as_str() == *s))
                .map(|s| Value::String(s.to_owned()))
                .ok_or_else(|| format!("设置「{id}」的值不在 options 中")),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Action {
    pub id: String,
    pub label: String,
    /// `track`: the clicked row. `player`: the playing page.
    pub context: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pack {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub permissions: Vec<String>,
    /// The systems it works on (`windows`, `macos`, …); empty for all.
    pub platforms: Vec<String>,
    pub min_host_version: Option<String>,
    pub main: String,
    pub icon: Option<String>,
    pub settings: Vec<Setting>,
    pub actions: Vec<Action>,
}

pub(super) fn text(value: &Value, key: &str) -> String {
    value[key].as_str().map(str::trim).unwrap_or("").to_owned()
}

fn array<'a>(value: &'a Value, key: &str) -> Result<&'a [Value], String> {
    match value.get(key) {
        None => Ok(&[]),
        Some(v) => v.as_array().map(Vec::as_slice).ok_or_else(|| format!("插件声明的 {key} 必须是数组")),
    }
}

fn texts(value: &Value, key: &str) -> Result<Vec<String>, String> {
    let mut result = Vec::new();
    for entry in array(value, key)? {
        let item = entry.as_str().ok_or_else(|| format!("插件声明的 {key} 需要字符串"))?.trim();
        if !item.is_empty() && !result.iter().any(|s| s == item) {
            result.push(item.to_owned());
        }
    }
    Ok(result)
}

pub(super) fn valid_id(id: &str) -> bool {
    !id.is_empty() && id != "." && id != ".." && id.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

// Backslashes and drive prefixes are unsafe even when an archive was prepared on Unix.
fn local_file(name: &str) -> bool {
    !name.is_empty()
        && !name.contains(['\\', ':', '\0'])
        && !name.ends_with('/')
        && Path::new(name).components().all(|p| matches!(p, Component::Normal(_) | Component::CurDir))
        && Path::new(name).file_name().is_some()
}

// minHostVersion is one stable release, not an expression or semver range.
fn release(text: &str) -> Option<[u64; 3]> {
    let mut parts = text.split('.');
    let mut result = [0; 3];
    for n in &mut result {
        let part = parts.next()?;
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) || part.len() > 1 && part.starts_with('0') {
            return None;
        }
        *n = part.parse().ok()?;
    }
    parts.next().is_none().then_some(result)
}

fn declarations<T>(value: &Value, key: &str, parse: impl Fn(&Value, String, String) -> Result<T, String>) -> Result<Vec<T>, String> {
    let entries = array(value, key)?;
    if entries.len() > 64 {
        return Err(format!("插件声明最多允许 64 项 {key}"));
    }
    let mut ids = HashSet::new();
    entries.iter().map(|entry| {
        let id = text(entry, "id");
        let label = text(entry, "label");
        if !valid_id(&id) || id.len() > 64 || !ids.insert(id.clone()) {
            return Err(format!("{key} 有无效或重复的 id「{id}」"));
        }
        if label.is_empty() || label.chars().count() > 128 {
            return Err(format!("{key} 的「{id}」需要 1 到 128 个字符的 label"));
        }
        parse(entry, id, label)
    }).collect()
}

impl Pack {
    pub fn from_json(value: &Value) -> Result<Self, String> {
        let (id, name, version) = (text(value, "id"), text(value, "name"), text(value, "version"));
        if id.is_empty() || name.is_empty() || version.is_empty() {
            return Err("插件声明缺少 id、name 或 version".into());
        }
        if !valid_id(&id) {
            return Err(format!("插件 id「{id}」不是有效的标识符"));
        }
        let min_host_version = value.get("minHostVersion").map(|v| {
            v.as_str().filter(|s| release(s).is_some()).map(str::to_owned)
                .ok_or_else(|| "minHostVersion 必须是 major.minor.patch 三段数字版本号".to_owned())
        }).transpose()?;
        // Legacy packs may leave main blank; the script still defaults to main.js.
        let main = Some(text(value, "main")).filter(|s| !s.is_empty()).unwrap_or_else(|| "main.js".into());
        if !local_file(&main) { return Err("main 必须是插件包内的文件名".into()); }
        let icon = Some(text(value, "icon")).filter(|s| !s.is_empty());
        if icon.as_deref().is_some_and(|s| !local_file(s)) {
            return Err("icon 必须是插件包内的文件名".into());
        }
        let settings = declarations(value, "settings", |entry, id, label| {
            let ty = entry["type"].as_str();
            if ty != Some("number") && (entry.get("min").is_some() || entry.get("max").is_some())
                || ty != Some("enum") && entry.get("options").is_some() {
                return Err(format!("设置「{id}」的范围或选项与声明的类型不符"));
            }
            let kind = match ty {
                Some("boolean") => SettingKind::Boolean,
                Some("number") => {
                    let bounds = entry["min"].as_f64().zip(entry["max"].as_f64());
                    let (min, max) = bounds.filter(|(a,b)| a.is_finite() && b.is_finite() && a < b && (b - a).is_finite())
                        .ok_or_else(|| format!("设置「{id}」需要有限数值范围 min < max"))?;
                    SettingKind::Number { min, max }
                }
                Some("enum") => {
                    let options = array(entry, "options")?;
                    if options.is_empty() || options.len() > 64 {
                        return Err(format!("设置「{id}」需要 1 到 64 个枚举选项"));
                    }
                    let mut names = Vec::with_capacity(options.len());
                    for option in options {
                        let name = option.as_str().filter(|s| !s.trim().is_empty() && s.chars().count() <= 128)
                            .ok_or_else(|| format!("设置「{id}」需要非空字符串枚举选项"))?;
                        if names.iter().any(|s| s == name) {
                            return Err(format!("设置「{id}」的枚举选项不能重复"));
                        }
                        names.push(name.to_owned());
                    }
                    SettingKind::Enum { options: names }
                }
                _ => return Err(format!("设置「{id}」的 type 应为 boolean、number 或 enum")),
            };
            let default = kind.validate(&entry["default"], &id).map_err(|e| format!("默认值不正确：{e}"))?;
            Ok(Setting { id, label, kind, default })
        })?;
        let actions = declarations(value, "actions", |entry, id, label| {
            let context = entry["context"].as_str().filter(|s| matches!(*s, "track" | "player"))
                .ok_or_else(|| format!("动作「{id}」的 context 应为 track 或 player"))?.to_owned();
            Ok(Action { id, label, context })
        })?;
        Ok(Self { id, name, version, description: text(value, "description"), author: text(value, "author"),
            permissions: texts(value, "permissions")?, platforms: texts(value, "platforms")?, min_host_version, main, icon, settings, actions })
    }

    pub fn check_compatibility(&self) -> Result<(), String> {
        if let Some(floor) = &self.min_host_version {
            let host = env!("CARGO_PKG_VERSION");
            let base = host.split('+').next().unwrap_or(host);
            let (base, prerelease) = base.split_once('-').map_or((base, false), |(s, _)| (s, true));
            let ok = release(base).zip(release(floor)).is_some_and(|(have, need)| have > need || have == need && !prerelease);
            if !ok {
                return Err(format!("插件「{}」需要 Enji {floor} 或更高版本，当前为 {host}", self.name));
            }
        }
        if !self.platforms.is_empty() && !self.platforms.iter().any(|p| p == platform_name()) {
            return Err(format!("插件「{}」只能用在 {}", self.name, self.platforms_label()));
        }
        Ok(())
    }

    pub fn fits_here(&self) -> bool {
        self.check_compatibility().is_ok()
    }

    pub(crate) fn has(&self, permission: &str) -> bool {
        self.permissions.iter().any(|p| p == permission)
    }

    pub fn platforms_label(&self) -> String {
        self.platforms.iter().map(|platform| match platform.as_str() {
            "windows" => "Windows", "macos" => "macOS", "linux" => "Linux", "android" => "Android",
            "ios" => "iOS", "tvos" => "tvOS", "watchos" => "watchOS", other => other,
        }).collect::<Vec<_>>().join(" / ")
    }
}
