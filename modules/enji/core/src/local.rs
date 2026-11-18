//! Music files in the folders the user added: found by walking them, each
//! known by the SHA-1 of its first 10 KB as Misuzu Music knows it, with the
//! tags, cover and lyrics the file carries. What was read is kept, so a file
//! that has not changed is not read again. Files are read on several threads
//! and reported as they are read, so the first songs can be played while the
//! rest are read; each cover is kept small, once per picture, and the full
//! picture is taken from the file when the song plays. Folders and files
//! are known by their URLs and read through `bern::files`: `file:` URLs,
//! and on Android the documents of a folder from the system's picker.

use crate::{audio::loudness::{self, Info}, lyrics, store};
use enji_kit::{
    convert,
    copies::{name_key, near},
};
use bern::{files::Entry, media::MediaSource};
use enji_protocol::enji::Song;
use lofty::{file::AudioFile as _, prelude::*};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{self, BufReader, BufWriter, Read, Seek, SeekFrom},
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
/// Bump when tag-reading semantics change so affected cached entries re-read once.
const LIBRARY_VERSION: u64 = 3;
const TEXT_REPAIR_VERSION: u64 = 2;

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
    /// True when loaded from an older library cache and carrying Latin-1
    /// bytes that may have been GBK/UTF-8 mojibake, so the next scan re-reads it.
    reread: bool,
    /// None for pre-loudness library records. A missing tag in a completed scan
    /// is a real result, not a reason to read the tags again on every play.
    loudness: Option<ScannedLoudness>,
}

#[derive(Clone, Debug)]
struct ScannedLoudness {
    key: Option<String>,
    tag: Option<Info>,
}

