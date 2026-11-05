//! The protocol of search: what was searched and found, what was searched
//! before here and what NetEase searches most now.

#[bern::protocol(namespace = "search", version = 1)]
pub mod search {
    use bern::prelude::*;
    use enji_protocol::enji::{Album, Artist, Playlist, Song};

    /// What a search on NetEase looks for.
    #[value]
    pub enum SearchKind {
        #[label(tr! { "zh-Hans" => "单曲", "en" => "Songs" })]
        Songs,
        #[label(tr! { "zh-Hans" => "专辑", "en" => "Albums" })]
        Albums,
        #[label(tr! { "zh-Hans" => "歌手", "en" => "Artists" })]
        Artists,
        #[label(tr! { "zh-Hans" => "歌单", "en" => "Playlists" })]
        Playlists,
    }

    /// A word to search for: one searched before, or one of NetEase's most
    /// searched, with its rank and a short note.
    #[value]
    pub struct SearchWord {
        pub key: i64,
        /// From 1; 0 for a word searched before.
        pub rank: i64,
        #[content]
        pub word: String,
        #[content]
        pub note: String,
        /// Among the three most searched.
        pub top: bool,
    }

    #[state]
    pub struct State {
        /// NetEase is on, so it is searched too.
        pub netease: bool,
        /// What a search looks for on NetEase.
        pub kind: SearchKind,
        /// What was searched last, and a count that changes with each
        /// search, for the field to show it.
        #[content]
        pub searched: String,
        pub search_tick: i64,
        pub searching: bool,
        /// The songs, albums, singers or playlists NetEase found.
        pub results: Vec<Song>,
        pub album_results: Vec<Album>,
        pub artist_results: Vec<Artist>,
        pub playlist_results: Vec<Playlist>,
        /// The songs of the local music that match.
        pub local_results: Vec<Song>,
        pub has_local_results: bool,
        /// A search found nothing.
        pub no_results: bool,
        /// What was searched here before, the latest first, and what is
        /// searched most on NetEase now.
        pub recent: Vec<SearchWord>,
        pub has_recent: bool,
        pub hot: Vec<SearchWord>,
        pub has_hot: bool,
        /// The most searched words are loading.
        pub loading: bool,
    }

    #[commands]
    pub trait Commands {
        /// Search songs by name, singer or album: the local music, and
        /// NetEase while it is on.
        fn search(&mut self, text: String) -> Result<(), CommandError>;
        /// Look for songs, albums, singers or playlists on NetEase.
        fn set_kind(&mut self, kind: SearchKind) -> Result<(), CommandError>;
        /// Forget what was searched before.
        fn clear_recent(&mut self) -> Result<(), CommandError>;
        /// NetEase was switched on or off.
        #[on(enji::netease_changed)]
        fn netease_changed(&mut self, on: bool) -> Result<(), CommandError>;
        /// The list of what is searched most was switched on or off.
        #[on(enji::hot_search_changed)]
        fn hot_search_changed(&mut self, on: bool) -> Result<(), CommandError>;
    }
}
