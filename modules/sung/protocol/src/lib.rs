//! How far the sung line has gone, which moves 25 times a second. Only the
//! text being sung reads it: whatever reads it draws again each time.

#[bern::protocol(namespace = "sung", version = 1)]
pub mod sung {
    #[state]
    pub struct State {
        /// How far through the sung line's characters the singing is, the
        /// one being sung in part; below zero when the line is not timed by
        /// the word.
        pub lyric_lit: f64,
    }

    #[commands]
    pub trait Commands {}
}
