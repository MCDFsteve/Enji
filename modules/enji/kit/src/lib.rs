//! What the engines of Enji's modules share with its core engine, apart
//! from the protocol: the NetEase client of the account signed in now, the
//! runtime its requests run on, NetEase's answers read as the protocol's
//! values, the files Enji keeps, and each song's local copy and play count.

pub mod convert;
pub mod copies;
pub mod plays;
pub mod store;

use enji_protocol::enji::Song;
use ncm_api_rs::{ApiClient, create_client};
use std::sync::{
    OnceLock, RwLock,
    atomic::{AtomicU64, Ordering},
};

/// The runtime NetEase's requests and other background work run on, shared
/// by the core and the modules.
pub fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("enji-net")
            .enable_all()
            .build()
            .expect("an async runtime")
    })
}

fn current() -> &'static RwLock<ApiClient> {
    static CLIENT: OnceLock<RwLock<ApiClient>> = OnceLock::new();
    CLIENT.get_or_init(|| RwLock::new(create_client(store::cookie())))
}

/// The NetEase client: the account's while one is signed in, anonymous
/// otherwise.
pub fn client() -> ApiClient {
    current().read().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Sign the client in with an account's cookie, or out with `None`.
pub fn set_cookie(cookie: Option<String>) {
    *current().write().unwrap_or_else(|e| e.into_inner()) = create_client(cookie);
}

/// Whether NetEase is on, as the settings were saved: what a module starts
/// with, before it hears from the core.
pub fn netease_on() -> bool {
    store::read("settings.json")["netease"].as_bool().unwrap_or(true)
}

static DRESSING: AtomicU64 = AtomicU64::new(0);

/// A count that goes up each time the local copies or the play counts
/// change, so a list of songs knows to dress itself again.
pub fn dressing() -> u64 {
    DRESSING.load(Ordering::Acquire)
}

fn redressed() {
    DRESSING.fetch_add(1, Ordering::AcqRel);
}

/// Every song of `songs` played from its copy here, where there is one,
/// and with its play count.
pub fn dress_all(songs: &mut [Song]) {
    for song in songs.iter_mut() {
        copies::dress(song);
    }
    plays::dress(songs);
}

/// `songs` numbered from the top: `index` from 0, `rank` from 1.
pub fn renumbered(songs: impl IntoIterator<Item = Song>) -> Vec<Song> {
    songs
        .into_iter()
        .enumerate()
        .map(|(i, song)| Song { index: i as i64, rank: i as i64 + 1, ..song })
        .collect()
}

/// The items of a NetEase list, read with `convert`.
pub fn list<T>(value: &serde_json::Value, convert: fn(&serde_json::Value) -> T) -> Vec<T> {
    value.as_array().map(|all| all.iter().map(convert).collect()).unwrap_or_default()
}
