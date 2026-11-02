//! Covers kept on this device ahead of need. The engine guesses the songs
//! likely to play soon (the next ones in play order, the one before, the
//! first of a list just opened) and fetches their covers while the music
//! plays, so a song's cover shows the moment it starts instead of after a
//! download. The newest few hundred are kept.

use enji_protocol::enji::Song;
use sha1::{Digest, Sha1};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

/// How many covers are kept; the oldest go first.
const KEPT: usize = 300;

fn dir() -> PathBuf {
    crate::store::dir().join("covers")
}

fn path_of(url: &str) -> PathBuf {
    let name: String = Sha1::digest(url.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect();
    dir().join(format!("{name}.jpg"))
}

/// `song` with each cover fetched already read from this device.
pub fn localized(mut song: Song) -> Song {
    for picture in [&mut song.cover, &mut song.artwork] {
        let path = path_of(&picture.url);
        if picture.url.starts_with("http") && path.is_file() {
            picture.url = crate::local::file_url(&path);
        }
    }
    song
}

/// The covers being fetched now.
fn fetching() -> &'static Mutex<HashSet<String>> {
    static FETCHING: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    FETCHING.get_or_init(Mutex::default)
}

/// Fetch the covers of `songs` this device does not keep yet, one after
/// another, away from the worker.
pub fn prefetch(http: &reqwest::Client, songs: &[Song]) {
    let mut wanted = Vec::new();
    {
        let mut fetching = fetching().lock().unwrap_or_else(|e| e.into_inner());
        for song in songs {
            for url in [&song.artwork.url, &song.cover.url] {
                if url.starts_with("http") && !path_of(url).is_file() && fetching.insert(url.clone()) {
                    wanted.push(url.clone());
                }
            }
        }
    }
    if wanted.is_empty() {
        return;
    }
    let http = http.clone();
    tokio::spawn(async move {
        for url in wanted {
            let got = async { http.get(&url).send().await?.error_for_status()?.bytes().await };
            if let Ok(bytes) = got.await
                && !bytes.is_empty()
            {
                let path = path_of(&url);
                let partial = path.with_extension("part");
                let _ = std::fs::create_dir_all(dir());
                // Whole or not at all: a cover is read only once it is in place.
                if std::fs::write(&partial, &bytes).is_ok() {
                    let _ = std::fs::rename(&partial, &path);
                }
            }
            fetching().lock().unwrap_or_else(|e| e.into_inner()).remove(&url);
        }
        tidy();
    });
}

/// Keep the newest covers only.
fn tidy() {
    let Ok(entries) = std::fs::read_dir(dir()) else { return };
    let mut kept: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|extension| extension == "jpg"))
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    if kept.len() <= KEPT {
        return;
    }
    kept.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, path) in kept.drain(KEPT..) {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_cover_has_a_file_of_its_own() {
        let small = path_of("https://p1.music.126.net/a.jpg?param=200y200");
        let large = path_of("https://p1.music.126.net/a.jpg?param=800y800");
        assert_ne!(small, large);
        assert_eq!(small, path_of("https://p1.music.126.net/a.jpg?param=200y200"));
        assert_eq!(small.extension().unwrap(), "jpg");
    }
}
