//! Songs from NetEase kept on this device: the full song in the chosen
//! quality, its name, singers, album and cover written into the file, and
//! its lyrics beside it as an LRC (a translation as a second line at the
//! same moment), in a folder the library reads. NetEase gives only a
//! preview of a song for members to those who are not; such a song is not
//! downloaded. Which NetEase song each downloaded file is, is kept, so the
//! library knows it without searching.

use crate::{local, lyrics, store};
use enji_protocol::enji::Song;
use lofty::{
    config::WriteOptions,
    picture::{MimeType, Picture, PictureType},
    prelude::*,
    tag::Tag,
};
use ncm_api_rs::{ApiClient, Query};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

/// Where downloads go: the Music folder's `Enji` on a computer, the
/// `Downloads` folder among the music the Files app shows on an iPhone,
/// the app's own folder on Android, Apple Watch and Apple TV.
pub fn dir() -> PathBuf {
    #[cfg(target_os = "ios")]
    {
        store::phone_music_dir().unwrap_or_else(store::dir).join("Downloads")
    }
    #[cfg(any(target_os = "android", target_os = "tvos", target_os = "watchos"))]
    {
        store::dir().join("Downloads")
    }
    #[cfg(not(any(target_os = "ios", target_os = "android", target_os = "tvos", target_os = "watchos")))]
    {
        dirs::audio_dir().map(|music| music.join("Enji")).unwrap_or_else(|| store::dir().join("Downloads"))
    }
}

/// Whether the file at `url` is one Enji downloaded: it is in the
/// downloads folder.
pub fn holds(url: &str) -> bool {
    let folder = local::file_url(&dir());
    let folder = folder.trim_end_matches('/');
    url.strip_prefix(folder).is_some_and(|rest| rest.starts_with('/'))
}

/// The NetEase song of a downloaded file: its id, album and first singer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Origin {
    pub id: i64,
    pub album: i64,
    pub artist: i64,
}

/// The NetEase songs of the downloaded files, by the files' URLs.
#[derive(Default)]
pub struct Index(HashMap<String, Origin>);

fn index_path() -> PathBuf {
    store::dir().join("downloads.json")
}

impl Index {
    pub fn load() -> Index {
        let value: Value = fs::read(index_path()).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
        let read = |entry: &Value, key: &str| entry[key].as_i64().unwrap_or(0);
        Index(
            value
                .as_object()
                .map(|entries| {
                    entries
                        .iter()
                        .map(|(url, entry)| {
                            (url.clone(), Origin { id: read(entry, "id"), album: read(entry, "album"), artist: read(entry, "artist") })
                        })
                        .filter(|(_, origin)| origin.id > 0)
                        .collect()
                })
                .unwrap_or_default(),
        )
    }

    fn save(&self) {
        let entries: serde_json::Map<String, Value> = self
            .0
            .iter()
            .map(|(url, origin)| (url.clone(), json!({ "id": origin.id, "album": origin.album, "artist": origin.artist })))
            .collect();
        let _ = fs::create_dir_all(store::dir());
        let _ = fs::write(index_path(), Value::Object(entries).to_string());
    }

    pub fn insert(&mut self, url: String, origin: Origin) {
        self.0.insert(url, origin);
        self.save();
    }

    pub fn get(&self, url: &str) -> Option<Origin> {
        self.0.get(url).copied()
    }
}

/// A file name for `song`: `Singers - Name`, without the characters file
/// systems refuse, not too long.
fn file_stem(song: &Song) -> String {
    // Singers NetEase lists as `A / B` read `A, B` in a name.
    let artists = song.artists.replace(" / ", ", ");
    let name = if artists.is_empty() { song.name.clone() } else { format!("{artists} - {}", song.name) };
    let clean: String = name
        .chars()
        .map(|c| if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() { '_' } else { c })
        .collect();
    let clean = clean.trim().trim_matches('.').to_string();
    let clean: String = clean.chars().take(120).collect();
    if clean.is_empty() { song.id.to_string() } else { clean }
}

