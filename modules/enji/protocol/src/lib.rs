//! The protocol of Enji Music: what the interface shows, and the commands
//! that change it. The interface, the command line and the MCP server all
//! go through these commands.

#[bern::protocol(namespace = "enji", version = 1)]
pub mod enji {
    use bern::prelude::*;

    /// A song in a list, and its place there, to play the list from it.
    #[value]
    pub struct Song {
        pub id: i64,
        pub index: i64,
        /// Its number in the list, from 1.
        pub rank: i64,
        #[content]
        pub name: String,
        #[content]
        pub artists: String,
        #[content]
        pub album: String,
        pub cover: MediaSource,
        /// The cover large and sharp, for the playing song.
        pub artwork: MediaSource,
        /// Length in seconds.
        pub seconds: f64,
        #[content]
        pub duration: String,
        /// Grey on NetEase: there is no right to play it.
        pub unavailable: bool,
        pub album_id: i64,
        pub artist_id: i64,
        /// The URL of the file on this device for a song from a local
        /// folder; empty for a song only on NetEase.
        pub file: String,
        /// How many times it was played: here, or on NetEase where its own
        /// count is higher.
        pub plays: i64,
        /// When it went into the list it is in (a playlist, the local
        /// folders), in milliseconds since 1970; 0 when not known.
        pub added: i64,
        /// Its place in its list as the list itself orders it, from 0.
        pub place: i64,
        /// Enji downloaded it: its file is in the downloads folder.
        pub downloaded: bool,
    }

    /// An album: its cover, and its singer and year.
    #[value]
    pub struct Album {
        pub id: i64,
        #[content]
        pub name: String,
        pub cover: MediaSource,
        #[content]
        pub detail: String,
        pub artist_id: i64,
    }

    /// A singer, with how many songs and albums.
    #[value]
    pub struct Artist {
        pub id: i64,
        #[content]
        pub name: String,
        pub picture: MediaSource,
        #[content]
        pub detail: String,
    }

    /// What shows over the page: a playlist, an album or a singer opened
    /// from it, the last opened on top.
    #[value]
    pub enum Detail {
        #[label(tr! { "zh-Hans" => "页面", "en" => "Page" })]
        Page,
        #[label(tr! { "zh-Hans" => "歌单", "en" => "Playlist" })]
        Playlist,
        #[label(tr! { "zh-Hans" => "专辑", "en" => "Album" })]
        Album,
        #[label(tr! { "zh-Hans" => "歌手", "en" => "Artist" })]
        Artist,
    }

    /// A folder whose music is in the local library.
    #[value]
    pub struct Folder {
        pub key: i64,
        /// Where it is on this device; empty for a folder the system holds,
        /// such as one from Android's picker.
        #[content]
        pub path: String,
        #[content]
        pub name: String,
        pub count: i64,
    }

    /// A song played, when it was last, and how many times.
    #[value]
    pub struct Played {
        pub key: i64,
        pub song: Song,
        #[content]
        pub when: String,
        pub plays: i64,
    }

    /// A comment on the playing song.
    #[value]
    pub struct Comment {
        pub id: i64,
        #[content]
        pub user: String,
        pub avatar: MediaSource,
        #[content]
        pub content: String,
        /// How many liked it, short, as `1.2万`.
        #[content]
        pub likes: String,
        #[content]
        pub time: String,
    }

    /// About the playing song: its album and its singer.
    #[value]
    pub struct Details {
        #[content]
        pub album: String,
        pub album_cover: MediaSource,
        #[content]
        pub released: String,
        #[content]
        pub company: String,
        #[content]
        pub album_description: String,
        #[content]
        pub artist: String,
        pub artist_picture: MediaSource,
        #[content]
        pub artist_description: String,
    }

    /// What follows a song: the list again, the same song, or the list in
    /// a shuffled order.
    #[value]
    pub enum PlayMode {
        #[label(tr! { "zh-Hans" => "列表循环", "en" => "Repeat All" })]
        RepeatAll,
        #[label(tr! { "zh-Hans" => "单曲循环", "en" => "Repeat One" })]
        RepeatOne,
        #[label(tr! { "zh-Hans" => "随机播放", "en" => "Shuffle" })]
        Shuffle,
    }

