//! Music files in the folders the user added: found by walking them, each
//! known by the SHA-1 of its first 10 KB as Misuzu Music knows it, with the
//! tags, cover and lyrics the file carries. What was read is kept, so a file
//! that has not changed is not read again. Files are read on several threads
//! and reported as they are read, so the first songs can be played while the
//! rest are read; each cover is kept small, once per picture, and the full
//! picture is taken from the file when the song plays. Folders and files
//! are known by their URLs and read through `bern::files`: `file:` URLs,
//! and on Android the documents of a folder from the system's picker.

use crate::{lyrics, store};
use enji_kit::{
    convert,
    copies::{name_key, near},
};
use bern::{files::Entry, media::MediaSource};
use enji_protocol::enji::Song;
use lofty::prelude::*;
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{BufReader, BufWriter, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant, UNIX_EPOCH},
};

/// The longer side of a cover as lists show it, in pixels.
const THUMB: u32 = 256;
/// Read files are reported at least this often while a scan goes on.
const REPORT_EVERY: Duration = Duration::from_millis(400);

/// The kinds of files read as music.
const KINDS: &[&str] = &["mp3", "flac", "m4a", "aac", "wav", "ogg", "oga", "aif", "aiff", "alac"];

/// A music file as it was last read.
#[derive(Clone, Debug)]
pub struct Track {
    /// Where the file is, which reads and plays it.
    pub url: String,
    /// The file's name, such as `01 Intro.mp3`.
    pub name: String,
    /// The URL of the added folder it was found in.
    pub folder: String,
    /// The `.lrc` beside it, if there is one.
    pub lyrics: Option<String>,
    pub hash: String,
    pub title: String,
    pub artists: String,
    pub album: String,
    pub seconds: f64,
    /// The picture the file carries, written out once.
    pub cover: Option<PathBuf>,
    /// The cover found on NetEase for a file without one.
    pub online_cover: String,
    /// The same song on NetEase, 0 for none; its album and first singer.
    pub netease_id: i64,
    pub netease_album: i64,
    pub netease_artist: i64,
    /// How it was matched: not yet (empty), `auto`, `manual`, or `none`
    /// when nothing was found or the person said it is no NetEase song.
    pub matched: String,
    size: u64,
    modified: i64,
    /// When it came into the library, in milliseconds since 1970.
    pub added: i64,
}

/// The library as it is kept: the folders and what was found in them.
#[derive(Clone, Debug, Default)]
pub struct Library {
    pub folders: Vec<Folder>,
    pub tracks: Vec<Track>,
}

/// A folder the user added.
#[derive(Clone, Debug, PartialEq)]
pub struct Folder {
    pub url: String,
    /// The name the user knows it by.
    pub name: String,
}

impl Folder {
    /// The folder at `url`, by the name the system gives it.
    pub fn new(url: String) -> Self {
        let name = bern::files::name(&url).unwrap_or_default();
        Self { url, name }
    }

    /// Where it is on this device, for a local folder; empty for one the
    /// system holds, such as a folder from Android's picker.
    pub fn place(&self) -> String {
        bern::files::file_path(&self.url).map(|path| path.display().to_string()).unwrap_or_default()
    }
}

fn library_path() -> PathBuf {
    store::dir().join("library.json")
}

fn covers_dir() -> PathBuf {
    store::dir().join("covers")
}

/// Small covers for lists, one per picture.
fn thumbs_dir() -> PathBuf {
    covers_dir().join("thumbs")
}

/// Full pictures, taken from a file when it plays.
fn full_dir() -> PathBuf {
    covers_dir().join("full")
}

impl Library {
    pub fn load() -> Self {
        fs::read_to_string(library_path())
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .map(|saved| Self::from_json(&saved))
            .unwrap_or_default()
    }