/// `stem.extension` in `folder`, or `stem (2).extension` and so on when a
/// file of that name is there already.
fn free_path(folder: &Path, stem: &str, extension: &str) -> PathBuf {
    let first = folder.join(format!("{stem}.{extension}"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|n| folder.join(format!("{stem} ({n}).{extension}")))
        .find(|path| !path.exists())
        .expect("a free name")
}

/// Lyrics as an LRC: each line at its moment, a translation as a second
/// line at the same moment.
fn lrc(lines: &[lyrics::Line]) -> String {
    let stamp = |time: f64| {
        let centis = (time.max(0.0) * 100.0).round() as i64;
        format!("[{:02}:{:02}.{:02}]", centis / 6000, centis / 100 % 60, centis % 100)
    };
    let mut text = String::new();
    for line in lines {
        text.push_str(&format!("{}{}\n", stamp(line.time), line.text));
        if let Some(translation) = line.translation.as_deref().filter(|t| !t.trim().is_empty()) {
            text.push_str(&format!("{}{}\n", stamp(line.time), translation));
        }
    }
    text
}

/// Write `song`'s name, singers, album and `cover` into the file at
/// `path`. A file lofty cannot tag keeps the music as it is.
fn tag(path: &Path, song: &Song, cover: Option<Vec<u8>>) {
    let Ok(mut file) = lofty::read_from_path(path) else { return };
    if file.primary_tag().is_none() {
        let kind = file.primary_tag_type();
        file.insert_tag(Tag::new(kind));
    }
    let Some(tag) = file.primary_tag_mut() else { return };
    tag.set_title(song.name.clone());
    tag.set_artist(song.artists.clone());
    if !song.album.is_empty() {
        tag.set_album(song.album.clone());
    }
    if let Some(data) = cover {
        let mime = if data.starts_with(&[0x89, b'P', b'N', b'G']) { MimeType::Png } else { MimeType::Jpeg };
        tag.push_picture(Picture::unchecked(data).pic_type(PictureType::CoverFront).mime_type(mime).build());
    }
    let _ = tag.save_to_path(path, WriteOptions::default());
}

/// Download `song` at `level` into the downloads folder, calling
/// `progress` with the part done as the music comes; the file's path, or
/// why it could not be downloaded.
pub async fn fetch(
    client: &ApiClient,
    http: &reqwest::Client,
    song: &Song,
    level: &str,
    progress: impl Fn(f64),
) -> Result<PathBuf, String> {
    let id = song.id.to_string();
    let response = crate::net::song_url(client, &song.name, &id, level).await?;
    let data = &response.body["data"][0];
    let url = data["url"].as_str().filter(|url| !url.is_empty()).ok_or_else(|| format!("《{}》暂时不能下载", song.name))?;
    if !data["freeTrialInfo"].is_null() {
        return Err(format!("《{}》需要黑胶会员，只能试听，不能下载完整版", song.name));
    }
    let extension = match data["type"].as_str().unwrap_or("mp3").to_lowercase().as_str() {
        "" => "mp3".to_string(),
        kind => kind.to_string(),
    };
    let lines = client
        .lyric_new(&Query::new().param("id", &id))
        .await
        .map(|r| {
            lyrics::parse(r.body["lrc"]["lyric"].as_str().unwrap_or(""), r.body["tlyric"]["lyric"].as_str().unwrap_or(""))
        })
        .unwrap_or_default();
    let cover = match song.artwork.url.starts_with("http") {
        true => match http.get(&song.artwork.url).send().await.and_then(|r| r.error_for_status()) {
            Ok(response) => response.bytes().await.ok().map(|bytes| bytes.to_vec()),
            Err(_) => None,
        },
        false => None,
    };

    let folder = dir();
    fs::create_dir_all(&folder).map_err(|e| format!("建不了下载文件夹 {}：{e}", folder.display()))?;
    let stem = file_stem(song);
    let partial = folder.join(format!(".{stem}.{extension}.part"));
    let mut response = http
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("下载《{}》失败：{e}", song.name))?;
    // NetEase says the size too, for a response that does not.
    let length = response.content_length().filter(|&n| n > 0).or(data["size"].as_u64()).unwrap_or(0);
    let mut file = fs::File::create(&partial).map_err(|e| format!("写不了 {}：{e}", partial.display()))?;
    let mut written = 0u64;
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if let Err(e) = file.write_all(&chunk) {
                    let _ = fs::remove_file(&partial);
                    return Err(format!("写不了 {}：{e}", partial.display()));
                }
                written += chunk.len() as u64;
                if length > 0 {
                    progress(written as f64 / length as f64);
                }
            }
            Ok(None) => break,
            Err(e) => {
                let _ = fs::remove_file(&partial);
                return Err(format!("下载《{}》时中断：{e}", song.name));
            }
        }
    }
    drop(file);
    if length > 0 && written < length {
        let _ = fs::remove_file(&partial);
        return Err(format!("下载《{}》时中断", song.name));
    }
    let path = free_path(&folder, &stem, &extension);
    fs::rename(&partial, &path).map_err(|e| format!("存不了 {}：{e}", path.display()))?;
    // Tagged under its own name: lofty knows a file's kind by its extension.
    tag(&path, song, cover);
    if !lines.is_empty() {
        let _ = fs::write(path.with_extension("lrc"), lrc(&lines));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lyrics_keep_their_translation_as_a_second_line() {
        let lines = lyrics::parse("[00:01.50]你好\n[01:02.25]再见\n", "[00:01.50]Hello\n");
        let text = lrc(&lines);
        assert_eq!(text, "[00:01.50]你好\n[00:01.50]Hello\n[01:02.25]再见\n");
        // Read back, it is the same lyrics.
        let again = lyrics::parse(&text, "");
        assert_eq!(again.len(), 2);
        assert_eq!(again[0].translation.as_deref(), Some("Hello"));
    }

    #[test]
    fn file_names_drop_what_file_systems_refuse() {
        let song = |name: &str, artists: &str| {
            crate::convert::song(&json!({ "id": 7, "name": name, "ar": [{ "name": artists }] }), 0, None)
        };
        assert_eq!(file_stem(&song("A/B: C?", "X")), "X - A_B_ C_");
        assert_eq!(file_stem(&song("", "")), "7");
    }
}
