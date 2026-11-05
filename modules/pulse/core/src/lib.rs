//! The engine of the cover's pulses. The core's worker writes it in `enji_kit::live`;
//! `wake` takes it in.

use enji_kit::live::Live;
use pulse_protocol::pulse::{self, State};

pub struct PulseEngine {
    state: State,
}

fn state(live: &Live) -> State {
    State {
        cover_pulse_soft: live.pulse_soft,
        cover_pulse: live.pulse,
        cover_pulse_strong: live.pulse_strong,
    }
}

impl Default for PulseEngine {
    fn default() -> Self {
        Self { state: enji_kit::live::read(state) }
    }
}

impl pulse::Engine for PulseEngine {
    fn state(&self) -> &State {
        &self.state
    }

    fn wake(&mut self) {
        self.state = enji_kit::live::read(state);
    }
}

impl pulse::Commands for PulseEngine {}
