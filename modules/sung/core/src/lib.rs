//! The engine of how far the sung line has gone. The core's worker writes it in `enji_kit::live`;
//! `wake` takes it in.

use enji_kit::live::Live;
use sung_protocol::sung::{self, State};

pub struct SungEngine {
    state: State,
}

fn state(live: &Live) -> State {
    State {
        lyric_lit: live.lit,
    }
}

impl Default for SungEngine {
    fn default() -> Self {
        Self { state: enji_kit::live::read(state) }
    }
}

impl sung::Engine for SungEngine {
    fn state(&self) -> &State {
        &self.state
    }

    fn wake(&mut self) {
        self.state = enji_kit::live::read(state);
    }
}

impl sung::Commands for SungEngine {}
