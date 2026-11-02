//! How many times each song was played: counted here once a song has played
//! half its length or 30 seconds, whichever comes first, and NetEase's own
//! all-time counts for the account's most played songs; a song shows the
//! higher of the two. Kept for every list that shows songs, by song id (a
//! local file's matched NetEase id, or its own).

use enji_protocol::enji::Song;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Default)]
struct Counts {
    here: HashMap<i64, i64>,
    netease: HashMap<i64, i64>,
}

fn counts() -> &'static Mutex<Counts> {
    static COUNTS: OnceLock<Mutex<Counts>> = OnceLock::new();
    COUNTS.get_or_init(|| {
        Mutex::new(Counts {
            here: crate::store::plays(),
            netease: HashMap::new(),
        })
    })
}

impl Counts {
    fn of(&self, id: i64) -> i64 {
        let here = self.here.get(&id).copied().unwrap_or(0);
        let netease = self.netease.get(&id).copied().unwrap_or(0);
        here.max(netease)
    }
}

/// How long `seconds` of a song must play to count: half of it, at most 30
/// seconds.
pub fn threshold(seconds: f64) -> f64 {
    (seconds / 2.0).clamp(1.0, 30.0)
}

/// The song `id` was played once more.
pub fn played(id: i64) {
    if id == 0 {
        return;
    }
    let mut counts = counts().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *counts.here.entry(id).or_insert(0) += 1;
    crate::store::save_plays(&counts.here);
    crate::redressed();
}

/// NetEase's all-time counts for the account's most played songs.
pub fn set_netease(found: HashMap<i64, i64>) {
    counts().lock().unwrap_or_else(|poisoned| poisoned.into_inner()).netease = found;
    crate::redressed();
}

/// Each song with its count.
pub fn dress(songs: &mut [Song]) {
    let counts = counts().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    for song in songs {
        song.plays = counts.of(song.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_song_counts_after_half_of_it_or_thirty_seconds() {
        assert_eq!(threshold(240.0), 30.0);
        assert_eq!(threshold(20.0), 10.0);
        assert_eq!(threshold(0.5), 1.0, "even the shortest takes a second");
    }

    #[test]
    fn the_higher_count_shows() {
        let counts = Counts {
            here: HashMap::from([(1, 3), (2, 9)]),
            netease: HashMap::from([(1, 40)]),
        };
        assert_eq!(counts.of(1), 40);
        assert_eq!(counts.of(2), 9);
        assert_eq!(counts.of(3), 0);
    }
}
