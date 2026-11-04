//! Listening together in NetEase's own rooms, with Enji or NetEase's apps on
//! the other side. There is no channel to hear the room on: its state is
//! read every two seconds, and what happens here is reported as it happens.
//! The room keeps the shared list and the last playback command, stamped
//! with the server's clock, which also tells where the song is by now.
//!
//! What is here is the room's bookkeeping, apart from the network and the
//! player: which of the answers are news, and what to report.

use serde_json::{json, Value};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// How often the room is read, and the longest wait after failures.
const POLL: Duration = Duration::from_secs(2);
const LONGEST_POLL: Duration = Duration::from_secs(30);
/// How often its members are read.
const MEMBERS_EVERY: Duration = Duration::from_secs(15);
/// How long a command of ours may take to show in the room before the
/// room's own state counts again.
const ECHO_WAIT: Duration = Duration::from_secs(10);
/// How many of our sequence numbers are remembered, to know our commands.
const REMEMBERED: usize = 64;
/// A difference from the room smaller than this is left alone.
pub const CLOSE_ENOUGH: f64 = 2.0;

pub const STATUS: &str = "/api/listen/together/status/get";
pub const CREATE: &str = "/api/listen/together/room/create";
pub const ACCEPT: &str = "/api/listen/together/play/invitation/accept";
pub const END: &str = "/api/listen/together/end/v2";
pub const READ: &str = "/api/listen/together/sync/playlist/get";
pub const HEARTBEAT: &str = "/api/listen/together/heartbeat";
pub const COMMAND: &str = "/api/listen/together/play/command/report";
pub const LIST: &str = "/api/listen/together/sync/list/command/report";

/// The invitation in `text`: a link NetEase shares, as the room and who
/// invites. Rooms of many people are another protocol.
pub fn invitation(text: &str) -> Result<(String, i64), String> {
    let link = text.split_whitespace().find(|word| word.contains("st.music.163.com")).ok_or("没有找到一起听的邀请链接")?;
    if link.contains("multishare") {
        return Err("这是多人一起听的链接，Enji 只支持双人房间".into());
    }
    let query = link.split_once('?').map_or("", |(_, query)| query);
    let value = |key: &str| query.split('&').find_map(|pair| pair.strip_prefix(key)?.strip_prefix('=')).map(|value| value.split('#').next().unwrap_or(value));
    let room = value("roomId").filter(|room| !room.is_empty()).ok_or("邀请链接里没有房间")?;
    let inviter = value("inviterId").and_then(|id| id.parse().ok()).filter(|id| *id > 0).ok_or("邀请链接里没有邀请人")?;
    Ok((room.to_string(), inviter))
}

/// The link that invites someone to `room`.
pub fn share_link(room: &str, inviter: i64, song: i64) -> String {
    let song = if song > 0 { format!("&songId={song}") } else { String::new() };
    format!("https://st.music.163.com/listen-together/share/?roomId={room}&inviterId={inviter}{song}")
}

/// A play mode as the room names it.
pub fn mode_name(mode: &enji_protocol::enji::PlayMode) -> &'static str {
    use enji_protocol::enji::PlayMode;
    match mode {
        PlayMode::RepeatOne => "SINGLE_LOOP",
        PlayMode::Shuffle => "RANDOM",
        PlayMode::RepeatAll | PlayMode::Heart => "ORDER_LOOP",
    }
}

/// What plays here or in the room.
#[derive(Clone, Debug, PartialEq)]
pub struct Playback {
    pub song: i64,
    pub playing: bool,
    /// Where the song is, in milliseconds.
    pub progress: i64,
}

/// The room's last playback command.
#[derive(Clone, Debug, PartialEq)]
pub struct Command {
    pub playback: Playback,
    server_time: i64,
    client_seq: i64,
    user: i64,
}

impl Command {
    fn read(value: &Value) -> Option<Command> {
        let number = |key: &str| value[key].as_i64().or_else(|| value[key].as_str()?.parse().ok());
        let song = number("targetSongId").filter(|song| *song > 0)?;
        let playing = match value["playStatus"].as_str()? {
            "PLAY" => true,
            "PAUSE" => false,
            _ => return None,
        };
        Some(Command {
            playback: Playback { song, playing, progress: number("progress").unwrap_or(0).max(0) },
            server_time: number("serverSeq").unwrap_or(0),
            client_seq: number("clientSeq").unwrap_or(0),
            user: number("userId").unwrap_or(0),
        })
    }

