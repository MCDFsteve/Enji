//! Checks that ncm-api-rs still talks to NetEase before Enji Music is built on it.
//!
//! `enji-probe anon`  — endpoints that work without an account
//! `enji-probe login` — QR login with the NetEase app; saves the cookie locally
//! `enji-probe auth`  — read-only checks with the saved cookie
//!
//! Requests are spaced out on purpose; nothing here writes to the account.

use std::{fs, path::PathBuf, time::Duration};

use anyhow::{bail, Context, Result};
use ncm_api_rs::{create_client, ApiClient, ApiResponse, Query};
use serde_json::Value;

const GAP: Duration = Duration::from_millis(1200);
const PUBLIC_PLAYLIST: &str = "3778678"; // 热歌榜

#[tokio::main]
async fn main() -> Result<()> {
    match std::env::args().nth(1).as_deref() {
        Some("anon") => anon().await,
        Some("login") => login().await,
        Some("auth") => auth().await,
        _ => {
            eprintln!("usage: enji-probe <anon|login|auth>");
            Ok(())
        }
    }
}

fn cookie_path() -> Result<PathBuf> {
    let home = std::env::var("HOME").context("HOME not set")?;
    Ok(PathBuf::from(home).join("Library/Application Support/Enji/probe-cookie.txt"))
}

fn code(res: &ApiResponse) -> i64 {
    res.body["code"].as_i64().unwrap_or(res.status)
}

fn report(name: &str, ok: bool, detail: String) {
    println!("{} {name}: {detail}", if ok { "OK  " } else { "FAIL" });
}

async fn call<F, Fut>(name: &str, f: F) -> Option<ApiResponse>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<ApiResponse, ncm_api_rs::NcmError>>,
{
    tokio::time::sleep(GAP).await;
    match f().await {
        Ok(res) => Some(res),
        Err(e) => {
            report(name, false, format!("request error: {e}"));
            None
        }
    }
}

/// NetEase returns `unikey` at the top level; the Node API wraps it in `data`.
fn qr_key(body: &Value) -> Option<String> {
    body["unikey"].as_str().or(body["data"]["unikey"].as_str()).map(str::to_string)
}

fn len(v: &Value) -> usize {
    v.as_array().map_or(0, Vec::len)
}

async fn anon() -> Result<()> {
    let c = create_client(None);

    let q = Query::new().param("keywords", "海猫鸣泣之时").param("type", "1").param("limit", "5");
    let Some(r) = call("cloudsearch", || c.cloudsearch(&q)).await else { bail!("search failed") };
    let song = &r.body["result"]["songs"][0];
    let song_id = song["id"].as_i64().unwrap_or(0).to_string();
    let artist_id = song["ar"][0]["id"].as_i64().unwrap_or(0).to_string();
    report(
        "cloudsearch",
        code(&r) == 200 && song_id != "0",
        format!("{} results, first: {} (id {song_id}, fee {})", len(&r.body["result"]["songs"]), song["name"], song["fee"]),
    );

    let q = Query::new().param("ids", &song_id);
    if let Some(r) = call("song_detail", || c.song_detail(&q)).await {
        let p = &r.body["privileges"][0];
        report("song_detail", code(&r) == 200, format!("name {}, privilege st {} pl {}", r.body["songs"][0]["name"], p["st"], p["pl"]));
    }

    let q = Query::new().param("id", &song_id);
    if let Some(r) = call("lyric_new", || c.lyric_new(&q)).await {
        let has = |k: &str| r.body[k]["lyric"].as_str().is_some_and(|s| !s.is_empty());
        report("lyric_new", code(&r) == 200, format!("lrc {}, word-by-word yrc {}, translation {}", has("lrc"), has("yrc"), has("tlyric")));
    }

    // The first hit may well be grey, so pick a free, playable song for the URL check.
    let q = Query::new().param("keywords", "钢琴曲").param("type", "1").param("limit", "30");
    let playable = call("cloudsearch (free song)", || c.cloudsearch(&q)).await.and_then(|r| {
        r.body["result"]["songs"].as_array()?.iter().find_map(|s| {
            let free = matches!(s["fee"].as_i64(), Some(0 | 8));
            let open = s["privilege"]["st"].as_i64().is_some_and(|st| st >= 0);
            (free && open).then(|| s["id"].as_i64().unwrap_or(0).to_string())
        })
    });
    for (label, id) in [("grey song", Some(song_id.clone())), ("free song", playable)] {
        let Some(id) = id else {
            report("song_url_v1", false, format!("{label}: no candidate"));
            continue;
        };
        let q = Query::new().param("id", &id).param("level", "standard");
        if let Some(r) = call("song_url_v1", || c.song_url_v1(&q)).await {
            let d = &r.body["data"][0];
            let has_url = d["url"].as_str().is_some();
            report(
                "song_url_v1 (anonymous)",
                code(&r) == 200,
                format!("{label} {id}: url {has_url}, level {}, br {}, trial {}", d["level"], d["br"], !d["freeTrialInfo"].is_null()),
            );
        }
    }

    let q = Query::new().param("type", "0").param("id", &song_id).param("sortType", "2").param("pageSize", "3");
    if let Some(r) = call("comment_new", || c.comment_new(&q)).await {
        let data = &r.body["data"];
        report("comment_new", code(&r) == 200, format!("total {}, page {}", data["totalCount"], len(&data["comments"])));
    }

    let q = Query::new().param("id", &song_id);
    if let Some(r) = call("song_wiki_summary", || c.song_wiki_summary(&q)).await {
        report("song_wiki_summary", code(&r) == 200, format!("blocks {}", len(&r.body["data"]["blocks"])));
    }

    let q = Query::new().param("id", &artist_id);
    if let Some(r) = call("artist_desc", || c.artist_desc(&q)).await {
        let brief = r.body["briefDesc"].as_str().unwrap_or("");
        report("artist_desc", code(&r) == 200, format!("brief {} chars, sections {}", brief.chars().count(), len(&r.body["introduction"])));
    }

    let q = Query::new().param("id", PUBLIC_PLAYLIST);
    if let Some(r) = call("playlist_detail", || c.playlist_detail(&q)).await {
        let p = &r.body["playlist"];
        report("playlist_detail", code(&r) == 200, format!("trackCount {}, trackIds {}", p["trackCount"], len(&p["trackIds"])));
    }

    let q = Query::new().param("id", PUBLIC_PLAYLIST).param("limit", "50");
    if let Some(r) = call("playlist_track_all", || c.playlist_track_all(&q)).await {
        report("playlist_track_all", code(&r) == 200, format!("songs {}", len(&r.body["songs"])));
    }

    let q = Query::new();
    if let Some(r) = call("login_qr_key", || c.login_qr_key(&q)).await {
        let key = qr_key(&r.body).unwrap_or_default();
        report("login_qr_key", !key.is_empty(), format!("key {}", if key.is_empty() { "missing" } else { "present" }));
    }
    Ok(())
}

