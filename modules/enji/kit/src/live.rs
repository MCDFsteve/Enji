//! What changes many times a second while a song plays: the core's worker
//! writes it here, and the `live` module's engine reads it.

use std::sync::{Mutex, MutexGuard, OnceLock};

#[derive(Clone, Debug, PartialEq)]
pub struct Live {
    /// How far the song is, from 0 to 1.
    pub progress: f64,
    pub elapsed: String,
    /// How far through the sung line's characters the singing is; below
    /// zero when the line is not timed by the word.
    pub lit: f64,
    pub pulse_soft: i64,
    pub pulse: i64,
    pub pulse_strong: i64,
}

fn live() -> MutexGuard<'static, Live> {
    static LIVE: OnceLock<Mutex<Live>> = OnceLock::new();
    LIVE.get_or_init(|| {
        Mutex::new(Live { progress: 0.0, elapsed: "0:00".into(), lit: -1.0, pulse_soft: 0, pulse: 0, pulse_strong: 0 })
    })
    .lock()
    .unwrap_or_else(|e| e.into_inner())
}

pub fn read<T>(look: impl FnOnce(&Live) -> T) -> T {
    look(&live())
}

/// Whether this changed anything.
pub fn update(change: impl FnOnce(&mut Live)) -> bool {
    let mut live = live();
    let was = live.clone();
    change(&mut live);
    *live != was
}
