//! NetEase's JSON into the protocol's values.

use bern::media::MediaSource;
use enji_protocol::enji::{Album, Artist, Comment, Details, Playlist, Song};
use serde_json::{json, Value};

/// A NetEase picture at `size` points square, over HTTPS.
pub fn cover(url: &str, size: u32) -> MediaSource {
    if url.is_empty() {
        return MediaSource::new("");
    }
    MediaSource::new(format!(
        "{}?param={size}y{size}",
        url.replacen("http://", "https://", 1)
    ))
}

/// `m:ss`.
pub fn clock(seconds: f64) -> String {
    let whole = seconds.max(0.0) as i64;
    format!("{}:{:02}", whole / 60, whole % 60)
}

fn names(list: &Value) -> String {
    list.as_array()
        .map(|all| {
            all.iter()
                .filter_map(|one| one["name"].as_str())
                .collect::<Vec<_>>()
                .join(" / ")
        })
        .unwrap_or_default()
}

/// A song from a playlist, a search or the daily recommendation; `privilege`
/// tells whether it can be played.
pub fn song(value: &Value, index: usize, privilege: Option<&Value>) -> Song {
    let album = if value["al"].is_object() { &value["al"] } else { &value["album"] };
    let artists = if value["ar"].is_array() { &value["ar"] } else { &value["artists"] };
    let millis = value["dt"].as_f64().or(value["duration"].as_f64()).unwrap_or(0.0);
    let privilege = privilege.unwrap_or(&value["privilege"]);
    Song {
        id: value["id"].as_i64().unwrap_or(0),
        index: index as i64,
        rank: index as i64 + 1,
        name: value["name"].as_str().unwrap_or("").to_string(),
        artists: names(artists),
        album: album["name"].as_str().unwrap_or("").to_string(),
        cover: cover(album["picUrl"].as_str().unwrap_or(""), 200),
        // Twice the 400 points it is shown at, for a sharp picture on Retina.
        artwork: cover(album["picUrl"].as_str().unwrap_or(""), 800),
        seconds: millis / 1000.0,
        duration: clock(millis / 1000.0),
        unavailable: privilege["st"].as_i64().is_some_and(|st| st < 0),
        album_id: album["id"].as_i64().unwrap_or(0),
        artist_id: artists[0]["id"].as_i64().unwrap_or(0),
        file: String::new(),
        plays: 0,
        added: 0,
        place: index as i64,
    }
}

/// A song as the play history keeps it.
pub fn song_json(song: &Song) -> Value {
    json!({
        "id": song.id,
        "name": song.name,
        "artists": song.artists,
        "album": song.album,
        "cover": song.cover.url,
        "artwork": song.artwork.url,
        "seconds": song.seconds,
        "album_id": song.album_id,
        "artist_id": song.artist_id,
        "file": song.file,
    })
}

/// A song the play history kept, at `index` of its list.
pub fn song_from_json(value: &Value, index: usize) -> Song {
    let text = |key: &str| value[key].as_str().unwrap_or("").to_string();
    let seconds = value["seconds"].as_f64().unwrap_or(0.0);
    Song {
        id: value["id"].as_i64().unwrap_or(0),
        index: index as i64,
        rank: index as i64 + 1,
        name: text("name"),
        artists: text("artists"),
        album: text("album"),
        cover: MediaSource::new(text("cover")),
        artwork: MediaSource::new(text("artwork")),
        seconds,
        duration: clock(seconds),
        unavailable: false,
        album_id: value["album_id"].as_i64().unwrap_or(0),
        artist_id: value["artist_id"].as_i64().unwrap_or(0),
        file: match value["file"].as_str() {
            Some(file) => file.to_string(),
            // Kept by its path before files were kept by URL.
            None => Some(text("path")).filter(|path| !path.is_empty()).map(|path| bern::resources::file_url(std::path::Path::new(&path))).unwrap_or_default(),
        },
        plays: 0,
        added: 0,
        place: index as i64,
    }
}

/// A count as it is read: `9876`, `1.2万`, `3.4亿`.
pub fn count(n: i64) -> String {
    match n {
        n if n >= 100_000_000 => format!("{:.1}亿", n as f64 / 1e8),
        n if n >= 10_000 => format!("{:.1}万", n as f64 / 1e4),
        n => n.to_string(),
    }
}

/// `2024-05-17` for a time in milliseconds since 1970.
pub fn date(millis: i64) -> String {
    // Civil date from days since the epoch (Howard Hinnant's algorithm).
    let days = millis.div_euclid(86_400_000) + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 { month_index + 3 } else { month_index - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year}-{month:02}-{day:02}")
}

pub fn comment(value: &Value) -> Comment {
    let time = value["timeStr"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value["time"].as_i64().map(date).unwrap_or_default());
    Comment {
        id: value["commentId"].as_i64().unwrap_or(0),
        user: value["user"]["nickname"].as_str().unwrap_or("").to_string(),
        avatar: cover(value["user"]["avatarUrl"].as_str().unwrap_or(""), 80),
        content: value["content"].as_str().unwrap_or("").to_string(),
        likes: count(value["likedCount"].as_i64().unwrap_or(0)),
        time,
    }
}