    /// What the playing page shows beside the cover.
    #[value]
    pub enum Panel {
        #[label(tr! { "zh-Hans" => "歌词", "en" => "Lyrics" })]
        Lyrics,
        #[label(tr! { "zh-Hans" => "评论", "en" => "Comments" })]
        Comments,
        #[label(tr! { "zh-Hans" => "详情", "en" => "Details" })]
        Details,
    }

    /// How the songs of a list are ordered.
    #[value]
    pub enum SongOrder {
        #[label(tr! { "zh-Hans" => "默认顺序", "en" => "Default" })]
        Default,
        #[label(tr! { "zh-Hans" => "播放次数", "en" => "Most Played" })]
        Plays,
        #[label(tr! { "zh-Hans" => "歌曲名", "en" => "Title" })]
        Name,
        #[label(tr! { "zh-Hans" => "歌手", "en" => "Artist" })]
        Artist,
        #[label(tr! { "zh-Hans" => "收藏时间", "en" => "Date Added" })]
        Added,
    }

    /// The quality songs are fetched in; above 320k only for members.
    #[value]
    pub enum Quality {
        #[label(tr! { "zh-Hans" => "标准", "en" => "Standard" })]
        Standard,
        #[label(tr! { "zh-Hans" => "较高", "en" => "Higher" })]
        Higher,
        #[label(tr! { "zh-Hans" => "极高", "en" => "Very High" })]
        Exhigh,
        #[label(tr! { "zh-Hans" => "无损", "en" => "Lossless" })]
        Lossless,
        #[label(tr! { "zh-Hans" => "Hi-Res", "en" => "Hi-Res" })]
        Hires,
    }

    #[value]
    pub enum Appearance {
        #[label(tr! { "zh-Hans" => "自动", "en" => "Auto" })]
        System,
        #[label(tr! { "zh-Hans" => "浅色", "en" => "Light" })]
        Light,
        #[label(tr! { "zh-Hans" => "深色", "en" => "Dark" })]
        Dark,
    }

    #[value]
    pub struct Playlist {
        pub id: i64,
        #[content]
        pub name: String,
        pub cover: MediaSource,
        /// Songs and owner, or how often a chart is updated.
        #[content]
        pub detail: String,
        /// The account made it, so it can add songs to it and delete it.
        pub owned: bool,
        /// The account's 我喜欢的音乐: the songs it likes.
        pub liked: bool,
    }

    /// A line of the lyrics: when it is sung, and its translation if any.
    #[value]
    pub struct LyricLine {
        pub key: i64,
        pub time: f64,
        #[content]
        pub text: String,
        #[content]
        pub translation: String,
        pub translated: bool,
        /// It is the line being sung.
        pub current: bool,
        /// How blurred it is: none for the sung line, a little for those
        /// beside it, more towards the edges, none while the lyrics are
        /// browsed.
        pub blur: f64,
    }

    /// An installed plugin.
    #[value]
    pub struct Plugin {
        #[content]
        pub id: String,
        #[content]
        pub name: String,
        #[content]
        pub version: String,
        #[content]
        pub description: String,
        #[content]
        pub author: String,
        pub enabled: bool,
        /// It works on this system.
        pub fits: bool,
        /// The systems it works on, when not all, such as `Windows`.
        #[content]
        pub platforms: String,
    }

    /// A plugin the plugin market lists.
    #[value]
    pub struct Offer {
        #[content]
        pub id: String,
        #[content]
        pub name: String,
        #[content]
        pub version: String,
        #[content]
        pub description: String,
        #[content]
        pub author: String,
        pub installed: bool,
        /// A newer version than the installed one.
        pub newer: bool,
        pub fits: bool,
        #[content]
        pub platforms: String,
    }

    /// A song being downloaded, and how far along.
    #[value]
    pub struct Download {
        pub id: i64,
        #[content]
        pub name: String,
        #[content]
        pub artists: String,
        /// From 0 to 1.
        pub progress: f64,
    }

    /// The list a song is played from.
    #[value]
    pub enum Source {
        #[label(tr! { "zh-Hans" => "每日推荐", "en" => "Daily" })]
        Daily,
        #[label(tr! { "zh-Hans" => "歌单", "en" => "Playlist" })]
        Playlist,
        #[label(tr! { "zh-Hans" => "搜索结果", "en" => "Search" })]
        Search,
        #[label(tr! { "zh-Hans" => "本地音乐", "en" => "Local Music" })]
        Local,
        #[label(tr! { "zh-Hans" => "本地搜索结果", "en" => "Local Results" })]
        LocalResults,
        #[label(tr! { "zh-Hans" => "播放记录", "en" => "History" })]
        History,
        #[label(tr! { "zh-Hans" => "专辑", "en" => "Album" })]
        Album,
        #[label(tr! { "zh-Hans" => "歌手的热门歌曲", "en" => "Artist's Top Songs" })]
        Artist,
    }

