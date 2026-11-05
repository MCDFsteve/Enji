//! The pulses a plugin gives the playing cover, apart from what moves on its
//! own, so the pages around the cover draw again only on a pulse.

#[bern::protocol(namespace = "pulse", version = 1)]
pub mod pulse {
    #[state]
    pub struct State {
        /// Each pulse adds one, by how strong it is.
        pub cover_pulse_soft: i64,
        pub cover_pulse: i64,
        pub cover_pulse_strong: i64,
    }

    #[commands]
    pub trait Commands {}
}
