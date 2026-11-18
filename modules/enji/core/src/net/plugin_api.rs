//! Public plugin views. Never serialize the internal Song (it carries a local file URL),
//! nor attach a queue or a lyrics document to high-frequency audio events.
use super::*;
use enji_protocol::enji::{PluginAction, PluginChoice, PluginNumber, PluginOption, PluginSwitch};
use plugins::{Pack, SettingKind};

pub(super) fn writes_granted(pack: &Pack, granted: impl Fn(&str) -> bool) -> bool {
    ["playback_control", "audio_gain"].iter().all(|&p| !pack.has(p) || granted(p))
}

fn same_track(a: &Song, b: &Song) -> bool {
    if a.file.is_empty() && b.file.is_empty() { a.id == b.id } else { a.file == b.file }
}

pub(super) fn public_song(song: &Song) -> Value {
    // The local i64 id need not fit an ECMAScript Number. Always offer an exact string key.
    let key = if song.file.is_empty() { format!("song:{}", song.id) }
        else { format!("local:{}", normalization::digest(song.file.as_bytes())) };
    json!({"trackKey":key, "id":song.id, "name":song.name, "artists":song.artists,
        "album":song.album, "duration":song.seconds})
}

fn public_line(line: &lyrics::Line) -> Value {
    let mut start = 0;
    let words: Vec<_> = line.words.iter().map(|word| {
        let item = json!({"time":word.time, "duration":word.length, "start":start, "end":word.end});
        start = word.end;
        item
    }).collect();
    let ruby: Vec<_> = line.ruby.iter().map(|r| json!({"start":r.start,"end":r.end,"reading":r.reading})).collect();
    json!({"time":line.time, "text":line.text, "translation":line.translation, "words":words, "ruby":ruby})
}

pub(super) fn mode_name(mode: &PlayMode) -> &'static str {
    match mode {
        PlayMode::RepeatAll => "repeatAll", PlayMode::RepeatOne => "repeatOne",
        PlayMode::Shuffle => "shuffle", PlayMode::Heart => "heart",
    }
}

/// As displayed, the queue starts with the current track. A queue may already exist
/// before its selected song starts.
fn current_index(s: &State) -> Option<usize> {
    s.queue.first().zip(s.current.as_ref()).filter(|(first, song)| first.index == s.queue_at && same_track(first, song)).map(|_| 0)
}

pub(super) fn installed_row(pack: &Pack, enabled: bool, values: &Value) -> Plugin {
    let mut row = Plugin { id: pack.id.clone(), name: pack.name.clone(), version: pack.version.clone(),
        description: pack.description.clone(), author: pack.author.clone(), enabled, fits: pack.fits_here(),
        platforms: pack.platforms_label(), playback_control: pack.has("playback_control"), audio_gain: pack.has("audio_gain"),
        switches: vec![], numbers: vec![], choices: vec![], player_actions: vec![], track_actions: vec![] };
    for item in &pack.settings {
        let (id, label) = (item.id.clone(), item.label.clone());
        let value = values.get(item.id.as_str()).unwrap_or(&item.default);
        match &item.kind {
            SettingKind::Boolean => row.switches.push(PluginSwitch { id, label, value: value.as_bool().unwrap_or(false) }),
            SettingKind::Number { min, max } => {
                let value = value.as_f64().unwrap_or(*min);
                let width = max - min;
                let position = if width == 0.0 { 0.0 } else if width.is_finite() { (value - min) / width }
                    else { (value * 0.5 - min * 0.5) / (max * 0.5 - min * 0.5) };
                row.numbers.push(PluginNumber { id, label, value, position: position.clamp(0.0, 1.0) });
            }
            SettingKind::Enum { options } => row.choices.push(PluginChoice { id, label,
                value: value.as_str().unwrap_or("").to_owned(),
                options: options.iter().map(|s| PluginOption { value: s.clone() }).collect() }),
        }
    }
    for action in &pack.actions {
        let actions = if action.context == "player" { &mut row.player_actions } else { &mut row.track_actions };
        actions.push(PluginAction { id: action.id.clone(), label: action.label.clone() });
    }
    row
}

impl Worker {
    pub(super) fn change_plugin_setting(&mut self, plugin: &str, key: &str, value: Value) {
        match self.plugins.set_setting(plugin, key, value) {
            Ok(()) => { self.publish_plugins(); self.tell_loudness(); }
            Err(err) => self.shared.fail(err),
        }
    }

    pub(super) fn change_plugin_number(&mut self, plugin: &str, key: &str, position: f64) {
        if !(0.0..=1.0).contains(&position) { return; }
        let packs = plugins::installed();
        let Some(setting) = packs.iter().find(|(p, _)| p.id == plugin && p.fits_here())
            .and_then(|(p, _)| p.settings.iter().find(|s| s.id == key)) else { return };
        if let SettingKind::Number { min, max } = &setting.kind {
            let value = ((1.0 - position) * min + position * max).clamp(*min, *max);
            self.change_plugin_setting(plugin, key, json!(value));
        }
    }

