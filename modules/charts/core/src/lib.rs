//! The engine of the Charts page. While NetEase is on it loads the charts
//! away from the engine; `wake` takes them in. A chart opens as any
//! playlist does, in the core.

use bern::CommandError;
use charts_protocol::charts::{self, State};
use enji_kit::{convert, list};
use enji_protocol::enji::{self, Playlist};
use ncm_api_rs::Query;
use serde_json::Value;
use std::sync::{Arc, Mutex};

pub struct ChartsEngine {
    state: State,
    /// What loads brought back, each with its number, for `wake`; `None`
    /// when NetEase could not be reached.
    inbox: Arc<Mutex<Vec<(u64, Option<Vec<Playlist>>)>>>,
    /// Each load gets a new number; what an older one brings is dropped.
    load: u64,
    /// The core's state was read once, as the engine started.
    started: bool,
}

impl Default for ChartsEngine {
    fn default() -> Self {
        Self {
            state: State {
                available: enji_kit::netease_on(),
                charts: Vec::new(),
                loading: false,
            },
            inbox: Arc::default(),
            load: 0,
            started: false,
        }
    }
}

/// A chart: a playlist that says how often it is updated.
fn chart(value: &Value) -> Playlist {
    Playlist {
        detail: value["updateFrequency"].as_str().unwrap_or("").to_string(),
        ..convert::playlist(value)
    }
}

impl ChartsEngine {
    fn reload(&mut self) {
        self.load += 1;
        self.state.loading = true;
        let (load, inbox, waker) = (self.load, self.inbox.clone(), bern::wake::waker());
        enji_kit::runtime().spawn(async move {
            let found = enji_kit::client().toplist(&Query::new()).await;
            let charts = found.ok().map(|r| list(&r.body["list"], chart));
            inbox.lock().unwrap_or_else(|e| e.into_inner()).push((load, charts));
            if let Some(waker) = waker {
                waker.wake();
            }
        });
    }
}

impl charts::Commands for ChartsEngine {
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
            self.state.charts.clear();
        }
        Ok(())
    }
}

impl charts::Engine for ChartsEngine {
    fn state(&self) -> &State {
        &self.state
    }

    fn wake(&mut self) {
        if !self.started {
            self.started = true;
            self.state.available = enji::current_state().is_some_and(|core| core.netease);
            if self.state.available {
                self.reload();
            }
        }
        let arrived: Vec<_> = {
            let mut inbox = self.inbox.lock().unwrap_or_else(|e| e.into_inner());
            inbox.drain(..).filter(|(load, _)| *load == self.load).map(|(_, charts)| charts).collect()
        };
        for charts in arrived {
            self.state.loading = false;
            match charts {
                Some(charts) => self.state.charts = charts,
                None => enji::calls::show_error("连不上网易云音乐，检查一下网络".into()),
            }
        }
    }
}
