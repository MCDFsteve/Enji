//! Where the sung line stands. What shows it moves along the line's own
//! route by itself: this changes when the clock jumps, stops or starts, and
//! with each line.

#[bern::protocol(namespace = "sung", version = 1)]
pub mod sung {
    #[state]
    pub struct State {
        /// Zero while the sung line lights up word by word; below zero when
        /// it is not timed by the word.
        pub lyric_lit: f64,
        /// The second of the song it was last set by.
        pub lyric_clock: f64,
        /// When that was, in seconds since the Unix epoch; zero while the
        /// song stands still.
        pub lyric_since: f64,
    }

    #[commands]
    pub trait Commands {}
}
