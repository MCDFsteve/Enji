//! Public plugin market downloads with domestic mirror fallback.
//! Mirrors are transport candidates only; non-official URLs never use them.

use super::Pack;
use reqwest::{Client, StatusCode, Url};
use serde_json::Value;
use std::future::Future;
use std::io::{Cursor, Read};
use std::sync::OnceLock;
use std::time::Duration;

const INDEX_LIMIT: usize = 1 << 20;
const PACKAGE_LIMIT: usize = 16 << 20;
const MANIFEST_LIMIT: u64 = 256 << 10;
const INDEX_TIMEOUT: Duration = Duration::from_secs(8);
const PACKAGE_TIMEOUT: Duration = Duration::from_secs(20);

const MIRRORS: [&str; 3] = [
    "https://gh-proxy.com/",
    "https://ghfast.top/",
    "https://ghproxy.net/",
];

#[derive(Clone, Copy)]
enum Resource<'a> {
    Index,
    Package(&'a Pack),
}

impl Resource<'_> {
    fn label(self) -> &'static str {
        match self {
            Self::Index => "插件索引",
            Self::Package(_) => "插件包",
        }
    }

    fn limit(self) -> usize {
        match self {
            Self::Index => INDEX_LIMIT,
            Self::Package(_) => PACKAGE_LIMIT,
        }
    }

    fn timeout(self) -> Duration {
        match self {
            Self::Index => INDEX_TIMEOUT,
            Self::Package(_) => PACKAGE_TIMEOUT,
        }
    }
}

pub(crate) fn eligible(address: &str) -> bool {
    if address.contains(['%', '\\', '?', '#'])
        || address.trim() != address
        || address.chars().any(char::is_control)
    {
        return false;
    }
    let Ok(url) = Url::parse(address) else {
        return false;
    };
    if url.scheme() != "https"
        || url.host_str() != Some("raw.githubusercontent.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return false;
    }
    let parts: Vec<_> = url.path().split('/').collect();
    let original_path = address.strip_prefix("https://raw.githubusercontent.com/");
    original_path.is_some_and(|path| !path.split('/').any(|p| matches!(p, "." | ".." | "")))
        && parts.len() >= 5
        && parts[1] == "MCDFsteve"
        && parts[2] == "enji-plugins"
}

fn candidates(address: &str) -> Vec<(&'static str, String)> {
    let mut urls = vec![("原站", address.to_string())];
    if eligible(address) {
        if let Some(rest) =
            address.strip_prefix("https://raw.githubusercontent.com/MCDFsteve/enji-plugins/main/")
        {
            urls.push((
                "jsDelivr",
                format!("https://cdn.jsdelivr.net/gh/MCDFsteve/enji-plugins@main/{rest}"),
            ));
        }
        for mirror in MIRRORS {
            urls.push((mirror, format!("{mirror}{address}")));
        }
    }
    urls
}

fn client() -> Result<&'static Client, String> {
    static HTTP: OnceLock<Result<Client, String>> = OnceLock::new();
    HTTP.get_or_init(|| {
        Client::builder()
            .user_agent("Enji-plugin-market")
            .connect_timeout(Duration::from_secs(4))
            .build()
            .map_err(|e| e.without_url().to_string())
    })
    .as_ref()
    .map_err(Clone::clone)
}

