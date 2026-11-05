//! How far the song is. A module's state travels whole, and what reads any
//! of it draws again, so this stays apart from the core's state and from
//! what moves faster still.

#[bern::protocol(namespace = "live", version = 1)]
pub mod live {
    #[state]
    pub struct State {
        /// How far the song is, from 0 to 1.
        pub progress: f64,
        #[content]
        pub elapsed: String,
    }

    #[commands]
    pub trait Commands {}
}
