//! The protocol of the For You page: the part of the day it greets in,
//! today's songs for the account and the playlists NetEase recommends.

#[bern::protocol(namespace = "recommend", version = 1)]
pub mod recommend {
    use bern::prelude::*;
    use enji_protocol::enji::{Playlist, Song};

    /// Morning, noon, afternoon or evening, at this device's hour.
    #[value]
    pub enum DayPart {
        Morning,
        Noon,
        Afternoon,
        Evening,
    }

    #[state]
    pub struct State {
        /// NetEase is on, so the page has something to show.
        pub available: bool,
        /// The part of the day the page greets in.
        pub day_part: DayPart,
        /// Today's songs for the account.
        pub daily: Vec<Song>,
        pub has_daily: bool,
        /// The playlists NetEase recommends.
        pub picks: Vec<Playlist>,
        pub loading: bool,
    }

    #[commands]
    pub trait Commands {
        /// Load today's songs and the recommended playlists again.
        fn refresh(&mut self) -> Result<(), CommandError>;
        /// NetEase was switched on or off.
        #[on(enji::netease_changed)]
        fn netease_changed(&mut self, on: bool) -> Result<(), CommandError>;
        /// An account signed in or out.
        #[on(enji::account_changed)]
        fn account_changed(&mut self, signed_in: bool) -> Result<(), CommandError>;
    }
}