async fn login() -> Result<()> {
    let c = create_client(None);
    let r = c.login_qr_key(&Query::new()).await?;
    let key = qr_key(&r.body).context("no qr key")?;
    let r = c.login_qr_create(&Query::new().param("key", &key)).await?;
    let url = r.body["data"]["qrurl"].as_str().context("no qr url")?;

    let qr = qrcode::QrCode::new(url.as_bytes())?;
    println!("{}", qr.render::<qrcode::render::unicode::Dense1x2>().quiet_zone(true).build());
    println!("用网易云音乐 App 扫码并确认（建议用小号）。");

    for _ in 0..60 {
        tokio::time::sleep(Duration::from_secs(3)).await;
        let r = c.login_qr_check(&Query::new().param("key", &key)).await?;
        match code(&r) {
            801 => {}
            802 => println!("已扫码，等待在手机上确认…"),
            803 => {
                let cookie = r
                    .cookie
                    .iter()
                    .filter_map(|s| s.split(';').next())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join("; ");
                if !cookie.contains("MUSIC_U=") {
                    bail!("登录成功但没有拿到 MUSIC_U");
                }
                let path = cookie_path()?;
                fs::create_dir_all(path.parent().unwrap())?;
                fs::write(&path, &cookie)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
                }
                println!("登录成功，Cookie 已存到 {}（权限 600）", path.display());
                return Ok(());
            }
            800 => bail!("二维码过期了，重新运行 login"),
            other => println!("状态码 {other}：{}", r.body["message"]),
        }
    }
    bail!("等待超时")
}

async fn auth() -> Result<()> {
    let cookie = fs::read_to_string(cookie_path()?).context("还没登录，先运行 enji-probe login")?;
    let c: ApiClient = create_client(Some(cookie.trim().to_string()));

    let q = Query::new();
    let Some(r) = call("login_status", || c.login_status(&q)).await else { bail!("login_status failed") };
    let profile = &r.body["profile"];
    let uid = profile["userId"].as_i64().unwrap_or(0).to_string();
    report("login_status", uid != "0", format!("uid {uid}, vipType {}", profile["vipType"]));
    if uid == "0" {
        bail!("Cookie 无效或已过期");
    }

    let q = Query::new().param("uid", &uid).param("limit", "50");
    let Some(r) = call("user_playlist", || c.user_playlist(&q)).await else { bail!("user_playlist failed") };
    let lists = r.body["playlist"].as_array().cloned().unwrap_or_default();
    report("user_playlist", code(&r) == 200, format!("{} playlists", lists.len()));

    let mut first_song = None;
    for p in lists.iter().take(5) {
        let id = p["id"].as_i64().unwrap_or(0).to_string();
        let q = Query::new().param("id", &id);
        let Some(d) = call("playlist_detail", || c.playlist_detail(&q)).await else { continue };
        let count = d.body["playlist"]["trackCount"].as_i64().unwrap_or(0);
        let ids = len(&d.body["playlist"]["trackIds"]);
        let limit = ids.max(1).to_string();
        let q = Query::new().param("id", &id).param("limit", &limit);
        let Some(t) = call("playlist_track_all", || c.playlist_track_all(&q)).await else { continue };
        let songs = len(&t.body["songs"]);
        if first_song.is_none() {
            first_song = t.body["songs"][0]["id"].as_i64();
        }
        report(
            "playlist",
            code(&t) == 200,
            format!("{id}: trackCount {count}, trackIds {ids}, returned {songs}, missing {}", count as usize - songs.min(count as usize)),
        );
    }

    if let Some(song) = first_song {
        let q = Query::new().param("id", &song.to_string()).param("level", "exhigh");
        if let Some(r) = call("song_url_v1", || c.song_url_v1(&q)).await {
            let d = &r.body["data"][0];
            report("song_url_v1 (exhigh)", d["url"].as_str().is_some(), format!("level {}, br {}, trial {}", d["level"], d["br"], !d["freeTrialInfo"].is_null()));
        }
    }

    let q = Query::new().param("uid", &uid);
    if let Some(r) = call("likelist", || c.likelist(&q)).await {
        report("likelist", code(&r) == 200, format!("{} liked songs", len(&r.body["ids"])));
    }
    Ok(())
}