    /// Where the song is by `now`, in seconds, in milliseconds since 1970:
    /// it went on from where it was when the server took the command, or
    /// from when it was received, on a clock that does not agree.
    pub fn position(&self, now: i64, received: i64) -> f64 {
        let progress = self.playback.progress;
        if !self.playback.playing {
            return progress as f64 / 1000.0;
        }
        let trusted = self.server_time > 1_000_000_000_000 && (now - self.server_time).abs() < 600_000;
        let from = if trusted { self.server_time } else { received };
        (progress + (now - from).max(0)) as f64 / 1000.0
    }
}

/// What is new in the room.
#[derive(Debug, Default, PartialEq)]
pub struct News {
    /// The shared list, when it changed.
    pub list: Option<Vec<i64>>,
    /// The play mode as the room names it, when the list changed.
    pub mode: Option<String>,
    pub command: Option<Command>,
}

/// A report to make, as the path and the body.
pub type Report = (&'static str, Value);

/// Being in a room.
pub struct Room {
    pub id: String,
    pub creator: i64,
    me: i64,
    sequence: i64,
    /// The numbers of the commands we sent, the latest last.
    sent: VecDeque<i64>,
    /// The last command acted on, and the newest server time seen.
    seen: Option<(i64, i64, i64)>,
    newest: i64,
    /// A command of ours still on its way into the room.
    awaiting: Option<(i64, Instant)>,
    /// What the room's list and mode were last, with each member's version.
    list_key: String,
    versions: Vec<Value>,
    /// What the room plays as far as we know.
    last: Option<Playback>,
    failures: u32,
    read_at: Option<Instant>,
    members_at: Option<Instant>,
    heartbeat_at: Option<Instant>,
    heartbeat_every: Duration,
}

impl Room {
    pub fn new(id: String, creator: i64, me: i64) -> Self {
        Room {
            id,
            creator,
            me,
            sequence: 0,
            sent: VecDeque::new(),
            seen: None,
            newest: 0,
            awaiting: None,
            list_key: String::new(),
            versions: Vec::new(),
            last: None,
            failures: 0,
            read_at: None,
            members_at: None,
            heartbeat_at: None,
            heartbeat_every: Duration::from_secs(20),
        }
    }

    /// Whether this side moves the room on when a song ends.
    pub fn leads(&self) -> bool {
        self.creator == self.me
    }

    /// Whether the room is due to be read.
    pub fn read_due(&self) -> bool {
        let wait = (POLL * 2u32.pow(self.failures.min(4))).min(LONGEST_POLL);
        self.read_at.is_none_or(|at| at.elapsed() >= wait)
    }

    pub fn members_due(&self) -> bool {
        self.members_at.is_none_or(|at| at.elapsed() >= MEMBERS_EVERY)
    }

    pub fn reading(&mut self) {
        self.read_at = Some(Instant::now());
    }

    pub fn reading_members(&mut self) {
        self.members_at = Some(Instant::now());
    }

    /// A read failed: the next one waits longer.
    pub fn failed(&mut self) {
        self.failures += 1;
    }

    /// Whether the connection is failing.
    pub fn struggling(&self) -> bool {
        self.failures > 1
    }

    /// What the room answered: its state, and what in it is news.
    pub fn read(&mut self, data: &Value) -> News {
        self.failures = 0;
        let playlist = &data["playlist"];
        if let Some(versions) = playlist["version"].as_array() {
            self.versions = versions.clone();
        }
        let command = Command::read(&data["playCommand"]);
        if let Some(command) = &command {
            // Our numbers go on from the room's.
            self.sequence = self.sequence.max(command.client_seq);
        }
        let mut news = News::default();
        let list = ids(&playlist["displayList"]["result"]);
        let mode = playlist["playMode"].as_str().unwrap_or("ORDER_LOOP").to_string();
        let key = format!("{list:?} {mode}");
        // While a command of ours is on its way, the room is behind us.
        if let Some((sequence, at)) = self.awaiting {
            match &command {
                Some(command) if self.is_ours(command) && command.client_seq >= sequence => {
                    self.remember(command);
                    self.awaiting = None;
                    self.list_key = key;
                    return news;
                }
                Some(command) if !self.is_ours(command) && command.server_time > self.newest => self.awaiting = None,
                _ if at.elapsed() < ECHO_WAIT => return news,
                _ => self.awaiting = None,
            }
        }
        if playlist["displayList"]["result"].is_array() && key != self.list_key {
            self.list_key = key;
            news.list = Some(list);
            news.mode = Some(mode);
        }
        if let Some(command) = command
            && !self.is_seen(&command)
            && command.server_time >= self.newest
        {
            self.remember(&command);
            if !self.is_ours(&command) {
                self.last = Some(command.playback.clone());
                news.command = Some(command);
            }
        }
        news
    }

    /// One of our own commands, and not the same account elsewhere.
    fn is_ours(&self, command: &Command) -> bool {
        command.user == self.me && self.sent.contains(&command.client_seq)
    }

    fn is_seen(&self, command: &Command) -> bool {
        self.seen == Some((command.server_time, command.user, command.client_seq))
    }

    fn remember(&mut self, command: &Command) {
        self.seen = Some((command.server_time, command.user, command.client_seq));
        self.newest = self.newest.max(command.server_time);
    }

    /// What to report for what plays here now: the list and mode if they
    /// changed, and a command if the room would not know. `seek` when the
    /// person moved within the song.
    pub fn report(&mut self, now: Playback, seek: bool, list: &[i64], mode: &str) -> Vec<Report> {
        let mut reports = Vec::new();
        let key = format!("{list:?} {mode}");
        if key != self.list_key {
            self.list_key = key;
            reports.push(self.list_report(list, mode));
        }
        let kind = match &self.last {
            Some(last) if last.song != now.song => Some("GOTO"),
            None => Some("GOTO"),
            Some(_) if seek => Some("PROGRESS"),
            Some(last) if last.playing != now.playing => Some(if now.playing { "PLAY" } else { "PAUSE" }),
            Some(_) => None,
        };
        if let Some(kind) = kind {
            reports.push(self.command_report(kind, &now));
            self.last = Some(now);
        }
        reports
    }

    fn list_report(&mut self, list: &[i64], mode: &str) -> Report {
        let own = self.versions.iter().find(|one| one["userId"].as_i64() == Some(self.me)).and_then(|one| one["version"].as_i64()).unwrap_or(0);
        let me = self.me;
        self.versions.retain(|one| one["userId"].as_i64() != Some(me));
        self.versions.push(json!({ "userId": me, "version": own + 1 }));
        let ids: Vec<String> = list.iter().map(i64::to_string).collect();
        let param = json!({
            "commandType": "REPLACE",
            "version": self.versions,
            "anchorSongId": "",
            "anchorPosition": -1,
            "playMode": mode,
            "displayList": ids,
            "randomList": ids,
        });
        (LIST, json!({ "roomId": self.id, "playlistParam": param.to_string() }))
    }

    fn command_report(&mut self, kind: &str, now: &Playback) -> Report {
        self.sequence += 1;
        self.sent.push_back(self.sequence);
        if self.sent.len() > REMEMBERED {
            self.sent.pop_front();
        }
        self.awaiting = Some((self.sequence, Instant::now()));
        let former = self.last.as_ref().map_or_else(|| "-1".to_string(), |last| last.song.to_string());
        let info = json!({
            "commandType": kind,
            "progress": now.progress,
            "playStatus": if now.playing { "PLAY" } else { "PAUSE" },
            "formerSongId": former,
            "targetSongId": now.song.to_string(),
            "clientSeq": self.sequence,
        });
        (COMMAND, json!({ "roomId": self.id, "commandInfo": info.to_string() }))
    }

    /// The room keeps going only while it is told someone is in it.
    pub fn heartbeat(&mut self, now: &Playback) -> Option<Report> {
        if self.heartbeat_at.is_some_and(|at| at.elapsed() < self.heartbeat_every) {
            return None;
        }
        self.heartbeat_at = Some(Instant::now());
        Some((
            HEARTBEAT,
            json!({
                "roomId": self.id,
                "songId": now.song.to_string(),
                "playStatus": if now.playing { "PLAY" } else { "PAUSE" },
                "progress": now.progress.to_string(),
            }),
        ))
    }

    /// The room says how often it wants to hear from us.
    pub fn heartbeat_answered(&mut self, data: &Value) {
        if let Some(seconds) = data["timeSpan"].as_u64().filter(|seconds| *seconds > 0) {
            self.heartbeat_every = Duration::from_secs(seconds.clamp(5, 60));
        }
    }
}

/// Song ids as the room lists them: numbers, strings, or objects with one.
fn ids(value: &Value) -> Vec<i64> {
    let one = |item: &Value| -> Option<i64> {
        let id = if item.is_object() {
            if item["songId"].is_null() { &item["id"] } else { &item["songId"] }
        } else {
            item
        };
        id.as_i64().or_else(|| id.as_str()?.parse().ok()).filter(|id| *id > 0)
    };
    let mut seen = std::collections::HashSet::new();
    value.as_array().into_iter().flatten().filter_map(one).filter(|id| seen.insert(*id)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ME: i64 = 7;
    const FRIEND: i64 = 8;

    fn room() -> Room {
        Room::new("r1".into(), ME, ME)
    }

    fn state(list: &[i64], command: Value) -> Value {
        json!({ "playlist": { "displayList": { "result": list }, "playMode": "ORDER_LOOP", "version": [{ "userId": ME, "version": 1 }] }, "playCommand": command })
    }

    fn command(user: i64, seq: i64, song: i64, status: &str, progress: i64, server: i64) -> Value {
        json!({ "userId": user, "clientSeq": seq, "targetSongId": song.to_string(), "playStatus": status, "progress": progress, "serverSeq": server, "commandType": "GOTO" })
    }

    fn playing(song: i64) -> Playback {
        Playback { song, playing: true, progress: 1000 }
    }

    #[test]
    fn it_reads_the_links_netease_shares() {
        let text = "一起来听歌 https://st.music.163.com/listen-together/share/?songId=1&roomId=abc-1&inviterId=42 快来";
        assert_eq!(invitation(text), Ok(("abc-1".into(), 42)));
        assert!(invitation("https://st.music.163.com/multishare/x?roomId=a&inviterId=1").unwrap_err().contains("多人"));
        assert!(invitation("https://music.163.com/song?id=1").is_err());
        assert!(invitation("https://st.music.163.com/listen-together/share/?roomId=a").is_err());
        let link = share_link("r1", 42, 9);
        assert_eq!(invitation(&link), Ok(("r1".into(), 42)));
    }

    #[test]
    fn what_the_other_side_does_is_news_once() {
        let mut room = room();
        let answer = state(&[1, 2], command(FRIEND, 3, 2, "PLAY", 5000, 2_000_000_000_000));
        let news = room.read(&answer);
        assert_eq!(news.list, Some(vec![1, 2]));
        assert_eq!(news.command.as_ref().unwrap().playback, Playback { song: 2, playing: true, progress: 5000 });
        assert_eq!(room.read(&answer), News::default());
    }

    #[test]
    fn our_own_commands_do_not_come_back_as_news() {
        let mut room = room();
        room.read(&state(&[1, 2], command(FRIEND, 1, 1, "PLAY", 0, 100)));
        let reports = room.report(playing(2), false, &[1, 2], "ORDER_LOOP");
        assert_eq!(reports.len(), 1);
        let info: Value = serde_json::from_str(reports[0].1["commandInfo"].as_str().unwrap()).unwrap();
        assert_eq!(info["commandType"], "GOTO");
        assert_eq!(info["formerSongId"], "1");
        let seq = info["clientSeq"].as_i64().unwrap();
        // The room still shows the old command for a moment: that is no news.
        assert_eq!(room.read(&state(&[1, 2], command(FRIEND, 1, 1, "PLAY", 0, 100))), News::default());
        // Then ours, which must not move us back to where it was sent from.
        assert_eq!(room.read(&state(&[1, 2], command(ME, seq, 2, "PLAY", 1000, 200))), News::default());
        // And the friend's next one is heard again.
        let news = room.read(&state(&[1, 2], command(FRIEND, seq + 1, 1, "PAUSE", 0, 300)));
        assert_eq!(news.command.unwrap().playback.playing, false);
    }

    #[test]
    fn the_same_account_on_another_device_is_the_other_side() {
        let mut room = room();
        let news = room.read(&state(&[1], command(ME, 9, 1, "PLAY", 0, 100)));
        assert!(news.command.is_some(), "a command we did not send");
    }

    #[test]
    fn an_old_answer_does_not_undo_a_newer_one() {
        let mut room = room();
        room.read(&state(&[1, 2], command(FRIEND, 5, 2, "PLAY", 0, 500)));
        let news = room.read(&state(&[1, 2], command(FRIEND, 4, 1, "PLAY", 0, 400)));
        assert!(news.command.is_none());
    }

    #[test]
    fn only_what_changed_is_reported() {
        let mut room = room();
        let first = room.report(playing(1), false, &[1, 2], "ORDER_LOOP");
        assert_eq!(first.iter().map(|one| one.0).collect::<Vec<_>>(), vec![LIST, COMMAND]);
        assert!(room.report(playing(1), false, &[1, 2], "ORDER_LOOP").is_empty());
        let kind = |reports: &Vec<Report>| {
            let info: Value = serde_json::from_str(reports[0].1["commandInfo"].as_str().unwrap()).unwrap();
            info["commandType"].as_str().unwrap().to_string()
        };
        let paused = Playback { playing: false, ..playing(1) };
        assert_eq!(kind(&room.report(paused.clone(), false, &[1, 2], "ORDER_LOOP")), "PAUSE");
        assert_eq!(kind(&room.report(playing(1), false, &[1, 2], "ORDER_LOOP")), "PLAY");
        assert_eq!(kind(&room.report(playing(1), true, &[1, 2], "ORDER_LOOP")), "PROGRESS");
        assert_eq!(kind(&room.report(playing(2), false, &[1, 2], "ORDER_LOOP")), "GOTO");
    }

    #[test]
    fn each_side_keeps_its_own_version_of_the_list() {
        let mut room = room();
        room.versions = vec![json!({ "userId": ME, "version": 4 }), json!({ "userId": FRIEND, "version": 9 })];
        let reports = room.report(playing(1), false, &[1], "RANDOM");
        let param: Value = serde_json::from_str(reports[0].1["playlistParam"].as_str().unwrap()).unwrap();
        assert_eq!(param["playMode"], "RANDOM");
        assert_eq!(param["displayList"], json!(["1"]));
        let versions = param["version"].as_array().unwrap();
        assert!(versions.contains(&json!({ "userId": ME, "version": 5 })));
        assert!(versions.contains(&json!({ "userId": FRIEND, "version": 9 })));
    }

    #[test]
    fn a_playing_song_is_further_on_than_the_command_said() {
        let at = 2_000_000_000_000;
        let room_command = Command::read(&command(FRIEND, 1, 1, "PLAY", 10_000, at)).unwrap();
        assert_eq!(room_command.position(at + 3000, at + 3000), 13.0);
        // A paused one stays.
        let paused = Command::read(&command(FRIEND, 1, 1, "PAUSE", 10_000, at)).unwrap();
        assert_eq!(paused.position(at + 3000, at + 3000), 10.0);
        // A clock far off is not believed: from when it was received.
        assert_eq!(room_command.position(at + 3_600_000, at + 3_599_000), 11.0);
    }

    #[test]
    fn failures_slow_the_reads_down() {
        let mut room = room();
        assert!(room.read_due());
        room.reading();
        assert!(!room.read_due());
        assert!(!room.struggling());
        room.failed();
        room.failed();
        assert!(room.struggling());
        room.read(&state(&[], Value::Null));
        assert!(!room.struggling());
    }

    #[test]
    fn the_room_sets_how_often_it_hears_from_us() {
        let mut room = room();
        assert!(room.heartbeat(&playing(1)).is_some());
        assert!(room.heartbeat(&playing(1)).is_none());
        room.heartbeat_answered(&json!({ "timeSpan": 30 }));
        assert_eq!(room.heartbeat_every, Duration::from_secs(30));
        room.heartbeat_answered(&json!({ "timeSpan": 9000 }));
        assert_eq!(room.heartbeat_every, Duration::from_secs(60));
    }

    #[test]
    fn it_reads_song_ids_however_the_room_lists_them() {
        assert_eq!(ids(&json!([1, "2", { "songId": 3 }, { "id": "4" }, 0, 1])), vec![1, 2, 3, 4]);
        assert_eq!(ids(&Value::Null), Vec::<i64>::new());
    }
}
