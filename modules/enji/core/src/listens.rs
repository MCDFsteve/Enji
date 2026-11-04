//! What was listened to, told to NetEase the way its web player tells it:
//! that a song started, and when it stopped, the seconds it really played,
//! leaving out what was sought past or paused. The account's play counts,
//! rankings and annual report are made of these. Reports wait in
//! `listens.json` for the network, and the song playing when Enji closes is
//! reported the next time it starts.

use serde_json::{json, Value};
use std::time::{Duration, Instant};

const FILE: &str = "listens.json";
/// How many reports wait at most: the oldest go first.
const KEPT: usize = 500;
/// How many go in one request.
const BATCH: usize = 50;
/// How often the playing song is written down, for when Enji closes.
const SAVE_EVERY: Duration = Duration::from_secs(15);
/// A jump of the position longer than this was a seek, not listening.
const LONGEST_STEP: f64 = 1.5;

#[derive(Clone, Debug, PartialEq)]
struct Playing {
    id: i64,
    /// How long it was really heard.
    seconds: f64,
    /// Where it was when last heard.
    at: f64,
}

/// That the song `id` started.
fn started(id: i64) -> Value {
    json!({ "action": "startPlay", "json": { "mainSite": "2", "mainSiteWeb": "2", "id": id.to_string(), "type": "song", "content": "" } })
}

impl Playing {
    /// Its report, if it was heard at all.
    fn report(&self) -> Option<Value> {
        let time = self.seconds.round() as i64;
        (time > 0).then(|| {
            json!({
                "action": "play",
                "json": { "mainSite": "2", "mainSiteWeb": "2", "id": self.id.to_string(), "type": "song", "content": "", "time": time, "wifi": 0, "download": 0 },
            })
        })
    }
}

#[derive(Default)]
pub struct Listens {
    playing: Option<Playing>,
    waiting: Vec<Value>,
    /// How many of the first reports are on their way.
    sending: usize,
    saved_at: Option<Instant>,
}

impl Listens {
    /// What was left the last time: the reports that waited, and the song
    /// that played then, which ended with Enji.
    pub fn load() -> Self {
        Self::from_json(&enji_kit::store::read(FILE))
    }

    fn from_json(saved: &Value) -> Self {
        let mut listens = Listens { waiting: saved["waiting"].as_array().cloned().unwrap_or_default(), ..Listens::default() };
        let left = &saved["playing"];
        if let (Some(id), Some(seconds)) = (left["id"].as_i64(), left["seconds"].as_f64()) {
            listens.waiting.extend(Playing { id, seconds, at: 0.0 }.report());
        }
        listens
    }

    fn to_json(&self) -> Value {
        let playing = self.playing.as_ref().map(|song| json!({ "id": song.id, "seconds": song.seconds }));
        json!({ "waiting": self.waiting, "playing": playing })
    }

    fn save(&mut self) {
        self.saved_at = Some(Instant::now());
        enji_kit::store::write(FILE, &self.to_json());
    }

    /// The song `id` starts.
    pub fn begin(&mut self, id: i64) {
        self.playing = Some(Playing { id, seconds: 0.0, at: 0.0 });
        self.waiting.push(started(id));
        self.trim();
    }

    /// The playing song is at `position` now: the time since it was last
    /// here was heard, unless that was a seek.
    pub fn heard(&mut self, position: f64) {
        let Some(song) = &mut self.playing else { return };
        let step = position - song.at;
        song.at = position;
        if step > 0.0 && step <= LONGEST_STEP {
            song.seconds += step;
        }
        if self.saved_at.is_none_or(|at| at.elapsed() >= SAVE_EVERY) {
            self.save();
        }
    }

    /// The playing song stopped: true when that gave a report to send.
    pub fn finish(&mut self) -> bool {
        let Some(song) = self.playing.take() else { return false };
        let report = song.report();
        let reported = report.is_some();
        self.waiting.extend(report);
        self.trim();
        self.save();
        reported
    }

    fn trim(&mut self) {
        let extra = self.waiting.len().saturating_sub(KEPT);
        self.waiting.drain(..extra);
        self.sending = self.sending.saturating_sub(extra);
    }

