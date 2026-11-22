//! Messages into an individual JS context, and the calls allowed back out of it.

use super::Pack;
use serde_json::{Value, json};

pub(super) const PAGE_LIMIT: usize = 100;
pub(super) const PAGES_IN_FLIGHT: usize = 16;
pub(super) const MAX_OFFSET: u64 = if usize::BITS < 53 { usize::MAX as u64 } else { (1 << 53) - 1 };

#[derive(Clone, Debug, PartialEq)]
pub enum Call {
    OverlayShow(String, Value),
    OverlayHide(String),
    /// Transient in-memory desktop lyric color override (0x000000..=0xffffff); None restores the user's color.
    DesktopLyricColor { primary: Option<i64>, secondary: Option<i64> },
    /// Opt in/out of visual animation frame events while an overlay or desktop lyrics are visible.
    WantFrames(bool),
    CoverPulse(f64),
    Toast(String),
    StorageSet(String, String),
    Log(String),
    QueuePage { request_id: String, offset: usize, limit: usize },
    LyricsTimeline { request_id: String, playback_token: String, offset: usize, limit: usize },
    /// operation is resume, pause, next, previous or seek (seconds only for seek).
    PlaybackControl { operation: String, playback_token: String, seconds: Option<f64> },
    /// A suggestion for the source's next start, never a live gain setter.
    SuggestTrackGain { source_key: String, gain_db: f64 },
}

fn bounded_text(value: &Value, units: usize) -> Option<String> {
    value.as_str().filter(|s| !s.trim().is_empty() && s.encode_utf16().take(units + 1).count() <= units).map(str::to_owned)
}

fn index(value: &Value) -> Option<usize> {
    usize::try_from(value.as_u64().filter(|&n| n <= MAX_OFFSET)?).ok()
}

impl Call {
    pub(super) fn parse(value: &Value) -> Option<Self> {
        let args = value.get(1)?.as_array()?;
        let arg = |i: usize| args.get(i).and_then(Value::as_str).unwrap_or("").to_owned();
        let page = |start: usize| -> Option<(usize, usize)> {
            let offset = index(args.get(start)?)?;
            let limit = index(args.get(start + 1)?).filter(|n| (1..=PAGE_LIMIT).contains(n))?;
            Some((offset, limit))
        };
        Some(match value.get(0)?.as_str()? {
            "overlayShow" => Self::OverlayShow(arg(0), args.get(1).cloned().unwrap_or(Value::Null)),
            "overlayHide" => Self::OverlayHide(arg(0)),
            "desktopLyricColor" if args.len() == 2 => {
                let rgb = |v: &Value| -> Option<Option<i64>> {
                    if v.is_null() {
                        Some(None)
                    } else {
                        v.as_i64().filter(|c| (0..=0xffffff).contains(c)).map(Some)
                    }
                };
                Self::DesktopLyricColor { primary: rgb(&args[0])?, secondary: rgb(&args[1])? }
            }
            "wantFrames" if args.len() == 1 => Self::WantFrames(args[0].as_bool()?),
            "coverPulse" => Self::CoverPulse(args.first().and_then(Value::as_f64).unwrap_or(0.5).clamp(0.0, 1.0)),
            "toast" => Self::Toast(arg(0)),
            "storageSet" => Self::StorageSet(arg(0), arg(1)),
            "log" => Self::Log(arg(0)),
            "queuePage" if args.len() == 3 => {
                let (offset, limit) = page(1)?;
                Self::QueuePage { request_id: bounded_text(&args[0], 64)?, offset, limit }
            }
            "lyricsTimeline" if args.len() == 4 => {
                let (offset, limit) = page(2)?;
                Self::LyricsTimeline { request_id: bounded_text(&args[0], 64)?, playback_token: bounded_text(&args[1], 128)?, offset, limit }
            }
            "playbackControl" => {
                let operation = args.first()?.as_str()?;
                let seconds = match operation {
                    "seek" if args.len() == 3 => Some(args[2].as_f64().filter(|s| s.is_finite() && *s >= 0.0)?),
                    "resume" | "pause" | "next" | "previous" if args.len() == 2 => None,
                    _ => return None,
                };
                Self::PlaybackControl { operation: operation.to_owned(), playback_token: bounded_text(&args[1], 128)?, seconds }
            }
            "suggestTrackGain" if args.len() == 2 => Self::SuggestTrackGain {
                source_key: bounded_text(&args[0], 4096)?,
                gain_db: args[1].as_f64().filter(|db| db.is_finite() && (-24.0..=12.0).contains(db))?,
            },
            _ => return None,
        })
    }

