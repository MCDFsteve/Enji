//! Where playback stood, so Enji opens where it closed: the queue, written
//! when it changes, and the playing song's place in it, written every few
//! seconds and at each pause, because a phone may end Enji without a word.

use enji_kit::convert;
use enji_protocol::enji::Song;
use serde_json::{json, Value};
use std::fs;
use std::io::{self, BufWriter, Write};
use std::time::{Duration, Instant};

const QUEUE: &str = "queue.json";
const PLACE: &str = "place.json";
/// How often the place of a playing song is written down.
const SAVE_EVERY: Duration = Duration::from_secs(5);

/// What to open with: the queue, the song in it, and how far it was.
pub struct Left {
    pub songs: Vec<Song>,
    pub at: usize,
    pub position: f64,
    pub play_next_count: usize,
}

fn left_from(queue: &Value, place: &Value) -> Option<Left> {
    let songs: Vec<Song> = queue
        .as_array()?
        .iter()
        .enumerate()
        .map(|(index, song)| convert::song_from_json(song, index))
        .filter(|song| song.id != 0)
        .collect();
    let at = usize::try_from(place["at"].as_i64()?).ok()?;
    // A crash can leave the previous checkpoint next to a newly saved queue.
    if place["queueMark"].as_u64().is_some_and(|saved| saved != mark(&songs)) {
        return None;
    }
    (at < songs.len()).then(|| {
        let play_next_count = place["playNext"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .filter(|&n| n <= songs.len() - at - 1)
            .unwrap_or(0);
        Left { songs, at, position: place["position"].as_f64().unwrap_or(0.0).max(0.0), play_next_count }
    })
}

/// The queue that was left, if there is one.
pub fn load() -> Option<Left> {
    left_from(&enji_kit::store::read(QUEUE), &enji_kit::store::read(PLACE))
}

/// Which songs a queue holds, in which order.
fn mark(songs: &[Song]) -> u64 {
    songs.iter().fold(songs.len() as u64, |mark, song| mark.wrapping_mul(31).wrapping_add(song.id as u64))
}

fn write_queue(out: &mut impl Write, songs: &[Song]) -> io::Result<()> {
    out.write_all(b"[")?;
    for (index, song) in songs.iter().enumerate() {
        if index > 0 {
            out.write_all(b",")?;
        }
        serde_json::to_writer(&mut *out, &convert::song_json(song))?;
    }
    out.write_all(b"]")
}

fn save_queue(songs: &[Song]) -> io::Result<()> {
    let dir = enji_kit::store::dir();
    fs::create_dir_all(&dir)?;
    let temp = dir.join(format!("queue.{}.tmp", std::process::id()));
    let save = || {
        let mut out = BufWriter::new(fs::File::create(&temp)?);
        write_queue(&mut out, songs)?;
        out.flush()?;
        drop(out);
        fs::rename(&temp, dir.join(QUEUE))
    };
    let result = save();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

#[derive(Default)]
pub struct Resume {
    queue: Option<u64>,
    play_next_count: usize,
    saved_at: Option<Instant>,
}

impl Resume {
    /// Record a queue that was just loaded from disk without writing it back.
    pub fn restore(&mut self, songs: &[Song], play_next_count: usize) {
        self.queue = Some(mark(songs));
        self.play_next_count = play_next_count;
    }

    /// Save the current queue and checkpoint after a song change or queue edit.
    pub fn update(&mut self, songs: &[Song], at: usize, position: f64, play_next_count: usize) {
        let now = mark(songs);
        if self.queue != Some(now) {
            self.queue = save_queue(songs).ok().map(|()| now);
        }
        self.play_next_count = play_next_count;
        self.place(at, position);
    }

    /// Where it is now, written down every few seconds.
    pub fn heard(&mut self, at: usize, position: f64) {
        if self.saved_at.is_none_or(|saved| saved.elapsed() >= SAVE_EVERY) {
            self.place(at, position);
        }
    }

    /// Where it is now, written down at once.
    pub fn place(&mut self, at: usize, position: f64) {
        // A checkpoint must refer to a queue that was successfully saved.
        let Some(queue_mark) = self.queue else { return };
        self.saved_at = Some(Instant::now());
        enji_kit::store::write(
            PLACE,
            &json!({
                "at": at,
                "position": position,
                "playNext": self.play_next_count,
                "queueMark": queue_mark,
            }),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn songs() -> Value {
        json!([{ "id": 7, "name": "a" }, { "id": 8, "name": "b" }])
    }

    #[test]
    fn it_opens_with_the_song_and_place_it_left() {
        let left = left_from(&songs(), &json!({ "at": 1, "position": 83.5 })).unwrap();
        assert_eq!(left.songs.len(), 2);
        assert_eq!(left.at, 1);
        assert_eq!(left.position, 83.5);
        assert_eq!(left.play_next_count, 0);
    }

    #[test]
    fn only_restore_requests_that_fit_after_the_current_song() {
        let queue = songs();
        for (at, saved, expected) in [(0, 1, 1), (1, 1, 0), (0, -1, 0), (0, 5, 0)] {
            let left = left_from(&queue, &json!({ "at": at, "playNext": saved })).unwrap();
            assert_eq!(left.play_next_count, expected);
        }
    }

    #[test]
    fn a_checkpoint_from_a_replaced_queue_cannot_restore_its_requests() {
        let queue = songs();
        let old = left_from(&queue, &json!({ "at": 0 })).unwrap();
        let place = json!({ "at": 0, "playNext": 1, "queueMark": mark(&old.songs) });
        assert_eq!(left_from(&queue, &place).unwrap().play_next_count, 1);
        assert!(left_from(&json!([{ "id": 8 }, { "id": 7 }]), &place).is_none());
    }

    #[test]
    fn streamed_queue_preserves_the_existing_file_format() {
        for entries in [json!([]), json!([{ "id": 1, "name": "楽曲 \"A\"" }, { "id": 2, "file": "a\\b" }])] {
            let queue: Vec<_> = entries
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(index, song)| convert::song_from_json(song, index))
                .collect();
            let mut bytes = Vec::new();
            write_queue(&mut bytes, &queue).unwrap();
            let expected = Value::Array(queue.iter().map(convert::song_json).collect());
            assert_eq!(bytes, expected.to_string().as_bytes());
        }
    }

    #[test]
    fn nothing_opens_without_a_place_in_the_queue() {
        assert!(left_from(&songs(), &Value::Null).is_none());
        assert!(left_from(&Value::Null, &json!({ "at": 0, "position": 1.0 })).is_none());
        assert!(left_from(&songs(), &json!({ "at": 5, "position": 1.0 })).is_none());
    }

    #[test]
    fn another_order_is_another_queue() {
        let queue: Vec<Song> = songs().as_array().unwrap().iter().map(|song| convert::song_from_json(song, 0)).collect();
        let turned: Vec<Song> = queue.iter().rev().cloned().collect();
        assert_ne!(mark(&queue), mark(&turned));
        assert_eq!(mark(&queue), mark(&queue.clone()));
    }
}