    /// The next reports to send, while none are on their way.
    pub fn next_batch(&mut self) -> Option<Vec<Value>> {
        if self.sending > 0 || self.waiting.is_empty() {
            return None;
        }
        self.sending = self.waiting.len().min(BATCH);
        Some(self.waiting[..self.sending].to_vec())
    }

    /// NetEase took the reports on their way, or they stay to try again.
    pub fn sent(&mut self, taken: bool) {
        if taken {
            self.waiting.drain(..self.sending.min(self.waiting.len()));
        }
        self.sending = 0;
        self.save();
    }

    /// Another account, or none: what waits was this one's.
    pub fn forget(&mut self) {
        *self = Listens::default();
        self.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Steps through `from`..`to` as the worker hears them.
    fn play(listens: &mut Listens, from: f64, to: f64) {
        let mut at = from;
        while at < to {
            at += 0.2;
            listens.heard(at);
        }
    }

    fn quiet() -> Listens {
        Listens { saved_at: Some(Instant::now()), ..Listens::default() }
    }

    #[test]
    fn a_report_says_how_long_the_song_was_really_heard() {
        let mut listens = quiet();
        listens.begin(1);
        play(&mut listens, 0.0, 40.0);
        // A seek ahead is not listening, nor is a seek back.
        listens.heard(150.0);
        play(&mut listens, 150.0, 170.0);
        listens.heard(10.0);
        play(&mut listens, 10.0, 20.0);
        let seconds = listens.playing.as_ref().unwrap().seconds;
        assert!((seconds - 70.0).abs() < 1.0, "{seconds}");
    }

    #[test]
    fn a_song_says_it_started_then_how_long_it_played() {
        let mut listens = quiet();
        listens.begin(42);
        assert_eq!(listens.waiting[0]["action"], "startPlay");
        assert_eq!(listens.waiting[0]["json"]["id"], "42");
        assert_eq!(listens.waiting[0]["json"]["mainSite"], "2");
        assert!(listens.waiting[0]["json"]["time"].is_null());
        let finished = Playing { id: 42, seconds: 199.6, at: 199.6 }.report().unwrap();
        assert_eq!(finished["action"], "play");
        assert_eq!(finished["json"]["id"], "42");
        assert_eq!(finished["json"]["type"], "song");
        assert_eq!(finished["json"]["time"], 200);
    }

    #[test]
    fn a_song_never_heard_has_no_report() {
        assert!(Playing { id: 1, seconds: 0.2, at: 0.2 }.report().is_none());
    }

    #[test]
    fn reports_wait_until_netease_took_them() {
        let mut listens = quiet();
        listens.waiting = (0..60).map(|i| json!({ "n": i })).collect();
        let first = listens.next_batch().unwrap();
        assert_eq!(first.len(), BATCH);
        // One batch at a time.
        assert!(listens.next_batch().is_none());
        // Refused, the same ones go again.
        listens.sending = 0;
        assert_eq!(listens.next_batch().unwrap()[0]["n"], 0);
        // Taken, the rest follow.
        let taken = listens.sending;
        listens.waiting.drain(..taken);
        listens.sending = 0;
        assert_eq!(listens.next_batch().unwrap().len(), 10);
    }

    #[test]
    fn the_song_playing_when_enji_closed_is_reported_next_time() {
        let mut before = Listens::default();
        before.playing = Some(Playing { id: 9, seconds: 95.0, at: 95.0 });
        before.waiting.push(json!({ "n": 1 }));
        let after = Listens::from_json(&before.to_json());
        assert!(after.playing.is_none());
        assert_eq!(after.waiting.len(), 2);
        assert_eq!(after.waiting[1]["json"]["id"], "9");
        assert_eq!(after.waiting[1]["json"]["time"], 95);
    }

    #[test]
    fn the_oldest_reports_go_first_when_too_many_wait() {
        let mut listens = quiet();
        listens.waiting = (0..KEPT + 1).map(|i| json!({ "n": i })).collect();
        listens.sending = 3;
        listens.trim();
        assert_eq!(listens.waiting.len(), KEPT);
        assert_eq!(listens.waiting[0]["n"], 1);
        // One of those on their way is gone.
        assert_eq!(listens.sending, 2);
    }
}