impl Track {
    /// Scan snapshot, not a substitute for validating the file being played.
    pub fn loudness(&self) -> (Option<String>, Option<Info>) {
        self.loudness.as_ref().map_or((None, None), |s| (s.key.clone(), s.tag))
    }
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
        let version = saved["version"].as_u64().unwrap_or(1);
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
                        .map(|(one, url, name, folder)| {
                            let title = text(one, "title");
                            let artists = text(one, "artists");
                            let album = text(one, "album");
                            let reread = version < TEXT_REPAIR_VERSION
                                && [&title, &artists, &album].iter().any(|s| has_latin1_supplement(s));
                            Track {
                                url,
                                name,
                                folder,
                                lyrics: one["lyrics"].as_str().filter(|url| !url.is_empty()).map(str::to_string),
                                hash: text(one, "hash"),
                                title,
                                artists,
                                album,
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
                                reread,
                                loudness: one["loudness"].as_object().map(|snapshot| ScannedLoudness {
                                    key: snapshot.get("key").and_then(Value::as_str).filter(|k| {
                                        k.strip_prefix("local-v1:").is_some_and(|hex| hex.len() == 40 && hex.bytes().all(|c| c.is_ascii_hexdigit()))
                                    }).map(str::to_owned),
                                    tag: snapshot.get("tag").and_then(Info::from_json).filter(|info| info.source() == "replaygain"),
                                }),
                            }
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
                    "loudness": track.loudness.as_ref().map(|s| json!({ "key": s.key, "tag": s.tag.map(Info::to_json) })),
                })
            })
            .collect();
        let folders: Vec<Value> = self.folders.iter().map(|folder| json!({ "url": folder.url, "name": folder.name })).collect();
        let _ = fs::create_dir_all(store::dir());
        let _ = fs::write(library_path(), json!({ "version": LIBRARY_VERSION, "folders": folders, "tracks": tracks }).to_string());
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
                Some(track) if !track.reread && track.size == file.entry.size && track.modified == modified && small(track)
                    && track.loudness.as_ref().is_some_and(|s| s.key == file_version(&file.entry.url, file.entry.size)) => {
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
                                let changed = known.title != track.title || known.artists != track.artists || known.album != track.album;
                                if !(known.matched == "none" && changed) {
                                    track.matched = known.matched.clone();
                                }
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

fn has_latin1_supplement(text: &str) -> bool {
    text.chars().any(|c| ('\u{0080}'..='\u{00FF}').contains(&c))
}

/// Recover GBK/GB18030 (or UTF-8) text that `lofty` decoded byte-for-byte as Latin-1.
///
/// `lofty::util::text::latin1_decode` maps every byte `b` directly to `b as char`
/// (`U+0000..=U+00FF`). This recovers the original bytes and decodes them when
/// they are valid non-ASCII GB18030 (with at least one multi-byte high-byte pair,
/// so Western Latin-1 like `Mötley Crüe` or `café` is never misread) or valid
/// UTF-8 containing CJK/Kana/Hangul characters.
fn repair_latin1(text: &str) -> Option<String> {
    if text.is_ascii() || text.chars().any(|c| (c as u32) > 0xFF) {
        return None;
    }
    let bytes: Vec<u8> = text.chars().map(|c| c as u8).collect();
    if bytes.contains(&0) {
        let mut repaired = false;
        let parts: Vec<String> = bytes
            .split(|&b| b == 0)
            .map(|part| {
                let s: String = part.iter().map(|&b| b as char).collect();
                match repair_latin1_bytes(part) {
                    Some(fixed) => {
                        repaired = true;
                        fixed
                    }
                    None => s,
                }
            })
            .collect();
        return repaired.then(|| parts.join("\0"));
    }
    repair_latin1_bytes(&bytes)
}

fn repair_latin1_bytes(bytes: &[u8]) -> Option<String> {
    if bytes.iter().all(u8::is_ascii) {
        return None;
    }
    // Check UTF-8 only when it decodes to real CJK/Kana/Hangul (>= U+3000) without
    // accidental 2-byte Latin/Cyrillic sequences that collide with GBK bytes (e.g. "魔").
    if let Ok(utf8) = std::str::from_utf8(bytes)
        && utf8.chars().any(is_cjk_or_kana_or_hangul)
        && utf8.chars().all(|c| c.is_ascii() || !('\u{0080}'..'\u{0800}').contains(&c))
    {
        return Some(utf8.to_string());
    }
    if let Some(decoded) = decode_gb18030_strict(bytes) {
        return Some(decoded);
    }
    // ID3v1 fields cap at 28 or 30 bytes and buggy taggers sometimes slice a
    // trailing multi-byte GBK character in half at the field boundary.
    if bytes.len() >= 28 {
        return decode_gb18030_strict(&bytes[..bytes.len() - 1]);
    }
    None
}

fn is_cjk_or_kana_or_hangul(c: char) -> bool {
    matches!(
        c,
        '\u{3000}'..='\u{9FFF}'
            | '\u{AC00}'..='\u{D7AF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{FF00}'..='\u{FFEF}'
    )
}

fn decode_gb18030_strict(bytes: &[u8]) -> Option<String> {
    if !has_gbk_high_pair(bytes) {
        return None;
    }
    let (decoded, had_errors) = encoding_rs::GB18030.decode_without_bom_handling(bytes);
    (!had_errors).then(|| decoded.into_owned())
}

/// True when `bytes` contains at least one two-byte sequence whose lead byte is
/// in `0x81..=0xFE` and whose trail byte is `>= 0x80`. Western Latin-1 text
/// (`Mötley Crüe`, `Céline Dion`, `Señorita`) places accented bytes (`>= 0x80`)
/// beside ASCII letters (`< 0x80`), so requiring a high-high pair keeps it safe.
fn has_gbk_high_pair(bytes: &[u8]) -> bool {
    let mut i = 0;
    let mut high_high = false;
    while i < bytes.len() {
        let b = bytes[i];
        if b < 0x80 {
            i += 1;
        } else if (0x81..=0xFE).contains(&b) && i + 1 < bytes.len() {
            if bytes[i + 1] >= 0x80 {
                high_high = true;
            }
            i += 2;
        } else {
            return false;
        }
    }
    high_high
}

fn repair_id3v1(tag: &mut lofty::id3::v1::Id3v1Tag) {
    for field in [&mut tag.title, &mut tag.artist, &mut tag.album, &mut tag.comment] {
        if let Some(text) = field.as_mut()
            && let Some(fixed) = repair_latin1(text)
        {
            *text = fixed;
        }
    }
}

fn repair_id3v2(tag: &mut lofty::id3::v2::Id3v2Tag) {
    use lofty::{TextEncoding, id3::v2::Frame};

    tag.retain_mut(|frame| {
        match frame {
            Frame::Text(f) if f.encoding == TextEncoding::Latin1 => {
                if let Some(fixed) = repair_latin1(&f.value) {
                    f.value = fixed.into();
                    f.encoding = TextEncoding::UTF8;
                }
            }
            Frame::UserText(f) if f.encoding == TextEncoding::Latin1 => {
                if let Some(fixed) = repair_latin1(&f.content) {
                    f.content = fixed.into();
                }
                if let Some(fixed) = repair_latin1(&f.description) {
                    f.description = fixed.into();
                }
                f.encoding = TextEncoding::UTF8;
            }
            Frame::Comment(f) if f.encoding == TextEncoding::Latin1 => {
                if let Some(fixed) = repair_latin1(&f.content) {
                    f.content = fixed.into();
                }
                if let Some(fixed) = repair_latin1(&f.description) {
                    f.description = fixed.into();
                }
                f.encoding = TextEncoding::UTF8;
            }
            Frame::UnsynchronizedText(f) if f.encoding == TextEncoding::Latin1 => {
                if let Some(fixed) = repair_latin1(&f.content) {
                    f.content = fixed.into();
                }
                if let Some(fixed) = repair_latin1(&f.description) {
                    f.description = fixed.into();
                }
                f.encoding = TextEncoding::UTF8;
            }
            _ => {}
        }
        true
    });
}

fn decode_lrc_bytes(bytes: &[u8]) -> String {
    if let Some((encoding, bom_len)) = encoding_rs::Encoding::for_bom(bytes) {
        return encoding.decode_without_bom_handling(&bytes[bom_len..]).0.into_owned();
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }
    encoding_rs::GB18030.decode_without_bom_handling(bytes).0.into_owned()
}

fn ordered_tags(file: &lofty::file::TaggedFile) -> Vec<&lofty::tag::Tag> {
    let primary = file.primary_tag();
    let mut tags = Vec::with_capacity(file.tags().len());
    if let Some(tag) = primary {
        tags.push(tag);
    }
    for tag in file.tags() {
        if primary.is_none_or(|p| !std::ptr::eq(p, tag)) {
            tags.push(tag);
        }
    }
    tags
}

/// Keep the native handle available: stat'ing a URL after opening it can pair a
/// replacement's version with the bytes of the old, still-open file. Other URL
/// schemes continue through Bern (including Android document providers).
pub struct AudioFile {
    reader: AudioReader,
    len: u64,
}

enum AudioReader {
    Native(fs::File),
    Provider(bern::files::SourceFile),
}

pub fn open_audio(url: &str) -> io::Result<AudioFile> {
    if let Some(path) = bern::files::file_path(url) {
        let file = fs::File::open(path)?;
        let meta = file.metadata()?;
        if meta.is_dir() { return Err(io::ErrorKind::IsADirectory.into()); }
        Ok(AudioFile { reader: AudioReader::Native(file), len: meta.len() })
    } else {
        let file = bern::files::open(url).map_err(|e| io::Error::other(e.message))?;
        let len = file.len();
        Ok(AudioFile { reader: AudioReader::Provider(file), len })
    }
}

impl AudioFile {
    pub fn len(&self) -> u64 { self.len }

    fn revision(&self, url: &str) -> Option<String> {
        let AudioReader::Native(file) = &self.reader else { return None };
        metadata_key(url, self.len, &file.metadata().ok()?, Some(file))
    }

    /// At EOF do not associate a mixture of revisions with the starting key.
    /// An unversioned document can still be measured under a digest of its bytes.
    pub fn revision_matches(&self, url: &str, key: Option<&str>) -> bool {
        key.is_none_or(|key| self.revision(url).as_deref() == Some(key))
    }
}

impl Read for AudioFile {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        match &mut self.reader { AudioReader::Native(f) => f.read(out), AudioReader::Provider(f) => f.read(out) }
    }
}

