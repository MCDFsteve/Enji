//! The engine of how far the song is. The core's worker writes it in
//! `enji_kit::live`; `wake` takes it in.

use enji_kit::live::Live;
use live_protocol::live::{self, State};

pub struct LiveEngine {
    state: State,
}

fn state(live: &Live) -> State {
    State {
        progress: live.progress,
        elapsed: live.elapsed.clone(),
    }
}

impl Default for LiveEngine {
    fn default() -> Self {
        Self { state: enji_kit::live::read(state) }
    }
}

impl live::Engine for LiveEngine {
    fn state(&self) -> &State {
        &self.state
    }

    fn wake(&mut self) {
        self.state = enji_kit::live::read(state);
    }
}

impl live::Commands for LiveEngine {}
