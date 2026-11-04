//! Where playback stood, so Enji opens where it closed: the queue, written
//! when it changes, and the playing song's place in it, written every few
//! seconds and at each pause, because a phone may end Enji without a word.

use enji_kit::convert;
use enji_protocol::enji::Song;
use serde_json::{json, Value};
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
    (at < songs.len()).then(|| Left { songs, at, position: place["position"].as_f64().unwrap_or(0.0).max(0.0) })
}

/// The queue that was left, if there is one.
pub fn load() -> Option<Left> {
    left_from(&enji_kit::store::read(QUEUE), &enji_kit::store::read(PLACE))
}

/// Which songs a queue holds, in which order.
fn mark(songs: &[Song]) -> u64 {
    songs.iter().fold(songs.len() as u64, |mark, song| mark.wrapping_mul(31).wrapping_add(song.id as u64))
}

#[derive(Default)]
pub struct Resume {
    queue: u64,
    saved_at: Option<Instant>,
}

impl Resume {
    /// The song at `at` of `songs` starts, or was left as it is.
    pub fn song(&mut self, songs: &[Song], at: usize) {
        let now = mark(songs);
        if now != self.queue {
            self.queue = now;
            let saved: Vec<Value> = songs.iter().map(convert::song_json).collect();
            enji_kit::store::write(QUEUE, &Value::Array(saved));
        }
        self.place(at, 0.0);
    }

    /// Where it is now, written down every few seconds.
    pub fn heard(&mut self, at: usize, position: f64) {
        if self.saved_at.is_none_or(|saved| saved.elapsed() >= SAVE_EVERY) {
            self.place(at, position);
        }
    }

    /// Where it is now, written down at once.
    pub fn place(&mut self, at: usize, position: f64) {
        self.saved_at = Some(Instant::now());
        enji_kit::store::write(PLACE, &json!({ "at": at, "position": position }));
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