    #[value]
    pub enum Login {
        #[label(tr! { "zh-Hans" => "未登录", "en" => "Signed out" })]
        Out,
        #[label(tr! { "zh-Hans" => "正在获取二维码", "en" => "Getting the code" })]
        Loading,
        #[label(tr! { "zh-Hans" => "等待扫码", "en" => "Waiting for the scan" })]
        Waiting,
        #[label(tr! { "zh-Hans" => "已扫码", "en" => "Scanned" })]
        Scanned,
        #[label(tr! { "zh-Hans" => "二维码已失效", "en" => "Code expired" })]
        Expired,
        #[label(tr! { "zh-Hans" => "已登录", "en" => "Signed in" })]
        In,
    }

    #[state]
    pub struct State {
        pub login: Login,
        #[content]
        pub nickname: String,
        pub avatar: MediaSource,
        /// The QR code to scan with the NetEase app while signing in, and who
        /// scanned it.
        pub qr: MediaSource,
        #[content]
        pub scanner: String,
        pub scanner_avatar: MediaSource,
        /// The code can be saved to this phone's photos, for the NetEase
        /// app on it to scan from the album; whether this one was, and why
        /// it was not.
        pub saves_code: bool,
        pub code_saved: bool,
        #[content]
        pub save_error: String,

        /// The account's own and saved playlists.
        pub mine: Vec<Playlist>,
        /// Of them: the songs the account likes, those it made, and those it
        /// saved from others.
        pub liked_list: Option<Playlist>,
        pub created: Vec<Playlist>,
        pub has_created: bool,
        pub collected: Vec<Playlist>,
        pub has_collected: bool,
        /// The playlist the account's page opens straight into; 0 for none.
        pub default_playlist: i64,

        /// What shows over the page.
        pub detail: Detail,
        /// An open album: its songs, what it says about itself, who sang it.
        pub album: Option<Album>,
        pub album_songs: Vec<Song>,
        #[content]
        pub album_description: String,
        #[content]
        pub album_artist: String,
        /// When it came out, how many songs, which company.
        #[content]
        pub album_meta: String,
        pub album_loading: bool,
        /// An open singer: about them, their top songs and their albums.
        pub artist: Option<Artist>,
        pub artist_songs: Vec<Song>,
        pub artist_albums: Vec<Album>,
        #[content]
        pub artist_description: String,
        pub artist_loading: bool,

        /// The playlist open in the main area, its songs and its description.
        pub opened: Option<Playlist>,
        pub opened_songs: Vec<Song>,
        #[content]
        pub opened_description: String,
        pub opened_loading: bool,
        /// Who made the open playlist, and how many songs and plays it has.
        #[content]
        pub opened_owner: String,
        pub opened_owner_avatar: MediaSource,
        #[content]
        pub opened_meta: String,
        pub opened_owned: bool,
        /// The open playlist is the one the account's page opens into.
        pub opened_is_default: bool,
        /// How the songs of a playlist and of the local folders are ordered.
        pub order: SongOrder,
        /// Rows show how many times each song was played.
        pub show_plays: bool,

        /// The local library: its songs, the newest first, how long they
        /// play together, and the folders they are found in.
        pub local: Vec<Song>,
        pub has_local: bool,
        pub local_count: i64,
        pub local_hours: i64,
        pub local_minutes: i64,
        pub folders: Vec<Folder>,
        pub has_folders: bool,
        /// The folders are being read; how many of how many files to read
        /// have been read.
        pub scanning: bool,
        pub scan_done: i64,
        pub scan_total: i64,
        /// Local songs being matched with NetEase's, how many of how many.
        pub auto_matching: bool,
        pub match_done: i64,
        pub match_total: i64,
        /// A local song whose NetEase song is being chosen by hand, and the
        /// songs NetEase found for it.
        pub matching: Option<Song>,
        pub match_candidates: Vec<Song>,
        pub match_loading: bool,
        pub has_match_candidates: bool,
        /// Where a phone looks for music: the app's folder in Files.
        #[content]
        pub phone_folder: String,

        /// What was played, the latest first.
        pub history: Vec<Played>,
        pub has_history: bool,