impl Seek for AudioFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match &mut self.reader { AudioReader::Native(f) => f.seek(pos), AudioReader::Provider(f) => f.seek(pos) }
    }
}

/// Only the scan's cheap unchanged-file test uses a path, never an open reader.
fn file_version(url: &str, len: u64) -> Option<String> {
    let path = bern::files::file_path(url)?;
    #[cfg(windows)]
    {
        let file = fs::File::open(path).ok()?;
        metadata_key(url, len, &file.metadata().ok()?, Some(&file))
    }
    #[cfg(not(windows))]
    { metadata_key(url, len, &fs::metadata(path).ok()?, None) }
}

#[cfg(not(any(unix, windows)))]
fn metadata_key(_: &str, _: u64, _: &fs::Metadata, _: Option<&fs::File>) -> Option<String> { None }

#[cfg(any(unix, windows))]
fn metadata_key(url: &str, len: u64, meta: &fs::Metadata, _file: Option<&fs::File>) -> Option<String> {
    if !meta.is_file() || len == 0 || meta.len() != len { return None; }
    let mtime = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_nanos();
    #[cfg(unix)]
    let id = {
        use std::os::unix::fs::MetadataExt;
        if meta.ino() == 0 || (meta.ctime() == 0 && meta.ctime_nsec() == 0) { return None; }
        format!("{}:{}:{}:{}", meta.dev(), meta.ino(), meta.ctime(), meta.ctime_nsec())
    };
    #[cfg(windows)]
    let id = windows_revision(_file?)?;
    Some(format!("local-v1:{}", loudness::hex_digest(format!("{url}\0{len}:{mtime}:{id}").as_bytes())))
}

