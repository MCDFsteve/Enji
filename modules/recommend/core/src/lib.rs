//! The engine of the For You page. While NetEase is on it loads today's
//! songs for the account and the playlists NetEase recommends, away from
//! the engine; `wake` takes them in and hands today's songs to the core,
//! which plays a song of them by its place.

use bern::CommandError;
use enji_kit::{convert, list};
use enji_protocol::enji::{self, Login, Playlist, Song, Source};
use ncm_api_rs::Query;
use recommend_protocol::recommend::{self, DayPart, State};
use serde_json::Value;
use std::sync::{Arc, Mutex};

/// What a load brought back.
enum Loaded {
    Picks(Vec<Playlist>),
    Daily(Vec<Song>),
    /// NetEase could not be reached.
    Failed,
}

pub struct RecommendEngine {
    state: State,
    /// What loads brought back, each with its number, for `wake`.
    inbox: Arc<Mutex<Vec<(u64, Loaded)>>>,
    /// Each load gets a new number; what an older one brings is dropped.
    load: u64,
    signed_in: bool,
    /// The core's state was read once, as the engine started.
    started: bool,
    /// The local copies and play counts today's songs show.
    dressed: u64,
}

impl Default for RecommendEngine {
    fn default() -> Self {
        Self {
            state: State {
                available: enji_kit::netease_on(),
                day_part: day_part(),
                daily: Vec::new(),
                has_daily: false,
                picks: Vec::new(),
                loading: false,
            },
            inbox: Arc::default(),
            load: 0,
            signed_in: false,
            started: false,
            dressed: enji_kit::dressing(),
        }
    }
}

/// The part of the day at this device's hour.
fn day_part() -> DayPart {
    use chrono::Timelike;
    match chrono::Local::now().hour() {
        5..=10 => DayPart::Morning,
        11..=12 => DayPart::Noon,
        13..=17 => DayPart::Afternoon,
        _ => DayPart::Evening,
    }
}

impl RecommendEngine {
    /// Load the page again, the songs of the day only for an account.
    fn reload(&mut self) {
        self.load += 1;
        self.state.loading = true;
        self.state.day_part = day_part();
        let (load, inbox, waker, signed_in) = (self.load, self.inbox.clone(), bern::wake::waker(), self.signed_in);
        enji_kit::runtime().spawn(async move {
            let client = enji_kit::client();
            let picks = client.personalized(&Query::new().param("limit", "18")).await;
            let daily = if signed_in { client.recommend_songs(&Query::new()).await.ok() } else { None };
            let mut arrived = vec![match picks {
                Ok(r) => Loaded::Picks(list(&r.body["result"], convert::playlist)),
                Err(_) => Loaded::Failed,
            }];
            if let Some(r) = daily {
                arrived.push(Loaded::Daily(convert::songs(&r.body["data"]["dailySongs"], &Value::Null)));
            }
            inbox.lock().unwrap_or_else(|e| e.into_inner()).extend(arrived.into_iter().map(|one| (load, one)));
            if let Some(waker) = waker {
                waker.wake();
            }
        });
    }

    fn set_daily(&mut self, mut songs: Vec<Song>) {
        enji_kit::dress_all(&mut songs);
        self.dressed = enji_kit::dressing();
        self.state.has_daily = !songs.is_empty();
        self.state.daily = songs;
        // The core plays them by their place.
        enji::calls::set_songs(Source::Daily, self.state.daily.clone());
    }
}

impl recommend::Commands for RecommendEngine {
    fn refresh(&mut self) -> Result<(), CommandError> {
        if self.state.available {
            self.reload();
        }
        Ok(())
    }

    fn netease_changed(&mut self, on: bool) -> Result<(), CommandError> {
        self.state.available = on;
        if on {
            self.reload();
        } else {
            self.load += 1;
            self.state.loading = false;
            self.state.picks.clear();
            self.set_daily(Vec::new());
        }
        Ok(())
    }

    fn account_changed(&mut self, signed_in: bool) -> Result<(), CommandError> {
        self.signed_in = signed_in;
        if !signed_in {
            self.set_daily(Vec::new());
        }
        if self.state.available {
            self.reload();
        }
        Ok(())
    }
}

impl recommend::Engine for RecommendEngine {
    fn state(&self) -> &State {
        &self.state
    }

    fn wake(&mut self) {
        if !self.started {
            self.started = true;
            if let Some(core) = enji::current_state() {
                self.state.available = core.netease;
                self.signed_in = core.login == Login::In;
            }
            if self.state.available {
                self.reload();
            }
        }
        let arrived: Vec<Loaded> = {
            let mut inbox = self.inbox.lock().unwrap_or_else(|e| e.into_inner());
            inbox.drain(..).filter(|(load, _)| *load == self.load).map(|(_, one)| one).collect()
        };
        if !arrived.is_empty() {
            self.state.loading = false;
        }
        for one in arrived {
            match one {
                Loaded::Picks(picks) => self.state.picks = picks,
                Loaded::Daily(songs) => self.set_daily(songs),
                Loaded::Failed => enji::calls::show_error("连不上网易云音乐，检查一下网络".into()),
            }
        }
        // A copy found or a song played since: shown, and played, so.
        if enji_kit::dressing() != self.dressed {
            self.dressed = enji_kit::dressing();
            if self.state.has_daily {
                let songs = std::mem::take(&mut self.state.daily);
                self.set_daily(songs);
            }
        }
    }
}