    pub(super) fn permission(&self) -> Option<&'static str> {
        match self {
            Self::OverlayShow(..)
            | Self::OverlayHide(_)
            | Self::DesktopLyricColor { .. }
            | Self::WantFrames(_) => Some("overlay"),
            Self::CoverPulse(_) => Some("cover"),
            Self::QueuePage { .. } => Some("queue"),
            Self::LyricsTimeline { .. } => Some("lyrics"),
            Self::PlaybackControl { .. } => Some("playback_control"),
            Self::SuggestTrackGain { .. } => Some("audio_gain"),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PageKind { Queue, Lyrics }

impl PageKind {
    pub(super) fn from_event(event: &str) -> Option<Self> {
        match event { "queuePage" => Some(Self::Queue), "lyricsTimeline" => Some(Self::Lyrics), _ => None }
    }

    pub(super) fn permission(self) -> &'static str {
        match self { Self::Queue => "queue", Self::Lyrics => "lyrics" }
    }
}

pub(super) struct PageRequest {
    pub kind: PageKind,
    pub limit: usize,
    pub token: Option<String>,
}

impl PageRequest {
    pub(super) fn from_call(call: &Call) -> Option<(&str, Self)> {
        match call {
            Call::QueuePage { request_id, limit, .. } => Some((request_id, Self { kind: PageKind::Queue, limit: *limit, token: None })),
            Call::LyricsTimeline { request_id, playback_token, limit, .. } => Some((request_id, Self { kind: PageKind::Lyrics, limit: *limit, token: Some(playback_token.clone()) })),
            _ => None,
        }
    }

    pub(super) fn page(&self, value: Value) -> Result<Value, &'static str> {
        let items = value["items"].as_array().filter(|a| a.len() <= self.limit).ok_or("invalid_page")?;
        index(&value["offset"]).ok_or("invalid_page")?;
        index(&value["total"]).ok_or("invalid_page")?;
        if self.kind == PageKind::Lyrics && value["playbackToken"].as_str() != self.token.as_deref() { return Err("stale_playback"); }
        let mut out = fields(&value, &["offset", "total", "revision", "playbackToken", "currentIndex"]);
        out["items"] = match self.kind {
            PageKind::Queue => Value::Array(items.iter().map(track).collect()),
            PageKind::Lyrics => Value::Array(items.clone()),
        };
        Ok(out)
    }
}

fn fields(value: &Value, keys: &[&str]) -> Value {
    Value::Object(keys.iter().filter_map(|&k| value.get(k).map(|v| (k.to_owned(), v.clone()))).collect())
}

pub(super) fn track(value: &Value) -> Value {
    if !value.is_object() { return Value::Null; }
    fields(value, &["id", "name", "artists", "album", "duration", "trackKey"])
}

fn can_read_loudness(pack: &Pack) -> bool { pack.has("loudness") || pack.has("audio_gain") }

const PLAYBACK: &[&str] = &["playing", "position", "duration", "mode", "playbackToken", "desktopLyrics"];

/// This is a projection, not removal of known sensitive keys: new worker fields stay private until opted in.
pub(super) fn snapshot(pack: &Pack, value: &Value) -> Value {
    let mut out = fields(value, PLAYBACK);
    out["song"] = track(&value["song"]);
    if can_read_loudness(pack) { out["loudness"] = value["loudness"].clone(); }
    out
}

/// Replies, actions and settings are never broadcast, not even if the caller passes their names here.
pub(super) fn broadcast(pack: &Pack, event: &str, data: &Value) -> Option<Value> {
    Some(match event {
        "song" => track(data),
        "playback" => fields(data, PLAYBACK),
        "lyric" if pack.has("lyrics") => fields(data, &["index", "text", "translation", "next"]),
        "audio" if pack.has("audio") => fields(data, &["level", "bass", "beat", "bpm", "position"]),
        "queueChanged" if pack.has("queue") => fields(data, &["revision", "total", "currentIndex", "playbackToken"]),
        "lyricsChanged" if pack.has("lyrics") => fields(data, &["revision", "total", "playbackToken"]),
        "loudness" if can_read_loudness(pack) => data.clone(),
        _ => return None,
    })
}

/// Bern transports only JS source. Embed *one string literal* for JSON.parse, never a JS expression from a
/// manifest, setting, track or plugin call. Older JS engines also need line separators escaped in literals.
pub(super) fn json_string(data: &Value) -> String {
    json!(data.to_string()).to_string().replace('\u{2028}', "\\u2028").replace('\u{2029}', "\\u2029")
}