/// Creation time alone is not identity: NTFS name tunneling may preserve it
/// across a delete/rename. ChangeTime also detects edits that restore mtime.
#[cfg(windows)]
fn windows_revision(file: &fs::File) -> Option<String> {
    use std::{mem::size_of, os::windows::io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_BASIC_INFO, FILE_ID_INFO, FileBasicInfo, FileIdInfo, GetFileInformationByHandleEx,
    };
    let mut id = FILE_ID_INFO::default();
    let mut times = FILE_BASIC_INFO::default();
    let handle = file.as_raw_handle();
    // SAFETY: both buffers have the requested information class, correct size,
    // and outlive the synchronous call. The borrowed file keeps its handle open.
    unsafe {
        if GetFileInformationByHandleEx(handle, FileIdInfo, (&raw mut id).cast(), size_of::<FILE_ID_INFO>() as u32) == 0
            || GetFileInformationByHandleEx(handle, FileBasicInfo, (&raw mut times).cast(), size_of::<FILE_BASIC_INFO>() as u32) == 0
        { return None; }
    }
    if id.FileId.Identifier == [0; 16] || times.ChangeTime == 0 { return None; }
    Some(format!("{}:{:032x}:{}", id.VolumeSerialNumber, u128::from_le_bytes(id.FileId.Identifier), times.ChangeTime))
}

fn track_gain(tags: &[&lofty::tag::Tag]) -> Option<Info> {
    // Only a complete track-gain tag, with its own optional peak. Never marry
    // an album peak or a second, potentially stale tag to this gain.
    tags.iter().find_map(|tag| {
        let mut gains = tag.get_strings(ItemKey::ReplayGainTrackGain);
        let gain = gains.next()?;
        if !gains.all(|s| s.trim().eq_ignore_ascii_case(gain.trim())) { return None; }
        let mut peaks = tag.get_strings(ItemKey::ReplayGainTrackPeak);
        let peak = peaks.next().filter(|p| peaks.all(|s| s.trim() == p.trim()));
        Info::replaygain(gain, peak)
    })
}

/// A pre-play probe can skip to tags at the end but must not scan a large
/// document or read a whole picture before Ready. Exhausting this budget just
/// means no tag decision; it does not change playback or the bytes to analyze.
struct TagBudget<'a> {
    file: &'a mut AudioFile,
    left: usize,
}
impl Read for TagBudget<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() { return Ok(0); }
        if self.left == 0 { return Err(io::Error::other("loudness tag byte budget")); }
        let limit = out.len().min(self.left);
        let read = self.file.read(&mut out[..limit])?;
        self.left -= read;
        Ok(read)
    }
}
impl Seek for TagBudget<'_> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> { self.file.seek(pos) }
}

/// On the producer's local I/O thread, before Ready, with the very file that
/// will supply the playback stream. Prefer validated scan results. Otherwise
/// probe tags only, not duration, art or all PCM. An unseekable document is
/// untouched. If a previously seekable provider fails to restore its cursor,
/// return an error instead of playing/measuring the remainder as a whole file.
pub fn opened_loudness(
    track: Option<&Track>, url: &str, name: &str, file: &mut AudioFile,
) -> io::Result<(Option<String>, Option<Info>)> {
    let key = file.revision(url);
    if key.is_some() && let Some(track) = track.filter(|t| t.url == url && t.size == file.len()) {
        let snapshot = track.loudness();
        if snapshot.0 == key { return Ok(snapshot); }
    }
    let Ok(original) = file.stream_position() else { return Ok((key, None)); };
    let restore = |file: &mut AudioFile| -> io::Result<()> {
        if file.seek(SeekFrom::Start(original))? != original { return Err(io::Error::other("source did not restore its position")); }
        Ok(())
    };
    match file.seek(SeekFrom::Start(0)) {
        Ok(0) => {}
        Err(_) => return Ok((key, None)), // no bytes consumed from a non-seekable source
        Ok(_) => { restore(file)?; return Ok((key, None)); }
    }
    let tag = {
        use lofty::{config::ParseOptions, file::FileType, probe::Probe};
        let reader = BufReader::new(TagBudget { file, left: 512 * 1024 });
        let options = ParseOptions::default().read_properties(false).read_cover_art(false);
        let probe = Probe::new(reader).options(options);
        let probe = match FileType::from_ext(extension(name)) {
            Some(kind) => Some(probe.set_file_type(kind)),
            None => probe.guess_file_type().ok(),
        };
        probe.and_then(|probe| probe.read().ok()).and_then(|tags| track_gain(&ordered_tags(&tags)))
    };
    restore(file)?;
    if !file.revision_matches(url, key.as_deref()) { return Ok((None, None)); }
    Ok((key, tag))
}

