//! The protocol of the Charts page: NetEase's charts.

#[bern::protocol(namespace = "charts", version = 1)]
pub mod charts {
    use bern::prelude::*;
    use enji_protocol::enji::Playlist;

    #[state]
    pub struct State {
        /// NetEase is on, so the page has something to show.
        pub available: bool,
        /// The charts, each with how often it is updated.
        pub charts: Vec<Playlist>,
        pub loading: bool,
    }

    #[commands]
    pub trait Commands {
        /// Load the charts again.
        fn refresh(&mut self) -> Result<(), CommandError>;
        /// NetEase was switched on or off.
        #[on(enji::netease_changed)]
        fn netease_changed(&mut self, on: bool) -> Result<(), CommandError>;
    }
}