async fn fetch_once(address: &str, limit: usize, timeout: Duration) -> Result<Vec<u8>, String> {
    let mut res = client()?
        .get(address)
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    if res.status() != StatusCode::OK {
        return Err(format!("HTTP {}", res.status()));
    }
    if res
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.split(';').next().is_some_and(|mime| {
                matches!(
                    mime.trim().to_ascii_lowercase().as_str(),
                    "text/html" | "application/xhtml+xml"
                )
            })
        })
    {
        return Err("服务器返回了 HTML 页面而不是资源".into());
    }
    if res.content_length().is_some_and(|len| len > limit as u64) {
        return Err("文件超过大小限制".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = res.chunk().await.map_err(|e| e.without_url().to_string())? {
        if chunk.len() > limit - bytes.len() {
            return Err("文件超过大小限制".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn validate(resource: Resource<'_>, bytes: &[u8]) -> Result<(), String> {
    match resource {
        Resource::Index => {
            let value: Value =
                serde_json::from_slice(bytes).map_err(|_| "插件索引不是有效 JSON".to_string())?;
            if !value.get("plugins").is_some_and(Value::is_array) {
                return Err("插件索引里没有 plugins 数组".into());
            }
            Ok(())
        }
        Resource::Package(expected) => {
            let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
                .map_err(|_| "不是有效的 .enj 插件包".to_string())?;
            let pack = {
                let file = archive
                    .by_name("pack.json")
                    .map_err(|_| "插件包里没有 pack.json".to_string())?;
                let mut data = Vec::new();
                file.take(MANIFEST_LIMIT + 1)
                    .read_to_end(&mut data)
                    .map_err(|e| e.to_string())?;
                if data.len() as u64 > MANIFEST_LIMIT {
                    return Err("pack.json 太大".into());
                }
                let value: Value = serde_json::from_slice(&data)
                    .map_err(|e| format!("pack.json 不是有效的 JSON：{e}"))?;
                Pack::from_json(&value)?
            };
            pack.check_compatibility()?;
            if pack.id != expected.id
                || pack.version != expected.version
                || pack.permissions.len() != expected.permissions.len()
                || pack.permissions.iter().any(|p| !expected.has(p))
            {
                return Err("插件包的 id、version 或 permissions 与商店声明不符".into());
            }
            let main = archive
                .by_name(&pack.main)
                .map_err(|_| format!("插件包里没有脚本 {}", pack.main))?;
            let mut script = String::new();
            main.take(PACKAGE_LIMIT as u64)
                .read_to_string(&mut script)
                .map_err(|e| format!("插件脚本不是可读的 UTF-8 文本：{e}"))?;
            Ok(())
        }
    }
}

async fn read_with<F, Fut>(
    address: &str,
    resource: Resource<'_>,
    mut fetch: F,
) -> Result<Vec<u8>, String>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Result<Vec<u8>, String>>,
{
    let limit = resource.limit();
    let mut failures = Vec::new();
    for (source, candidate) in candidates(address) {
        let result = fetch(candidate).await.and_then(|bytes| {
            if bytes.len() > limit {
                return Err("文件超过大小限制".into());
            }
            validate(resource, &bytes)?;
            Ok(bytes)
        });
        match result {
            Ok(bytes) => return Ok(bytes),
            Err(error) => failures.push(format!("{source}：{error}")),
        }
    }
    Err(format!(
        "{}读取失败（{}）",
        resource.label(),
        failures.join("；")
    ))
}

pub async fn read_index(address: &str) -> Result<String, String> {
    let bytes = read_with(address, Resource::Index, |candidate| async move {
        fetch_once(&candidate, INDEX_LIMIT, INDEX_TIMEOUT).await
    })
    .await?;
    String::from_utf8(bytes).map_err(|_| "插件索引不是有效的 UTF-8 文本".into())
}

pub async fn read_package(address: &str, expected: &Pack) -> Result<Vec<u8>, String> {
    expected.check_compatibility()?;
    let timeout = Resource::Package(expected).timeout();
    let limit = Resource::Package(expected).limit();
    read_with(address, Resource::Package(expected), |candidate| async move {
        fetch_once(&candidate, limit, timeout).await
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const INDEX_JSON: &[u8] = br#"{"plugins":[]}"#;

    fn pack_zip(id: &str, version: &str) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("pack.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        write!(
            zip,
            r#"{{"id":"{id}","name":"Test","version":"{version}","main":"main.js"}}"#
        )
        .unwrap();
        zip.start_file("main.js", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"console.log('ok');").unwrap();
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn only_official_repository_urls_use_mirrors() {
        assert!(eligible(crate::plugins::INDEX));
        assert!(eligible(
            "https://raw.githubusercontent.com/MCDFsteve/enji-plugins/main/dist/a-1.0.0.enj"
        ));
        for url in [
            "http://raw.githubusercontent.com/MCDFsteve/enji-plugins/main/plugins.json",
            "https://raw.githubusercontent.com.evil/MCDFsteve/enji-plugins/main/plugins.json",
            "https://user:secret@raw.githubusercontent.com/MCDFsteve/enji-plugins/main/plugins.json",
            "https://raw.githubusercontent.com:443/MCDFsteve/enji-plugins/main/plugins.json",
            "https://raw.githubusercontent.com/Other/enji-plugins/main/plugins.json",
            "https://raw.githubusercontent.com/MCDFsteve/enji-plugins_extra/main/plugins.json",
            "https://raw.githubusercontent.com/MCDFsteve/enji-plugins/main/../plugins.json",
            "https://raw.githubusercontent.com/MCDFsteve/enji-plugins/main/%2e%2e/plugins.json",
            "https://raw.githubusercontent.com/MCDFsteve/enji-plugins/main//plugins.json",
            "https://raw.githubusercontent.com/MCDFsteve/enji-plugins/main/plugins.json?token=1",
            "https://raw.githubusercontent.com/MCDFsteve/enji-plugins/main/plugins.json#frag",
            "https://example.com/plugins.json",
        ] {
            assert!(!eligible(url), "{url}");
            assert_eq!(candidates(url).len(), 1);
        }
    }

    #[test]
    fn original_success_stops_before_mirrors_and_invalid_responses_fall_back() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(async {
            let mut calls = Vec::new();
            let ok = read_with(crate::plugins::INDEX, Resource::Index, |url| {
                calls.push(url);
                async { Ok(INDEX_JSON.to_vec()) }
            })
            .await
            .unwrap();
            assert_eq!(ok, INDEX_JSON);
            assert_eq!(calls, vec![crate::plugins::INDEX]);

            let mut fallback_calls = Vec::new();
            let mut attempt = 0usize;
            let ok = read_with(crate::plugins::INDEX, Resource::Index, |url| {
                fallback_calls.push(url);
                attempt += 1;
                async move {
                    match attempt {
                        1 => Err("timeout".into()),
                        2 => Ok(b"<html>error</html>".to_vec()),
                        3 => Ok(br#"{"error":"bad"}"#.to_vec()),
                        _ => Ok(INDEX_JSON.to_vec()),
                    }
                }
            })
            .await
            .unwrap();
            assert_eq!(ok, INDEX_JSON);
            assert_eq!(fallback_calls.len(), 4);
        });
    }

    #[test]
    fn corrupt_or_mismatched_package_falls_back_to_next_mirror() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let expected = Pack::from_json(&serde_json::json!({
            "id": "enji.cover_pulse",
            "name": "Test",
            "version": "1.1.0",
        }))
        .unwrap();
        let valid_zip = pack_zip("enji.cover_pulse", "1.1.0");
        let wrong_zip = pack_zip("enji.other", "1.1.0");
        let url = "https://raw.githubusercontent.com/MCDFsteve/enji-plugins/main/dist/enji.cover_pulse-1.1.0.enj";
        rt.block_on(async {
            let mut attempt = 0usize;
            let bytes = read_with(url, Resource::Package(&expected), |_| {
                attempt += 1;
                let wrong = wrong_zip.clone();
                let valid = valid_zip.clone();
                async move {
                    match attempt {
                        1 => Ok(b"not a zip".to_vec()),
                        2 => Ok(wrong),
                        _ => Ok(valid),
                    }
                }
            })
            .await
            .unwrap();
            assert_eq!(bytes, valid_zip);
            assert_eq!(attempt, 3);
        });
    }
}