/// The tags, length and pictures of the music file `file`, named `name`:
/// of the kind its name says, or else the one its first bytes show.
/// ID3v1 and ID3v2 Latin-1 frames carrying GBK/GB18030 (or UTF-8) bytes are
/// repaired before converting to a generic `TaggedFile`.
fn tagged(file: impl Read + Seek, name: &str) -> Option<lofty::file::TaggedFile> {
    use lofty::{config::ParseOptions, file::FileType};

    let probe = lofty::probe::Probe::new(BufReader::new(file));
    let probe = match FileType::from_ext(extension(name)) {
        Some(kind) => probe.set_file_type(kind),
        None => probe.guess_file_type().ok()?,
    };
    let kind = probe.file_type()?;
    let mut reader = probe.into_inner();
    let options = ParseOptions::default();

    let tagged = match kind {
        FileType::Mpeg => {
            let mut file = lofty::mpeg::MpegFile::read_from(&mut reader, options).ok()?;
            if let Some(tag) = file.id3v2_mut() {
                repair_id3v2(tag);
            }
            if let Some(tag) = file.id3v1_mut() {
                repair_id3v1(tag);
            }
            file.into()
        }
        FileType::Flac => {
            let mut file = lofty::flac::FlacFile::read_from(&mut reader, options).ok()?;
            if let Some(tag) = file.id3v2_mut() {
                repair_id3v2(tag);
            }
            file.into()
        }
        FileType::Aac => {
            let mut file = lofty::aac::AacFile::read_from(&mut reader, options).ok()?;
            if let Some(tag) = file.id3v2_mut() {
                repair_id3v2(tag);
            }
            if let Some(tag) = file.id3v1_mut() {
                repair_id3v1(tag);
            }
            file.into()
        }
        FileType::Wav => {
            let mut file = lofty::iff::wav::WavFile::read_from(&mut reader, options).ok()?;
            if let Some(tag) = file.id3v2_mut() {
                repair_id3v2(tag);
            }
            file.into()
        }
        FileType::Aiff => {
            let mut file = lofty::iff::aiff::AiffFile::read_from(&mut reader, options).ok()?;
            if let Some(tag) = file.id3v2_mut() {
                repair_id3v2(tag);
            }
            file.into()
        }
        FileType::Ape => {
            let mut file = lofty::ape::ApeFile::read_from(&mut reader, options).ok()?;
            if let Some(tag) = file.id3v2_mut() {
                repair_id3v2(tag);
            }
            if let Some(tag) = file.id3v1_mut() {
                repair_id3v1(tag);
            }
            file.into()
        }
        FileType::Mpc => {
            let mut file = lofty::musepack::MpcFile::read_from(&mut reader, options).ok()?;
            if let Some(tag) = file.id3v2_mut() {
                repair_id3v2(tag);
            }
            if let Some(tag) = file.id3v1_mut() {
                repair_id3v1(tag);
            }
            file.into()
        }
        FileType::WavPack => {
            let mut file = lofty::wavpack::WavPackFile::read_from(&mut reader, options).ok()?;
            if let Some(tag) = file.id3v1_mut() {
                repair_id3v1(tag);
            }
            file.into()
        }
        _ => lofty::probe::Probe::new(reader).set_file_type(kind).read().ok()?,
    };
    Some(tagged)
}