        /// A song being added to one of the account's playlists, while the
        /// list of them shows; a playlist to delete, while that is asked.
        pub adding: Option<Song>,
        pub deleting: Option<Playlist>,
        /// A new playlist is being named.
        pub naming: bool,

        pub current: Option<Song>,
        /// The playing song's id, 0 without one, to mark its row in a list.
        pub current_id: i64,
        /// The account likes the playing song.
        pub current_liked: bool,
        /// What is played, in order, and the place of the playing song in it.
        pub queue: Vec<Song>,
        pub queue_at: i64,
        pub mode: PlayMode,
        pub playing: bool,
        /// The song is being fetched before it can play.
        pub buffering: bool,
        /// Only a short preview: the song is for members.
        pub preview: bool,
        /// The installed plugins, by name.
        pub plugins: Vec<Plugin>,
        pub has_plugins: bool,
        /// What the plugin market lists, once asked.
        pub offers: Vec<Offer>,
        pub has_offers: bool,
        pub offers_loading: bool,
        /// Each pulse a plugin gives the playing cover adds one.
        pub cover_pulse: i64,
        /// The songs being downloaded, the first started first.
        pub downloads: Vec<Download>,
        pub has_downloads: bool,
        /// The playing song is downloading, and how far it is, from 0 to 1.
        pub current_downloading: bool,
        pub current_download: f64,
        /// How the playing song is encoded, such as `FLAC · 921 kbps · 44.1
        /// kHz · 16 bit`; empty until it is known.
        #[content]
        pub format: String,
        /// The line being sung, its translation and the line after it, for
        /// the cover on a phone; empty without lyrics.
        #[content]
        pub lyric_now: String,
        #[content]
        pub lyric_now_translation: String,
        #[content]
        pub lyric_next: String,
        /// How far the song is, from 0 to 1.
        pub progress: f64,
        #[content]
        pub elapsed: String,
        #[content]
        pub total: String,
        pub volume: f64,
        pub lyrics: Vec<LyricLine>,
        pub has_lyrics: bool,
        /// The key of the line being sung.
        pub line: i64,
        /// The key of the line the lyrics scroll to, the sung one, and a
        /// count that changes each time they should scroll there.
        pub scroll_to: i64,
        pub scroll_tick: i64,
        /// The lyrics are scrolled by hand: no line is blurred and they stay
        /// where they are, until three seconds after the scrolling, or until
        /// the music plays on.
        pub browsing: bool,

        /// Beside the playing cover: lyrics, comments or details.
        pub panel: Panel,
        /// On a phone, the panel shows in place of the cover.
        pub panel_open: bool,
        pub comments: Vec<Comment>,
        pub comments_total: i64,
        pub comments_loading: bool,
        /// More comments can be loaded.
        pub comments_more: bool,
        pub details: Option<Details>,
        pub details_loading: bool,

        // Settings, kept on this computer.
        /// The NetEase Cloud Music module; off, Enji plays local music only.
        pub netease: bool,
        /// Lyrics and covers looked up on NetEase for local songs without them.
        pub online_info: bool,
        pub quality: Quality,
        pub show_translation: bool,
        /// The playing song's cover, blurred, behind the whole window.
        pub backdrop: bool,
        /// The playing song's encoding under its name.
        pub show_format: bool,
        /// On a phone, the sung line under the cover.
        pub cover_lyrics: bool,
        /// The sung line floating over everything else on the screen.
        pub desktop_lyrics: bool,
        /// Clicks go through the desktop lyrics; unlocked, they can be moved.
        pub desktop_lyrics_locked: bool,
        /// This system shows desktop lyrics.
        pub desktop_lyrics_supported: bool,
        /// A phone: its playing page can show the sung line under the
        /// cover, and its desktop lyrics, with no pointer over them, unlock
        /// in Settings.
        pub phone: bool,
        pub appearance: Appearance,

        /// A short word that something went well, such as a welcome.
        #[content]
        pub notice: String,
        pub notice_key: i64,
        pub has_notice: bool,
        #[content]
        pub error: String,
        pub error_key: i64,
        pub has_error: bool,
        /// A folder was not added: it cannot be read here.
        pub folder_refused: bool,
        pub folder_refused_key: i64,
    }