    fn from_json(saved: &Value) -> Self {
        let text = |value: &Value, key: &str| value[key].as_str().unwrap_or("").to_string();
        let folders: Vec<Folder> = saved["folders"]
            .as_array()
            .map(|all| {
                all.iter()
                    .map(|one| match one.as_str() {
                        // Kept by its path before folders were kept by URL.
                        Some(path) => Folder::new(file_url(Path::new(path))),
                        None => Folder { url: text(one, "url"), name: text(one, "name") },
                    })
                    .filter(|folder| !folder.url.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        // A file kept by its path, from before files were kept by URL: its
        // folder the added one its path is in.
        let by_path = |path: &str| {
            let path = Path::new(path);
            let folder = folders
                .iter()
                .find(|folder| bern::files::file_path(&folder.url).is_some_and(|dir| path.starts_with(dir)))
                .map(|folder| folder.url.clone())
                .unwrap_or_default();
            let name = path.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_default();
            (file_url(path), name, folder)
        };
        Self {
            tracks: saved["tracks"]
                .as_array()
                .map(|all| {
                    all.iter()
                        .map(|one| {
                            let (url, name, folder) = match one["url"].as_str() {
                                Some(url) => (url.to_string(), text(one, "name"), text(one, "folder")),
                                None => by_path(&text(one, "path")),
                            };
                            (one, url, name, folder)
                        })
                        .map(|(one, url, name, folder)| Track {
                            url,
                            name,
                            folder,
                            lyrics: one["lyrics"].as_str().filter(|url| !url.is_empty()).map(str::to_string),
                            hash: text(one, "hash"),
                            title: text(one, "title"),
                            artists: text(one, "artists"),
                            album: text(one, "album"),
                            seconds: one["seconds"].as_f64().unwrap_or(0.0),
                            cover: one["cover"].as_str().filter(|path| !path.is_empty()).map(PathBuf::from),
                            online_cover: text(one, "online_cover"),
                            netease_id: one["netease_id"].as_i64().unwrap_or(0),
                            netease_album: one["netease_album"].as_i64().unwrap_or(0),
                            netease_artist: one["netease_artist"].as_i64().unwrap_or(0),
                            matched: text(one, "matched"),
                            size: one["size"].as_u64().unwrap_or(0),
                            modified: one["modified"].as_i64().unwrap_or(0),
                            added: one["added"].as_i64().unwrap_or(0),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            folders,
        }
    }

    pub fn save(&self) {
        let tracks: Vec<Value> = self
            .tracks
            .iter()
            .map(|track| {
                json!({
                    "url": track.url,
                    "name": track.name,
                    "folder": track.folder,
                    "lyrics": track.lyrics.clone().unwrap_or_default(),
                    "hash": track.hash,
                    "title": track.title,
                    "artists": track.artists,
                    "album": track.album,
                    "seconds": track.seconds,
                    "cover": track.cover.as_ref().map(|path| path.to_string_lossy().to_string()).unwrap_or_default(),
                    "online_cover": track.online_cover,
                    "netease_id": track.netease_id,
                    "netease_album": track.netease_album,
                    "netease_artist": track.netease_artist,
                    "matched": track.matched,
                    "size": track.size,
                    "modified": track.modified,
                    "added": track.added,
                })
            })
            .collect();
        let folders: Vec<Value> = self.folders.iter().map(|folder| json!({ "url": folder.url, "name": folder.name })).collect();
        let _ = fs::create_dir_all(store::dir());
        let _ = fs::write(library_path(), json!({ "folders": folders, "tracks": tracks }).to_string());
    }

    /// The folders read again: new and changed files read, files gone
    /// dropped, the rest as they were. Files are read on several threads;
    /// `report` gets what was read every little while, with how many of how
    /// many files have been read. The files of one scan come into the
    /// library in the order of their paths.
    pub fn scan(&self, now: i64, report: impl Fn(Vec<Track>, usize, usize)) -> Library {
        let mut found = Vec::new();
        let mut seen = HashSet::new();
        for folder in &self.folders {
            walk(folder, &mut found, &mut seen);
        }
        let known: HashMap<&str, &Track> = self.tracks.iter().map(|track| (track.url.as_str(), track)).collect();
        // Each file as it was, or the files to read: their places, times
        // and what was known of them.
        let mut tracks: Vec<Option<Track>> = vec![None; found.len()];
        let mut wanted = Vec::new();
        for (place, file) in found.iter().enumerate() {
            let modified = file.entry.modified.and_then(|time| time.duration_since(UNIX_EPOCH).ok()).map_or(0, |since| since.as_millis() as i64);
            let known = known.get(file.entry.url.as_str()).copied();
            // A cover kept full size, from before covers were kept small,
            // is read again.
            let small = |track: &Track| track.cover.as_ref().is_none_or(|cover| cover.starts_with(thumbs_dir()));
            match known {
                Some(track) if track.size == file.entry.size && track.modified == modified && small(track) => {
                    tracks[place] = Some(Track { folder: file.folder.clone(), lyrics: file.lyrics.clone(), ..track.clone() });
                }
                _ => wanted.push((place, modified, known)),
            }
        }
        let total = wanted.len();
        if total > 0 {
            let next = AtomicUsize::new(0);
            let (send, receive) = mpsc::channel();
            let threads = std::thread::available_parallelism().map_or(2, |n| n.get()).clamp(1, 8);
            std::thread::scope(|scope| {
                for _ in 0..threads {
                    let (next, send, wanted, found) = (&next, send.clone(), &wanted, &found);
                    scope.spawn(move || {
                        while let Some(&(place, modified, known)) = wanted.get(next.fetch_add(1, Ordering::Relaxed)) {
                            // A new file is newer than the ones after it,
                            // so the newest-first library lists them in order.
                            let added = known.map_or(now - place as i64, |track| track.added);
                            let mut track = read(&found[place], modified, added);
                            if let Some(track) = track.as_mut()
                                && let Some(known) = known.filter(|known| known.hash == track.hash)
                            {
                                track.online_cover = known.online_cover.clone();
                                track.netease_id = known.netease_id;
                                track.netease_album = known.netease_album;
                                track.netease_artist = known.netease_artist;
                                track.matched = known.matched.clone();
                            }
                            if send.send((place, track)).is_err() {
                                break;
                            }
                        }
                    });
                }
                drop(send);
                let mut batch = Vec::new();
                let mut done = 0;
                let mut reported = Instant::now();
                for (place, track) in receive {
                    done += 1;
                    if let Some(track) = track {
                        batch.push(track.clone());
                        tracks[place] = Some(track);
                    }
                    if reported.elapsed() >= REPORT_EVERY || done == total {
                        report(std::mem::take(&mut batch), done, total);
                        reported = Instant::now();
                    }
                }
            });
        }
        Library { folders: self.folders.clone(), tracks: tracks.into_iter().flatten().collect() }
    }
}

/// The small covers no song of `library` uses any more removed, and the
/// full-size ones kept before covers were kept small.
pub fn tidy_covers(library: &Library) {
    let used: HashSet<&Path> = library.tracks.iter().filter_map(|track| track.cover.as_deref()).collect();
    for dir in [covers_dir(), thumbs_dir()] {
        let Ok(entries) = fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_file()) && !used.contains(path.as_path()) {
                let _ = fs::remove_file(path);
            }
        }
    }
}

/// A music file a scan found: in which folder, and the `.lrc` beside it.
struct Found {
    entry: Entry,
    folder: String,
    lyrics: Option<String>,
}

/// The music files in `folder` and the folders inside it, hidden ones left
/// out, in the order of their places in it; a file another folder had
/// already is left out too.
fn walk(folder: &Folder, found: &mut Vec<Found>, seen: &mut HashSet<String>) {
    let Ok(mut entries) = bern::files::walk(&folder.url, |entry| !entry.name.starts_with('.')) else { return };
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let lyrics: HashMap<&str, &str> = entries
        .iter()
        .filter(|entry| !entry.folder && extension(&entry.name) == "lrc")
        .map(|entry| (without_extension(&entry.path), entry.url.as_str()))
        .collect();
    for entry in &entries {
        if !entry.folder && KINDS.contains(&extension(&entry.name).as_str()) && seen.insert(entry.url.clone()) {
            found.push(Found {
                lyrics: lyrics.get(without_extension(&entry.path)).map(|url| url.to_string()),
                entry: entry.clone(),
                folder: folder.url.clone(),
            });
        }
    }
}

/// A file name's extension, lowercased; empty for none.
fn extension(name: &str) -> String {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext.to_lowercase(),
        _ => String::new(),
    }
}

/// A name or a place in a folder without the extension of its last name.
fn without_extension(path: &str) -> &str {
    let name = path.rfind('/').map_or(0, |slash| slash + 1);
    match path[name..].rfind('.') {
        Some(dot) if dot > 0 => &path[..name + dot],
        _ => path,
    }
}

/// The SHA-1 of the first 10 KB `file` holds from where it is, in hex.
fn hash(file: impl Read) -> Option<String> {
    let mut start = Vec::with_capacity(10 * 1024);
    file.take(10 * 1024).read_to_end(&mut start).ok()?;
    Some(Sha1::digest(&start).iter().map(|byte| format!("{byte:02x}")).collect())
}

/// The tags, length and pictures of the music file `file`, named `name`:
/// of the kind its name says, or else the one its first bytes show.
fn tagged(file: impl Read + Seek, name: &str) -> Option<lofty::file::TaggedFile> {
    let probe = lofty::probe::Probe::new(BufReader::new(file));
    let probe = match lofty::file::FileType::from_ext(extension(name)) {
        Some(kind) => probe.set_file_type(kind),
        None => probe.guess_file_type().ok()?,
    };
    probe.read().ok()
}

/// A music file's tags, length and picture; its name for a title when it
/// has none.
fn read(found: &Found, modified: i64, added: i64) -> Option<Track> {
    let mut file = bern::files::open(&found.entry.url).ok()?;
    let hash = hash(&mut file)?;
    let name = found.entry.name.clone();
    let mut track = Track {
        url: found.entry.url.clone(),
        title: without_extension(&name).to_string(),
        name,
        folder: found.folder.clone(),
        lyrics: found.lyrics.clone(),
        hash,
        artists: String::new(),
        album: String::new(),
        seconds: 0.0,
        cover: None,
        online_cover: String::new(),
        netease_id: 0,
        netease_album: 0,
        netease_artist: 0,
        matched: String::new(),
        size: found.entry.size,
        modified,
        added,
    };
    let Some(file) = file.seek(SeekFrom::Start(0)).ok().and_then(|_| tagged(file, &track.name)) else {
        return Some(track);
    };
    track.seconds = file.properties().duration().as_secs_f64();
    if let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) {
        if let Some(title) = tag.title().filter(|title| !title.trim().is_empty()) {
            track.title = title.trim().to_string();
        }
        track.artists = tag.artist().map(|artist| artist.trim().to_string()).unwrap_or_default();
        track.album = tag.album().map(|album| album.trim().to_string()).unwrap_or_default();
        if let Some(picture) = tag.pictures().first() {
            track.cover = thumbnail(picture.data());
        }
    }
    Some(track)
}

/// A name for a picture: the SHA-1 of its bytes, so a picture many files
/// carry, as an album's, is kept once.
fn picture_key(data: &[u8]) -> String {
    Sha1::digest(data).iter().take(12).map(|byte| format!("{byte:02x}")).collect()
}

/// The small cover lists show for a picture, written once: its longer side
/// `THUMB` pixels, as a JPEG.
fn thumbnail(data: &[u8]) -> Option<PathBuf> {
    let path = thumbs_dir().join(format!("{}.jpg", picture_key(data)));
    if path.exists() {
        return Some(path);
    }
    let picture = image::load_from_memory(data).ok()?;
    let small = picture.thumbnail(THUMB, THUMB).into_rgb8();
    fs::create_dir_all(thumbs_dir()).ok()?;
    // Written aside and moved in, so a thread reading the same picture
    // never finds half a file.
    let partial = path.with_extension(format!("{:?}.part", std::thread::current().id()).replace(['(', ')'], ""));
    let written = fs::File::create(&partial).ok().and_then(|file| {
        let mut out = BufWriter::new(file);
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85).encode_image(&small).ok()
    });
    if written.is_none() || fs::rename(&partial, &path).is_err() {
        let _ = fs::remove_file(&partial);
        return None;
    }
    Some(path)
}

/// The tags of the file at `url`, named `name`.
fn tags_of(url: &str, name: &str) -> Option<lofty::file::TaggedFile> {
    tagged(bern::files::open(url).ok()?, name)
}

/// The full picture a file carries, written out the first time its song
/// plays; `None` for a file without one.
pub fn artwork(url: &str, name: &str) -> Option<PathBuf> {
    let file = tags_of(url, name)?;
    let tag = file.primary_tag().or_else(|| file.first_tag())?;
    let picture = tag.pictures().first()?;
    let ext = match picture.mime_type().map(|mime| mime.as_str()) {
        Some("image/png") => "png",
        _ => "jpg",
    };
    let full = full_dir().join(format!("{}.{ext}", picture_key(picture.data())));
    if !full.exists() {
        fs::create_dir_all(full_dir()).ok()?;
        fs::write(&full, picture.data()).ok()?;
    }
    Some(full)
}

/// The lyrics that come with the file at `url`, named `name`: the `.lrc`
/// beside it at `beside`, or timed lyrics in its tags.
pub fn lyrics(url: &str, name: &str, beside: Option<&str>) -> Vec<lyrics::Line> {
    let beside = beside.and_then(|beside| {
        let mut text = String::new();
        bern::files::open(beside).ok()?.read_to_string(&mut text).ok()?;
        Some(lyrics::parse(&text, ""))
    });
    if let Some(lines) = beside.filter(|lines| !lines.is_empty()) {
        return lines;
    }
    let Some(file) = tags_of(url, name) else { return Vec::new() };
    let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) else { return Vec::new() };
    tag.get_string(ItemKey::Lyrics)
        .or_else(|| tag.get_string(ItemKey::UnsyncLyrics))
        .map(|text| lyrics::parse(text, ""))
        .unwrap_or_default()
}

/// A `file:` URL for `path`, its special characters escaped.
pub fn file_url(path: &Path) -> String {
    bern::resources::file_url(path)
}

/// The id of a local song: below 0, from its hash, so it never meets a
/// NetEase id.
pub fn id(hash: &str) -> i64 {
    let high = i64::from_str_radix(hash.get(..15).unwrap_or("1"), 16).unwrap_or(1);
    -(high.max(1))
}

/// A track as a song of the library at `index`.
pub fn song(track: &Track, index: usize) -> Song {
    let cover = match (&track.cover, track.online_cover.is_empty()) {
        (Some(cover), _) => MediaSource::new(file_url(cover)),
        (None, false) => convert::cover(&track.online_cover, 200),
        (None, true) => MediaSource::new(""),
    };
    let artwork = match (&track.cover, track.online_cover.is_empty()) {
        (Some(cover), _) => MediaSource::new(file_url(cover)),
        (None, false) => convert::cover(&track.online_cover, 800),
        (None, true) => MediaSource::new(""),
    };
    Song {
        id: if track.netease_id > 0 { track.netease_id } else { id(&track.hash) },
        index: index as i64,
        rank: index as i64 + 1,
        name: track.title.clone(),
        artists: track.artists.clone(),
        album: track.album.clone(),
        cover,
        artwork,
        seconds: track.seconds,
        duration: convert::clock(track.seconds),
        unavailable: false,
        album_id: track.netease_album,
        artist_id: track.netease_artist,
        file: track.url.clone(),
        plays: 0,
        added: track.added,
        place: index as i64,
    }
}

/// How well a file keeps the music, to choose among copies of one song:
/// lossless first, then the larger.
fn quality(track: &Track) -> (u8, u64) {
    let rank = match extension(&track.name).as_str() {
        "flac" | "alac" | "wav" | "aif" | "aiff" => 3,
        "m4a" | "aac" => 2,
        "mp3" => 1,
        _ => 0,
    };
    (rank, track.size)
}

/// The tracks as songs of the library, the newest first: copies of one
/// song — the same NetEase song, or the same name, singer and length —
/// shown once, by their best file.
pub fn library_songs(tracks: &[Track]) -> Vec<Song> {
    let mut sorted: Vec<&Track> = tracks.iter().collect();
    sorted.sort_by(|a, b| b.added.cmp(&a.added).then_with(|| a.title.cmp(&b.title)));
    let mut kept: Vec<&Track> = Vec::new();
    // The kept songs by NetEase id, and by name and singer.
    let mut by_id: HashMap<i64, usize> = HashMap::new();
    let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
    for track in sorted {
        let key = name_key(&track.title, &track.artists);
        let same = if track.netease_id > 0 {
            by_id.get(&track.netease_id).copied()
        } else {
            None
        }
        .or_else(|| {
            // The same name and length, unless both are NetEase songs and
            // so told apart by their ids.
            by_name.get(&key)?.iter().copied().find(|&place| {
                let other = kept[place];
                !(track.netease_id > 0 && other.netease_id > 0) && near(track.seconds, other.seconds)
            })
        });
        match same {
            Some(place) => {
                if quality(track) > quality(kept[place]) {
                    kept[place] = track;
                }
                if track.netease_id > 0 {
                    by_id.entry(track.netease_id).or_insert(place);
                }
            }
            None => {
                let place = kept.len();
                kept.push(track);
                if track.netease_id > 0 {
                    by_id.insert(track.netease_id, place);
                }
                by_name.entry(key).or_default().push(place);
            }
        }
    }
    kept.iter().enumerate().map(|(i, track)| song(track, i)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(name: &str, title: &str, seconds: f64, netease_id: i64, size: u64) -> Track {
        Track {
            url: name.into(),
            name: name.into(),
            folder: String::new(),
            lyrics: None,
            hash: name.into(),
            title: title.into(),
            artists: "Singer".into(),
            album: String::new(),
            seconds,
            cover: None,
            online_cover: String::new(),
            netease_id,
            netease_album: 0,
            netease_artist: 0,
            matched: String::new(),
            size,
            modified: 0,
            added: 0,
        }
    }

    #[test]
    fn copies_of_one_song_are_shown_once_by_their_best_file() {
        let songs = library_songs(&[
            track("a.mp3", "Song", 200.0, 0, 5),
            // The same name and length: one song, the lossless file kept.
            track("a.flac", "song ", 201.0, 0, 30),
            // The same name, another length: another recording.
            track("b.mp3", "Song", 260.0, 0, 5),
            track("c.mp3", "Other", 100.0, 7, 5),
            // The same NetEase song under another name.
            track("c.m4a", "Renamed", 100.0, 7, 9),
            // A NetEase song with the name and length of a plain file.
            track("d.mp3", "Song", 200.5, 8, 5),
            // Two NetEase songs of one name are told apart by their ids.
            track("e.mp3", "Other", 100.0, 9, 5),
        ]);
        let files: Vec<&str> = songs.iter().map(|song| song.file.as_str()).collect();
        assert_eq!(files, ["c.m4a", "e.mp3", "a.flac", "b.mp3"]);
    }

    #[test]
    fn a_scan_reads_new_files_in_path_order_and_reports_them() {
        let folder = std::env::temp_dir().join(format!("enji-scan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(folder.join("sub")).unwrap();
        fs::create_dir_all(folder.join(".hidden")).unwrap();
        for (name, byte) in [
            ("02.mp3", 2u8),
            ("01.mp3", 1),
            ("03.flac", 3),
            ("sub/04.mp3", 4),
            ("notes.txt", 5),
            (".hidden.mp3", 6),
            (".hidden/05.mp3", 7),
        ] {
            fs::write(folder.join(name), vec![byte; 64]).unwrap();
        }
        fs::write(folder.join("sub/04.LRC"), "[00:01.00]第一行").unwrap();
        let library = Library { folders: vec![Folder::new(file_url(&folder))], tracks: Vec::new() };
        let reported = std::sync::Mutex::new((0, 0));
        let scanned = library.scan(1000, |tracks, done, total| {
            let mut seen = reported.lock().unwrap();
            *seen = (seen.0 + tracks.len(), done.max(seen.1));
            assert_eq!(total, 4);
        });
        assert_eq!(*reported.lock().unwrap(), (4, 4));
        let titles: Vec<&str> = scanned.tracks.iter().map(|track| track.title.as_str()).collect();
        assert_eq!(titles, ["01", "02", "03", "04"]);
        let added: Vec<i64> = scanned.tracks.iter().map(|track| track.added).collect();
        assert_eq!(added, [1000, 999, 998, 997]);
        let fourth = &scanned.tracks[3];
        assert_eq!(bern::files::file_path(&fourth.url), Some(folder.join("sub/04.mp3")));
        assert_eq!((fourth.name.as_str(), fourth.folder.as_str()), ("04.mp3", library.folders[0].url.as_str()));
        let lines = lyrics(&fourth.url, &fourth.name, fourth.lyrics.as_deref());
        assert_eq!(lines.len(), 1, "the .lrc beside it is its lyrics");
        assert!(scanned.tracks[..3].iter().all(|track| track.lyrics.is_none()));
        // Nothing changed: nothing is read again.
        let again = scanned.scan(2000, |_, _, _| panic!("an unchanged file was read again"));
        assert_eq!(again.tracks.len(), 4);
        // The .lrc taken away goes with the next scan.
        fs::remove_file(folder.join("sub/04.LRC")).unwrap();
        let again = again.scan(3000, |_, _, _| panic!("an unchanged file was read again"));
        assert!(again.tracks[3].lyrics.is_none());
        let _ = fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_library_kept_by_paths_is_read_as_urls() {
        let kept = json!({
            "folders": ["/Music/My Songs"],
            "tracks": [{ "path": "/Music/My Songs/a b.mp3", "hash": "1", "title": "A", "size": 5, "matched": "auto", "netease_id": 7 }],
        });
        let library = Library::from_json(&kept);
        assert_eq!(library.folders, [Folder { url: "file:///Music/My%20Songs".into(), name: "My Songs".into() }]);
        assert_eq!(library.folders[0].place(), "/Music/My Songs");
        let track = &library.tracks[0];
        assert_eq!(track.url, "file:///Music/My%20Songs/a%20b.mp3");
        assert_eq!((track.name.as_str(), track.folder.as_str()), ("a b.mp3", "file:///Music/My%20Songs"));
        assert_eq!((track.netease_id, track.matched.as_str()), (7, "auto"));
    }

    #[test]
    fn local_ids_are_below_zero_and_urls_are_escaped() {
        assert!(id("da39a3ee5e6b4b0d3255bfef95601890afd80709") < 0);
        assert_eq!(file_url(Path::new("/Music/a b#1.mp3")), "file:///Music/a%20b%231.mp3");
    }
}
