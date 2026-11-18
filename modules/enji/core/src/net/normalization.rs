//! Track measurements and policy decisions have different lifetimes: a measurement describes
//! an encode, a plugin hint also depends on that plugin's version and settings, and the
//! gain sent to the output is frozen for one play. None of them is the user's volume.
use super::Job;
use crate::{audio::{loudness::{self, Cache, Info}, Stream}, store};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::{
    collections::{HashMap, VecDeque}, fs, io::Read,
    sync::{Arc, atomic::{AtomicBool, Ordering}, mpsc::{self, SyncSender}},
    time::{Duration, Instant},
};
use tokio::sync::{mpsc::UnboundedSender, watch};

pub(super) fn digest(value: &[u8]) -> String {
    Sha1::digest(value).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// An ETag is a version of a *resource*, not of an entire service. Do not reuse weak
/// validators, strip signed query strings, or key by a song id / requested quality alone.
/// A signed URL that changes can cause a cache miss; that is preferable to a wrong gain.
 pub(super) fn etag_key(id: i64, response: &reqwest::Response) -> Option<String> {
    let tag = response.headers().get(reqwest::header::ETAG)?.to_str().ok()?.trim();
    if tag.len() < 3 || !tag.starts_with('"') || !tag.ends_with('"') { return None; }
    Some(format!("http:{}", digest(json!([id, response.url().as_str(), tag]).to_string().as_bytes())))
}

#[derive(Default)]
pub(crate) struct SourceLevel {
    key: Option<String>,
    info: Option<Info>,
    status: &'static str,
    eligible: bool,
}

impl SourceLevel {
    pub(super) fn new(key: Option<String>, info: Option<Info>, preview: bool) -> Self {
        if preview { return Self { status: "preview", ..Self::default() }; }
        Self { key, info, status: if info.is_some() { "ready" } else { "pending" }, eligible: true }
    }

    pub(super) fn partial() -> Self { Self { status: "unavailable", ..Self::default() } }
}

struct Measure {
    token: u64,
    stream: Stream,
    key: Option<String>,
    seconds: f64,
    cancel: Arc<AtomicBool>,
}

/// Spawned lazily on the first complete track. One decoder, at most one waiting stream;
/// full PCM is never retained, the audio thread never waits, and nothing is downloaded here.
fn analyzer(mut cache: Cache, jobs: UnboundedSender<Job>) -> Option<SyncSender<Measure>> {
    let (tx, rx) = mpsc::sync_channel::<Measure>(1);
    std::thread::Builder::new().name("enji-loudness".into()).spawn(move || {
        for task in rx {
            if task.cancel.load(Ordering::Relaxed) { continue; }
            let key = task.key.or_else(|| loudness::content_key(&task.stream, &task.cancel));
            let info = key.as_deref().and_then(|key| {
                if let Some(info) = cache.get(key) { return Some(info); }
                let info = loudness::measure(&task.stream, task.seconds, &task.cancel)?;
                if task.cancel.load(Ordering::Relaxed) { return None; }
                cache.insert(key.to_owned(), info);
                if let Err(err) = cache.save() { eprintln!("loudness cache: {err}"); }
                Some(info)
            });
            if !task.cancel.load(Ordering::Relaxed) {
                let _ = jobs.send(Job::SourceMeasured { token: task.token, key, info });
            }
        }
    }).ok()?;
    Some(tx)
}

const MAX_HINTS: usize = 512;
type Hints = VecDeque<(String, String, f32)>;

fn load_hints() -> Hints {
    let Ok(file) = fs::File::open(store::dir().join("plugin-gains.json")) else { return Hints::new() };
    let rows: Hints = serde_json::from_reader(file.take(256 * 1024)).unwrap_or_default();
    rows.into_iter().filter(|(scope, key, db)| scope.len() == 40 && key.len() <= 256 && (-24.0..=12.0).contains(db))
        .take(MAX_HINTS).collect()
}

/// One serial writer. A busy plugin can only replace the pending snapshot, not queue an
/// unbounded series of file writes. Audio callbacks never access this file.
fn hint_writer() -> watch::Sender<Hints> {
    let (tx, mut rx) = watch::channel(Hints::new());
    tokio::spawn(async move {
        while rx.changed().await.is_ok() {
            let entries = rx.borrow_and_update().clone();
            let _ = tokio::task::spawn_blocking(move || -> std::io::Result<()> {
                let dir = store::dir();
                fs::create_dir_all(&dir)?;
                let tmp = dir.join(format!("plugin-gains.{}.tmp", std::process::id()));
                let data = serde_json::to_vec(&entries)?;
                fs::write(&tmp, data)?;
                fs::rename(tmp, dir.join("plugin-gains.json"))
            }).await.map(|r| if let Err(err) = r { eprintln!("plugin gain cache: {err}"); });
        }
    });
    tx
}

pub(super) struct Normalization {
    current: Option<SourceLevel>,
    cache: Cache,
    analysis: Option<SyncSender<Measure>>,
    cancel: Arc<AtomicBool>,
    /// Report the gain the output acknowledges, never the one we merely requested.
    actual: Option<(f32, bool)>,
    pub(super) scopes: HashMap<String, String>,
    hints: Hints,
    writer: Option<watch::Sender<Hints>>,
    dirty: bool,
    last_save: Option<Instant>,
}

impl Normalization {
    pub(super) fn load() -> Self {
        Self { current: None, cache: Cache::load(), analysis: None, cancel: Arc::new(AtomicBool::new(false)),
            actual: None, scopes: HashMap::new(), hints: load_hints(), writer: None, dirty: false, last_save: None }
    }

    pub(super) fn reset_track(&mut self) { self.current = None; self.actual = None; }

    pub(super) fn prepare(&mut self, mut level: SourceLevel) {
        if let Some(info) = level.key.as_deref().and_then(|key| self.cache.get(key)) {
            level.info = Some(info);
            level.status = "ready";
        }
        self.current = Some(level);
    }

    pub(super) fn cancel_measurements(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        if let Some(source) = self.current.as_mut().filter(|s| s.status == "measuring") {
            source.status = if source.info.is_some() { "ready" } else { "deferred" };
        }
    }

    pub(super) fn incomplete(&mut self) {
        if let Some(source) = &mut self.current { source.eligible = false; source.status = "unavailable"; }
    }

    pub(super) fn analyze(&mut self, token: u64, stream: Stream, seconds: f64, jobs: &UnboundedSender<Job>) {
        let Some(source) = &mut self.current else { return };
        if !source.eligible || source.status == "measuring" || source.info.is_some_and(|info| info.true_peak_dbtp().is_some()) { return; }
        if self.analysis.is_none() { self.analysis = analyzer(self.cache.clone(), jobs.clone()); }
        let task = Measure { token, stream, key: source.key.clone(), seconds, cancel: self.cancel.clone() };
        source.status = if self.analysis.as_ref().is_some_and(|tx| tx.try_send(task).is_ok()) { "measuring" } else { "deferred" };
    }

    pub(super) fn measured(&mut self, same_play: bool, key: Option<String>, info: Option<Info>) {
        if let (Some(key), Some(info)) = (&key, info) { self.cache.insert(key.clone(), info); }
        if let Some(source) = self.current.as_mut().filter(|_| same_play) {
            if key.is_some() { source.key = key; }
            source.info = info.or(source.info);
            source.status = if source.info.is_some() { "ready" } else { "unavailable" };
        }
    }

    pub(super) fn gain_for_play(&self, enabled: bool, provider: &str) -> f32 {
        let Some(source) = self.current.as_ref().filter(|s| enabled && s.eligible) else { return 0.0 };
        let Some(info) = source.info else { return 0.0 };
        let hint = self.scopes.get(provider).zip(source.key.as_ref()).and_then(|(scope, key)| {
            self.hints.iter().find(|(owner, src, _)| owner == scope && src == key).map(|(_, _, db)| *db)
        });
        hint.map_or_else(|| info.gain_db(), |db| info.limit_gain_db(db))
    }

    /// Accept only a hint for the selected plugin and the exact advertised encode. This
    /// method intentionally has no access to Audio: a late result cannot alter this play.
    pub(super) fn suggest(&mut self, provider: &str, key: &str, db: f64) {
        let Some(scope) = self.scopes.get(provider) else { return };
        if !(-24.0..=12.0).contains(&db) || !self.current.as_ref().is_some_and(|s| s.eligible && s.key.as_deref() == Some(key)) { return; }
        if let Some(i) = self.hints.iter().position(|(owner, src, _)| owner == scope && src == key) {
            if self.hints[i].2 == db as f32 { return; }
            self.hints.remove(i);
        }
        self.hints.push_front((scope.clone(), key.to_owned(), db as f32));
        self.hints.truncate(MAX_HINTS);
        self.dirty = true;
    }

    pub(super) fn flush_hints(&mut self) {
        if self.dirty && self.last_save.is_none_or(|last| last.elapsed() >= Duration::from_secs(1)) {
            self.writer.get_or_insert_with(hint_writer).send_replace(self.hints.clone());
            self.dirty = false;
            self.last_save = Some(Instant::now());
        }
    }

    pub(super) fn acknowledged(&mut self, db: f32, bypass: bool) -> bool {
        if self.current.is_none() || self.actual == Some((db, bypass)) { return false; }
        self.actual = Some((db, bypass));
        true
    }

    pub(super) fn snapshot(&self, token: u64, enabled: bool, provider: &str) -> Value {
        let Some(s) = &self.current else { return Value::Null };
        json!({"playbackToken":token.to_string(), "sourceKey":s.key, "enabled":enabled,
            "provider":(!provider.is_empty()).then_some(provider), "status": if enabled { s.status } else { "disabled" },
            "source":s.info.map(|v| v.source()), "integratedLufs":s.info.and_then(|v| v.integrated_lufs()),
            "truePeakDbtp":s.info.and_then(|v| v.true_peak_dbtp()), "samplePeakDbfs":s.info.and_then(|v| v.sample_peak_dbfs()),
            "recommendedGainDb":s.info.map(|v| v.gain_db()), "appliedGainDb":self.actual.map(|(db, _)| db),
            "bitPerfectBypass":self.actual.is_some_and(|(_, bypass)| bypass)})
    }
}

impl Drop for Normalization {
    fn drop(&mut self) { self.cancel.store(true, Ordering::Relaxed); }
}