    pub(super) fn open_plugin_actions(&mut self, song: Song, player: bool) {
        self.menu_playback = self.shared.read(|s| s.current.as_ref().filter(|c| same_track(c, &song)).map(|_| self.token));
        self.shared.update(|s| {
            s.plugin_action_token = s.plugin_action_token.wrapping_add(1);
            s.plugin_action_player = player;
            let available = if player { s.has_player_actions } else { s.has_track_actions };
            s.plugin_action_song = available.then_some(song);
        });
    }

    pub(super) fn close_plugin_actions(&mut self, token: i64) {
        if self.shared.read(|s| token == s.plugin_action_token) {
            self.menu_playback = None;
            self.shared.update(|s| s.plugin_action_song = None);
        }
    }

    pub(super) fn run_plugin_action(&mut self, plugin: &str, id: &str, token: i64) {
        let target = self.shared.read(|s| {
            (s.plugin_action_token == token).then(|| s.plugin_action_song.as_ref().map(|song| (
                if s.plugin_action_player { "player" } else { "track" }, public_song(song),
                s.current.as_ref().is_some_and(|c| same_track(c, song))
            ))).flatten()
        });
        let Some((context, track, playing_target)) = target else { return };
        let can_control = playing_target && self.menu_playback == Some(self.token);
        self.close_plugin_actions(token);
        let mut snapshot = self.plugin_snapshot();
        if !can_control { snapshot["playbackToken"] = Value::Null; }
        if let Err(err) = self.plugins.dispatch_action(plugin, id, context, &track, &snapshot) { self.shared.fail(err); }
    }

    pub(super) fn tell_loudness(&mut self) {
        if self.plugins.is_empty() { return; }
        let snap = self.plugin_snapshot();
        self.plugins.send("loudness", &snap["loudness"], &snap);
    }

    pub(super) fn tell_queue(&mut self) {
        self.queue_revision = self.queue_revision.wrapping_add(1);
        if self.plugins.is_empty() { return; }
        let header = self.shared.read(|s| json!({"revision":self.queue_revision, "total":s.queue.len(),
            "currentIndex":current_index(s), "playbackToken":s.current.as_ref().map(|_| self.token.to_string())}));
        self.send_plugins("queueChanged", header);
    }

    pub(super) fn tell_lyrics(&mut self) {
        self.lyrics_revision = self.lyrics_revision.wrapping_add(1);
        self.send_plugins("lyricsChanged", json!({"revision":self.lyrics_revision, "total":self.lines.len(), "playbackToken":self.token.to_string()}));
    }

    pub(super) fn tell_playback(&mut self) {
        if self.plugins.is_empty() { return; }
        let snap = self.plugin_snapshot();
        let data = json!({"playing":snap["playing"], "position":snap["position"], "duration":snap["duration"],
            "mode":snap["mode"], "playbackToken":snap["playbackToken"]});
        self.plugins.send("playback", &data, &snap);
    }

    pub(super) fn plugin_queue_page(&mut self, plugin: &str, req: &str, offset: usize, limit: usize) {
        let page = self.shared.read(|s| json!({"revision":self.queue_revision, "offset":offset, "total":s.queue.len(),
            "currentIndex":current_index(s), "playbackToken":s.current.as_ref().map(|_| self.token.to_string()),
            "items":s.queue.iter().skip(offset).take(limit.min(100)).map(public_song).collect::<Vec<_>>() }));
        let snap = self.plugin_snapshot();
        self.plugins.reply_page(plugin, "queuePage", req, Ok(page), &snap);
    }

    pub(super) fn plugin_lyrics_page(&mut self, plugin: &str, req: &str, token: &str, offset: usize, limit: usize) {
        let snap = self.plugin_snapshot();
        let result = if snap["playbackToken"].as_str() == Some(token) {
            Ok(json!({"playbackToken":token, "revision":self.lyrics_revision, "offset":offset, "total":self.lines.len(),
                "items":self.lines.iter().skip(offset).take(limit.min(100)).map(public_line).collect::<Vec<_>>() }))
        } else { Err("播放曲目已改变") };
        self.plugins.reply_page(plugin, "lyricsTimeline", req, result, &snap);
    }

    pub(super) fn plugin_control(&mut self, op: &str, token: &str, seconds: Option<f64>) {
        if token != self.token.to_string() { return; }
        let Some((playing, buffering, duration)) = self.shared.read(|s| s.current.as_ref().map(|song| (s.playing, s.buffering, song.seconds))) else { return };
        match op {
            "pause" if playing && !buffering => self.toggle(),
            "resume" if !playing && !buffering && (self.paused || self.left_at.is_some()) => self.toggle(),
            "previous" => self.handle(Job::Previous), "next" => self.handle(Job::Next),
            "seek" if !buffering && duration > 0.0 => if let Some(s) = seconds.filter(|s| s.is_finite() && *s >= 0.0) {
                self.handle(Job::Seek((s / duration).clamp(0.0, 1.0)));
            },
            _ => {}
        }
    }
}