    #[commands]
    pub trait Commands {
        /// Load the account's playlists again.
        fn refresh(&mut self) -> Result<(), CommandError>;
        /// Show a QR code to sign in with the NetEase Cloud Music app.
        fn sign_in(&mut self) -> Result<(), CommandError>;
        fn cancel_sign_in(&mut self) -> Result<(), CommandError>;
        /// Save the sign-in QR code to this phone's photos, for the NetEase
        /// app on it to scan from the album.
        fn save_sign_in_code(&mut self) -> Result<(), CommandError>;
        /// Forget the account on this computer.
        fn sign_out(&mut self) -> Result<(), CommandError>;
        /// Open a playlist in the main area and load its songs.
        fn open_playlist(&mut self, id: i64) -> Result<(), CommandError>;
        /// Open a playlist shown as a card: its name and cover show at once,
        /// while its songs load.
        fn show_playlist(&mut self, list: Playlist) -> Result<(), CommandError>;
        /// Close everything opened over the page.
        fn close_playlist(&mut self) -> Result<(), CommandError>;
        fn open_album(&mut self, id: i64) -> Result<(), CommandError>;
        fn open_artist(&mut self, id: i64) -> Result<(), CommandError>;
        /// Close what was opened last, showing what was under it.
        fn back(&mut self) -> Result<(), CommandError>;
        /// Add a folder's music to the local library.
        fn add_folder(&mut self, folder: MediaSource) -> Result<(), CommandError>;
        fn remove_folder(&mut self, key: i64) -> Result<(), CommandError>;
        /// Read the local folders again.
        fn rescan(&mut self) -> Result<(), CommandError>;
        /// Choose by hand which NetEase song the local song at `index` of a
        /// list is.
        fn begin_matching(&mut self, source: Source, index: i64) -> Result<(), CommandError>;
        fn cancel_matching(&mut self) -> Result<(), CommandError>;
        /// Search NetEase with other words for the song being matched.
        fn search_matching(&mut self, text: String) -> Result<(), CommandError>;
        /// The song being matched is the NetEase song `id`.
        fn choose_match(&mut self, id: i64) -> Result<(), CommandError>;
        /// The song being matched is on NetEase under no song.
        fn unmatch(&mut self) -> Result<(), CommandError>;
        /// Play the song at `index` of a list after the playing one.
        fn play_next(&mut self, source: Source, index: i64) -> Result<(), CommandError>;
        /// Put the song at `index` of a list at the end of the queue.
        fn add_to_queue(&mut self, source: Source, index: i64) -> Result<(), CommandError>;
        fn remove_from_queue(&mut self, index: i64) -> Result<(), CommandError>;
        fn remove_history(&mut self, key: i64) -> Result<(), CommandError>;
        fn clear_history(&mut self) -> Result<(), CommandError>;
        /// Choose one of the account's playlists for the song at `index` of
        /// a list.
        fn choose_playlist(&mut self, source: Source, index: i64) -> Result<(), CommandError>;
        fn cancel_choosing(&mut self) -> Result<(), CommandError>;
        /// Add the song being chosen for to the playlist `id`.
        fn add_to_playlist(&mut self, id: i64) -> Result<(), CommandError>;
        /// Take the song at `index` out of the open playlist.
        fn remove_from_playlist(&mut self, index: i64) -> Result<(), CommandError>;
        /// Ask for a new playlist's name; `create_playlist` makes it.
        fn begin_naming(&mut self) -> Result<(), CommandError>;
        fn cancel_naming(&mut self) -> Result<(), CommandError>;
        fn create_playlist(&mut self, name: String) -> Result<(), CommandError>;
        /// Ask before deleting the playlist `id`; `confirm_delete` deletes it.
        fn ask_delete_playlist(&mut self, id: i64) -> Result<(), CommandError>;
        fn confirm_delete(&mut self) -> Result<(), CommandError>;
        fn cancel_delete(&mut self) -> Result<(), CommandError>;
        /// Open the playlist `id` and play it from its first song.
        fn play_playlist(&mut self, id: i64) -> Result<(), CommandError>;
        fn set_netease(&mut self, on: bool) -> Result<(), CommandError>;
        fn set_online_info(&mut self, on: bool) -> Result<(), CommandError>;
        /// Play the song at `index` of a list, then the rest of the list.
        fn play(&mut self, source: Source, index: i64) -> Result<(), CommandError>;
        /// Pause, or play on.
        fn toggle(&mut self) -> Result<(), CommandError>;
        fn next_song(&mut self) -> Result<(), CommandError>;
        fn previous_song(&mut self) -> Result<(), CommandError>;
        /// Go to a point of the song, as a fraction of its length.
        fn seek(&mut self, progress: f64) -> Result<(), CommandError>;
        /// Go to where a lyric line is sung.
        fn seek_line(&mut self, key: i64) -> Result<(), CommandError>;
        fn set_volume(&mut self, volume: f64) -> Result<(), CommandError>;
        /// No sound, or the volume from before again.
        fn toggle_mute(&mut self) -> Result<(), CommandError>;
        /// Like the playing song, or no longer; it needs an account.
        fn toggle_like(&mut self) -> Result<(), CommandError>;
        /// Play a list from a random song, shuffling.
        fn shuffle(&mut self, source: Source) -> Result<(), CommandError>;
        /// The lyrics are being scrolled by hand, or no longer.
        fn browse_lyrics(&mut self, active: bool) -> Result<(), CommandError>;
        /// Play the song at `index` of the queue.
        fn play_queued(&mut self, index: i64) -> Result<(), CommandError>;
        /// Repeat all, repeat one, shuffle, and round again.
        fn next_mode(&mut self) -> Result<(), CommandError>;
        /// Show `panel` beside the cover; shown already, the lyrics again.
        fn show_panel(&mut self, panel: Panel) -> Result<(), CommandError>;
        /// On a phone: show `panel` in place of the cover; shown already, the
        /// cover again.
        fn toggle_panel(&mut self, panel: Panel) -> Result<(), CommandError>;
        /// The next page of comments.
        fn more_comments(&mut self) -> Result<(), CommandError>;
        fn set_quality(&mut self, quality: Quality) -> Result<(), CommandError>;
        /// Order the songs of the open playlist and of the local folders.
        fn set_order(&mut self, order: SongOrder) -> Result<(), CommandError>;
        fn set_show_plays(&mut self, shown: bool) -> Result<(), CommandError>;
        /// The playlist the account's page opens straight into; 0 for none.
        fn set_default_playlist(&mut self, id: i64) -> Result<(), CommandError>;
        /// Another page was chosen: close what was open over the page; on
        /// the account's page (`library`), open its playlist, if it has one.
        fn page_changed(&mut self, library: bool) -> Result<(), CommandError>;
        fn set_show_translation(&mut self, shown: bool) -> Result<(), CommandError>;
        fn set_backdrop(&mut self, shown: bool) -> Result<(), CommandError>;
        fn set_show_format(&mut self, shown: bool) -> Result<(), CommandError>;
        /// Download the song at `index` of a list from NetEase: the full
        /// song, tagged, with its lyrics beside it.
        fn download(&mut self, source: Source, index: i64) -> Result<(), CommandError>;
        /// Download the playing song.
        fn download_current(&mut self) -> Result<(), CommandError>;
        /// Turn an installed plugin on or off.
        fn set_plugin_enabled(&mut self, id: String, on: bool) -> Result<(), CommandError>;
        fn remove_plugin(&mut self, id: String) -> Result<(), CommandError>;
        /// Ask the plugin market what it lists.
        fn load_offers(&mut self) -> Result<(), CommandError>;
        /// Install, or update, a plugin the market lists.
        fn install_plugin(&mut self, id: String) -> Result<(), CommandError>;
        /// Install a plugin from a `.enj` package.
        fn import_plugin(&mut self, file: MediaSource) -> Result<(), CommandError>;
        fn set_cover_lyrics(&mut self, shown: bool) -> Result<(), CommandError>;
        fn set_desktop_lyrics(&mut self, shown: bool) -> Result<(), CommandError>;
        fn set_desktop_lyrics_locked(&mut self, locked: bool) -> Result<(), CommandError>;
        fn set_appearance(&mut self, appearance: Appearance) -> Result<(), CommandError>;
        fn dismiss_error(&mut self) -> Result<(), CommandError>;
        /// The songs a module lists under `source`, so the commands that
        /// take a song by its place in a list find it: today's songs, what
        /// a search found.
        fn set_songs(&mut self, source: Source, songs: Vec<Song>) -> Result<(), CommandError>;
        /// Show `message` as a failure, as the core shows its own; for the
        /// modules.
        fn show_error(&mut self, message: String) -> Result<(), CommandError>;
    }

    #[events]
    pub trait Events {
        /// An account signed in, or out.
        fn account_changed(&self, signed_in: bool);
        /// NetEase was switched on or off.
        fn netease_changed(&self, on: bool);
    }
}