/// A music file's tags, length and picture; its name for a title when it
/// has none.
fn read(found: &Found, modified: i64, added: i64) -> Option<Track> {
    let mut file = open_audio(&found.entry.url).ok()?;
    let key = file.revision(&found.entry.url);
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
        size: file.len(),
        modified,
        added,
        reread: false,
        loudness: Some(ScannedLoudness { key: key.clone(), tag: None }),
    };
    let Some(tagged) = file.seek(SeekFrom::Start(0)).ok().and_then(|_| tagged(&mut file, &track.name)) else {
        return Some(track);
    };
    track.seconds = tagged.properties().duration().as_secs_f64();
    let tags = ordered_tags(&tagged);
    track.loudness = file.revision_matches(&track.url, key.as_deref()).then(|| ScannedLoudness { key, tag: track_gain(&tags) });
    if let Some(title) = tags.iter().find_map(|tag| tag.title().map(|t| t.trim().to_string()).filter(|t| !t.is_empty())) {
        track.title = title;
    }
    track.artists = tags
        .iter()
        .find_map(|tag| tag.artist().map(|a| a.trim().to_string()).filter(|a| !a.is_empty()))
        .unwrap_or_default();
    track.album = tags
        .iter()
        .find_map(|tag| tag.album().map(|a| a.trim().to_string()).filter(|a| !a.is_empty()))
        .unwrap_or_default();
    if let Some(picture) = tags.iter().find_map(|tag| tag.pictures().first()) {
        track.cover = thumbnail(picture.data());
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

/// How a file keeps its music, such as `FLAC · 921 kbps · 44.1 kHz · 16
/// bit`: what its header says, empty when it says nothing.
pub fn format(url: &str, name: &str) -> String {
    use lofty::file::FileType;
    let Some(file) = tags_of(url, name) else {
        return String::new();
    };
    let properties = file.properties();
    let codec = match file.file_type() {
        FileType::Flac => "FLAC",
        FileType::Mpeg => "MP3",
        // ALAC keeps samples of a depth, AAC none.
        FileType::Mp4 if properties.bit_depth().is_some() => "ALAC",
        FileType::Mp4 | FileType::Aac => "AAC",
        FileType::Wav => "WAV",
        FileType::Aiff => "AIFF",
        FileType::Vorbis => "Ogg Vorbis",
        FileType::Opus => "Opus",
        FileType::Ape => "APE",
        FileType::WavPack => "WavPack",
        _ => "",
    };
    let bitrate = properties.audio_bitrate().or(properties.overall_bitrate()).filter(|&kbps| kbps > 0);
    let mut parts = Vec::new();
    if !codec.is_empty() {
        parts.push(codec.to_string());
    }
    if let Some(kbps) = bitrate {
        parts.push(format!("{kbps} kbps"));
    }
    if let Some(rate) = properties.sample_rate().filter(|&rate| rate > 0) {
        parts.push(kilohertz(rate));
    }
    if let Some(depth) = properties.bit_depth().filter(|&depth| depth > 0) {
        parts.push(format!("{depth} bit"));
    }
    parts.join(" · ")
}

/// A sample rate in kilohertz as players write it: `44.1 kHz`, `48 kHz`.
fn kilohertz(rate: u32) -> String {
    let khz = rate as f64 / 1000.0;
    if khz.fract() == 0.0 { format!("{khz:.0} kHz") } else { format!("{:.1} kHz", khz) }
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
        let mut bytes = Vec::new();
        bern::files::open(beside).ok()?.read_to_end(&mut bytes).ok()?;
        Some(lyrics::parse(&decode_lrc_bytes(&bytes), ""))
    });
    if let Some(lines) = beside.filter(|lines| !lines.is_empty()) {
        return lines;
    }
    let Some(file) = tags_of(url, name) else { return Vec::new() };
    ordered_tags(&file)
        .into_iter()
        .find_map(|tag| tag.get_string(ItemKey::Lyrics).or_else(|| tag.get_string(ItemKey::UnsyncLyrics)))
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
        downloaded: crate::downloads::holds(&track.url),
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
            reread: false,
            loudness: None,
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

    fn as_latin1(bytes: &[u8]) -> String {
        bytes.iter().map(|&b| b as char).collect()
    }

    fn synchsafe(value: u32) -> [u8; 4] {
        [
            ((value >> 21) & 0x7F) as u8,
            ((value >> 14) & 0x7F) as u8,
            ((value >> 7) & 0x7F) as u8,
            (value & 0x7F) as u8,
        ]
    }

    fn id3v23_text_frame(id: &[u8; 4], encoding: u8, text_bytes: &[u8]) -> Vec<u8> {
        let size = (1 + text_bytes.len()) as u32;
        let mut frame = Vec::new();
        frame.extend_from_slice(id);
        frame.extend_from_slice(&size.to_be_bytes());
        frame.extend_from_slice(&[0, 0]);
        frame.push(encoding);
        frame.extend_from_slice(text_bytes);
        frame
    }

    fn id3v23_tag(frames: &[Vec<u8>]) -> Vec<u8> {
        let body: Vec<u8> = frames.iter().flat_map(|f| f.iter().copied()).collect();
        let mut tag = Vec::new();
        tag.extend_from_slice(b"ID3\x03\x00\x00");
        tag.extend_from_slice(&synchsafe(body.len() as u32));
        tag.extend_from_slice(&body);
        tag
    }

    fn id3v1_tag(title: &[u8], artist: &[u8], album: &[u8]) -> [u8; 128] {
        let mut buf = [0u8; 128];
        buf[..3].copy_from_slice(b"TAG");
        let put = |dst: &mut [u8], src: &[u8]| {
            let n = src.len().min(dst.len());
            dst[..n].copy_from_slice(&src[..n]);
        };
        put(&mut buf[3..33], title);
        put(&mut buf[33..63], artist);
        put(&mut buf[63..93], album);
        buf
    }

    fn minimal_mp3(id3v2: Option<&[u8]>, id3v1: Option<&[u8; 128]>) -> Vec<u8> {
        let mut data = Vec::new();
        if let Some(v2) = id3v2 {
            data.extend_from_slice(v2);
        }
        // Two MPEG-1 Layer III 128 kbps 44.1 kHz frames (417 bytes each) so lofty's sync check passes.
        for _ in 0..2 {
            data.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
            data.resize(data.len() + 413, 0);
        }
        if let Some(v1) = id3v1 {
            data.extend_from_slice(v1);
        }
        data
    }

    #[test]
    fn loudness_probe_leaves_an_unseekable_document_at_its_first_byte() {
        struct Pipe(io::Cursor<Vec<u8>>);
        impl Read for Pipe {
            fn read(&mut self, into: &mut [u8]) -> io::Result<usize> { self.0.read(into) }
        }
        impl Seek for Pipe {
            fn seek(&mut self, _: SeekFrom) -> io::Result<u64> { Err(io::ErrorKind::NotSeekable.into()) }
        }
        let original = b"provider bytes, not a local path";
        let provider = bern::files::SourceFile::new(Pipe(io::Cursor::new(original.to_vec())), 0);
        let mut file = AudioFile { reader: AudioReader::Provider(provider), len: 0 };
        assert_eq!(opened_loudness(None, "content://library/4", "a.mp3", &mut file).unwrap(), (None, None));
        let mut played = Vec::new();
        file.read_to_end(&mut played).unwrap();
        assert_eq!(played.as_slice(), original);
        assert!(file.revision_matches("content://library/4", None));
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn replaced_master_uses_its_own_tags_even_if_size_and_mtime_match() {
        let folder = std::env::temp_dir().join(format!("enji-loudness-revision-{}-{:016x}", std::process::id(), rand::random::<u64>()));
        fs::create_dir(&folder).unwrap();
        let path = folder.join("private-name.mp3");
        let url = file_url(&path);
        let bytes = |gain| {
            let tags = id3v23_tag(&[id3v23_text_frame(b"TXXX", 0, format!("REPLAYGAIN_TRACK_GAIN\0-{gain}.0 dB").as_bytes())]);
            minimal_mp3(Some(&tags), None)
        };
        let old_bytes = bytes(8);
        fs::write(&path, &old_bytes).unwrap();
        let mut old = open_audio(&url).unwrap();
        let (key, tag) = opened_loudness(None, &url, "private-name.mp3", &mut old).unwrap();
        assert_eq!(tag.unwrap().gain_db(), -8.0);
        assert!(!key.as_deref().unwrap().contains("private-name"));
        let mut snapshot = track(&url, "same catalogue song", 180.0, 34, old.len());
        snapshot.loudness = Some(ScannedLoudness { key: key.clone(), tag });
        let original_mtime = fs::metadata(&path).unwrap().modified().unwrap();
        // Keep the old handle open while the path begins naming a new inode/file ID.
        fs::rename(&path, folder.join("old.mp3")).unwrap();
        let replacement = bytes(3);
        assert_eq!(replacement.len(), old_bytes.len());
        fs::write(&path, &replacement).unwrap();
        fs::OpenOptions::new().write(true).open(&path).unwrap().set_modified(original_mtime).unwrap();
        let mut new = open_audio(&url).unwrap();
        let (new_key, new_tag) = opened_loudness(Some(&snapshot), &url, "private-name.mp3", &mut new).unwrap();
        assert!(new_key.is_some() && new_key != key);
        assert_eq!(new_tag.unwrap().gain_db(), -3.0);
        // Even after the rename an existing handle's probe must not use the new file's tags/key.
        let mut new_snapshot = snapshot.clone();
        new_snapshot.loudness = Some(ScannedLoudness { key: new_key.clone(), tag: new_tag });
        let (old_key_now, old_tag) = opened_loudness(Some(&new_snapshot), &url, "private-name.mp3", &mut old).unwrap();
        assert_ne!(old_key_now, new_key);
        assert_eq!(old_tag.unwrap().gain_db(), -8.0);
        let mut played = Vec::new();
        new.read_to_end(&mut played).unwrap();
        assert_eq!(played, replacement);
        assert!(new.revision_matches(&url, new_key.as_deref()));
        let next_mtime = original_mtime + Duration::from_secs(4);
        fs::write(&path, bytes(6)).unwrap();
        fs::OpenOptions::new().write(true).open(&path).unwrap().set_modified(next_mtime).unwrap();
        assert!(!new.revision_matches(&url, new_key.as_deref()));
        drop((old, new));
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn repairs_gbk_and_utf8_in_latin1_without_corrupting_western_latin1() {
        // GBK Simplified & Traditional Chinese decoded as Latin-1
        for sample in ["多成", "周杰伦", "01. 稻香 (Live)", "張學友", "劉德華", "雙截棍"] {
            let gbk = encoding_rs::GB18030.encode(sample).0;
            let mojibake = as_latin1(&gbk);
            assert_eq!(repair_latin1(&mojibake).as_deref(), Some(sample), "failed for {sample}");
        }

        // UTF-8 CJK stuffed into Latin-1
        for sample in ["周杰伦", "稻香", "十年"] {
            let mojibake = as_latin1(sample.as_bytes());
            assert_eq!(repair_latin1(&mojibake).as_deref(), Some(sample));
        }

        // Real Western Latin-1 and Unicode strings stay untouched
        for intact in [
            "café",
            "Mötley Crüe",
            "Beyoncé",
            "Tiësto",
            "Björk",
            "Naïve",
            "Céline Dion",
            "Señorita",
            "façade",
            "© 2005",
            "周杰伦",
            "Dvořák",
        ] {
            assert_eq!(repair_latin1(intact), None, "false positive for {intact}");
        }

        // Truncated 30-byte ID3v1 field ending mid-GBK-character (1 ASCII + 14.5 Chinese chars)
        let mut thirty = b"1".to_vec();
        for _ in 0..14 {
            thirty.extend_from_slice(&encoding_rs::GB18030.encode("周").0);
        }
        thirty.push(encoding_rs::GB18030.encode("周").0[0]);
        assert_eq!(thirty.len(), 30);
        let expected = format!("1{}", "周".repeat(14));
        assert_eq!(repair_latin1(&as_latin1(&thirty)).as_deref(), Some(expected.as_str()));
    }

    #[test]
    fn reads_gbk_tags_from_id3v1_and_id3v2_latin1_while_preserving_utf8_and_western_latin1() {
        let gbk = |s: &str| encoding_rs::GB18030.encode(s).0.into_owned();

        // 1. MP3 with ID3v1-only GBK tags
        let v1_only = minimal_mp3(None, Some(&id3v1_tag(&gbk("稻香"), &gbk("周杰伦"), &gbk("魔杰座"))));
        let parsed = tagged(std::io::Cursor::new(v1_only), "a.mp3").unwrap();
        let tag = parsed.primary_tag().or_else(|| parsed.first_tag()).unwrap();
        assert_eq!(tag.title().as_deref(), Some("稻香"));
        assert_eq!(tag.artist().as_deref(), Some("周杰伦"));
        assert_eq!(tag.album().as_deref(), Some("魔杰座"));

        // 2. MP3 with ID3v2 Latin-1 (encoding=0) containing GBK + ID3v1
        let v2_gbk = id3v23_tag(&[
            id3v23_text_frame(b"TIT2", 0, &gbk("青花瓷")),
            id3v23_text_frame(b"TPE1", 0, &gbk("周杰伦")),
            id3v23_text_frame(b"TALB", 0, &gbk("我很忙")),
        ]);
        let both = minimal_mp3(Some(&v2_gbk), Some(&id3v1_tag(&gbk("旧标题"), &gbk("旧歌手"), &gbk("旧专辑"))));
        let parsed = tagged(std::io::Cursor::new(both), "b.mp3").unwrap();
        let tag = parsed.primary_tag().unwrap();
        assert_eq!(tag.title().as_deref(), Some("青花瓷"));
        assert_eq!(tag.artist().as_deref(), Some("周杰伦"));
        assert_eq!(tag.album().as_deref(), Some("我很忙"));

        // 3. MP3 with ID3v2 Latin-1 (encoding=0) containing genuine Western Latin-1
        let v2_latin1 = id3v23_tag(&[
            id3v23_text_frame(b"TIT2", 0, &b"Caf\xe9 del Mar"[..]),
            id3v23_text_frame(b"TPE1", 0, &b"M\xf6tley Cr\xfce"[..]),
        ]);
        let western = minimal_mp3(Some(&v2_latin1), None);
        let parsed = tagged(std::io::Cursor::new(western), "c.mp3").unwrap();
        let tag = parsed.primary_tag().unwrap();
        assert_eq!(tag.title().as_deref(), Some("Café del Mar"));
        assert_eq!(tag.artist().as_deref(), Some("Mötley Crüe"));

        // 4. MP3 with ID3v2 UTF-8 (encoding=3, using ID3v2.4 header) stays untouched
        let mut v2_utf8 = id3v23_tag(&[
            id3v23_text_frame(b"TIT2", 3, "富士山下".as_bytes()),
            id3v23_text_frame(b"TPE1", 3, "陈奕迅".as_bytes()),
        ]);
        v2_utf8[3] = 4; // ID3v2.4
        let utf8_mp3 = minimal_mp3(Some(&v2_utf8), None);
        let parsed = tagged(std::io::Cursor::new(utf8_mp3), "d.mp3").unwrap();
        let tag = parsed.primary_tag().unwrap();
        assert_eq!(tag.title().as_deref(), Some("富士山下"));
        assert_eq!(tag.artist().as_deref(), Some("陈奕迅"));
    }

    #[test]
    fn old_library_cache_marks_latin1_mojibake_tracks_for_reread_and_decodes_gbk_lrc() {
        let mojibake = as_latin1(&encoding_rs::GB18030.encode("多成").0);
        let saved = json!({
            "folders": ["/Music"],
            "tracks": [
                { "url": "file:///Music/a.mp3", "name": "a.mp3", "folder": "file:///Music", "hash": "1", "title": mojibake, "artists": "", "album": "", "size": 5, "modified": 10 },
                { "url": "file:///Music/b.mp3", "name": "b.mp3", "folder": "file:///Music", "hash": "2", "title": "十年", "artists": "陈奕迅", "album": "", "size": 5, "modified": 10 },
            ],
        });
        let loaded = Library::from_json(&saved);
        assert!(loaded.tracks[0].reread);
        assert!(!loaded.tracks[1].reread);

        let gbk_lrc = encoding_rs::GB18030.encode("[00:01.00]多成").0;
        let lines = lyrics::parse(&decode_lrc_bytes(&gbk_lrc), "");
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "多成");
    }
}