/// The album of `album` and the singer of `artist`, as their pages send them.
pub fn details(album: &Value, artist: &Value) -> Details {
    Details {
        album: album["name"].as_str().unwrap_or("").to_string(),
        album_cover: cover(album["picUrl"].as_str().unwrap_or(""), 300),
        released: album["publishTime"].as_i64().filter(|&t| t > 0).map(date).unwrap_or_default(),
        company: album["company"].as_str().unwrap_or("").to_string(),
        album_description: album["description"].as_str().unwrap_or("").trim().to_string(),
        artist: artist["name"].as_str().unwrap_or("").to_string(),
        artist_picture: cover(artist["picUrl"].as_str().or(artist["img1v1Url"].as_str()).unwrap_or(""), 300),
        artist_description: artist["briefDesc"].as_str().unwrap_or("").trim().to_string(),
    }
}

/// Songs of `list`, with `privileges` alongside them when NetEase sends them
/// apart.
pub fn songs(list: &Value, privileges: &Value) -> Vec<Song> {
    list.as_array()
        .map(|all| {
            all.iter()
                .enumerate()
                .map(|(i, one)| {
                    let privilege = privileges
                        .as_array()
                        .and_then(|all| all.iter().find(|p| p["id"] == one["id"]));
                    song(one, i, privilege)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// A playlist from the recommendations, the account's lists or a detail.
pub fn playlist(value: &Value) -> Playlist {
    let picture = value["coverImgUrl"]
        .as_str()
        .or(value["picUrl"].as_str())
        .unwrap_or("");
    let count = value["trackCount"].as_i64().unwrap_or(0);
    let owner = value["creator"]["nickname"].as_str().unwrap_or("");
    let detail = match (count, owner) {
        (0, _) => String::new(),
        (n, "") => format!("{n} 首"),
        (n, owner) => format!("{n} 首 · {owner}"),
    };
    Playlist {
        id: value["id"].as_i64().unwrap_or(0),
        name: value["name"].as_str().unwrap_or("").to_string(),
        cover: cover(picture, 400),
        detail,
        owned: false,
        liked: false,
    }
}

/// How many songs a playlist has and how often it was played, as its page
/// shows them: `200 首 · 142.2亿次播放`.
pub fn playlist_meta(value: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(n) = value["trackCount"].as_i64() {
        parts.push(format!("{n} 首"));
    }
    if let Some(n) = value["playCount"].as_i64().filter(|&n| n > 0) {
        parts.push(format!("{}次播放", count(n)));
    }
    parts.join(" · ")
}

/// The year of a time in milliseconds since 1970, as `date` reads it.
fn year(millis: i64) -> String {
    date(millis).get(..4).unwrap_or("").to_string()
}

/// An album from a search, a singer's albums or an album's page: its
/// singer and year beside it.
pub fn album(value: &Value) -> Album {
    let singer = value["artist"]["name"].as_str().unwrap_or("");
    let published = value["publishTime"].as_i64().filter(|&t| t > 0).map(year).unwrap_or_default();
    let detail = match (singer.is_empty(), published.is_empty()) {
        (false, false) => format!("{singer} · {published}"),
        (false, true) => singer.to_string(),
        _ => published,
    };
    Album {
        id: value["id"].as_i64().unwrap_or(0),
        name: value["name"].as_str().unwrap_or("").to_string(),
        cover: cover(value["picUrl"].as_str().unwrap_or(""), 400),
        detail,
        artist_id: value["artist"]["id"].as_i64().unwrap_or(0),
    }
}

/// When an album came out, how many songs it has and its company:
/// `2024-05-17 · 13 首 · 杰威尔`.
pub fn album_meta(value: &Value) -> String {
    let mut parts = Vec::new();
    if let Some(t) = value["publishTime"].as_i64().filter(|&t| t > 0) {
        parts.push(date(t));
    }
    if let Some(n) = value["size"].as_i64().filter(|&n| n > 0) {
        parts.push(format!("{n} 首"));
    }
    if let Some(company) = value["company"].as_str().map(str::trim).filter(|c| !c.is_empty()) {
        parts.push(company.to_string());
    }
    parts.join(" · ")
}

/// A singer, with how many songs and albums they have.
pub fn artist(value: &Value) -> Artist {
    let picture = value["picUrl"].as_str().or(value["img1v1Url"].as_str()).unwrap_or("");
    let songs = value["musicSize"].as_i64().unwrap_or(0);
    let albums = value["albumSize"].as_i64().unwrap_or(0);
    let detail = match (songs, albums) {
        (0, 0) => String::new(),
        (songs, 0) => format!("单曲 {songs}"),
        (0, albums) => format!("专辑 {albums}"),
        (songs, albums) => format!("单曲 {songs} · 专辑 {albums}"),
    };
    Artist {
        id: value["id"].as_i64().unwrap_or(0),
        name: value["name"].as_str().unwrap_or("").to_string(),
        picture: cover(picture, 300),
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn counts_and_dates_read_as_people_write_them() {
        assert_eq!(count(9876), "9876");
        assert_eq!(count(12_345), "1.2万");
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(1_715_904_000_000), "2024-05-17");
    }

    #[test]
    fn songs_read_their_artists_album_length_and_rights() {
        let list = json!([{ "id": 7, "name": "夏", "ar": [{ "name": "A" }, { "name": "B" }],
            "al": { "name": "潮騒", "picUrl": "http://p1.music.126.net/x.jpg" }, "dt": 245000 }]);
        let privileges = json!([{ "id": 7, "st": -200 }]);
        let songs = songs(&list, &privileges);
        assert_eq!(songs[0].artists, "A / B");
        assert_eq!(songs[0].duration, "4:05");
        assert!(songs[0].unavailable);
        assert_eq!(songs[0].cover.url, "https://p1.music.126.net/x.jpg?param=200y200");
    }
}
