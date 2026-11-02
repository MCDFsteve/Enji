//! The local copies of songs: a song from NetEase with a file here plays
//! from the file and shows its tags.

use enji_protocol::enji::Song;

/// A song's name and first singer, as two copies of it are compared:
/// without case or spaces.
pub fn name_key(title: &str, artists: &str) -> String {
    let first = artists
        .split(['/', ';', ',', '、', '&', '，'])
        .next()
        .unwrap_or("")
        .split(" feat")
        .next()
        .unwrap_or("");
    let squash = |text: &str| text.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect::<String>();
    format!("{}\u{1f}{}", squash(title), squash(first))
}

/// Whether two lengths in seconds are of one recording.
pub fn near(a: f64, b: f64) -> bool {
    a <= 0.0 || b <= 0.0 || (a - b).abs() < 3.0
}

/// The local copies of songs, for songs from NetEase to be played from
/// them and shown with their tags.
#[derive(Default)]
struct Copies {
    by_id: std::collections::HashMap<i64, Song>,
    by_name: std::collections::HashMap<String, Vec<Song>>,
}

static COPIES: std::sync::RwLock<Option<Copies>> = std::sync::RwLock::new(None);

/// The copies known now: those of `songs`, the library's songs.
pub fn set_copies(songs: &[Song]) {
    let mut copies = Copies::default();
    for song in songs {
        if song.id > 0 {
            copies.by_id.insert(song.id, song.clone());
        }
        copies.by_name.entry(name_key(&song.name, &song.artists)).or_default().push(song.clone());
    }
    *COPIES.write().unwrap_or_else(|e| e.into_inner()) = Some(copies);
    crate::redressed();
}

/// A song from NetEase with a copy here: played from the file, shown with
/// its tags, its cover kept when the file has none.
pub fn dress(song: &mut Song) {
    if !song.file.is_empty() {
        return;
    }
    let copies = COPIES.read().unwrap_or_else(|e| e.into_inner());
    let Some(copies) = copies.as_ref() else { return };
    let copy = copies.by_id.get(&song.id).or_else(|| {
        copies
            .by_name
            .get(&name_key(&song.name, &song.artists))?
            .iter()
            .find(|copy| copy.id < 0 && near(copy.seconds, song.seconds))
    });
    if let Some(copy) = copy {
        song.file = copy.file.clone();
        song.name = copy.name.clone();
        if !copy.artists.is_empty() {
            song.artists = copy.artists.clone();
        }
        if !copy.album.is_empty() {
            song.album = copy.album.clone();
        }
        if !copy.cover.url.is_empty() && copy.cover.url.starts_with("file:") {
            song.cover = copy.cover.clone();
            song.artwork = copy.artwork.clone();
        }
    }
}
