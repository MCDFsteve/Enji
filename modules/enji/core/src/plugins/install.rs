//! Validate the real archive, stage it completely, then swap it for the previous version.

use super::{Pack, dir, manifest::valid_id, settings, store};
use serde_json::Value;
use std::{fs, io::{self, Read}, path::{Component, PathBuf}, sync::{Mutex, atomic::{AtomicU64, Ordering}}};

const PACKAGE_LIMIT: u64 = 16 << 20;
const MANIFEST_LIMIT: u64 = 256 << 10;
static INSTALL: Mutex<()> = Mutex::new(());

/// Only the folder bearing this exact id may run under its permissions/settings namespace.
pub fn installed() -> Vec<(Pack, PathBuf)> {
    let _lock = INSTALL.lock().unwrap_or_else(|e| e.into_inner());
    let mut found: Vec<_> = fs::read_dir(dir()).into_iter().flatten().filter_map(Result::ok).filter_map(|entry| {
        if !entry.file_type().ok()?.is_dir() { return None; }
        let folder = entry.path();
        let value: Value = serde_json::from_reader(fs::File::open(folder.join("pack.json")).ok()?.take(MANIFEST_LIMIT)).ok()?;
        let pack = Pack::from_json(&value).ok()?;
        (entry.file_name().to_str() == Some(&pack.id)).then_some((pack, folder))
    }).collect();
    found.sort_by(|a, b| a.0.name.cmp(&b.0.name));
    found
}

/// Local import: the package's own validated declaration is authoritative.
pub fn unpack(package: &[u8]) -> Result<Pack, String> {
    install(package, None)
}

/// For a store install the id, version and permissions must be those which the store offered.
pub fn unpack_for(package: &[u8], expected: &Pack) -> Result<Pack, String> {
    expected.check_compatibility()?;
    install(package, Some(expected))
}

fn stage() -> io::Result<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let root = store::dir().join("plugin-installs");
    fs::create_dir_all(&root)?;
    loop {
        let path = root.join(format!("{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
}

fn install(package: &[u8], expected: Option<&Pack>) -> Result<Pack, String> {
    let mut archive = zip::ZipArchive::new(io::Cursor::new(package)).map_err(|_| "这不是 Enji 插件包（.enj）".to_owned())?;
    let pack = {
        let file = archive.by_name("pack.json").map_err(|_| "插件包里没有 pack.json".to_owned())?;
        let mut bytes = Vec::new();
        file.take(MANIFEST_LIMIT + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MANIFEST_LIMIT { return Err("pack.json 太大".into()); }
        Pack::from_json(&serde_json::from_slice(&bytes).map_err(|e| format!("pack.json 不是有效的 JSON：{e}"))?)?
    };
    pack.check_compatibility()?;
    if let Some(offered) = expected
        && (pack.id != offered.id || pack.version != offered.version || pack.permissions.len() != offered.permissions.len()
            || pack.permissions.iter().any(|p| !offered.has(p))) {
        return Err("插件包的 id、version 或 permissions 与商店声明不符，未安装".into());
    }
    let staging = stage().map_err(|e| e.to_string())?;
    let new = staging.join("new");
    let old = staging.join("previous");
    let result = (|| -> Result<(), String> {
        fs::create_dir(&new).map_err(|e| e.to_string())?;
        let mut total = 0;
        for i in 0..archive.len() {
            let file = archive.by_index(i).map_err(|e| e.to_string())?;
            let name = file.enclosed_name().ok_or_else(|| format!("插件包里有越出插件文件夹的路径：{}", file.name()))?;
            if file.name().contains(['\\', ':', '\0']) || !name.components().all(|p| matches!(p, Component::Normal(_) | Component::CurDir)) {
                return Err("插件包中的路径不安全".into());
            }
            let path = new.join(name);
            if file.is_dir() {
                fs::create_dir_all(&path).map_err(|e| e.to_string())?;
                continue;
            }
            if file.size() > PACKAGE_LIMIT - total { return Err("插件包解开后太大".into()); }
            fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
            // Never follow/create archive symlinks, and don't let a second entry replace the checked manifest.
            let mut out = fs::OpenOptions::new().write(true).create_new(true).open(&path).map_err(|e| format!("无法解包 {}：{e}", file.name()))?;
            total += io::copy(&mut file.take(PACKAGE_LIMIT - total + 1), &mut out).map_err(|e| e.to_string())?;
            if total > PACKAGE_LIMIT { return Err("插件包解开后太大".into()); }
        }
        if !new.join(&pack.main).is_file() { return Err(format!("插件包里没有脚本 {}", pack.main)); }
        // Also reject a script the host could not read after starting an update.
        fs::read_to_string(new.join(&pack.main)).map_err(|e| format!("插件脚本不是可读的 UTF-8 文本：{e}"))?;
        let _lock = INSTALL.lock().unwrap_or_else(|e| e.into_inner());
        fs::create_dir_all(dir()).map_err(|e| e.to_string())?;
        let dest = dir().join(&pack.id);
        let updating = dest.exists();
        if updating {
            let previous: Value = serde_json::from_reader(fs::File::open(dest.join("pack.json")).map_err(|e| e.to_string())?)
                .map_err(|e| format!("无法读取旧插件的 id：{e}"))?;
            if previous["id"].as_str().map(str::trim) != Some(pack.id.as_str()) { return Err("目标文件夹属于另一个插件".into()); }
            fs::rename(&dest, &old).map_err(|e| format!("无法暂存旧插件：{e}"))?;
        }
        if let Err(e) = fs::rename(&new, &dest) {
            if updating && let Err(restore) = fs::rename(&old, &dest) {
                return Err(format!("安装失败：{e}；旧插件保留在 {}：{restore}", old.display()));
            }
            return Err(format!("安装失败：{e}"));
        }
        Ok(())
    })();
    // A failed rollback is the only case where the staging folder holds the user's only good copy.
    if result.is_ok() || !old.exists() { let _ = fs::remove_dir_all(staging); }
    result.map(|()| pack)
}

pub fn remove(id: &str) {
    if !valid_id(id) { return; }
    let _lock = INSTALL.lock().unwrap_or_else(|e| e.into_inner());
    let folder = dir().join(id);
    // A case-insensitive filesystem may resolve a different spelling to another plugin.
    if let Ok(file) = fs::File::open(folder.join("pack.json"))
        && serde_json::from_reader::<_, Value>(file.take(MANIFEST_LIMIT)).ok().is_some_and(|p| p["id"].as_str().map(str::trim) != Some(id)) { return; }
    let _ = fs::remove_dir_all(folder);
    settings::remove(id);
}
