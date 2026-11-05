//! The worker: a thread with its own async runtime that talks to NetEase,
//! keeps the play queue and drives the audio thread. It is the only one
//! that writes the shared state; each change wakes the engine, which copies
//! it for the interface.

use crate::{
    audio::{self, Audio, Stream},
    convert, downloads,
    listens,
    local, lyrics, plugins, resume,
    shuffle::Shuffle,
    store, together,
};
use bern::{media::MediaSource, wake::Waker};
use enji_kit::{list, renumbered};
use enji_protocol::enji::{
    Appearance, Detail, Download, Folder, Login, Offer, Plugin, LyricLine, Panel, PlayMode, Played, Playlist, Quality, Song,
    CloseAction, ListenYear, Medal, Profile, SongOrder, Source, State, TogetherUser, TopRange,
};
use ncm_api_rs::{ApiClient, ApiResponse, CryptoType, NcmError, Query, RequestOption, util::config::choose_user_agent};
use rand::Rng;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::Read,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, task::JoinHandle};

pub enum Job {
    Refresh,
    SignIn,
    CancelSignIn,
    /// Save the sign-in code to the phone's photos.
    SaveCode,
    SignOut,
    Open(i64),
    /// Open a playlist shown as a card, its name and cover known.
    Show(Playlist),
    Close,
    Play(Source, usize),
    Toggle,
    Next,
    Previous,
    Seek(f64),
    SeekLine(i64),
    Volume(f64),
    ToggleMute,
    Browse(bool),
    PlayQueued(usize),
    NextMode,
    ShowPanel(Panel),
    TogglePanel(Panel),
    MoreComments,
    SetQuality(Quality),
    SetOrder(SongOrder),
    SetShowPlays(bool),
    SetReportListening(bool),
    SetHeartMode(bool),
    SetTogether(bool),
    SetUsbExclusive(bool),
    SetWordLyrics(bool),
    SetHotSearch(bool),
    PlayFm,
    FmSongs(Vec<Song>),
    FmDislike,
    SetFmOnStart(bool),
    SetTogetherOpen(bool),
    CreateTogether,
    JoinTogether(String),
    LeaveTogether,
    CopyTogetherLink,
    /// The account is in this room: `fresh` when it was just created, for
    /// what plays here to become its list.
    TogetherEntered { info: Value, inviter: i64, fresh: bool },
    /// The account is in no room, or entering one failed with this.
    TogetherNotEntered(Option<String>),
    /// The room's state read, or the code it failed with.
    TogetherRead(Result<Value, i64>),
    /// Who is in the room; `None` when it is over.
    TogetherMembers(Option<Value>),
    TogetherHeartbeat(Value),
    /// The songs of the room's list, with the command that came with them.
    TogetherList { songs: Vec<Song>, command: Option<together::Command>, received: i64 },
    /// A song the room plays that is not in the queue.
    TogetherSong { song: Option<Song>, command: together::Command, received: i64 },
    /// What the heart mode plays next, as NetEase picked it.
    HeartSongs(Vec<Song>),
    /// NetEase took the listening reports on their way, or not.
    Reported(bool),
    SetDefaultPlaylist(i64),
    PageChanged(bool),
    SetTranslation(bool),
    SetBackdrop(bool),
    SetShowFormat(bool),
    SetCoverLyrics(bool),
    SetDesktopLyrics(bool),
    SetDesktopLyricsLocked(bool),
    Download(Source, usize),
    DownloadCurrent,
    DownloadProgress { id: i64, progress: f64 },
    Downloaded { song: Song, path: PathBuf },
    /// The playing song's cover, read for the system's now playing.
    NowArtwork { token: u64, bytes: Arc<Vec<u8>> },
    SetPluginEnabled(String, bool),
    RemovePlugin(String),
    OpenAccount,
    SetAnnualReport(bool),
    SetOnClose(CloseAction),
    /// What NetEase says of the account.
    ProfileRead(Box<AccountRead>),
    SetTopRange(TopRange),
    OpenReport(i64),
    CloseReport,
    /// The address of the latest annual report.
    SetPluginsOpen(bool),
    LoadOffers,
    Offers(Vec<plugins::Listing>),
    InstallPlugin(String),
    ImportPlugin(String),
    /// A plugin package was unpacked, or why it was not.
    PluginInstalled(Result<plugins::Pack, String>),
    DownloadFailed { id: i64, message: String },
    SetAppearance(Appearance),
    ToggleLike,
    Shuffle(Source),
    OpenAlbum(i64),
    OpenArtist(i64),
    Back,
    Liked(Vec<i64>),
    DismissError,
    AddFolder(String),
    BeginMatching(Source, usize),
    CancelMatching,
    SearchMatching(String),
    ChooseMatch(i64),
    Unmatch,
    /// A local file matched with NetEase in the background: `None` when the
    /// search failed, to be tried again; `Some(None)` when nothing matched.
    Matched { url: String, found: Option<Option<Found>> },
    MatchingDone,
    /// A folder that cannot be read here was not added.
    RefuseFolder,
    Naming(bool),
    RemoveFolder(usize),
    Rescan,
    PlayNext(Source, usize),
    AddToQueue(Source, usize),
    RemoveFromQueue(usize),
    RemoveHistory(usize),
    ClearHistory,
    ChoosePlaylist(Source, usize),
    CancelChoosing,
    AddToPlaylist(i64),
    RemoveFromPlaylist(usize),
    CreatePlaylist(String),
    AskDelete(i64),
    ConfirmDelete,
    CancelDelete,
    PlayPlaylist(i64),
    /// The songs a module lists under a source.
    SetSongs(Source, Vec<Song>),
    /// A module's failure, said to the person.
    Fail(String),
    SetNetease(bool),
    SetOnlineInfo(bool),
    // Results of the worker's own tasks.
    /// Files a scan has read so far, and how many of how many.
    ScanRead { tracks: Vec<local::Track>, done: usize, total: usize },
    Scanned(local::Library),
    /// The full picture of the playing local file.
    Artwork { token: u64, path: PathBuf },
    /// The open playlist's songs arrived.
    Opened(i64),
    /// Play counts changed: the lists show them again.
    Counted,
    /// Lyrics and a cover found on NetEase for a local song.
    LocalInfo { token: u64, lines: Vec<lyrics::Line>, cover: String },
    SignedIn(String),
    Account(i64),
    Ready { token: u64, stream: Stream, preview: bool, lines: Vec<lyrics::Line>, format: String },
    /// How the playing local file is encoded, read from its header.
    Format { token: u64, format: String },
    Failed { token: u64, message: String },
}

/// The state at launch, with the settings saved last.
pub fn initial_state() -> State {
    let saved = store::settings();
    let pick = |name: &str| saved[name].as_str().unwrap_or("").to_string();
    let state = State {
        login: Login::Out,
        nickname: String::new(),
        avatar: MediaSource::new(""),
        qr: MediaSource::new(""),
        scanner: String::new(),
        scanner_avatar: MediaSource::new(""),
        // A phone cannot scan a code it shows: the NetEase app on it scans
        // it from the album.
        saves_code: cfg!(any(target_os = "ios", target_os = "android")),
        code_saved: false,
        save_error: String::new(),
        mine: Vec::new(),
        liked_list: None,
        created: Vec::new(),
        has_created: false,
        collected: Vec::new(),
        has_collected: false,
        default_playlist: saved["default_playlist"].as_i64().unwrap_or(0),
        detail: Detail::Page,
        album: None,
        album_songs: Vec::new(),
        album_description: String::new(),
        album_artist: String::new(),
        album_meta: String::new(),
        album_loading: false,
        artist: None,
        artist_songs: Vec::new(),
        artist_albums: Vec::new(),
        artist_description: String::new(),
        artist_loading: false,
        opened: None,
        opened_songs: Vec::new(),
        opened_description: String::new(),
        opened_loading: false,
        opened_owner: String::new(),
        opened_owner_avatar: MediaSource::new(""),
        opened_meta: String::new(),
        opened_is_default: false,
        order: match pick("order").as_str() {
            "Plays" => SongOrder::Plays,
            "Name" => SongOrder::Name,
            "Artist" => SongOrder::Artist,
            "Added" => SongOrder::Added,
            _ => SongOrder::Default,
        },
        show_plays: saved["show_plays"].as_bool().unwrap_or(true),
        opened_owned: false,
        local: Vec::new(),
        has_local: false,
        local_count: 0,
        local_hours: 0,
        local_minutes: 0,
        folders: Vec::new(),
        has_folders: false,
        scanning: false,
        scan_done: 0,
        scan_total: 0,
        auto_matching: false,
        match_done: 0,
        match_total: 0,
        matching: None,
        match_candidates: Vec::new(),
        match_loading: false,
        has_match_candidates: false,
        phone_folder: String::new(),
        history: Vec::new(),
        has_history: false,
        adding: None,
        deleting: None,
        naming: false,
        current: None,
        current_id: 0,
        current_liked: false,
        queue: Vec::new(),
        queue_at: 0,
        mode: match pick("mode").as_str() {
            "RepeatOne" => PlayMode::RepeatOne,
            "Shuffle" => PlayMode::Shuffle,
            _ => PlayMode::RepeatAll,
        },
        playing: false,
        buffering: false,
        preview: false,
        profile: None,
        listen_years: Vec::new(),
        has_listen_years: false,
        top_songs: Vec::new(),
        has_top_songs: false,
        top_range: TopRange::All,
        medals: Vec::new(),
        has_medals: false,
        medal_count: 0,
        annual_report: saved["annual_report"].as_bool().unwrap_or(true),
        // On a Mac closing a window has never quit an application.
        on_close: match pick("on_close").as_str() {
            "Background" => CloseAction::Background,
            "Quit" => CloseAction::Quit,
            _ if cfg!(target_os = "macos") => CloseAction::Background,
            _ => CloseAction::Quit,
        },
        desktop: cfg!(not(any(target_os = "android", target_os = "ios", target_os = "tvos"))),
        report_url: String::new(),
        cookie_file: String::new(),
        cookie_domain: ".music.163.com".into(),
        report_close: ".btn-close".into(),
        report_agent: "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 NeteaseMusic/9.4.30".into(),
        plugins_open: false,
        plugins: Vec::new(),
        has_plugins: false,
        offers: Vec::new(),
        has_offers: false,
        offers_loading: false,
        downloads: Vec::new(),
        has_downloads: false,
        current_downloading: false,
        current_download: 0.0,
        format: String::new(),
        lyric_now: String::new(),
        lyric_now_translation: String::new(),
        lyric_next: String::new(),
        total: "0:00".into(),
        volume: saved["volume"].as_f64().unwrap_or(0.8),
        lyrics: Vec::new(),
        lyrics_near: Vec::new(),
        has_lyrics: false,
        line: 0,
        scroll_to: 0,
        scroll_tick: 0,
        browsing: false,
        panel: Panel::Lyrics,
        panel_open: false,
        comments: Vec::new(),
        comments_total: 0,
        comments_loading: false,
        comments_more: false,
        details: None,
        details_loading: false,
        netease: saved["netease"].as_bool().unwrap_or(true),
        online_info: saved["online_info"].as_bool().unwrap_or(true),
        report_listening: saved["report_listening"].as_bool().unwrap_or(true),
        heart_mode: saved["heart_mode"].as_bool().unwrap_or(false),
        fm: false,
        fm_on_start: saved["fm_on_start"].as_bool().unwrap_or(false),
        fm_opened: 0,
        android: cfg!(target_os = "android"),
        windows: cfg!(target_os = "windows"),
        usb_exclusive: saved["usb_exclusive"].as_bool().unwrap_or(false),
        usb_available: crate::audio::exclusive_available(),
        usb_plugged: false,
        usb_refused: false,
        usb_format: String::new(),
        together: saved["together"].as_bool().unwrap_or(false),
        together_open: false,
        together_room: String::new(),
        together_busy: false,
        together_struggling: false,
        together_users: Vec::new(),
        together_alone: false,
        together_link: String::new(),
        quality: match pick("quality").as_str() {
            "Standard" => Quality::Standard,
            "Higher" => Quality::Higher,
            "Lossless" => Quality::Lossless,
            "Hires" => Quality::Hires,
            _ => Quality::Exhigh,
        },
        show_translation: saved["show_translation"].as_bool().unwrap_or(true),
        backdrop: saved["backdrop"].as_bool().unwrap_or(true),
        show_format: saved["show_format"].as_bool().unwrap_or(true),
        cover_lyrics: saved["cover_lyrics"].as_bool().unwrap_or(true),
        word_lyrics: saved["word_lyrics"].as_bool().unwrap_or(true),
        hot_search: saved["hot_search"].as_bool().unwrap_or(true),
        desktop_lyrics: saved["desktop_lyrics"].as_bool().unwrap_or(false),
        desktop_lyrics_locked: saved["desktop_lyrics_locked"].as_bool().unwrap_or(false),
        desktop_lyrics_supported: bern::overlay::supported(),
        phone: cfg!(any(target_os = "ios", target_os = "android")),
        appearance: match pick("appearance").as_str() {
            "Light" => Appearance::Light,
            "Dark" => Appearance::Dark,
            _ => Appearance::System,
        },
        notice: String::new(),
        notice_key: 0,
        has_notice: false,
        error: String::new(),
        error_key: 0,
        has_error: false,
        folder_refused: false,
        folder_refused_key: 0,
    };
    state
}

/// The settings of `state` as they are kept.
fn save_settings(state: &State) {
    store::save_settings(&serde_json::json!({
        "volume": state.volume,
        "mode": format!("{:?}", state.mode),
        "quality": format!("{:?}", state.quality),
        "show_translation": state.show_translation,
        "backdrop": state.backdrop,
        "show_format": state.show_format,
        "cover_lyrics": state.cover_lyrics,
        "word_lyrics": state.word_lyrics,
        "hot_search": state.hot_search,
        "desktop_lyrics": state.desktop_lyrics,
        "desktop_lyrics_locked": state.desktop_lyrics_locked,
        "plugins": state.plugins.iter().filter(|plugin| plugin.enabled).map(|plugin| plugin.id.clone()).collect::<Vec<_>>(),
        "appearance": format!("{:?}", state.appearance),
        "netease": state.netease,
        "online_info": state.online_info,
        "report_listening": state.report_listening,
        "annual_report": state.annual_report,
        "on_close": format!("{:?}", state.on_close),
        "heart_mode": state.heart_mode,
        "fm_on_start": state.fm_on_start,
        "usb_exclusive": state.usb_exclusive,
        "together": state.together,
        "order": format!("{:?}", state.order),
        "show_plays": state.show_plays,
        "default_playlist": state.default_playlist,
    }));
}

/// How long to wait before each new try at a song's address, all told a
/// few seconds, as long as a refusal has been seen to last.
const SONG_URL_WAITS: [Duration; 9] = [
    Duration::from_millis(200),
    Duration::from_millis(300),
    Duration::from_millis(500),
    Duration::from_millis(700),
    Duration::from_millis(1000),
    Duration::from_millis(1000),
    Duration::from_millis(1500),
    Duration::from_millis(1500),
    Duration::from_millis(2000),
];

/// `call` until it is not refused, waiting `waits` between tries. NetEase
/// can answer a call for a song's address with code -460, "your network
/// looks risky", and give the address on another try; the client reports
/// any such code as an invalid parameter, which these calls, with their
/// fixed parameters, never have.
async fn retried<T, F: Future<Output = Result<T, NcmError>>>(waits: &[Duration], mut call: impl FnMut() -> F) -> Result<T, NcmError> {
    for wait in waits {
        match call().await {
            Err(NcmError::InvalidParam(_)) => tokio::time::sleep(*wait).await,
            answer => return answer,
        }
    }
    call().await
}

/// The answer to where the song `name` plays from, or what to tell the
/// listener: a refusal is tried again, and shown only if it lasts.
pub(crate) async fn song_url(client: &ApiClient, name: &str, id: &str, level: &str) -> Result<ApiResponse, String> {
    let mut query = Query::new().param("id", id).param("level", level);
    // The client's cookies call it a Windows PC, on every system. Beside
    // them its default user agent, an iPhone's, gets half of these calls
    // refused; this one none in thirty.
    query.ua = Some(choose_user_agent("api", "pc").to_string());
    retried(&SONG_URL_WAITS, || client.song_url_v1(&query)).await.map_err(|e| match e {
        NcmError::InvalidParam(_) => format!("网易云暂时取不到《{name}》的音源，请稍后再试"),
        e => format!("取不到《{name}》的音源：{e}"),
    })
}

/// The level `song_url_v1` takes for `quality`.
fn level(quality: Quality) -> &'static str {
    match quality {
        Quality::Standard => "standard",
        Quality::Higher => "higher",
        Quality::Exhigh => "exhigh",
        Quality::Lossless => "lossless",
        Quality::Hires => "hires",
    }
}

/// Writes the shared state and wakes the engine.
#[derive(Clone)]
struct Shared {
    state: Arc<Mutex<State>>,
    waker: Option<Waker>,
    /// The state changed since the engine last took it in.
    changed: Arc<AtomicBool>,
}

impl Shared {
    fn update(&self, change: impl FnOnce(&mut State)) {
        change(&mut self.state.lock().unwrap_or_else(|e| e.into_inner()));
        self.changed.store(true, Ordering::Release);
        if let Some(waker) = &self.waker {
            waker.wake();
        }
    }

    /// Change what moves many times a second. The core's state, with every
    /// list in it, stays out of this: it costs too much to send so often.
    fn live(&self, change: impl FnOnce(&mut enji_kit::live::Live)) {
        enji_kit::live::update(change);
        if let Some(waker) = &self.waker {
            waker.wake();
        }
    }

    fn read<T>(&self, read: impl FnOnce(&State) -> T) -> T {
        read(&self.state.lock().unwrap_or_else(|e| e.into_inner()))
    }

    fn tell(&self, message: impl Into<String>) {
        let message = message.into();
        // One at a time: the newest says what just happened.
        self.update(|s| {
            s.notice = message;
            s.notice_key += 1;
            s.has_notice = true;
            s.has_error = false;
        });
    }

    fn fail(&self, message: impl Into<String>) {
        let message = message.into();
        self.update(|s| {
            s.error = message;
            s.error_key += 1;
            s.has_error = true;
            s.has_notice = false;
        });
    }
}

struct Worker {
    http: reqwest::Client,
    jobs: mpsc::UnboundedSender<Job>,
    shared: Shared,
    audio: Audio,
    uid: i64,
    signing_in: Option<JoinHandle<()>>,
    queue: Vec<Song>,
    at: usize,
    /// Each song started gets a new token; late answers for an older one
    /// are dropped.
    token: u64,
    /// The token of the song last counted as played.
    counted: u64,
    /// Songs that failed one after the other, so a list of grey songs does
    /// not skip forever.
    failures: u32,
    lines: Vec<lyrics::Line>,
    position: f64,
    paused: bool,
    /// The song downloading, stopped when another starts.
    stream: Option<Stream>,
    /// The lyrics are being scrolled by hand right now, and when that last
    /// stopped.
    scrolling: bool,
    scrolled_at: Instant,
    /// The queue's shuffled order, kept as Misuzu Music keeps it.
    shuffle: Shuffle,
    /// The songs put to play next, the latest first.
    play_next: Vec<i64>,
    /// The songs whose comments and details are loaded, and the comment
    /// page reached.
    comments_for: i64,
    comments_page: u32,
    details_for: i64,
    /// The songs the account likes.
    liked: std::collections::HashSet<i64>,
    /// The volume before muting, to come back to.
    unmuted: f64,
    /// The local library as it is kept.
    library: local::Library,
    /// The NetEase songs of the downloaded files.
    downloaded: downloads::Index,
    /// The playing song's cover for the system's now playing, by the token
    /// of the song it is for.
    now_artwork: Option<(u64, Arc<Vec<u8>>)>,
    /// The enabled plugins, running.
    plugins: plugins::Plugins,
    /// The ids of the enabled plugins, kept in the settings.
    enabled_plugins: Vec<String>,
    /// What the plugin market lists.
    listings: Vec<plugins::Listing>,
    /// The overlays plugins put up, to take down with them.
    plugin_overlays: std::collections::HashSet<String>,
    /// How many beats the plugins heard of.
    beats_told: u32,
    /// What was played, the latest first.
    history: Vec<Entry>,
    /// What was listened to, for the account's record on NetEase.
    listens: listens::Listens,
    /// What the account listened to most, of all time and this week.
    top_all: Vec<Song>,
    top_week: Vec<Song>,
    /// Where playback stands, kept for the next launch.
    resume: resume::Resume,
    /// Opened with the song left last time: how far it was, until it plays.
    left_at: Option<f64>,
    /// Where to go once the song that is starting has loaded.
    start_at: Option<f64>,
    /// The playlist the queue was started from; 0 for any other list.
    queue_list: i64,
    /// When the listening reports were last sent.
    reported_at: Instant,
    /// What played before the heart mode: the queue and its playlist.
    before_heart: Option<(Vec<Song>, i64)>,
    /// The heart mode asked NetEase for songs, and has some.
    heart_asking: bool,
    heart_filled: bool,
    fm_asking: bool,
    /// What comes back starts the roaming, rather than going on with it.
    fm_starting: bool,
    /// The next song was asked for before NetEase sent any.
    fm_waiting: bool,
    /// It may start by itself, once, as the app opens.
    fm_on_start_due: bool,
    /// Where a seek is going, and when it was asked.
    seeking: Option<(f64, Instant)>,
    /// The room listened together in.
    room: Option<together::Room>,
    /// Its reports, sent in order by a task of their own.
    room_reports: Option<mpsc::UnboundedSender<together::Report>>,
    /// The room was asked for its state.
    room_reading: bool,
    /// What happens now comes from the room: it is not reported back.
    following: bool,
    /// Where the room's song should be once it has started here.
    room_pending: Option<(u64, together::Command, i64)>,
    /// The song ended and the other side moves the room on.
    room_song_ended: Option<Instant>,
    /// A playlist to play once its songs arrive.
    play_opened: Option<i64>,
    /// The songs the modules list: today's songs, what a search found in
    /// NetEase and in the local music.
    daily: Vec<Song>,
    results: Vec<Song>,
    local_results: Vec<Song>,
    /// What is open over the page, the last on top, each with its id.
    stack: Vec<(Detail, i64)>,
    /// Local files are being matched with NetEase in the background.
    auto_matching: bool,
    /// A scan is reading the folders; another was asked for meanwhile.
    scanning: bool,
    scan_again: bool,
    /// When the library was last shown while a scan reads.
    shown_at: Instant,
    /// The playlists opened before, by the account and the list.
    kept: Arc<Mutex<HashMap<(i64, i64), Kept>>>,
}

/// The NetEase song a local file is.
pub struct Found {
    id: i64,
    album: i64,
    artist: i64,
    cover: String,
}

/// A song in the play history.
struct Entry {
    song: Song,
    /// When it last played, in milliseconds since 1970.
    at: i64,
    plays: i64,
}

/// How many songs the play history keeps.
const HISTORY: usize = 200;
/// How close to a seek's target the player must say it is, and how long it
/// is waited for.
const SEEK_ARRIVED: f64 = 3.0;
const SEEK_WAIT: Duration = Duration::from_secs(20);
/// How long after a song ended the side that follows waits to be moved on.
const ROOM_MOVES_ON: Duration = Duration::from_secs(3);
/// Where NetEase's web player sends what it played, and the page it says
/// it comes from: from anywhere else the log is taken and then ignored.
const LOG_URL: &str = "https://clientlogusf.music.163.com/weapi/feedback/weblog";
const LOG_PAGE: &str = "https://music.163.com";
/// How soon listening reports that could not be sent are tried again.
const REPORT_AGAIN: Duration = Duration::from_secs(300);

/// A playlist as it was when last opened, shown at once when it opens
/// again.
#[derive(Clone)]
struct Kept {
    /// When NetEase says its songs last changed.
    changed: i64,
    info: Value,
    songs: Vec<Song>,
}

fn now_ms() -> i64 {
    chrono::Local::now().timestamp_millis()
}

/// When a song was played, as the history shows it: the time today, the
/// day before as yesterday, the date before that.
fn when(at: i64) -> String {
    use chrono::{Datelike, Local, TimeZone};
    let Some(time) = Local.timestamp_millis_opt(at).single() else { return String::new() };
    let today = Local::now().date_naive();
    let day = time.date_naive();
    if day == today {
        time.format("%H:%M").to_string()
    } else if Some(day) == today.pred_opt() {
        format!("昨天 {}", time.format("%H:%M"))
    } else if day.year() == today.year() {
        time.format("%m-%d").to_string()
    } else {
        time.format("%Y-%m-%d").to_string()
    }
}

pub async fn run(
    mut receive: mpsc::UnboundedReceiver<Job>,
    jobs: mpsc::UnboundedSender<Job>,
    state: Arc<Mutex<State>>,
    changed: Arc<AtomicBool>,
    waker: Option<Waker>,
) {
    let shared = Shared { state, waker, changed };
    let (volume, exclusive) = shared.read(|s| (s.volume as f32, s.usb_exclusive));
    let cookie = store::cookie();
    let mut worker = Worker {
        http: reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)")
            .build()
            .unwrap_or_default(),
        jobs,
        shared,
        audio: Audio::start(volume, exclusive),
        uid: 0,
        signing_in: None,
        queue: Vec::new(),
        at: 0,
        token: 0,
        counted: 0,
        failures: 0,
        lines: Vec::new(),
        position: 0.0,
        paused: false,
        stream: None,
        scrolling: false,
        scrolled_at: Instant::now(),
        shuffle: Shuffle::default(),
        play_next: Vec::new(),
        comments_for: 0,
        comments_page: 0,
        details_for: 0,
        liked: Default::default(),
        unmuted: 0.8,
        library: local::Library::load(),
        downloaded: downloads::Index::load(),
        now_artwork: None,
        plugins: plugins::Plugins::default(),
        enabled_plugins: store::settings()["plugins"]
            .as_array()
            .map(|ids| ids.iter().filter_map(|id| id.as_str().map(String::from)).collect())
            .unwrap_or_default(),
        listings: Vec::new(),
        plugin_overlays: std::collections::HashSet::new(),
        beats_told: 0,
        history: Vec::new(),
        listens: listens::Listens::load(),
        top_all: Vec::new(),
        top_week: Vec::new(),
        resume: resume::Resume::default(),
        left_at: None,
        start_at: None,
        queue_list: 0,
        reported_at: Instant::now(),
        before_heart: None,
        heart_asking: false,
        heart_filled: false,
        fm_asking: false,
        fm_starting: false,
        fm_waiting: false,
        fm_on_start_due: true,
        seeking: None,
        room: None,
        room_reports: None,
        room_reading: false,
        following: false,
        room_pending: None,
        room_song_ended: None,
        play_opened: None,
        daily: Vec::new(),
        results: Vec::new(),
        local_results: Vec::new(),
        stack: Vec::new(),
        auto_matching: false,
        scanning: false,
        scan_again: false,
        shown_at: Instant::now(),
        kept: Default::default(),
    };
    worker.load_history();
    if let Some(folder) = store::phone_music_dir() {
        let _ = std::fs::create_dir_all(&folder);
        worker.library.folders = vec![local::Folder::new(local::file_url(&folder))];
        worker.shared.update(|s| s.phone_folder = folder.to_string_lossy().to_string());
    }
    worker.publish_library();
    worker.open_left();
    worker.start_plugins();
    worker.rescan();
    if cookie.is_some() && worker.shared.read(|s| s.netease) {
        worker.fetch_account(false);
    }
    worker.refresh();
    let mut tick = tokio::time::interval(Duration::from_millis(200));
    // Plugins hear the music's loudness and answer more often than that:
    // often enough for a pulse to land on the beat.
    let mut quick = tokio::time::interval(Duration::from_millis(25));
    let mut words = tokio::time::interval(Duration::from_millis(40));
    loop {
        tokio::select! {
            job = receive.recv() => match job {
                Some(job) => worker.handle(job),
                None => break,
            },
            _ = tick.tick() => worker.tick(),
            _ = quick.tick(), if !worker.plugins.is_empty() => worker.plugin_tick(),
            _ = words.tick(), if worker.lines.iter().any(|line| !line.words.is_empty()) => worker.word_tick(),
        }
    }
}

fn param(value: impl ToString) -> String {
    value.to_string()
}

impl Worker {
    fn handle(&mut self, job: Job) {
        match job {
            Job::Refresh => self.refresh(),
            Job::SignIn => {
                if self.shared.read(|s| s.login != Login::In) {
                    self.show_code();
                }
            }
            Job::CancelSignIn => {
                if let Some(task) = self.signing_in.take() {
                    task.abort();
                }
                self.shared.update(|s| s.login = Login::Out);
            }
            Job::SaveCode => self.save_code(),
            Job::SignOut => {
                store::forget_cookie();
                enji_kit::set_cookie(None);
                self.leave_room();
                self.uid = 0;
                self.listens.forget();
                if self.in_heart_mode() {
                    self.set_mode(PlayMode::RepeatAll);
                }
                self.liked.clear();
                self.stack.retain(|(kind, _)| *kind != Detail::Account);
                self.show_top();
                self.shared.update(|s| {
                    s.login = Login::Out;
                    s.current_liked = false;
                    s.nickname.clear();
                    s.avatar = MediaSource::new("");
                    s.profile = None;
                    s.listen_years.clear();
                    s.has_listen_years = false;
                    s.top_songs.clear();
                    s.has_top_songs = false;
                    s.medals.clear();
                    s.has_medals = false;
                    s.medal_count = 0;
                    s.mine.clear();
                    sort_playlists(s);
                });
            }
            Job::SignedIn(cookie) => {
                if let Err(e) = store::save_cookie(&cookie) {
                    self.shared.fail(format!("无法保存登录状态：{e}"));
                }
                self.signing_in = None;
                enji_kit::set_cookie(Some(cookie));
                // Signing in is not the app opening.
                self.fm_on_start_due = false;
                self.fetch_account(true);
            }
            Job::Account(uid) => {
                self.uid = uid;
                if std::mem::take(&mut self.fm_on_start_due) && self.shared.read(|s| s.fm_on_start && s.netease) {
                    self.shared.update(|s| s.fm_opened += 1);
                    self.start_fm();
                }
                self.refresh();
                self.load_likes();
                self.load_plays();
                self.report_listens();
                self.find_room();
            }
            Job::Counted => self.show_counts(),
            Job::SetReportListening(on) => {
                self.shared.update(|s| {
                    s.report_listening = on;
                    save_settings(s);
                });
                if on {
                    self.report_listens();
                } else {
                    self.listens.forget();
                }
            }
            Job::Reported(taken) => {
                self.listens.sent(taken);
                // More may wait than one request carries.
                if taken {
                    self.report_listens();
                }
            }
            Job::Liked(ids) => {
                self.liked = ids.into_iter().collect();
                let liked = &self.liked;
                self.shared.update(|s| s.current_liked = liked.contains(&s.current_id));
            }
            Job::ToggleLike => self.toggle_like(),
            Job::Shuffle(source) => {
                let list = self.list_of(&source);
                if !list.is_empty() {
                    self.failures = 0;
                    self.shared.update(|s| {
                        s.mode = PlayMode::Shuffle;
                        save_settings(s);
                    });
                    let first = rand::thread_rng().gen_range(0..list.len());
                    self.set_queue(list, first);
                    self.start(first);
                }
            }
            Job::Open(id) => {
                self.push(Detail::Playlist, id);
                self.open(id, None);
            }
            Job::Show(list) => {
                self.push(Detail::Playlist, list.id);
                self.open(list.id, Some(list));
            }
            Job::OpenAlbum(id) => {
                self.push(Detail::Album, id);
                self.load_album(id);
            }
            Job::OpenArtist(id) => {
                self.push(Detail::Artist, id);
                self.load_artist(id);
            }
            Job::Back => {
                self.stack.pop();
                self.show_top();
            }
            Job::Close => {
                self.stack.clear();
                self.shared.update(|s| {
                    s.detail = Detail::Page;
                    s.opened = None;
                    s.album = None;
                    s.artist = None;
                });
            }
            Job::Play(source, index) => {
                let list = self.list_of(&source);
                if index < list.len() {
                    self.failures = 0;
                    self.queue_list = if source == Source::Playlist { self.shared.read(|s| s.opened.as_ref().map_or(0, |list| list.id)) } else { 0 };
                    self.set_queue(list, index);
                    self.start(index);
                    if self.in_heart_mode() {
                        // The heart mode goes on from the new song.
                        self.before_heart = None;
                        self.enter_heart();
                    }
                }
            }
            Job::PlayQueued(index) => {
                if index < self.queue.len() {
                    self.failures = 0;
                    self.start(index);
                }
            }
            // Roaming has no modes: it goes on from song to song.
            Job::NextMode if self.in_fm() => {}
            Job::NextMode => {
                let heart = self.can_heart();
                let mode = self.shared.read(|s| match s.mode {
                    PlayMode::RepeatAll => PlayMode::RepeatOne,
                    PlayMode::RepeatOne => PlayMode::Shuffle,
                    PlayMode::Shuffle if heart => PlayMode::Heart,
                    PlayMode::Shuffle | PlayMode::Heart => PlayMode::RepeatAll,
                });
                self.set_mode(mode);
            }
            Job::SetHeartMode(on) => {
                self.shared.update(|s| {
                    s.heart_mode = on;
                    save_settings(s);
                });
                if !on && self.in_heart_mode() {
                    self.set_mode(PlayMode::RepeatAll);
                }
            }
            Job::HeartSongs(songs) => self.heart_songs(songs),
            Job::PlayFm => self.start_fm(),
            Job::FmSongs(songs) => self.fm_songs(songs),
            Job::FmDislike => self.fm_dislike(),
            Job::SetFmOnStart(on) => self.shared.update(|s| {
                s.fm_on_start = on;
                save_settings(s);
            }),
            Job::SetHotSearch(on) => self.shared.update(|s| {
                s.hot_search = on;
                save_settings(s);
            }),
            Job::SetWordLyrics(on) => {
                self.shared.update(|s| {
                    s.word_lyrics = on;
                    save_settings(s);
                });
                self.shared.live(|live| live.lit = -1.0);
                // The song's own lines as the setting has them now.
                if let Some(song) = self.queue.get(self.at).filter(|song| song.file.is_empty()).cloned() {
                    self.lines.clear();
                    self.lyrics_of(song.id);
                }
            }
            Job::SetUsbExclusive(on) => {
                self.audio.exclusive(on);
                self.shared.update(|s| {
                    s.usb_exclusive = on;
                    save_settings(s);
                });
            }
            Job::SetTogether(on) => {
                self.shared.update(|s| {
                    s.together = on;
                    save_settings(s);
                });
                if on {
                    self.find_room();
                } else {
                    self.leave_room();
                }
            }
            Job::SetTogetherOpen(open) => self.shared.update(|s| s.together_open = open),
            Job::CreateTogether => self.enter_room(None),
            Job::JoinTogether(link) => match together::invitation(&link) {
                Ok(invitation) => self.enter_room(Some(invitation)),
                Err(message) => self.shared.fail(message),
            },
            Job::LeaveTogether => self.leave_room(),
            Job::CopyTogetherLink => {
                let link = self.shared.read(|s| s.together_link.clone());
                match bern::platform::copy_text(&link) {
                    Ok(()) => self.shared.tell("已复制邀请链接"),
                    Err(_) => self.shared.fail("无法复制邀请链接"),
                }
            }
            Job::TogetherEntered { info, inviter, fresh } => self.room_entered(&info, inviter, fresh),
            Job::TogetherNotEntered(message) => {
                self.shared.update(|s| s.together_busy = false);
                if let Some(message) = message {
                    self.shared.fail(message);
                }
            }
            Job::TogetherRead(answer) => self.room_read(answer),
            Job::TogetherMembers(info) => self.room_members(info),
            Job::TogetherHeartbeat(data) => {
                if let Some(room) = &mut self.room {
                    room.heartbeat_answered(&data);
                }
            }
            Job::TogetherList { songs, command, received } => {
                if self.room.is_some() {
                    let at = command.as_ref().and_then(|command| songs.iter().position(|song| song.id == command.playback.song)).unwrap_or(0);
                    self.following = true;
                    self.queue_list = 0;
                    self.set_queue(songs, at);
                    self.following = false;
                    if let Some(command) = command {
                        self.follow_room(command, received);
                    }
                }
            }
            Job::TogetherSong { song, command, received } => {
                if let (Some(song), true) = (song, self.room.is_some()) {
                    self.following = true;
                    self.queue.push(song);
                    self.publish_queue();
                    self.following = false;
                    self.follow_room(command, received);
                }
            }
            Job::Toggle => self.toggle(),
            Job::Next => self.advance(true, false),
            Job::Previous => {
                if self.position > 3.0 || self.queue.len() < 2 {
                    self.audio.seek(0.0);
                    self.show_now_playing(Some(0.0));
                } else {
                    self.advance(false, false);
                }
            }
            Job::ShowPanel(panel) => {
                self.shared.update(|s| s.panel = if s.panel == panel { Panel::Lyrics } else { panel });
                self.load_panel();
            }
            Job::TogglePanel(panel) => {
                self.shared.update(|s| {
                    if s.panel_open && s.panel == panel {
                        s.panel_open = false;
                    } else {
                        s.panel = panel;
                        s.panel_open = true;
                    }
                });
                self.load_panel();
            }
            Job::MoreComments => {
                let song = self.shared.read(|s| s.current.as_ref().map_or(0, |song| song.id));
                if song != 0 && song == self.comments_for {
                    self.comments_page += 1;
                    self.load_comments(song, self.comments_page);
                }
            }
            Job::SetOrder(order) => self.shared.update(|s| {
                arrange(&mut s.opened_songs, &order);
                arrange(&mut s.local, &order);
                s.order = order;
                save_settings(s);
            }),
            Job::SetShowPlays(shown) => self.shared.update(|s| {
                s.show_plays = shown;
                save_settings(s);
            }),
            Job::SetDefaultPlaylist(id) => self.shared.update(|s| {
                s.default_playlist = id;
                s.opened_is_default = id != 0 && s.opened.as_ref().is_some_and(|list| list.id == id);
                save_settings(s);
            }),
            Job::PageChanged(library) => {
                self.handle(Job::Close);
                let (open, signed_in) = self.shared.read(|s| (s.default_playlist, s.login == Login::In));
                if library && open != 0 && signed_in {
                    self.handle(Job::Open(open));
                }
            }
            Job::SetQuality(quality) => self.shared.update(|s| {
                s.quality = quality;
                save_settings(s);
            }),
            Job::SetTranslation(shown) => self.shared.update(|s| {
                s.show_translation = shown;
                save_settings(s);
            }),
            Job::SetBackdrop(shown) => self.shared.update(|s| {
                s.backdrop = shown;
                save_settings(s);
            }),
            Job::SetShowFormat(shown) => self.shared.update(|s| {
                s.show_format = shown;
                save_settings(s);
            }),
            Job::SetCoverLyrics(shown) => self.shared.update(|s| {
                s.cover_lyrics = shown;
                save_settings(s);
            }),
            Job::SetDesktopLyrics(shown) => {
                // Android asks the user first; the lyrics show once allowed.
                if shown && !bern::overlay::allowed() {
                    bern::overlay::ask();
                }
                self.shared.update(|s| {
                    s.desktop_lyrics = shown;
                    save_settings(s);
                });
                self.desktop_lyrics();
            }
            Job::SetDesktopLyricsLocked(locked) => {
                self.shared.update(|s| {
                    s.desktop_lyrics_locked = locked;
                    save_settings(s);
                });
                self.desktop_lyrics();
            }
            Job::Download(source, index) => {
                if let Some(song) = self.list_of(&source).get(index).cloned() {
                    self.download(song);
                }
            }
            Job::DownloadCurrent => {
                if let Some(song) = self.shared.read(|s| s.current.clone()) {
                    self.download(song);
                }
            }
            Job::DownloadProgress { id, progress } => self.shared.update(|s| {
                if let Some(download) = s.downloads.iter_mut().find(|download| download.id == id) {
                    download.progress = progress;
                }
                downloads_changed(s);
            }),
            Job::Downloaded { song, path } => {
                let url = local::file_url(&path);
                self.downloaded.insert(url.clone(), downloads::Origin { id: song.id, album: song.album_id, artist: song.artist_id });
                self.shared.update(|s| {
                    s.downloads.retain(|download| download.id != song.id);
                    downloads_changed(s);
                });
                self.shared.tell(format!("《{}》已下载", song.name));
                // The library reads the downloads folder.
                let covered = self.library.folders.iter().any(|folder| {
                    let folder = folder.url.trim_end_matches('/');
                    url.strip_prefix(folder).is_some_and(|rest| rest.starts_with('/'))
                });
                if !covered {
                    self.library.folders.push(local::Folder::new(local::file_url(&downloads::dir())));
                    self.library.save();
                    self.publish_library();
                }
                self.rescan();
            }
            Job::DownloadFailed { id, message } => {
                self.shared.update(|s| {
                    s.downloads.retain(|download| download.id != id);
                    downloads_changed(s);
                });
                self.shared.fail(message);
            }
            Job::SetAppearance(appearance) => self.shared.update(|s| {
                s.appearance = appearance;
                save_settings(s);
            }),
            Job::Seek(progress) => {
                let length = self.shared.read(|s| s.current.as_ref().map_or(0.0, |song| song.seconds));
                if self.left_at.is_some() {
                    return self.show_left(progress.clamp(0.0, 1.0) * length);
                }
                self.audio.seek(progress.clamp(0.0, 1.0) * length);
                self.show_now_playing(Some(progress.clamp(0.0, 1.0) * length));
                self.moved_to(progress.clamp(0.0, 1.0) * length);
                self.report_to_room(true);
            }
            Job::SeekLine(key) => {
                if let Some(line) = self.lines.get(key as usize) {
                    if self.left_at.is_some() {
                        let time = line.time;
                        self.show_left(time);
                        return self.toggle();
                    }
                    self.audio.seek(line.time);
                    self.show_now_playing(Some(line.time));
                    self.moved_to(line.time);
                    self.report_to_room(true);
                    self.scrolling = false;
                    self.settle(Some(key));
                    if self.paused {
                        self.toggle();
                    }
                }
            }
            Job::Browse(active) => {
                self.scrolling = active;
                if active {
                    self.shared.update(|s| {
                        if !s.browsing {
                            s.browsing = true;
                            dress(&mut s.lyrics, s.line, true);
                        }
                    });
                } else {
                    self.scrolled_at = Instant::now();
                }
            }
            Job::Volume(volume) => self.set_volume(volume),
            Job::ToggleMute => {
                let volume = self.shared.read(|s| s.volume);
                if volume > 0.0 {
                    self.unmuted = volume;
                    self.set_volume(0.0);
                } else {
                    self.set_volume(self.unmuted.max(0.05));
                }
            }
            Job::DismissError => self.shared.update(|s| s.has_error = false),
            Job::AddFolder(url) => {
                if !self.library.folders.iter().any(|folder| folder.url == url) {
                    self.library.folders.push(local::Folder::new(url));
                    self.library.save();
                    self.publish_library();
                    self.rescan();
                }
            }
            Job::RemoveFolder(index) => {
                if index < self.library.folders.len() {
                    let folder = self.library.folders.remove(index);
                    self.library.tracks.retain(|track| track.folder != folder.url);
                    self.library.save();
                    self.publish_library();
                }
            }
            Job::Rescan => self.rescan(),
            Job::RefuseFolder => self.shared.update(|s| {
                s.folder_refused = true;
                s.folder_refused_key += 1;
            }),
            Job::Naming(naming) => self.shared.update(|s| s.naming = naming),
            Job::ScanRead { tracks, done, total } => {
                // Songs come in as they are read and can be played at once.
                let places: HashMap<String, usize> =
                    self.library.tracks.iter().enumerate().map(|(i, track)| (track.url.clone(), i)).collect();
                let folders = &self.library.folders;
                let mut tracks = tracks;
                self.downloaded_origins(&mut tracks);
                for track in tracks.into_iter().filter(|track| folders.iter().any(|folder| track.folder == folder.url)) {
                    match places.get(&track.url) {
                        Some(&i) => self.library.tracks[i] = track,
                        None => self.library.tracks.push(track),
                    }
                }
                self.shared.update(|s| {
                    s.scan_done = done as i64;
                    s.scan_total = total as i64;
                });
                if done == total || self.shown_at.elapsed() >= Duration::from_secs(1) {
                    self.shown_at = Instant::now();
                    self.publish_library();
                }
            }
            Job::Scanned(mut library) => {
                self.downloaded_origins(&mut library.tracks);
                // Folders added or removed while it read stay as they are now.
                let folders = self.library.folders.clone();
                self.library = local::Library {
                    tracks: library.tracks.into_iter().filter(|track| folders.iter().any(|folder| track.folder == folder.url)).collect(),
                    folders,
                };
                self.library.save();
                self.scanning = false;
                self.shared.update(|s| {
                    s.scanning = false;
                    s.scan_done = 0;
                    s.scan_total = 0;
                });
                self.publish_library();
                if std::mem::take(&mut self.scan_again) {
                    self.rescan();
                } else {
                    let library = self.library.clone();
                    tokio::task::spawn_blocking(move || local::tidy_covers(&library));
                    self.match_library();
                }
            }
            Job::Artwork { token, path } => {
                if token == self.token {
                    let artwork = MediaSource::new(local::file_url(&path));
                    self.shared.update(|s| {
                        if let Some(current) = s.current.as_mut() {
                            current.artwork = artwork;
                        }
                    });
                    if let Ok(bytes) = std::fs::read(&path) {
                        self.now_artwork = Some((token, Arc::new(bytes)));
                        self.show_now_playing(None);
                    }
                }
            }
            Job::NowArtwork { token, bytes } => {
                if token == self.token {
                    self.now_artwork = Some((token, bytes));
                    self.show_now_playing(None);
                }
            }
            Job::SetPluginEnabled(id, on) => {
                if let Err(message) = self.plugins.set_running(&id, on) {
                    self.shared.fail(message);
                    return self.publish_plugins();
                }
                self.enabled_plugins.retain(|enabled| *enabled != id);
                if on {
                    self.enabled_plugins.push(id.clone());
                    self.greet_plugins();
                } else {
                    self.hide_plugin_overlays(&id);
                }
                self.publish_plugins();
                self.shared.update(|s| save_settings(s));
            }
            Job::RemovePlugin(id) => {
                let _ = self.plugins.set_running(&id, false);
                self.hide_plugin_overlays(&id);
                self.enabled_plugins.retain(|enabled| *enabled != id);
                plugins::remove(&id);
                self.publish_plugins();
                self.shared.update(|s| save_settings(s));
            }
            Job::OpenAccount => {
                if self.uid <= 0 {
                    return self.shared.fail("登录后才能看账号");
                }
                self.push(Detail::Account, self.uid);
                self.load_profile();
            }
            Job::ProfileRead(read) => {
                let read = *read;
                self.top_all = read.top_all;
                self.top_week = read.top_week;
                self.shared.update(|s| {
                    if read.profile.is_some() {
                        s.profile = read.profile;
                    }
                    s.has_listen_years = !read.years.is_empty();
                    s.listen_years = read.years;
                    s.has_medals = !read.medals.is_empty();
                    s.medals = read.medals;
                    s.medal_count = read.medal_count;
                });
                self.show_top_songs();
            }
            Job::SetTopRange(range) => {
                self.shared.update(|s| s.top_range = range);
                self.show_top_songs();
            }
            Job::SetOnClose(action) => self.shared.update(|s| {
                s.on_close = action;
                save_settings(s);
            }),
            Job::SetAnnualReport(on) => self.shared.update(|s| {
                s.annual_report = on;
                save_settings(s);
            }),
            Job::OpenReport(year) => {
                if self.uid <= 0 {
                    return self.shared.fail("登录后才能看年度听歌报告");
                }
                let file = enji_kit::store::cookie_path().to_string_lossy().to_string();
                self.shared.update(|s| {
                    s.report_url = report_address(year);
                    s.cookie_file = file;
                });
            }
            Job::CloseReport => self.shared.update(|s| s.report_url.clear()),
            Job::SetPluginsOpen(open) => {
                self.shared.update(|s| s.plugins_open = open);
                if open {
                    self.handle(Job::LoadOffers);
                }
            }
            Job::LoadOffers => {
                self.shared.update(|s| s.offers_loading = true);
                let (http, jobs) = (self.http.clone(), self.jobs.clone());
                tokio::spawn(async move {
                    let mut found = Vec::new();
                    for index in plugins::indexes() {
                        let text = if index.starts_with("http") {
                            match http.get(&index).send().await.and_then(|r| r.error_for_status()) {
                                Ok(response) => response.text().await.unwrap_or_default(),
                                Err(_) => String::new(),
                            }
                        } else {
                            std::fs::read_to_string(&index).unwrap_or_default()
                        };
                        found.extend(plugins::listings(&index, &text));
                    }
                    let _ = jobs.send(Job::Offers(found));
                });
            }
            Job::Offers(listings) => {
                if listings.is_empty() {
                    self.shared.fail("连不上插件商店，稍后再试");
                }
                self.listings = listings;
                self.shared.update(|s| s.offers_loading = false);
                self.publish_plugins();
            }
            Job::InstallPlugin(id) => {
                let Some(listing) = self.listings.iter().find(|listing| listing.pack.id == id).cloned() else { return };
                let (http, jobs) = (self.http.clone(), self.jobs.clone());
                tokio::spawn(async move {
                    let package = if listing.package.starts_with("http") {
                        match http.get(&listing.package).send().await.and_then(|r| r.error_for_status()) {
                            Ok(response) => response.bytes().await.map(|bytes| bytes.to_vec()).map_err(|e| e.to_string()),
                            Err(e) => Err(e.to_string()),
                        }
                    } else {
                        std::fs::read(&listing.package).map_err(|e| e.to_string())
                    };
                    let result = package
                        .map_err(|e| format!("下载插件「{}」失败：{e}", listing.pack.name))
                        .and_then(|bytes| plugins::unpack(&bytes));
                    let _ = jobs.send(Job::PluginInstalled(result));
                });
            }
            Job::ImportPlugin(url) => {
                let jobs = self.jobs.clone();
                tokio::task::spawn_blocking(move || {
                    let result = bern::files::open(&url)
                        .map_err(|e| format!("打不开插件包：{}", e.message))
                        .and_then(|mut file| {
                            let mut bytes = Vec::new();
                            file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
                            Ok(bytes)
                        })
                        .and_then(|bytes| plugins::unpack(&bytes));
                    let _ = jobs.send(Job::PluginInstalled(result));
                });
            }
            Job::PluginInstalled(result) => match result {
                Ok(pack) => {
                    // A running plugin starts again in its new version.
                    if self.enabled_plugins.contains(&pack.id) {
                        let _ = self.plugins.set_running(&pack.id, false);
                        self.hide_plugin_overlays(&pack.id);
                        if let Err(message) = self.plugins.set_running(&pack.id, true) {
                            self.shared.fail(message);
                        }
                        self.greet_plugins();
                    }
                    self.shared.tell(format!("已安装插件「{}」{}", pack.name, pack.version));
                    self.publish_plugins();
                }
                Err(message) => self.shared.fail(message),
            },
            Job::BeginMatching(source, index) => self.begin_matching(source, index),
            Job::CancelMatching => self.shared.update(|s| s.matching = None),
            Job::SearchMatching(text) => {
                if !text.trim().is_empty() && self.shared.read(|s| s.matching.is_some()) {
                    self.find_candidates(text.trim().to_string(), String::new());
                }
            }
            Job::ChooseMatch(id) => {
                let chosen = self.shared.read(|s| s.match_candidates.iter().find(|song| song.id == id).cloned());
                if let Some(chosen) = chosen {
                    let found = Found {
                        id: chosen.id,
                        album: chosen.album_id,
                        artist: chosen.artist_id,
                        cover: chosen.cover.url.split('?').next().unwrap_or("").to_string(),
                    };
                    self.set_match(Some(found), "manual");
                }
            }
            Job::Unmatch => self.set_match(None, "manual"),
            Job::Matched { url, found } => {
                if let Some(found) = found
                    && let Some(track) = self.library.tracks.iter_mut().find(|track| track.url == url)
                {
                    apply_match(track, found, "auto");
                }
                let done = self.shared.read(|s| s.match_done) + 1;
                self.shared.update(|s| s.match_done = done);
                if done % 10 == 0 {
                    self.library.save();
                    self.publish_library();
                }
            }
            Job::MatchingDone => {
                self.auto_matching = false;
                self.shared.update(|s| s.auto_matching = false);
                self.library.save();
                self.publish_library();
            }
            Job::PlayNext(source, index) => {
                if let Some(song) = self.list_of(&source).get(index).cloned() {
                    let was_empty = self.queue.is_empty();
                    let at = if was_empty { 0 } else { self.at + 1 };
                    self.mark_play_next(song.id);
                    self.queue.insert(at, song);
                    if was_empty {
                        self.at = 0;
                    }
                    if self.shuffling() {
                        self.shuffle.generate(self.queue.len(), self.at);
                    }
                    self.publish_queue();
                    if was_empty {
                        self.start(0);
                    } else {
                        self.shared.tell("将在下一首播放");
                    }
                }
            }
            Job::AddToQueue(source, index) => {
                if let Some(song) = self.list_of(&source).get(index).cloned() {
                    let was_empty = self.queue.is_empty();
                    self.queue.push(song);
                    if was_empty {
                        self.at = 0;
                    }
                    if self.shuffling() {
                        self.shuffle.generate(self.queue.len(), self.at);
                    }
                    self.publish_queue();
                    if was_empty {
                        self.start(0);
                    } else {
                        self.shared.tell("已加入播放队列");
                    }
                }
            }
            Job::RemoveFromQueue(index) => {
                if index < self.queue.len() && index != self.at {
                    let removed = self.queue.remove(index);
                    self.play_next.retain(|&id| id != removed.id);
                    if index < self.at {
                        self.at -= 1;
                    }
                    // The order no longer fits the queue: publishing it
                    // shuffles anew, as Misuzu Music does.
                    self.publish_queue();
                }
            }
            Job::RemoveHistory(index) => {
                let id = self.shared.read(|s| s.history.get(index).map(|played| played.song.id));
                if let Some(id) = id {
                    self.history.retain(|entry| entry.song.id != id);
                    self.save_history();
                }
            }
            Job::ClearHistory => {
                let netease = self.shared.read(|s| s.netease);
                // With NetEase off only the local songs show, and only they go.
                self.history.retain(|entry| !netease && entry.song.file.is_empty());
                self.save_history();
            }
            Job::ChoosePlaylist(source, index) => {
                let Some(song) = self.list_of(&source).get(index).cloned() else { return };
                if song.id <= 0 {
                    return self.shared.fail("本地歌曲还不能加入网易云歌单");
                }
                if self.uid == 0 {
                    return self.shared.fail("登录后才能把歌曲加入歌单");
                }
                self.shared.update(|s| s.adding = Some(song));
            }
            Job::CancelChoosing => self.shared.update(|s| s.adding = None),
            Job::AddToPlaylist(id) => self.add_to_playlist(id),
            Job::RemoveFromPlaylist(index) => self.remove_from_playlist(index),
            Job::CreatePlaylist(name) => self.create_playlist(name),
            Job::AskDelete(id) => self.shared.update(|s| {
                s.deleting = s.mine.iter().find(|list| list.id == id && list.owned).cloned();
            }),
            Job::CancelDelete => self.shared.update(|s| s.deleting = None),
            Job::ConfirmDelete => self.delete_playlist(),
            Job::PlayPlaylist(id) => {
                self.play_opened = Some(id);
                self.open(id, None);
            }
            Job::Fail(message) => self.shared.fail(message),
            Job::SetSongs(source, songs) => match source {
                Source::Daily => self.daily = songs,
                Source::Search => self.results = songs,
                Source::LocalResults => self.local_results = songs,
                // The core's own lists are its own.
                _ => {}
            },
            Job::Opened(id) => {
                // A list just opened is likely played from its top.
                let top = self.shared.read(|s| s.opened_songs.first().cloned());
                crate::covers::prefetch(&self.http, &top.into_iter().collect::<Vec<_>>());
                if self.play_opened == Some(id) {
                    self.play_opened = None;
                    let _ = self.jobs.send(Job::Play(Source::Playlist, 0));
                }
            }
            Job::SetNetease(on) => {
                self.shared.update(|s| {
                    s.netease = on;
                    if !on {
                        s.opened = None;
                    }
                    save_settings(s);
                });
                self.publish_history();
                if on {
                    if store::cookie().is_some() && self.uid == 0 {
                        self.fetch_account(false);
                    }
                    self.refresh();
                    self.match_library();
                }
            }
            Job::SetOnlineInfo(on) => {
                self.shared.update(|s| {
                    s.online_info = on;
                    save_settings(s);
                });
                self.match_library();
            }
            Job::LocalInfo { token, lines, cover } => {
                if token != self.token {
                    return;
                }
                if !lines.is_empty() && self.lines.is_empty() {
                    let shown = shown_lines(&lines);
                    self.lines = lines;
                    self.shared.update(|s| {
                        s.has_lyrics = true;
                        s.lyrics = shown;
                        s.line = 0;
                        dress(&mut s.lyrics, 0, s.browsing);
                        sung(s);
                        s.scroll_to = 0;
                        s.scroll_tick += 1;
                    });
                    self.desktop_lyrics();
                }
                if !cover.is_empty() {
                    if let Some(song) = self.queue.get(self.at).cloned() {
                        if let Some(track) = self.library.tracks.iter_mut().find(|track| track.url == song.file) {
                            track.online_cover = cover.clone();
                            self.library.save();
                        }
                        self.shared.update(|s| {
                            if let Some(current) = s.current.as_mut() {
                                current.cover = convert::cover(&cover, 200);
                                current.artwork = convert::cover(&cover, 800);
                            }
                        });
                        self.publish_library();
                    }
                }
            }
            Job::Ready { token, stream, preview, lines, format } => {
                if token != self.token {
                    stream.cancel();
                    return;
                }
                self.failures = 0;
                self.paused = false;
                self.stream = Some(stream.clone());
                self.audio.play(token, stream);
                let from = self.start_at.take().unwrap_or(0.0);
                if from > 0.0 {
                    self.audio.seek(from);
                    self.moved_to(from);
                }
                if let Some(song) = self.queue.get(self.at).cloned() {
                    self.record(song);
                }
                let shown = shown_lines(&lines);
                self.lines = lines;
                self.shared.update(|s| {
                    s.buffering = false;
                    s.playing = true;
                    s.preview = preview;
                    if !format.is_empty() {
                        s.format = format;
                    }
                    s.has_lyrics = !shown.is_empty();
                    s.lyrics = shown;
                    s.line = 0;
                    s.browsing = false;
                    dress(&mut s.lyrics, 0, false);
                    sung(s);
                    s.scroll_to = 0;
                    s.scroll_tick += 1;
                });
                self.show_now_playing(Some(from));
                self.desktop_lyrics();
            }
            Job::Format { token, format } => {
                if token == self.token {
                    self.shared.update(|s| s.format = format);
                }
            }
            Job::Failed { token, message } => {
                if token != self.token {
                    return;
                }
                self.shared.update(|s| {
                    s.buffering = false;
                    s.playing = false;
                });
                self.show_now_playing(None);
                self.shared.fail(message);
                self.failures += 1;
                if self.failures < 5 && self.queue.len() > 1 {
                    self.advance(true, false);
                }
            }
        }
    }

    fn set_volume(&mut self, volume: f64) {
        let volume = volume.clamp(0.0, 1.0);
        if volume > 0.0 {
            self.unmuted = volume;
        }
        self.audio.volume(volume as f32);
        self.shared.update(|s| {
            s.volume = volume;
            save_settings(s);
        });
    }

    // MARK: Local music, history and the queue

    /// The songs of a list a song can be played from.
    fn list_of(&self, source: &Source) -> Vec<Song> {
        self.shared.read(|s| match source {
            Source::Daily => self.daily.clone(),
            Source::Playlist => s.opened_songs.clone(),
            Source::Search => self.results.clone(),
            Source::Local => s.local.clone(),
            Source::LocalResults => self.local_results.clone(),
            Source::History => s.history.iter().map(|played| played.song.clone()).collect(),
            Source::Album => s.album_songs.clone(),
            Source::Top => s.top_songs.clone(),
            Source::Artist => s.artist_songs.clone(),
        })
    }

    /// A new queue, `at` playing; Misuzu Music's `setQueue`.
    fn set_queue(&mut self, songs: Vec<Song>, at: usize) {
        self.shared.update(|s| s.fm = false);
        self.fm_waiting = false;
        self.queue = songs;
        self.play_next.clear();
        self.shuffle.clear();
        self.at = at.min(self.queue.len().saturating_sub(1));
        if self.shuffling() && self.queue.len() > 1 {
            self.shuffle.generate(self.queue.len(), self.at);
        }
        self.publish_queue();
    }

    fn shuffling(&self) -> bool {
        self.shared.read(|s| s.mode == PlayMode::Shuffle)
    }

    fn mark_play_next(&mut self, id: i64) {
        self.play_next.retain(|&one| one != id);
        self.play_next.insert(0, id);
    }

    /// Where the latest song put to play next is in the queue, if it is
    /// still there and not playing.
    fn take_play_next(&mut self) -> Option<usize> {
        while !self.play_next.is_empty() {
            let id = self.play_next.remove(0);
            if let Some(index) = self.queue.iter().position(|song| song.id == id)
                && index != self.at
            {
                return Some(index);
            }
        }
        None
    }

    /// The queue as it plays from the playing song on, round to the songs
    /// before it: in shuffled order while shuffling. Each song keeps its
    /// place in the queue as `index`.
    fn publish_queue(&mut self) {
        self.queue = renumbered(std::mem::take(&mut self.queue));
        let (mut places, playing) = if self.shuffling() && self.queue.len() > 1 {
            self.shuffle.sync(self.queue.len(), self.at);
            (self.shuffle.order.clone(), self.shuffle.position)
        } else {
            ((0..self.queue.len()).collect::<Vec<_>>(), self.at)
        };
        let first = playing.min(places.len());
        places.rotate_left(first);
        // The songs likely to play soon get their covers fetched ahead: one
        // put to play next, the next two in play order, the one before,
        // and the playing one, for when it plays again.
        let soon: Vec<Song> = self
            .play_next
            .first()
            .and_then(|id| self.queue.iter().find(|song| song.id == *id))
            .into_iter()
            .chain(places.iter().skip(1).take(2).map(|&i| &self.queue[i]))
            .chain(places.last().filter(|_| places.len() > 3).map(|&i| &self.queue[i]))
            .chain(places.first().map(|&i| &self.queue[i]))
            .cloned()
            .collect();
        crate::covers::prefetch(&self.http, &soon);
        let shown = places.iter().map(|&i| self.queue[i].clone()).collect();
        let at = self.at as i64;
        self.shared.update(|s| {
            s.queue = shown;
            s.queue_at = at;
        });
        self.report_to_room(false);
    }

    /// Read the local folders again, away from the worker; while a scan
    /// reads, the next one waits for it.
    fn rescan(&mut self) {
        if self.library.folders.is_empty() {
            return;
        }
        if self.scanning {
            self.scan_again = true;
            return;
        }
        self.scanning = true;
        let library = self.library.clone();
        let jobs = self.jobs.clone();
        self.shared.update(|s| s.scanning = true);
        tokio::task::spawn_blocking(move || {
            let report = |tracks, done, total| {
                let _ = jobs.send(Job::ScanRead { tracks, done, total });
            };
            let library = library.scan(now_ms(), report);
            let _ = jobs.send(Job::Scanned(library));
        });
    }

    /// The playing song as the system shows it, at `position` seconds or
    /// where it plays now; nothing when no song is on.
    fn show_now_playing(&self, position: Option<f64>) {
        let current = self.shared.read(|s| s.current.clone().map(|song| (song, s.playing)));
        let Some((song, playing)) = current else {
            return bern::now_playing::update(None);
        };
        let artwork = self.now_artwork.as_ref().filter(|(token, _)| *token == self.token).map(|(_, bytes)| bytes.clone());
        bern::now_playing::update(Some(&bern::now_playing::NowPlaying {
            title: song.name,
            artist: song.artists,
            album: song.album,
            artwork,
            duration: song.seconds,
            position: position.unwrap_or(self.position),
            playing,
        }));
    }

    /// The sung line and the next (or its translation) floating over the
    /// screen while desktop lyrics are on; the song's name before its lyrics
    /// come; nothing when they are off or no song is on.
    fn desktop_lyrics(&mut self) {
        let line = self.shared.read(|s| {
            json!({ "text": s.lyric_now, "translation": s.lyric_now_translation, "next": s.lyric_next, "index": s.line })
        });
        self.send_plugins("lyric", line);
        self.show_desktop_lyrics();
    }

    /// The desktop lyrics as they are now, with their bar: playback, the
    /// lock and closing; locked, only the lock.
    fn show_desktop_lyrics(&self) {
        use bern::overlay::{Button, Line, Overlay};
        let shown = self.shared.read(|s| {
            let song = s.current.as_ref().filter(|_| s.desktop_lyrics)?;
            let (first, second) = if s.lyric_now.is_empty() {
                (song.name.clone(), song.artists.clone())
            } else if s.show_translation && !s.lyric_now_translation.is_empty() {
                (s.lyric_now.clone(), s.lyric_now_translation.clone())
            } else {
                (s.lyric_now.clone(), s.lyric_next.clone())
            };
            let lit = Some(enji_kit::live::read(|live| live.lit)).filter(|lit| *lit >= 0.0 && !s.lyric_now.is_empty());
            Some((first, second, lit, s.desktop_lyrics_locked, s.playing))
        });
        let Some((first, second, lit, locked, playing)) = shown else {
            return bern::overlay::hide("lyrics");
        };
        // Phones show it smaller, over another app.
        let (large, small) = if cfg!(target_os = "android") { (20.0, 14.0) } else { (30.0, 18.0) };
        let button = |id: &str, symbol: &str| Button { id: id.into(), symbol: symbol.into(), while_locked: false };
        bern::overlay::show(
            "lyrics",
            &Overlay {
                lines: vec![
                    Line { text: first, size: large, bold: true, color: [255, 255, 255, 255], lit },
                    Line { text: second, size: small, bold: false, color: [255, 255, 255, 215], lit: None },
                ],
                place: bern::overlay::Place::Free,
                locked,
                buttons: vec![
                    button("previous", "backward.fill"),
                    button("toggle", if playing { "pause.fill" } else { "play.fill" }),
                    button("next", "forward.fill"),
                    Button { id: "lock".into(), symbol: if locked { "lock.open.fill" } else { "lock.fill" }.into(), while_locked: true },
                    button("close", "xmark"),
                ],
            },
        );
    }

    /// Start the enabled plugins, show them, and tell them what plays.
    fn start_plugins(&mut self) {
        let (plugins, errors) = plugins::Plugins::start(&self.enabled_plugins);
        self.plugins = plugins;
        for message in errors {
            self.shared.fail(message);
        }
        self.publish_plugins();
        self.greet_plugins();
    }

    /// The installed plugins, and the market's with what is installed and
    /// newer.
    fn publish_plugins(&mut self) {
        let installed = plugins::installed();
        let enabled = &self.enabled_plugins;
        let shown: Vec<Plugin> = installed
            .iter()
            .map(|(pack, _)| Plugin {
                id: pack.id.clone(),
                name: pack.name.clone(),
                version: pack.version.clone(),
                description: pack.description.clone(),
                author: pack.author.clone(),
                enabled: enabled.contains(&pack.id) && pack.fits_here(),
                fits: pack.fits_here(),
                platforms: pack.platforms_label(),
            })
            .collect();
        let offers: Vec<Offer> = self
            .listings
            .iter()
            .map(|listing| {
                let have = installed.iter().find(|(pack, _)| pack.id == listing.pack.id);
                Offer {
                    id: listing.pack.id.clone(),
                    name: listing.pack.name.clone(),
                    version: listing.pack.version.clone(),
                    description: listing.pack.description.clone(),
                    author: listing.pack.author.clone(),
                    installed: have.is_some(),
                    newer: have.is_some_and(|(pack, _)| plugins::is_newer(&listing.pack.version, &pack.version)),
                    fits: listing.pack.fits_here(),
                    platforms: listing.pack.platforms_label(),
                }
            })
            .collect();
        self.shared.update(|s| {
            s.has_plugins = !shown.is_empty();
            s.plugins = shown;
            s.has_offers = !offers.is_empty();
            s.offers = offers;
        });
    }

    /// What the plugins may read with each event.
    fn plugin_snapshot(&self) -> Value {
        self.shared.read(|s| {
            json!({
                "playing": s.playing,
                "song": s.current.as_ref().map(|song| json!({
                    "id": song.id, "name": song.name, "artists": song.artists, "album": song.album, "duration": song.seconds,
                })),
            })
        })
    }

    fn send_plugins(&mut self, event: &str, data: Value) {
        if self.plugins.is_empty() {
            return;
        }
        let snapshot = self.plugin_snapshot();
        self.plugins.send(event, &data, &snapshot);
    }

    /// The song that plays and its sung line, for plugins that just
    /// started or a song that just began.
    fn greet_plugins(&mut self) {
        if self.plugins.is_empty() {
            return;
        }
        let song = self.plugin_snapshot()["song"].clone();
        if !song.is_null() {
            self.send_plugins("song", song);
        }
        let line = self.shared.read(|s| {
            json!({ "text": s.lyric_now, "translation": s.lyric_now_translation, "next": s.lyric_next, "index": s.line })
        });
        self.send_plugins("lyric", line);
    }

    /// The music's loudness for the plugins that hear it, and what the
    /// plugins asked for.
    fn plugin_tick(&mut self) {
        let playing = self.shared.read(|s| s.playing);
        let hearing = self.plugins.wants_audio();
        audio::follow_beat(hearing);
        if playing && hearing {
            let (level, bass) = audio::levels();
            let position = self.position;
            // Only a beat told since the last time; two would not be seen apart.
            let (told, beat) = audio::beat();
            let strength = if told != self.beats_told { beat.strength } else { 0.0 };
            self.beats_told = told;
            self.send_plugins("audio", json!({ "level": level, "bass": bass, "beat": strength, "bpm": beat.bpm, "position": position }));
        }
        for (plugin, call) in self.plugins.calls() {
            match call {
                plugins::Call::OverlayShow(id, spec) => {
                    let id = plugins::overlay_id(&plugin, &id);
                    bern::overlay::show(&id, &plugins::overlay(&spec));
                    self.plugin_overlays.insert(id);
                }
                plugins::Call::OverlayHide(id) => {
                    let id = plugins::overlay_id(&plugin, &id);
                    bern::overlay::hide(&id);
                    self.plugin_overlays.remove(&id);
                }
                plugins::Call::CoverPulse(strength) => self.shared.live(|live| {
                    if strength < 0.34 {
                        live.pulse_soft += 1;
                    } else if strength < 0.67 {
                        live.pulse += 1;
                    } else {
                        live.pulse_strong += 1;
                    }
                }),
                plugins::Call::Toast(message) => self.shared.tell(message),
                plugins::Call::Log(message) => eprintln!("plugin {plugin}: {message}"),
                plugins::Call::StorageSet(..) => {}
            }
        }
    }

    /// Take down the overlays a plugin put up.
    fn hide_plugin_overlays(&mut self, plugin: &str) {
        let prefix = plugins::overlay_id(plugin, "");
        let mine: Vec<String> = self.plugin_overlays.iter().filter(|id| id.starts_with(&prefix)).cloned().collect();
        for id in mine {
            bern::overlay::hide(&id);
            self.plugin_overlays.remove(&id);
        }
    }

    /// Read or download `song`'s cover for the system's now playing.
    fn fetch_now_artwork(&mut self, song: &Song) {
        self.now_artwork = None;
        let (url, http, jobs, token) = (song.artwork.url.clone(), self.http.clone(), self.jobs.clone(), self.token);
        if url.starts_with("file:") {
            if let Some(path) = bern::files::file_path(&url) {
                tokio::task::spawn_blocking(move || {
                    if let Ok(bytes) = std::fs::read(path) {
                        let _ = jobs.send(Job::NowArtwork { token, bytes: Arc::new(bytes) });
                    }
                });
            }
        } else if url.starts_with("http") {
            tokio::spawn(async move {
                if let Ok(response) = http.get(&url).send().await.and_then(|r| r.error_for_status())
                    && let Ok(bytes) = response.bytes().await
                {
                    let _ = jobs.send(Job::NowArtwork { token, bytes: Arc::new(bytes.to_vec()) });
                }
            });
        }
    }

    /// The downloaded files of `tracks` known as their NetEase songs, without
    /// searching.
    fn downloaded_origins(&self, tracks: &mut [local::Track]) {
        for track in tracks.iter_mut().filter(|track| track.netease_id <= 0) {
            if let Some(origin) = self.downloaded.get(&track.url) {
                track.netease_id = origin.id;
                track.netease_album = origin.album;
                track.netease_artist = origin.artist;
                track.matched = "manual".into();
            }
        }
    }

    /// Download `song` from NetEase, unless it is not a NetEase song, a copy
    /// is here already, or it is downloading.
    fn download(&mut self, song: Song) {
        if song.id <= 0 {
            return self.shared.fail(format!("《{}》是本地歌曲，不用下载", song.name));
        }
        if !song.file.is_empty() {
            return self.shared.tell(format!("《{}》已在本地", song.name));
        }
        let (netease, quality, busy) =
            self.shared.read(|s| (s.netease, s.quality.clone(), s.downloads.iter().any(|download| download.id == song.id)));
        if !netease {
            return self.shared.fail("网易云音乐已在设置中关闭");
        }
        if busy {
            return self.shared.tell(format!("《{}》正在下载", song.name));
        }
        self.shared.update(|s| {
            s.downloads.push(Download { id: song.id, name: song.name.clone(), artists: song.artists.clone(), progress: 0.0 });
            downloads_changed(s);
        });
        self.shared.tell(format!("开始下载《{}》", song.name));
        let (client, http, jobs) = (enji_kit::client(), self.http.clone(), self.jobs.clone());
        tokio::spawn(async move {
            let id = song.id;
            // Reported each percent, not each chunk.
            let shown = std::sync::atomic::AtomicI64::new(-1);
            let progress = |part: f64| {
                let percent = (part * 100.0) as i64;
                if shown.swap(percent, std::sync::atomic::Ordering::Relaxed) != percent {
                    let _ = jobs.send(Job::DownloadProgress { id, progress: part.clamp(0.0, 1.0) });
                }
            };
            let job = match downloads::fetch(&client, &http, &song, level(quality), progress).await {
                Ok(path) => Job::Downloaded { song, path },
                Err(message) => Job::DownloadFailed { id, message },
            };
            let _ = jobs.send(job);
        });
    }

    /// The library as the interface shows it: the newest songs first.
    fn publish_library(&mut self) {
        let songs = local::library_songs(&self.library.tracks);
        enji_kit::copies::set_copies(&songs);
        self.redress();
        let seconds: f64 = songs.iter().map(|song| song.seconds).sum();
        let minutes = (seconds / 60.0).round() as i64;
        let folders: Vec<Folder> = self
            .library
            .folders
            .iter()
            .enumerate()
            .map(|(i, folder)| Folder {
                key: i as i64,
                path: folder.place(),
                name: folder.name.clone(),
                count: self.library.tracks.iter().filter(|track| track.folder == folder.url).count() as i64,
            })
            .collect();
        self.shared.update(|s| {
            s.has_local = !songs.is_empty();
            s.local_count = songs.len() as i64;
            s.local_hours = minutes / 60;
            s.local_minutes = minutes % 60;
            let mut songs = songs;
            arrange(&mut songs, &s.order);
            s.local = songs;
            s.has_folders = !folders.is_empty();
            s.folders = folders;
        });
    }

    /// The songs from NetEase in the lists and the queue played from their
    /// local copies, where there are.
    fn redress(&mut self) {
        enji_kit::dress_all(&mut self.queue);
        self.shared.update(|s| {
            for list in [&mut s.opened_songs, &mut s.album_songs, &mut s.artist_songs, &mut s.queue] {
                enji_kit::dress_all(list);
            }
            // The playing song too: once it is downloaded, there is no more
            // to download.
            if let Some(current) = &mut s.current {
                enji_kit::copies::dress(current);
            }
        });
    }

    /// Match the local files not tried yet with NetEase's songs, one search
    /// at a time, a little apart.
    fn match_library(&mut self) {
        let (netease, online) = self.shared.read(|s| (s.netease, s.online_info));
        if self.auto_matching || !netease || !online {
            return;
        }
        let todo: Vec<(String, String, String, f64)> = self
            .library
            .tracks
            .iter()
            .filter(|track| track.matched.is_empty())
            .map(|track| (track.url.clone(), track.title.clone(), track.artists.clone(), track.seconds))
            .collect();
        if todo.is_empty() {
            return;
        }
        self.auto_matching = true;
        let total = todo.len() as i64;
        self.shared.update(|s| {
            s.auto_matching = true;
            s.match_done = 0;
            s.match_total = total;
        });
        let (client, jobs, shared) = (enji_kit::client(), self.jobs.clone(), self.shared.clone());
        tokio::spawn(async move {
            for (url, title, artists, seconds) in todo {
                // Turning NetEase or online info off stops it before the
                // next search; turned on again, it goes on from there.
                if !shared.read(|s| s.netease && s.online_info) {
                    break;
                }
                let found = match search_songs(&client, &title, &artists, 10).await {
                    Some(found) => Some(best_match(&found, &title, &artists, seconds).map(found_of)),
                    None => None,
                };
                if jobs.send(Job::Matched { url, found }).is_err() {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(1200)).await;
            }
            let _ = jobs.send(Job::MatchingDone);
        });
    }

    /// Search NetEase for the local song at `index` of a list, to choose
    /// its NetEase song by hand.
    fn begin_matching(&mut self, source: Source, index: usize) {
        let Some(song) = self.list_of(&source).get(index).cloned().filter(|song| !song.file.is_empty()) else { return };
        let (title, artists) = self
            .library
            .tracks
            .iter()
            .find(|track| track.url == song.file)
            .map(|track| (track.title.clone(), track.artists.clone()))
            .unwrap_or((song.name.clone(), song.artists.clone()));
        self.shared.update(|s| s.matching = Some(song));
        self.find_candidates(title, artists);
    }

    /// NetEase's songs for the words, to choose the one being matched from.
    fn find_candidates(&mut self, title: String, artists: String) {
        self.shared.update(|s| {
            s.match_candidates.clear();
            s.has_match_candidates = false;
            s.match_loading = true;
        });
        let (client, shared) = (enji_kit::client(), self.shared.clone());
        tokio::spawn(async move {
            let found = search_songs(&client, &title, &artists, 20).await.unwrap_or_default();
            let candidates = convert::songs(&Value::Array(found), &Value::Null);
            shared.update(|s| {
                s.has_match_candidates = !candidates.is_empty();
                s.match_candidates = candidates;
                s.match_loading = false;
            });
        });
    }

    /// The song being matched by hand is `found`, or no NetEase song.
    fn set_match(&mut self, found: Option<Found>, how: &str) {
        let Some(song) = self.shared.read(|s| s.matching.clone()) else { return };
        if let Some(track) = self.library.tracks.iter_mut().find(|track| track.url == song.file) {
            apply_match(track, found, how);
        }
        self.library.save();
        self.shared.update(|s| s.matching = None);
        self.publish_library();
        // The queue had the song by its old id.
        let Some(now) = self.shared.read(|s| s.local.iter().find(|one| one.file == song.file).cloned()) else { return };
        for one in self.queue.iter_mut().filter(|one| one.file == song.file) {
            *one = Song { index: one.index, rank: one.rank, ..now.clone() };
        }
        self.publish_queue();
    }

    fn load_history(&mut self) {
        self.history = store::history()
            .iter()
            .map(|value| Entry {
                song: convert::song_from_json(&value["song"], 0),
                at: value["at"].as_i64().unwrap_or(0),
                plays: value["plays"].as_i64().unwrap_or(1),
            })
            .filter(|entry| entry.song.id != 0)
            .collect();
        self.publish_history();
    }

    fn save_history(&self) {
        let kept: Vec<Value> = self
            .history
            .iter()
            .map(|entry| serde_json::json!({ "song": convert::song_json(&entry.song), "at": entry.at, "plays": entry.plays }))
            .collect();
        store::save_history(&kept);
        self.publish_history();
    }

    /// The history as it shows: with NetEase off, only the local songs.
    fn publish_history(&self) {
        let netease = self.shared.read(|s| s.netease);
        let played: Vec<Played> = self
            .history
            .iter()
            .filter(|entry| netease || !entry.song.file.is_empty())
            .enumerate()
            .map(|(i, entry)| Played {
                key: i as i64,
                song: Song { index: i as i64, rank: i as i64 + 1, ..entry.song.clone() },
                when: when(entry.at),
                plays: entry.plays,
            })
            .collect();
        self.shared.update(|s| {
            s.has_history = !played.is_empty();
            s.history = played;
        });
    }

    /// `song` played now: first in the history, once more.
    fn record(&mut self, song: Song) {
        let plays = match self.history.iter().position(|entry| entry.song.id == song.id) {
            Some(place) => self.history.remove(place).plays + 1,
            None => 1,
        };
        self.history.insert(0, Entry { song, at: now_ms(), plays });
        self.history.truncate(HISTORY);
        self.save_history();
    }

    /// Every list shows the play counts as they are now.
    fn show_counts(&mut self) {
        crate::plays::dress(&mut self.queue);
        self.shared.update(|s| {
            for list in [
                &mut s.opened_songs,
                &mut s.album_songs,
                &mut s.artist_songs,
                &mut s.queue,
                &mut s.local,
            ] {
                crate::plays::dress(list);
            }
            for played in &mut s.history {
                crate::plays::dress(std::slice::from_mut(&mut played.song));
            }
        });
    }

    /// NetEase's all-time counts of the account's most played songs.
    fn load_plays(&self) {
        let (client, jobs, uid) = (enji_kit::client(), self.jobs.clone(), self.uid);
        if uid <= 0 {
            return;
        }
        tokio::spawn(async move {
            let Ok(r) = client.user_record(&Query::new().param("uid", &param(uid)).param("type", "0")).await else {
                return;
            };
            let found: std::collections::HashMap<i64, i64> = r.body["allData"]
                .as_array()
                .map(|all| {
                    all.iter()
                        .filter_map(|one| Some((one["song"]["id"].as_i64()?, one["playCount"].as_i64()?)))
                        .collect()
                })
                .unwrap_or_default();
            crate::plays::set_netease(found);
            let _ = jobs.send(Job::Counted);
        });
    }

    // MARK: Albums and singers

    /// `kind` `id` opened over what is open; opened already on top, nothing
    /// more.
    fn push(&mut self, kind: Detail, id: i64) {
        if self.stack.last() != Some(&(kind.clone(), id)) {
            self.stack.push((kind.clone(), id));
        }
        self.shared.update(|s| s.detail = kind);
    }

    /// What is on top of the stack shown again, read again when what it
    /// shows was replaced by another of its kind.
    fn show_top(&mut self) {
        let Some((kind, id)) = self.stack.last().cloned() else {
            return self.shared.update(|s| {
                s.detail = Detail::Page;
                s.opened = None;
                s.album = None;
                s.artist = None;
            });
        };
        let shown = self.shared.read(|s| match kind {
            Detail::Playlist => s.opened.as_ref().map(|list| list.id),
            Detail::Album => s.album.as_ref().map(|album| album.id),
            Detail::Artist => s.artist.as_ref().map(|artist| artist.id),
            Detail::Account => Some(id),
            Detail::Page => None,
        });
        self.shared.update(|s| s.detail = kind.clone());
        if shown != Some(id) {
            match kind {
                Detail::Playlist => self.open(id, None),
                Detail::Album => self.load_album(id),
                Detail::Artist => self.load_artist(id),
                Detail::Account | Detail::Page => {}
            }
        }
    }

    /// The songs of the range chosen on the account page.
    fn show_top_songs(&self) {
        let (all, week) = (self.top_all.clone(), self.top_week.clone());
        self.shared.update(|s| {
            s.top_songs = if s.top_range == TopRange::Week { week } else { all };
            s.has_top_songs = !s.top_songs.is_empty();
        });
    }

    /// What NetEase says of the account, and which years have a report:
    /// asked each time, so a new report shows the day it is up.
    fn load_profile(&self) {
        let (client, http, jobs, id) = (enji_kit::client(), self.http.clone(), self.jobs.clone(), self.uid);
        tokio::spawn(async move {
            let uid = param(id);
            let by_uid = Query::new().param("uid", &uid);
            let nothing = Query::new();
            let all_time = Query::new().param("uid", &uid).param("type", "0");
            let this_week = Query::new().param("uid", &uid).param("type", "1");
            let (detail, total, level, counts, vip, years, all, week, medals, reports) = tokio::join!(
                client.user_detail(&by_uid),
                client.listen_data_total(&nothing),
                client.user_level(&nothing),
                client.user_subcount(&nothing),
                client.vip_info(&by_uid),
                client.listen_data_year_report(&nothing),
                client.user_record(&all_time),
                client.user_record(&this_week),
                client.user_medal(&by_uid),
                report_years(&http),
            );
            let body = |answer: Result<ncm_api_rs::ApiResponse, _>| answer.map(|answer| answer.body).unwrap_or(Value::Null);
            let (level, counts, vip) = (body(level), body(counts), body(vip));
            let medals = body(medals);
            let seconds = body(total)["data"]["totalDuration"].as_i64().unwrap_or(0);
            let read = AccountRead {
                profile: detail.ok().map(|detail| profile_of(id, &detail.body, seconds, &level, &counts, &vip)),
                years: listen_years(&body(years), &reports),
                top_all: top_songs(&body(all)["allData"]),
                top_week: top_songs(&body(week)["weekData"]),
                medal_count: medals["data"]["medalNum"].as_i64().unwrap_or(0),
                medals: medals_of(&medals),
            };
            let _ = jobs.send(Job::ProfileRead(Box::new(read)));
        });
    }

    fn load_album(&self, id: i64) {
        let (client, shared) = (enji_kit::client(), self.shared.clone());
        shared.update(|s| {
            s.album = Some(enji_protocol::enji::Album {
                id,
                name: String::new(),
                cover: MediaSource::new(""),
                detail: String::new(),
                artist_id: 0,
            });
            s.album_songs.clear();
            s.album_description.clear();
            s.album_artist.clear();
            s.album_meta.clear();
            s.album_loading = true;
        });
        tokio::spawn(async move {
            let found = client.album(&Query::new().param("id", &param(id))).await;
            let still_open = |s: &State| s.album.as_ref().is_some_and(|album| album.id == id);
            match found {
                Ok(r) => shared.update(|s| {
                    if !still_open(s) {
                        return;
                    }
                    let info = &r.body["album"];
                    s.album = Some(convert::album(info));
                    s.album_artist = info["artist"]["name"].as_str().unwrap_or("").to_string();
                    s.album_meta = convert::album_meta(info);
                    s.album_description = info["description"].as_str().unwrap_or("").trim().to_string();
                    s.album_songs = convert::songs(&r.body["songs"], &Value::Null);
                    enji_kit::dress_all(&mut s.album_songs);
                    s.album_loading = false;
                }),
                Err(e) => {
                    shared.update(|s| s.album_loading = false);
                    shared.fail(format!("专辑加载失败：{e}"));
                }
            }
        });
    }

    fn load_artist(&self, id: i64) {
        let (client, shared) = (enji_kit::client(), self.shared.clone());
        shared.update(|s| {
            s.artist = Some(enji_protocol::enji::Artist {
                id,
                name: String::new(),
                picture: MediaSource::new(""),
                detail: String::new(),
            });
            s.artist_songs.clear();
            s.artist_albums.clear();
            s.artist_description.clear();
            s.artist_loading = true;
        });
        tokio::spawn(async move {
            let id_text = param(id);
            let (about, albums_query) = (Query::new().param("id", &id_text), Query::new().param("id", &id_text).param("limit", "60"));
            let (found, albums) = tokio::join!(client.artists(&about), client.artist_album(&albums_query));
            let still_open = |s: &State| s.artist.as_ref().is_some_and(|artist| artist.id == id);
            match found {
                Ok(r) => shared.update(|s| {
                    if !still_open(s) {
                        return;
                    }
                    let info = &r.body["artist"];
                    s.artist = Some(convert::artist(info));
                    s.artist_description = info["briefDesc"].as_str().unwrap_or("").trim().to_string();
                    s.artist_songs = convert::songs(&r.body["hotSongs"], &Value::Null);
                    enji_kit::dress_all(&mut s.artist_songs);
                    if let Ok(r) = &albums {
                        s.artist_albums = list(&r.body["hotAlbums"], convert::album);
                    }
                    s.artist_loading = false;
                }),
                Err(e) => {
                    shared.update(|s| s.artist_loading = false);
                    shared.fail(format!("歌手加载失败：{e}"));
                }
            }
        });
    }

    // MARK: Playlists of the account

    /// Add the song being chosen for to the playlist `id`.
    fn add_to_playlist(&self, id: i64) {
        let Some(song) = self.shared.read(|s| s.adding.clone()) else { return };
        self.forget(id);
        let name =self.shared.read(|s| s.mine.iter().find(|list| list.id == id).map(|list| list.name.clone()).unwrap_or_default());
        self.shared.update(|s| s.adding = None);
        let (client, shared, jobs) = (enji_kit::client(), self.shared.clone(), self.jobs.clone());
        tokio::spawn(async move {
            let query = Query::new().param("op", "add").param("pid", &param(id)).param("tracks", &param(song.id));
            match client.playlist_tracks(&query).await {
                Ok(r) if r.body["code"].as_i64().or(r.body["body"]["code"].as_i64()) == Some(200) => {
                    shared.tell(format!("已加入歌单「{name}」"));
                    let _ = jobs.send(Job::Refresh);
                }
                Ok(r) => shared.fail(refusal(&r.body, "没能加入歌单")),
                Err(e) => shared.fail(format!("没能加入歌单：{e}")),
            }
        });
    }

    /// Take the song at `index` out of the open playlist, the account's own.
    fn remove_from_playlist(&self, index: usize) {
        let found = self.shared.read(|s| {
            let list = s.opened.as_ref().filter(|_| s.opened_owned)?;
            Some((list.id, s.opened_songs.get(index)?.clone()))
        });
        let Some((id, song)) = found else { return };
        self.forget(id);
        let (client, shared) = (enji_kit::client(), self.shared.clone());
        tokio::spawn(async move {
            let query = Query::new().param("op", "del").param("pid", &param(id)).param("tracks", &param(song.id));
            match client.playlist_tracks(&query).await {
                Ok(r) if r.body["code"].as_i64().or(r.body["body"]["code"].as_i64()) == Some(200) => shared.update(|s| {
                    if s.opened.as_ref().is_some_and(|list| list.id == id) {
                        let kept = s.opened_songs.iter().filter(|one| one.id != song.id).cloned();
                        s.opened_songs = renumbered(kept.collect::<Vec<_>>());
                    }
                }),
                Ok(r) => shared.fail(refusal(&r.body, "没能从歌单中删除")),
                Err(e) => shared.fail(format!("没能从歌单中删除：{e}")),
            }
        });
    }

    fn create_playlist(&self, name: String) {
        self.shared.update(|s| s.naming = false);
        if self.uid == 0 {
            return self.shared.fail("登录后才能新建歌单");
        }
        let (client, shared, jobs) = (enji_kit::client(), self.shared.clone(), self.jobs.clone());
        tokio::spawn(async move {
            match client.playlist_create(&Query::new().param("name", &name)).await {
                Ok(r) if r.body["code"].as_i64() == Some(200) => {
                    shared.tell(format!("已新建歌单「{name}」"));
                    let _ = jobs.send(Job::Refresh);
                }
                Ok(r) => shared.fail(refusal(&r.body, "没能新建歌单")),
                Err(e) => shared.fail(format!("没能新建歌单：{e}")),
            }
        });
    }

    /// Delete the playlist that was asked about.
    fn delete_playlist(&self) {
        let Some(list) = self.shared.read(|s| s.deleting.clone()) else { return };
        self.shared.update(|s| s.deleting = None);
        let (client, shared) = (enji_kit::client(), self.shared.clone());
        tokio::spawn(async move {
            match client.playlist_delete(&Query::new().param("id", &param(list.id))).await {
                Ok(r) if r.body["code"].as_i64() == Some(200) => {
                    shared.update(|s| {
                        s.mine.retain(|one| one.id != list.id);
                        sort_playlists(s);
                        if s.opened.as_ref().is_some_and(|one| one.id == list.id) {
                            s.opened = None;
                        }
                    });
                    shared.tell(format!("已删除歌单「{}」", list.name));
                }
                Ok(r) => shared.fail(refusal(&r.body, "没能删除歌单")),
                Err(e) => shared.fail(format!("没能删除歌单：{e}")),
            }
        });
    }

    // MARK: Lists

    /// The account's playlists again.
    fn refresh(&self) {
        let (shared, uid) = (self.shared.clone(), self.uid);
        if uid <= 0 || !shared.read(|s| s.netease) {
            return;
        }
        tokio::spawn(async move {
            let found = enji_kit::client()
                .user_playlist(&Query::new().param("uid", &param(uid)).param("limit", "100"))
                .await;
            match found {
                Ok(r) => shared.update(|s| {
                    s.mine = r.body["playlist"]
                        .as_array()
                        .map(|all| {
                            all.iter()
                                .map(|one| Playlist {
                                    owned: one["creator"]["userId"].as_i64() == Some(uid),
                                    // NetEase marks the account's liked songs so.
                                    liked: one["specialType"].as_i64() == Some(5),
                                    ..convert::playlist(one)
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    sort_playlists(s);
                }),
                Err(_) => shared.fail("连不上网易云音乐，检查一下网络"),
            }
        });
    }

    /// Open the playlist `id`, as it was when last opened or with what is
    /// `known` of it, until it arrives.
    fn open(&self, id: i64, known: Option<Playlist>) {
        let client = enji_kit::client();
        let (shared, kept) = (self.shared.clone(), self.kept.clone());
        let (jobs, uid) = (self.jobs.clone(), self.uid);
        let before = kept.lock().unwrap_or_else(|e| e.into_inner()).get(&(uid, id)).cloned();
        shared.update(|s| {
            let known = known.or_else(|| s.mine.iter().find(|p| p.id == id).cloned());
            s.opened = Some(known.unwrap_or(Playlist {
                id,
                name: String::new(),
                cover: MediaSource::new(""),
                detail: String::new(),
                owned: false,
                liked: false,
            }));
            s.opened_owned = false;
            s.opened_songs.clear();
            s.opened_description.clear();
            s.opened_owner.clear();
            s.opened_owner_avatar = MediaSource::new("");
            s.opened_meta.clear();
            s.opened_loading = true;
            if let Some(before) = &before {
                show_opened(s, id, &before.info, before.songs.clone(), uid);
            }
        });
        if before.is_some() {
            let _ = jobs.send(Job::Opened(id));
        }
        tokio::spawn(async move {
            let still_open = |s: &State| s.opened.as_ref().is_some_and(|p| p.id == id);
            let fresh = match outline(&client, id).await {
                Ok(info) => {
                    let changed = info["trackUpdateTime"].as_i64().unwrap_or(0);
                    match before.as_ref().filter(|before| changed != 0 && before.changed == changed) {
                        Some(before) => Ok(Kept { changed, info, songs: before.songs.clone() }),
                        None => songs_in(&client, &info).await.map(|songs| Kept { changed, info, songs }),
                    }
                }
                Err(e) => Err(e),
            };
            match fresh {
                Ok(fresh) => {
                    shared.update(|s| {
                        if still_open(s) {
                            show_opened(s, id, &fresh.info, fresh.songs.clone(), uid);
                        }
                    });
                    kept.lock().unwrap_or_else(|e| e.into_inner()).insert((uid, id), fresh);
                    if before.is_none() {
                        let _ = jobs.send(Job::Opened(id));
                    }
                }
                // What it was last time stays.
                Err(_) if before.is_some() => {}
                Err(e) => {
                    shared.update(|s| s.opened_loading = false);
                    shared.fail(format!("歌单加载失败：{e}"));
                }
            }
        });
    }

    /// Open the playlist `id` afresh the next time, after its songs changed.
    fn forget(&self, id: i64) {
        self.kept.lock().unwrap_or_else(|e| e.into_inner()).remove(&(self.uid, id));
    }

    // MARK: Account

    /// A new QR code to sign in with, scanned in the NetEase app, and the
    /// wait for it to be.
    fn show_code(&mut self) {
        if let Some(task) = self.signing_in.take() {
            task.abort();
        }
        let client = enji_kit::client();
        let shared = self.shared.clone();
        let jobs = self.jobs.clone();
        shared.update(|s| {
            s.login = Login::Loading;
            s.code_saved = false;
            s.save_error.clear();
            s.scanner.clear();
            s.scanner_avatar = MediaSource::new("");
        });
        self.signing_in = Some(tokio::spawn(async move {
            let key = match client.login_qr_key(&Query::new()).await {
                Ok(r) => r.body["unikey"].as_str().or(r.body["data"]["unikey"].as_str()).map(str::to_string),
                Err(_) => None,
            };
            let url = match &key {
                Some(key) => match client.login_qr_create(&Query::new().param("key", key)).await {
                    Ok(r) => r.body["data"]["qrurl"].as_str().map(str::to_string),
                    Err(_) => None,
                },
                None => None,
            };
            let serial = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() as u64);
            let (Some(key), Some(picture)) = (key, url.and_then(|url| store::qr_png(&url, serial).ok())) else {
                shared.update(|s| s.login = Login::Out);
                return shared.fail("拿不到登录二维码，检查一下网络");
            };
            shared.update(|s| {
                s.qr = MediaSource::file(&picture);
                s.login = Login::Waiting;
            });
            // NetEase lets a code live about two minutes.
            for _ in 0..90 {
                tokio::time::sleep(Duration::from_secs(2)).await;
                let Ok(r) = client.login_qr_check(&Query::new().param("key", &key)).await else {
                    continue;
                };
                match r.body["code"].as_i64().unwrap_or(r.status) {
                    802 => shared.update(|s| {
                        s.login = Login::Scanned;
                        s.scanner = r.body["nickname"].as_str().unwrap_or("").to_string();
                        s.scanner_avatar = convert::cover(r.body["avatarUrl"].as_str().unwrap_or(""), 160);
                    }),
                    803 => {
                        let cookie = cookie_of(&r.cookie);
                        if cookie.contains("MUSIC_U=") {
                            let _ = jobs.send(Job::SignedIn(cookie));
                        } else {
                            shared.update(|s| s.login = Login::Expired);
                            shared.fail("没拿到登录凭据，刷新二维码再试一次");
                        }
                        return;
                    }
                    800 => return shared.update(|s| s.login = Login::Expired),
                    _ => {}
                }
            }
            shared.update(|s| s.login = Login::Expired);
        }));
    }

    /// Save the sign-in code to the phone's photos, for the NetEase app on
    /// it to scan from the album.
    fn save_code(&self) {
        let Some(code) = store::qr_file() else { return };
        let shared = self.shared.clone();
        // Said in the sheet: a notice would show under it.
        tokio::task::spawn_blocking(move || {
            let saved = bern::platform::save_image(&code);
            shared.update(|s| match saved {
                Ok(()) => {
                    s.code_saved = true;
                    s.save_error.clear();
                }
                Err(e) if e.code == "refused" => s.save_error = "没有相册权限：在系统设置里允许 Enji 添加照片".into(),
                Err(e) => s.save_error = format!("没能存到相册：{}", e.message),
            });
        });
    }

    /// The signed-in account; `welcome` it when it has just signed in.
    fn fetch_account(&self, welcome: bool) {
        let client = enji_kit::client();
        let shared = self.shared.clone();
        let jobs = self.jobs.clone();
        tokio::spawn(async move {
            let status = client.login_status(&Query::new()).await;
            // NetEase sends the profile at the top; the Node server's copy of this
            // call wraps it in `data`.
            let profile = status
                .as_ref()
                .map(|r| match &r.body["profile"] {
                    Value::Null => r.body["data"]["profile"].clone(),
                    profile => profile.clone(),
                })
                .unwrap_or(Value::Null);
            let uid = profile["userId"].as_i64().unwrap_or(0);
            if uid == 0 {
                shared.update(|s| s.login = Login::Out);
                if status.is_ok() {
                    store::forget_cookie();
                    enji_kit::set_cookie(None);
                    shared.fail("登录已过期，重新登录");
                }
                return;
            }
            shared.update(|s| {
                s.login = Login::In;
                s.nickname = profile["nickname"].as_str().unwrap_or("").to_string();
                s.avatar = convert::cover(profile["avatarUrl"].as_str().unwrap_or(""), 160);
            });
            if welcome {
                let name = shared.read(|s| s.nickname.clone());
                shared.tell(format!("欢迎回来，{name}"));
            }
            let _ = jobs.send(Job::Account(uid));
        });
    }

    // MARK: Playback

    /// The song left last time, paused where it was, with its queue.
    fn open_left(&mut self) {
        let Some(left) = resume::load() else { return };
        self.set_queue(left.songs, left.at);
        let Some(song) = self.queue.get(self.at).cloned().map(crate::covers::localized) else { return };
        self.paused = true;
        self.resume.song(&self.queue, self.at);
        self.shared.update(|s| {
            s.current = Some(song.clone());
            s.current_id = song.id;
            downloads_changed(s);
            s.queue_at = self.at as i64;
            s.total = song.duration.clone();
        });
        self.show_left(left.position);
        self.publish_queue();
        self.load_panel();
        self.left_lyrics(&song);
    }

    /// The song left last time stands at `position`, for when it plays.
    fn show_left(&mut self, position: f64) {
        self.left_at = Some(position);
        self.position = position;
        self.resume.place(self.at, position);
        let length = self.shared.read(|s| s.current.as_ref().map_or(0.0, |song| song.seconds));
        self.shared.live(|live| {
            live.progress = if length > 0.0 { (position / length).clamp(0.0, 1.0) } else { 0.0 };
            live.elapsed = convert::clock(position);
        });
    }

    /// The lyrics of the song left last time, before it plays.
    fn left_lyrics(&self, song: &Song) {
        if !song.file.is_empty() {
            let token = self.token;
            let name = bern::files::name(&song.file).unwrap_or_default();
            let lines = local::lyrics(&song.file, &name, None);
            let _ = self.jobs.send(Job::LocalInfo { token, lines, cover: String::new() });
        } else if self.shared.read(|s| s.netease) {
            self.lyrics_of(song.id);
        }
    }

    fn start(&mut self, index: usize) {
        // Its covers from this device when they were fetched ahead.
        let Some(song) = self.queue.get(index).cloned().map(crate::covers::localized) else { return };
        self.finish_listening();
        self.begin_listening(&song);
        self.room_song_ended = None;
        self.at = index;
        self.position = 0.0;
        self.seeking = None;
        self.paused = false;
        self.left_at = None;
        self.start_at = None;
        self.resume.song(&self.queue, index);
        // More to come before the heart mode runs out.
        if song.id > 0 && index + 3 >= self.queue.len() && self.in_heart_mode() {
            self.ask_heart(song.id);
        }
        if index + 2 >= self.queue.len() && self.in_fm() {
            self.ask_fm();
        }
        // The queue is shown from the playing song on.
        self.publish_queue();
        self.token += 1;
        self.lines.clear();
        self.audio.stop();
        if let Some(stream) = self.stream.take() {
            stream.cancel();
        }
        self.comments_for = 0;
        self.details_for = 0;
        let quality = self.shared.read(|s| s.quality.clone());
        let liked = self.liked.contains(&song.id);
        self.shared.update(|s| {
            // Only songs on NetEase have comments and details.
            if song.id < 0 {
                s.panel = Panel::Lyrics;
            }
            s.current = Some(song.clone());
            s.current_id = song.id;
            downloads_changed(s);
            s.current_liked = liked;
            s.queue_at = index as i64;
            s.comments.clear();
            s.comments_total = 0;
            s.comments_more = false;
            s.details = None;
            s.preview = false;
            s.playing = false;
            s.buffering = true;
            s.total = song.duration.clone();
            s.lyrics.clear();
            s.lyrics_near.clear();
            s.has_lyrics = false;
            s.line = 0;
            s.browsing = false;
            s.format.clear();
            sung(s);
        });
        self.shared.live(|live| {
            live.progress = 0.0;
            live.elapsed = "0:00".into();
            live.lit = -1.0;
        });
        self.show_now_playing(Some(0.0));
        self.desktop_lyrics();
        self.greet_plugins();
        self.fetch_now_artwork(&song);
        self.load_panel();
        let (client, http, jobs, token) = (enji_kit::client(), self.http.clone(), self.jobs.clone(), self.token);
        if !song.file.is_empty() {
            return self.start_local(song);
        }
        if !self.shared.read(|s| s.netease) {
            let _ = jobs.send(Job::Failed { token, message: "网易云音乐已在设置中关闭".into() });
            return;
        }
        let by_word = self.shared.read(|s| s.word_lyrics);
        tokio::spawn(async move {
            let id = param(song.id);
            let url = async {
                let r = song_url(&client, &song.name, &id, level(quality)).await?;
                let data = &r.body["data"][0];
                let url = data["url"]
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| format!("《{}》暂时不能播放", song.name))?;
                // Only a preview of a song for members.
                Ok::<_, String>((url, !data["freeTrialInfo"].is_null(), streamed(data)))
            };
            let lyric_query = Query::new().param("id", &id);
            let lyric = client.lyric_new(&lyric_query);
            let (url, lyric) = tokio::join!(url, lyric);
            let lines = lyric.map(|r| lyrics::of_answer(&r.body, by_word)).unwrap_or_default();
            let response = match url {
                Ok((url, preview, format)) => match http.get(&url).send().await.and_then(|r| r.error_for_status()) {
                    Ok(response) => Ok((response, preview, format)),
                    Err(e) => Err(format!("下载《{}》失败：{e}", song.name)),
                },
                Err(message) => Err(message),
            };
            let (mut response, preview, format) = match response {
                Ok(found) => found,
                Err(message) => {
                    let _ = jobs.send(Job::Failed { token, message });
                    return;
                }
            };
            // It plays from the first bytes on; the rest follows.
            let stream = Stream::new(response.content_length());
            let _ = jobs.send(Job::Ready { token, stream: stream.clone(), preview, lines, format });
            while let Ok(Some(chunk)) = response.chunk().await {
                if stream.cancelled() {
                    return;
                }
                stream.push(&chunk);
            }
            stream.finish();
        });
    }

    /// A song from a local folder: read from its file, with the lyrics that
    /// come with it, or else those of the same song on NetEase.
    fn start_local(&mut self, song: Song) {
        let (jobs, token) = (self.jobs.clone(), self.token);
        let file = match bern::files::open(&song.file) {
            Ok(file) => file,
            Err(e) => {
                let _ = jobs.send(Job::Failed { token, message: format!("打不开《{}》：{}", song.name, e.message) });
                return;
            }
        };
        // Its name says what kind of file it is; a file no longer in the
        // library is asked for its name.
        let track = self.library.tracks.iter().find(|track| track.url == song.file);
        let name = track.map(|track| track.name.clone()).or_else(|| bern::files::name(&song.file)).unwrap_or_default();
        let lines = local::lyrics(&song.file, &name, track.and_then(|track| track.lyrics.as_deref()));
        {
            let (jobs, url, name) = (jobs.clone(), song.file.clone(), name.clone());
            tokio::task::spawn_blocking(move || {
                let _ = jobs.send(Job::Format { token, format: local::format(&url, &name) });
            });
        }
        // The full picture, which lists show small, is taken from the file.
        if song.cover.url.starts_with("file:") {
            let (jobs, url) = (jobs.clone(), song.file.clone());
            tokio::task::spawn_blocking(move || {
                if let Some(path) = local::artwork(&url, &name) {
                    let _ = jobs.send(Job::Artwork { token, path });
                }
            });
        }
        // Some providers hand over a stream without a length.
        let stream = Stream::new(Some(file.len()).filter(|&len| len > 0));
        let (online, netease) = self.shared.read(|s| (s.online_info, s.netease));
        if online && netease && song.id > 0 && lines.is_empty() {
            self.lyrics_of(song.id);
        } else if online && netease && song.id < 0 && (lines.is_empty() || song.cover.url.is_empty()) {
            self.look_up(song.clone(), lines.is_empty());
        }
        let _ = jobs.send(Job::Ready { token, stream: stream.clone(), preview: false, lines, format: String::new() });
        std::thread::spawn(move || {
            let mut file = file;
            let mut chunk = vec![0u8; 256 * 1024];
            loop {
                match file.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) if !stream.cancelled() => stream.push(&chunk[..n]),
                    Ok(_) => return,
                }
            }
            stream.finish();
        });
    }

    /// The lyrics of the NetEase song `id`, for its local copy.
    fn lyrics_of(&self, id: i64) {
        let (client, jobs, token) = (enji_kit::client(), self.jobs.clone(), self.token);
        let by_word = self.shared.read(|s| s.word_lyrics);
        tokio::spawn(async move {
            if let Ok(r) = client.lyric_new(&Query::new().param("id", &param(id))).await {
                let lines = lyrics::of_answer(&r.body, by_word);
                let _ = jobs.send(Job::LocalInfo { token, lines, cover: String::new() });
            }
        });
    }

    /// The same song on NetEase, found by name and length, for the lyrics
    /// and cover a local file lacks.
    fn look_up(&self, song: Song, want_lyrics: bool) {
        let (client, jobs, token) = (enji_kit::client(), self.jobs.clone(), self.token);
        let by_word = self.shared.read(|s| s.word_lyrics);
        tokio::spawn(async move {
            let words = format!("{} {}", song.name, song.artists);
            let query = Query::new().param("keywords", words.trim()).param("type", "1").param("limit", "8");
            let Ok(r) = client.cloudsearch(&query).await else { return };
            let found = r.body["result"]["songs"].as_array().cloned().unwrap_or_default();
            let near = |one: &&Value| {
                let seconds = one["dt"].as_f64().unwrap_or(0.0) / 1000.0;
                song.seconds <= 0.0 || (seconds - song.seconds).abs() < 3.0
            };
            let same_name = |one: &&Value| one["name"].as_str().is_some_and(|name| name.eq_ignore_ascii_case(&song.name));
            let Some(best) = found.iter().find(|one| near(one) && same_name(one)).or_else(|| found.iter().find(near)) else {
                return;
            };
            let cover = if song.cover.url.is_empty() {
                best["al"]["picUrl"].as_str().unwrap_or("").to_string()
            } else {
                String::new()
            };
            let mut lines = Vec::new();
            if want_lyrics {
                if let Some(id) = best["id"].as_i64() {
                    if let Ok(r) = client.lyric_new(&Query::new().param("id", &param(id))).await {
                        lines = lyrics::of_answer(&r.body, by_word);
                    }
                }
            }
            let _ = jobs.send(Job::LocalInfo { token, lines, cover });
        });
    }

    /// The song after the playing one, or before it, as the play mode has
    /// it; a song put to play next comes first, and a song that ended
    /// (`by_itself`) under repeat one plays again. Shuffling is Misuzu
    /// Music's: along the shuffled order, shuffled anew once through.
    fn advance(&mut self, forward: bool, by_itself: bool) {
        let count = self.queue.len();
        if count == 0 {
            return;
        }
        let roaming = self.in_fm();
        if roaming && forward && self.at + 1 >= count && self.play_next.is_empty() {
            self.fm_waiting = true;
            return self.ask_fm();
        }
        // Roaming goes from song to song, whatever the mode was.
        let mode = if roaming { PlayMode::RepeatAll } else { self.shared.read(|s| s.mode.clone()) };
        if forward && let Some(index) = self.take_play_next() {
            if mode == PlayMode::Shuffle && count > 1 {
                self.shuffle.sync(count, self.at);
                self.shuffle.position = self.shuffle.place_next(count, self.at, index);
            }
            return self.start(index);
        }
        if by_itself && mode == PlayMode::RepeatOne {
            return self.start(self.at);
        }
        let next = match (mode, forward) {
            (PlayMode::Shuffle, true) => self.shuffle.next(count, self.at),
            (PlayMode::Shuffle, false) => self.shuffle.previous(count, self.at),
            (_, true) => (self.at + 1) % count,
            (_, false) => (self.at + count - 1) % count,
        };
        self.start(next);
    }

    // MARK: Word by word

    /// How much of line `line` is lit at `at`, as the state tells it.
    fn lit_at(&self, line: i64, at: f64) -> f64 {
        let on = self.shared.read(|s| s.word_lyrics);
        let sung = self.lines.get(line as usize).filter(|_| on).and_then(|line| line.sung(at));
        // An eighth of a character at a time: finer shows no more, and each
        // step draws the line again.
        sung.map_or(-1.0, |lit| (lit * 8.0).floor() / 8.0)
    }

    /// The sung line up to the word that plays now, more often than the
    /// song's place shows.
    fn word_tick(&mut self) {
        let (line, desktop) = self.shared.read(|s| (s.line, s.desktop_lyrics));
        let was = enji_kit::live::read(|live| live.lit);
        let lit = self.lit_at(line, self.audio.position());
        if lit != was {
            self.shared.live(|live| live.lit = lit);
            if desktop {
                self.show_desktop_lyrics();
            }
        }
    }

    // MARK: Roaming

    fn in_fm(&self) -> bool {
        self.shared.read(|s| s.fm)
    }

    /// Songs NetEase picks for the account in place of the queue.
    fn start_fm(&mut self) {
        if self.uid == 0 {
            return self.shared.fail("登录后才能使用私人漫游");
        }
        self.fm_starting = true;
        self.ask_fm();
    }

    /// NetEase sends a few at a time, and only their ids are read: these
    /// songs come in the older shape, with less about them than a song's
    /// details have.
    fn ask_fm(&mut self) {
        if self.fm_asking {
            return;
        }
        self.fm_asking = true;
        let (client, jobs) = (enji_kit::client(), self.jobs.clone());
        tokio::spawn(async move {
            let picked = client.personal_fm(&Query::new()).await.map(|r| r.body).unwrap_or_default();
            let ids: Vec<i64> = picked["data"].as_array().into_iter().flatten().filter_map(|song| song["id"].as_i64()).collect();
            let _ = jobs.send(Job::FmSongs(songs_of(&client, &ids).await));
        });
    }

    fn fm_songs(&mut self, songs: Vec<Song>) {
        self.fm_asking = false;
        let songs = songs.into_iter().filter(|song| !song.unavailable);
        if std::mem::take(&mut self.fm_starting) {
            let songs: Vec<Song> = songs.collect();
            if songs.is_empty() {
                return self.shared.fail("私人漫游暂时没有可播放的歌曲");
            }
            if self.in_heart_mode() {
                self.set_mode(PlayMode::RepeatAll);
            }
            self.queue_list = 0;
            self.set_queue(songs, 0);
            self.shared.update(|s| s.fm = true);
            return self.start(0);
        }
        if !self.in_fm() {
            return;
        }
        let played: std::collections::HashSet<i64> = self.queue.iter().map(|song| song.id).collect();
        self.queue.extend(songs.filter(|song| !played.contains(&song.id)));
        self.publish_queue();
        if std::mem::take(&mut self.fm_waiting) && self.at + 1 < self.queue.len() {
            self.start(self.at + 1);
        }
    }

    /// NetEase hears that the playing song is not wanted, and the next plays.
    fn fm_dislike(&mut self) {
        let Some(song) = self.queue.get(self.at).filter(|_| self.in_fm()) else { return };
        let query = Query::new().param("id", &param(song.id)).param("time", &param(self.position as i64));
        let client = enji_kit::client();
        tokio::spawn(async move {
            let _ = client.fm_trash(&query).await;
        });
        self.advance(true, false);
    }

    // MARK: The heart mode

    fn in_heart_mode(&self) -> bool {
        self.shared.read(|s| s.mode == PlayMode::Heart)
    }

    /// It needs the switch in the settings and an account to pick for.
    fn can_heart(&self) -> bool {
        self.uid > 0 && self.shared.read(|s| s.netease && s.heart_mode)
    }

    /// Play on as `mode` has it from the playing song.
    fn set_mode(&mut self, mode: PlayMode) {
        let was_heart = self.in_heart_mode();
        let (shuffling, heart) = (mode == PlayMode::Shuffle, mode == PlayMode::Heart);
        self.shared.update(|s| {
            s.mode = mode;
            save_settings(s);
        });
        if was_heart && !heart {
            self.leave_heart();
        }
        if shuffling && self.queue.len() > 1 {
            self.shuffle.generate(self.queue.len(), self.at);
        }
        self.publish_queue();
        if heart {
            self.enter_heart();
        }
    }

    /// Go on from the playing song with what NetEase picks after it, or
    /// from the first song of the queue that is on NetEase.
    fn enter_heart(&mut self) {
        let seed = self.queue.get(self.at).into_iter().chain(&self.queue).find(|song| song.id > 0).map(|song| song.id);
        let Some(seed) = seed else {
            self.shared.fail("先播放一首网易云音乐的歌");
            return self.set_mode(PlayMode::RepeatAll);
        };
        if self.before_heart.is_none() {
            self.before_heart = Some((self.queue.clone(), self.queue_list));
        }
        self.heart_filled = false;
        self.ask_heart(seed);
    }

    /// Ask NetEase what goes well after `seed`: it picks from around the
    /// account's liked songs, the first of its playlists, and no other list.
    fn ask_heart(&mut self, seed: i64) {
        if self.heart_asking {
            return;
        }
        self.heart_asking = true;
        let list = self.shared.read(|s| s.mine.first().map_or(0, |list| list.id));
        let (client, jobs) = (enji_kit::client(), self.jobs.clone());
        tokio::spawn(async move {
            let query = Query::new().param("id", &param(seed)).param("pid", &param(list)).param("sid", &param(seed)).param("count", "1");
            let picked = client.playmode_intelligence_list(&query).await.map(|r| r.body).unwrap_or_default();
            let songs: Vec<Song> = picked["data"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
                .map(|(i, one)| convert::song(&one["songInfo"], i, Some(&one["songInfo"]["privilege"])))
                .filter(|song| song.id > 0 && !song.unavailable)
                .collect();
            let _ = jobs.send(Job::HeartSongs(songs));
        });
    }

    /// What NetEase picked follows the playing song.
    fn heart_songs(&mut self, songs: Vec<Song>) {
        self.heart_asking = false;
        if !self.in_heart_mode() {
            return;
        }
        if songs.is_empty() {
            // Once under way it goes on with the songs it has.
            if !self.heart_filled {
                self.shared.fail("心动模式暂时没有可播放的歌曲");
                self.set_mode(PlayMode::RepeatAll);
            }
            return;
        }
        self.heart_filled = true;
        self.queue.truncate(self.at + 1);
        let played: std::collections::HashSet<i64> = self.queue.iter().map(|song| song.id).collect();
        self.queue.extend(songs.into_iter().filter(|song| !played.contains(&song.id)));
        self.publish_queue();
    }

    /// Back to the list played before the heart mode, at the playing song.
    fn leave_heart(&mut self) {
        let Some((mut queue, list)) = self.before_heart.take() else { return };
        let playing = self.queue.get(self.at).cloned();
        let at = match playing {
            Some(song) => queue.iter().position(|one| one.id == song.id).unwrap_or_else(|| {
                queue.insert(0, song);
                0
            }),
            None => 0,
        };
        self.queue = queue;
        self.at = at;
        self.queue_list = list;
        self.play_next.clear();
    }

    /// The song was moved to `position`, which it reaches soon.
    fn moved_to(&mut self, position: f64) {
        self.position = position;
        self.seeking = Some((position, Instant::now()));
    }

    // MARK: Listening together

    /// What plays here as the room is told it, if it is on NetEase.
    fn room_playback(&self) -> Option<together::Playback> {
        let song = self.queue.get(self.at).filter(|song| song.id > 0)?;
        Some(together::Playback { song: song.id, playing: !self.paused, progress: (self.position * 1000.0) as i64 })
    }

    /// The account may be in a room already, from here or elsewhere.
    fn find_room(&mut self) {
        if self.uid <= 0 || self.room.is_some() || !self.shared.read(|s| s.together && s.netease) {
            return;
        }
        let jobs = self.jobs.clone();
        tokio::spawn(async move {
            let job = match room_call(together::STATUS, json!({})).await {
                Ok(data) if data["inRoom"].as_bool() == Some(true) => {
                    let inviter = data["roomInfo"]["creatorId"].as_i64().unwrap_or(0);
                    Job::TogetherEntered { info: data["roomInfo"].clone(), inviter, fresh: false }
                }
                _ => Job::TogetherNotEntered(None),
            };
            let _ = jobs.send(job);
        });
    }

    /// Join the room of `invitation`, or start one.
    fn enter_room(&mut self, invitation: Option<(String, i64)>) {
        if self.uid <= 0 {
            return self.shared.fail("登录后才能一起听");
        }
        if self.room.is_some() {
            return;
        }
        self.shared.update(|s| s.together_busy = true);
        let (jobs, me) = (self.jobs.clone(), self.uid);
        tokio::spawn(async move {
            let job = async {
                let status = room_call(together::STATUS, json!({})).await.map_err(|(_, message)| message)?;
                if status["inRoom"].as_bool() == Some(true) {
                    let inviter = status["roomInfo"]["creatorId"].as_i64().unwrap_or(me);
                    return Ok(Job::TogetherEntered { info: status["roomInfo"].clone(), inviter, fresh: false });
                }
                match invitation {
                    Some((room, inviter)) => {
                        room_call(together::ACCEPT, json!({ "refer": "inbox_invite", "roomId": room, "inviterId": param(inviter) })).await.map_err(|(_, message)| message)?;
                        let status = room_call(together::STATUS, json!({})).await.map_err(|(_, message)| message)?;
                        Ok(Job::TogetherEntered { info: status["roomInfo"].clone(), inviter, fresh: false })
                    }
                    None => {
                        let created = room_call(together::CREATE, json!({ "refer": "songplay_more" })).await.map_err(|(_, message)| message)?;
                        Ok(Job::TogetherEntered { info: created["roomInfo"].clone(), inviter: me, fresh: true })
                    }
                }
            }
            .await
            .unwrap_or_else(|message: String| Job::TogetherNotEntered(Some(message)));
            let _ = jobs.send(job);
        });
    }

    fn room_entered(&mut self, info: &Value, inviter: i64, fresh: bool) {
        let id = info["roomId"].as_str().map(str::to_string).or_else(|| info["roomId"].as_i64().map(|id| id.to_string())).unwrap_or_default();
        if id.is_empty() {
            self.shared.update(|s| s.together_busy = false);
            return self.shared.fail("没有拿到房间信息");
        }
        let creator = info["creatorId"].as_i64().unwrap_or(inviter);
        let (reports, mut outgoing) = mpsc::unbounded_channel::<together::Report>();
        let jobs = self.jobs.clone();
        tokio::spawn(async move {
            while let Some((path, body)) = outgoing.recv().await {
                if let Ok(data) = room_call(path, body).await
                    && path == together::HEARTBEAT
                {
                    let _ = jobs.send(Job::TogetherHeartbeat(data));
                }
            }
        });
        self.room_reports = Some(reports);
        self.room = Some(together::Room::new(id.clone(), creator, self.uid));
        let song = self.queue.get(self.at).map_or(0, |song| song.id);
        let link = together::share_link(&id, inviter, song);
        self.shared.update(|s| {
            s.together_busy = false;
            s.together_room = id;
            s.together_link = link;
        });
        self.show_members(info);
        if fresh {
            self.report_to_room(false);
        }
    }

    fn show_members(&mut self, info: &Value) {
        let me = self.uid;
        let users: Vec<TogetherUser> = info["roomUsers"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|user| TogetherUser {
                id: user["userId"].as_i64().unwrap_or(0),
                name: user["nickname"].as_str().unwrap_or("").to_string(),
                avatar: convert::cover(user["avatarUrl"].as_str().unwrap_or(""), 120),
            })
            .collect();
        self.shared.update(|s| {
            s.together_alone = !users.iter().any(|user| user.id != me);
            s.together_users = users;
        });
    }

    /// Say goodbye to the room, which ends it for both sides.
    fn leave_room(&mut self) {
        let Some(room) = self.room.take() else { return };
        self.room_reports = None;
        self.room_pending = None;
        self.room_song_ended = None;
        self.room_reading = false;
        tokio::spawn(async move {
            let _ = room_call(together::END, json!({ "roomId": room.id })).await;
        });
        self.clear_room();
    }

    fn clear_room(&mut self) {
        self.shared.update(|s| {
            s.together_room.clear();
            s.together_link.clear();
            s.together_users.clear();
            s.together_alone = false;
            s.together_struggling = false;
        });
    }

    /// Something changed here: tell the room what it needs to know.
    fn report_to_room(&mut self, seek: bool) {
        if self.following || self.room.is_none() {
            return;
        }
        let Some(now) = self.room_playback() else { return };
        let list: Vec<i64> = self.queue.iter().map(|song| song.id).filter(|id| *id > 0).collect();
        let mode = self.shared.read(|s| together::mode_name(&s.mode));
        let Some(room) = &mut self.room else { return };
        for report in room.report(now, seek, &list, mode) {
            if let Some(reports) = &self.room_reports {
                let _ = reports.send(report);
            }
        }
    }

    /// Read the room when it is time, and do what waits on the song.
    fn room_tick(&mut self) {
        if let Some((token, command, received)) = self.room_pending.clone()
            && token == self.token
            && !self.shared.read(|s| s.buffering)
        {
            self.room_pending = None;
            self.follow_room(command, received);
        }
        if self.room_song_ended.is_some_and(|at| at.elapsed() >= ROOM_MOVES_ON) {
            self.room_song_ended = None;
            self.advance(true, true);
        }
        let now = self.room_playback();
        let Some(room) = &mut self.room else { return };
        if room.read_due() && !self.room_reading {
            self.room_reading = true;
            room.reading();
            let (jobs, id) = (self.jobs.clone(), room.id.clone());
            tokio::spawn(async move {
                let answer = room_call(together::READ, json!({ "roomId": id })).await.map_err(|(code, _)| code);
                let _ = jobs.send(Job::TogetherRead(answer));
            });
        }
        if room.members_due() {
            room.reading_members();
            let jobs = self.jobs.clone();
            tokio::spawn(async move {
                if let Ok(data) = room_call(together::STATUS, json!({})).await {
                    let info = (data["inRoom"].as_bool() == Some(true)).then(|| data["roomInfo"].clone());
                    let _ = jobs.send(Job::TogetherMembers(info));
                }
            });
        }
        if let Some(now) = now
            && let Some(report) = room.heartbeat(&now)
            && let Some(reports) = &self.room_reports
        {
            let _ = reports.send(report);
        }
    }

    fn room_members(&mut self, info: Option<Value>) {
        let id = self.room.as_ref().map(|room| room.id.clone());
        match info {
            Some(info) if id.as_deref() == info["roomId"].as_str() => self.show_members(&info),
            _ if id.is_some() => self.room_over(),
            _ => {}
        }
    }

    /// The room ended without us leaving it.
    fn room_over(&mut self) {
        self.room = None;
        self.room_reports = None;
        self.room_pending = None;
        self.room_reading = false;
        self.clear_room();
        self.shared.tell("一起听已结束");
    }

    fn room_read(&mut self, answer: Result<Value, i64>) {
        self.room_reading = false;
        let Some(room) = &mut self.room else { return };
        let data = match answer {
            Ok(data) => data,
            // The room or the sign-in is gone.
            Err(301 | 488) => return self.room_over(),
            Err(_) => {
                room.failed();
                let struggling = room.struggling();
                return self.shared.update(|s| s.together_struggling = struggling);
            }
        };
        let news = room.read(&data);
        self.shared.update(|s| s.together_struggling = false);
        let received = now_ms();
        if let Some(mode) = news.mode.as_deref() {
            let mode = match mode {
                "SINGLE_LOOP" => PlayMode::RepeatOne,
                "RANDOM" => PlayMode::Shuffle,
                _ => PlayMode::RepeatAll,
            };
            if self.shared.read(|s| s.mode != mode && s.mode != PlayMode::Heart) {
                self.following = true;
                self.set_mode(mode);
                self.following = false;
            }
        }
        let here: Vec<i64> = self.queue.iter().map(|song| song.id).collect();
        match news.list.filter(|list| !list.is_empty() && *list != here) {
            Some(list) => {
                let (client, jobs, command) = (enji_kit::client(), self.jobs.clone(), news.command);
                tokio::spawn(async move {
                    let songs = songs_of(&client, &list).await;
                    let _ = jobs.send(Job::TogetherList { songs, command, received });
                });
            }
            None => {
                if let Some(command) = news.command {
                    self.follow_room(command, received);
                }
            }
        }
    }

    /// Do what the room's last command says.
    fn follow_room(&mut self, command: together::Command, received: i64) {
        self.room_song_ended = None;
        let target = command.playback.song;
        // What really plays: the queue may have just moved to it.
        if self.shared.read(|s| s.current_id) != target {
            match self.queue.iter().position(|song| song.id == target) {
                Some(index) => {
                    self.following = true;
                    self.start(index);
                    self.following = false;
                    self.room_pending = Some((self.token, command, received));
                }
                None => {
                    let (client, jobs) = (enji_kit::client(), self.jobs.clone());
                    tokio::spawn(async move {
                        let song = songs_of(&client, &[target]).await.into_iter().next();
                        let _ = jobs.send(Job::TogetherSong { song, command, received });
                    });
                }
            }
            return;
        }
        if self.shared.read(|s| s.buffering) {
            self.room_pending = Some((self.token, command, received));
            return;
        }
        self.following = true;
        let position = command.position(now_ms(), received);
        if (position - self.position).abs() > together::CLOSE_ENOUGH {
            self.audio.seek(position);
            self.show_now_playing(Some(position));
            self.moved_to(position);
        }
        if command.playback.playing == self.paused {
            self.toggle();
        }
        self.following = false;
    }

    // MARK: The listening record

    fn begin_listening(&mut self, song: &Song) {
        // Local songs, below 0, are not on NetEase.
        if song.id > 0 && self.uid > 0 && self.shared.read(|s| s.netease && s.report_listening) {
            self.listens.begin(song.id);
            self.report_listens();
        }
    }

    fn finish_listening(&mut self) {
        if self.listens.finish() {
            self.report_listens();
        }
    }

    /// Send NetEase what waits, one request at a time.
    fn report_listens(&mut self) {
        self.reported_at = Instant::now();
        if self.uid <= 0 || !self.shared.read(|s| s.netease && s.report_listening) {
            return;
        }
        let Some(cookie) = store::cookie() else { return };
        let Some(logs) = self.listens.next_batch() else { return };
        let (http, jobs) = (self.http.clone(), self.jobs.clone());
        tokio::spawn(async move {
            let taken = send_listens(&http, &cookie, logs).await;
            let _ = jobs.send(Job::Reported(taken));
        });
    }

    // MARK: Likes

    fn load_likes(&self) {
        let (client, jobs, uid) = (enji_kit::client(), self.jobs.clone(), self.uid);
        tokio::spawn(async move {
            if let Ok(r) = client.likelist(&Query::new().param("uid", &param(uid))).await {
                let ids = r.body["ids"].as_array().map(|all| all.iter().filter_map(Value::as_i64).collect()).unwrap_or_default();
                let _ = jobs.send(Job::Liked(ids));
            }
        });
    }

    /// Like the playing song, or no longer: shown at once, undone if
    /// NetEase refuses.
    fn toggle_like(&mut self) {
        let (song, signed_in) = self.shared.read(|s| (s.current_id, s.login == Login::In));
        // Local songs, below 0, are not on NetEase.
        if song <= 0 {
            return;
        }
        if !signed_in {
            return self.shared.fail("登录后才能收藏歌曲");
        }
        let like = !self.liked.contains(&song);
        if like {
            self.liked.insert(song);
        } else {
            self.liked.remove(&song);
        }
        self.shared.update(|s| s.current_liked = like);
        let (client, shared, jobs, uid) = (enji_kit::client(), self.shared.clone(), self.jobs.clone(), self.uid);
        tokio::spawn(async move {
            let query = Query::new().param("id", &param(song)).param("like", if like { "true" } else { "false" });
            let refused = match client.like(&query).await {
                Ok(r) => r.body["code"].as_i64() != Some(200),
                Err(_) => true,
            };
            if refused {
                shared.fail(if like { "收藏没有成功" } else { "取消收藏没有成功" });
            }
            // What NetEase has, either way.
            if let Ok(r) = client.likelist(&Query::new().param("uid", &param(uid))).await {
                let ids = r.body["ids"].as_array().map(|all| all.iter().filter_map(Value::as_i64).collect()).unwrap_or_default();
                let _ = jobs.send(Job::Liked(ids));
            }
        });
    }

    // MARK: Comments and details

    /// What the panel beside the cover shows, loaded for the playing song.
    fn load_panel(&mut self) {
        let (panel, song) = self.shared.read(|s| (s.panel.clone(), s.current.clone()));
        let Some(song) = song.filter(|song| song.id > 0) else { return };
        match panel {
            Panel::Comments if self.comments_for != song.id => {
                self.comments_for = song.id;
                self.comments_page = 1;
                self.load_comments(song.id, 1);
            }
            Panel::Details if self.details_for != song.id => {
                self.details_for = song.id;
                self.load_details(&song);
            }
            _ => {}
        }
    }

    fn load_comments(&self, song: i64, page: u32) {
        let client = enji_kit::client();
        let shared = self.shared.clone();
        shared.update(|s| s.comments_loading = true);
        tokio::spawn(async move {
            let query = Query::new()
                .param("type", "0")
                .param("id", &param(song))
                .param("sortType", "2")
                .param("pageNo", &param(page))
                .param("pageSize", "30");
            let found = client.comment_new(&query).await;
            let playing = |s: &State| s.current.as_ref().is_some_and(|c| c.id == song);
            match found {
                Ok(r) => shared.update(|s| {
                    if !playing(s) {
                        return;
                    }
                    let data = &r.body["data"];
                    let more: Vec<_> = data["comments"]
                        .as_array()
                        .map(|all| all.iter().map(convert::comment).collect())
                        .unwrap_or_default();
                    if page == 1 {
                        s.comments = more;
                    } else {
                        s.comments.extend(more);
                    }
                    s.comments_total = data["totalCount"].as_i64().unwrap_or(s.comments.len() as i64);
                    s.comments_more = data["hasMore"].as_bool().unwrap_or(false);
                    s.comments_loading = false;
                }),
                Err(e) => {
                    shared.update(|s| s.comments_loading = false);
                    shared.fail(format!("评论加载失败：{e}"));
                }
            }
        });
    }

    fn load_details(&self, song: &Song) {
        let client = enji_kit::client();
        let shared = self.shared.clone();
        let (id, album, artist) = (song.id, param(song.album_id), param(song.artist_id));
        shared.update(|s| s.details_loading = true);
        tokio::spawn(async move {
            let album_query = Query::new().param("id", &album);
            let artist_query = Query::new().param("id", &artist);
            let (album, artist) = tokio::join!(client.album(&album_query), client.artists(&artist_query));
            let album = album.map(|r| r.body["album"].clone()).unwrap_or(Value::Null);
            let artist = artist.map(|r| r.body["artist"].clone()).unwrap_or(Value::Null);
            shared.update(|s| {
                if s.current.as_ref().is_some_and(|c| c.id == id) {
                    s.details = Some(convert::details(&album, &artist));
                    s.details_loading = false;
                }
            });
        });
    }

    fn toggle(&mut self) {
        let (has_song, buffering) = self.shared.read(|s| (s.current.is_some(), s.buffering));
        if !has_song || buffering {
            return;
        }
        // The song left last time has yet to load.
        if let Some(position) = self.left_at {
            self.start(self.at);
            self.start_at = Some(position);
            self.resume.place(self.at, position);
            return;
        }
        self.paused = !self.paused;
        if self.paused {
            self.resume.place(self.at, self.position);
        }
        if self.paused {
            self.audio.pause();
        } else {
            self.audio.resume();
        }
        let playing = !self.paused;
        self.shared.update(|s| s.playing = playing);
        self.report_to_room(false);
        self.show_now_playing(None);
        self.show_desktop_lyrics();
        let position = self.position;
        self.send_plugins("playback", json!({ "playing": playing, "position": position }));
        // Playing on ends browsing the lyrics.
        if playing && !self.scrolling {
            self.settle(None);
        }
    }

    /// Back from browsing: the lines blurred again and the lyrics at the
    /// sung line, or at `line` when one was chosen.
    fn settle(&mut self, line: Option<i64>) {
        self.shared.update(|s| {
            if let Some(line) = line {
                s.line = line;
            }
            s.browsing = false;
            dress(&mut s.lyrics, s.line, false);
            sung(s);
            s.scroll_to = s.line;
            s.scroll_tick += 1;
        });
        self.desktop_lyrics();
    }

    /// Where the song is, its lyric line, and the next song when it ends.
    fn tick(&mut self) {
        // Media keys, headphones, Control Center and the like.
        for command in bern::now_playing::take_commands() {
            use bern::now_playing::Command;
            match command {
                Command::Play if self.paused => self.toggle(),
                Command::Pause if !self.paused => self.toggle(),
                Command::Play | Command::Pause => {}
                Command::Toggle => self.toggle(),
                Command::Next => self.handle(Job::Next),
                Command::Previous => self.handle(Job::Previous),
                Command::Seek(seconds) => {
                    let length = self.shared.read(|s| s.current.as_ref().map_or(0.0, |song| song.seconds));
                    if length > 0.0 {
                        self.handle(Job::Seek(seconds / length));
                    }
                }
            }
        }
        // The desktop lyrics' bar; plugins' overlays have none.
        for (overlay, button) in bern::overlay::take_presses() {
            if overlay != "lyrics" {
                continue;
            }
            match button.as_str() {
                "previous" => self.handle(Job::Previous),
                "toggle" => self.toggle(),
                "next" => self.handle(Job::Next),
                "lock" => {
                    let locked = self.shared.read(|s| s.desktop_lyrics_locked);
                    self.handle(Job::SetDesktopLyricsLocked(!locked));
                }
                "close" => self.handle(Job::SetDesktopLyrics(false)),
                _ => {}
            }
        }
        // Three seconds after the last scrolling by hand, while playing,
        // the lyrics go back to the sung line.
        let browsing = self.shared.read(|s| s.browsing);
        if browsing && !self.scrolling && !self.paused && self.scrolled_at.elapsed() >= Duration::from_secs(3) {
            self.settle(None);
        }
        let status = self.audio.status();
        if let Some(message) = status.error {
            self.shared.fail(message);
        }
        if self.shared.read(|s| (s.usb_plugged, s.usb_refused, &s.usb_format) != (status.usb_plugged, status.usb_refused, &status.usb_format)) {
            self.shared.update(|s| {
                s.usb_plugged = status.usb_plugged;
                s.usb_refused = status.usb_refused;
                s.usb_format = status.usb_format.clone();
            });
        }
        if status.ended == Some(self.token) {
            self.finish_listening();
            // In a room one side moves it on, and the other follows.
            if self.room.as_ref().is_some_and(|room| !room.leads()) {
                self.room_song_ended = Some(Instant::now());
            } else {
                return self.advance(true, true);
            }
        }
        self.room_tick();
        // What could not be sent is tried again now and then.
        if self.reported_at.elapsed() >= REPORT_AGAIN {
            self.report_listens();
        }
        // The song left last time has not loaded: the player has no place.
        if self.left_at.is_some() {
            return;
        }
        // A seek takes a while where the song has not downloaded yet: until
        // then the player still says where it was.
        if let Some((target, at)) = self.seeking {
            if (status.position - target).abs() > SEEK_ARRIVED && at.elapsed() < SEEK_WAIT {
                return;
            }
            self.seeking = None;
        }
        if (status.position - self.position).abs() < 0.05 {
            return;
        }
        self.position = status.position;
        self.listens.heard(status.position);
        self.resume.heard(self.at, status.position);
        let line = self
            .lines
            .iter()
            .rposition(|line| line.time <= status.position + 0.25)
            .unwrap_or(0) as i64;
        let position = status.position;
        // Played long enough to count once.
        let (id, length) = self.shared.read(|s| s.current.as_ref().map_or((0, 0.0), |song| (song.id, song.seconds)));
        if self.counted != self.token && id != 0 && length > 0.0 && position >= crate::plays::threshold(length) {
            self.counted = self.token;
            crate::plays::played(id);
            self.show_counts();
        }
        let (length, line_changed) =
            self.shared.read(|s| (s.current.as_ref().map_or(0.0, |song| song.seconds), s.line != line));
        let lit = self.lit_at(line, position);
        self.shared.live(|live| {
            live.progress = if length > 0.0 { (position / length).clamp(0.0, 1.0) } else { 0.0 };
            live.elapsed = convert::clock(position);
            if line_changed {
                live.lit = lit;
            }
        });
        if line_changed {
            self.shared.update(|s| {
                s.line = line;
                dress(&mut s.lyrics, line, s.browsing);
                sung(s);
                // Browsed lyrics stay where the person put them.
                if !s.browsing {
                    s.scroll_to = line;
                    s.scroll_tick += 1;
                }
            });
            self.desktop_lyrics();
        }
    }
}

/// The lines a TV shows above the sung one, and from it on.
const NEAR_SHOWN_BEFORE: usize = 1;
const NEAR_SHOWN_AFTER: usize = 5;

/// Lines above and below the sung one that are near it: about half of
/// those a lyrics page shows over it, with the sung line a third of the way
/// down, and under it.
const NEAR_ABOVE: i64 = 2;
const NEAR_BELOW: i64 = 3;

/// The sung line, and how blurred each line is: beside the sung one the
/// half of the lines above and below that is next to it a little, the half
/// towards the edges more; while browsing, none.
fn dress(lines: &mut [LyricLine], sung: i64, browsing: bool) {
    for line in lines {
        line.current = line.key == sung;
        let near = if line.key < sung { NEAR_ABOVE } else { NEAR_BELOW };
        line.blur = match (line.key - sung).abs() {
            _ if browsing => 0.0,
            0 => 0.0,
            d if d <= near => 1.2,
            _ => 3.5,
        };
    }
}

/// NetEase's songs for a local song's name and singers; `None` when the
/// search failed.
async fn search_songs(client: &ApiClient, title: &str, artists: &str, limit: usize) -> Option<Vec<Value>> {
    let words = format!("{title} {artists}");
    let query = Query::new().param("keywords", words.trim()).param("type", "1").param("limit", &limit.to_string());
    let r = client.cloudsearch(&query).await.ok()?;
    Some(r.body["result"]["songs"].as_array().cloned().unwrap_or_default())
}

/// The one of `found` that is the local song: the same name and first
/// singer, about as long.
fn best_match<'a>(found: &'a [Value], title: &str, artists: &str, seconds: f64) -> Option<&'a Value> {
    let wanted = enji_kit::copies::name_key(title, artists);
    found.iter().find(|one| {
        let singers: Vec<&str> = one["ar"].as_array().map(|all| all.iter().filter_map(|a| a["name"].as_str()).collect()).unwrap_or_default();
        let name = one["name"].as_str().unwrap_or("");
        let length = one["dt"].as_f64().unwrap_or(0.0) / 1000.0;
        let near = seconds <= 0.0 || length <= 0.0 || (length - seconds).abs() < 3.0;
        // Without singers in its tags, the name and length must do.
        let same = if artists.trim().is_empty() {
            enji_kit::copies::name_key(name, "") == enji_kit::copies::name_key(title, "")
        } else {
            singers.iter().any(|singer| enji_kit::copies::name_key(name, singer) == wanted)
        };
        same && near
    })
}

fn found_of(one: &Value) -> Found {
    Found {
        id: one["id"].as_i64().unwrap_or(0),
        album: one["al"]["id"].as_i64().unwrap_or(0),
        artist: one["ar"][0]["id"].as_i64().unwrap_or(0),
        cover: one["al"]["picUrl"].as_str().unwrap_or("").to_string(),
    }
}

/// A track known to be `found` on NetEase, or no NetEase song.
fn apply_match(track: &mut local::Track, found: Option<Found>, how: &str) {
    match found {
        Some(found) if found.id > 0 => {
            track.netease_id = found.id;
            track.netease_album = found.album;
            track.netease_artist = found.artist;
            if !found.cover.is_empty() {
                track.online_cover = found.cover;
            }
            track.matched = how.to_string();
        }
        _ => {
            track.netease_id = 0;
            track.netease_album = 0;
            track.netease_artist = 0;
            if how != "auto" {
                // The cover came with the song it was wrongly matched to.
                track.online_cover.clear();
            }
            track.matched = "none".into();
        }
    }
}

/// The cookies NetEase set, as one header.
fn cookie_of(cookies: &[String]) -> String {
    cookies
        .iter()
        .filter_map(|c| c.split(';').next())
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
}

/// What NetEase said when it refused, or `fallback`.
fn refusal(body: &Value, fallback: &str) -> String {
    body["message"]
        .as_str()
        .or(body["msg"].as_str())
        .or(body["body"]["message"].as_str())
        .filter(|text| !text.is_empty())
        .map_or_else(|| fallback.to_string(), |text| format!("{fallback}：{text}"))
}

/// The sung line, its translation and the next line with words, which a
/// phone shows under the cover.
fn sung(s: &mut State) {
    let at = s.lyrics.iter().position(|line| line.key == s.line);
    let now = at.map(|i| &s.lyrics[i]).filter(|line| !line.text.trim().is_empty());
    let next = at.and_then(|i| s.lyrics[i + 1..].iter().find(|line| !line.text.trim().is_empty()));
    let (text, translation) = now.map_or_else(Default::default, |line| (line.text.clone(), line.translation.clone()));
    let next = next.map(|line| line.text.clone()).unwrap_or_default();
    s.lyric_now = text;
    s.lyric_now_translation = translation;
    s.lyric_next = next;
    let sung_at = at.unwrap_or(0);
    let words = |line: &&LyricLine| !line.text.trim().is_empty();
    let before: Vec<LyricLine> = s.lyrics[..sung_at].iter().rev().filter(words).take(NEAR_SHOWN_BEFORE).cloned().collect();
    let after = s.lyrics[sung_at..].iter().filter(words).take(NEAR_SHOWN_AFTER).cloned();
    s.lyrics_near = before.into_iter().rev().chain(after).collect();
}

/// How NetEase streams a song, such as `FLAC · 900 kbps`, from the address
/// it handed over.
fn streamed(data: &serde_json::Value) -> String {
    let kind = data["type"].as_str().unwrap_or("").to_uppercase();
    let kbps = data["br"].as_i64().unwrap_or(0) / 1000;
    let mut parts = Vec::new();
    if !kind.is_empty() {
        parts.push(kind);
    }
    if kbps > 0 {
        parts.push(format!("{kbps} kbps"));
    }
    parts.join(" · ")
}

/// Lines of lyrics as the lyrics page shows them.
fn shown_lines(lines: &[lyrics::Line]) -> Vec<LyricLine> {
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| LyricLine {
            key: i as i64,
            time: line.time,
            text: line.text.clone(),
            translation: line.translation.clone().unwrap_or_default(),
            translated: line.translation.is_some(),
            current: false,
            blur: 0.0,
        })
        .collect()
}

/// `songs` numbered as a list of their own.
/// The account's playlists apart: the songs it likes, the lists it made and
/// those it saved from others.
/// The playlist `id` and the ids of its songs. `playlist_detail` sends the
/// first thousand songs too, which take most of its time.
async fn outline(client: &ApiClient, id: i64) -> Result<Value, NcmError> {
    let option = RequestOption { crypto: CryptoType::Eapi, ..Default::default() };
    let r = client.request("/api/v6/playlist/detail", json!({ "id": param(id), "n": 0, "s": 8 }), option).await?;
    Ok(r.body["playlist"].clone())
}

/// The first year NetEase keeps a report of at this address.
const FIRST_REPORT: i32 = 2023;

/// The annual report of `year`.
fn report_address(year: i64) -> String {
    format!("https://y.music.163.com/st/year{year}/?full_screen=true")
}

/// The years with a report, the latest first. The page of a year NetEase
/// has yet to report on, until around the end of December, has no title.
async fn report_years(http: &reqwest::Client) -> Vec<i64> {
    let this_year = chrono::Datelike::year(&chrono::Local::now());
    let mut years = Vec::new();
    for year in (FIRST_REPORT..=this_year).rev().map(i64::from) {
        let page = http.get(report_address(year)).send().await;
        let up = match page {
            Ok(page) => page.text().await.is_ok_and(|text| text.contains("<title>")),
            Err(_) => false,
        };
        if up {
            years.push(year);
        }
    }
    years
}

/// How many songs the account page ranks, and medals it shows.
const TOP_SONGS: usize = 8;
const MEDALS: usize = 16;

/// What the account page shows, read together.
pub struct AccountRead {
    profile: Option<Profile>,
    years: Vec<ListenYear>,
    top_all: Vec<Song>,
    top_week: Vec<Song>,
    medals: Vec<Medal>,
    medal_count: i64,
}

/// `1234567` as it reads: `1,234,567`.
fn grouped(number: i64) -> String {
    let digits = number.abs().to_string();
    let mut text = String::new();
    for (place, digit) in digits.chars().enumerate() {
        if place > 0 && (digits.len() - place).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    if number < 0 { format!("-{text}") } else { text }
}

/// What NetEase says of the account `id`, which has listened for `seconds`:
/// `user_detail`, `user_level`, `user_subcount` and `vip_info`.
fn profile_of(id: i64, detail: &Value, seconds: i64, level: &Value, counts: &Value, vip: &Value) -> Profile {
    let profile = &detail["profile"];
    let number = |value: &Value| grouped(value.as_i64().unwrap_or(0));
    let days = detail["createDays"].as_i64().unwrap_or(0);
    let member = vip["data"]["redVipLevel"].as_i64().unwrap_or(0) > 0;
    let until = vip["data"]["associator"]["expireTime"].as_i64().unwrap_or(0);
    let date = chrono::DateTime::from_timestamp_millis(until)
        .filter(|_| member)
        .map(|date| date.with_timezone(&chrono::Local).format("%Y.%m.%d").to_string());
    let badge = vip["data"]["redVipLevelIcon"].as_str().filter(|_| member).unwrap_or("");
    Profile {
        id,
        signature: profile["signature"].as_str().unwrap_or("").trim().to_string(),
        level: detail["level"].as_i64().unwrap_or(0),
        songs: number(&detail["listenSongs"]),
        hours: grouped(seconds / 3600),
        years: format!("{:.1}", days as f64 / 365.25),
        follows: number(&profile["follows"]),
        fans: number(&profile["followeds"]),
        events: number(&profile["eventCount"]),
        own_playlists: number(&counts["createdPlaylistCount"]),
        saved_playlists: number(&counts["subPlaylistCount"]),
        artists: number(&counts["artistCount"]),
        level_songs: number(&level["data"]["nowPlayCount"]),
        level_goal: number(&level["data"]["nextPlayCount"]),
        level_progress: level["data"]["progress"].as_f64().unwrap_or(0.0).clamp(0.0, 1.0),
        vip_badge: MediaSource::new(secure(badge)),
        vip_until: date.unwrap_or_default(),
    }
}

/// Pictures come by `https`, as iOS asks of them.
fn secure(url: &str) -> String {
    url.strip_prefix("http://").map_or_else(|| url.to_string(), |rest| format!("https://{rest}"))
}

/// The years of `listen_data_year_report`, and any with only a report.
fn listen_years(report: &Value, reports: &[i64]) -> Vec<ListenYear> {
    let items = report["data"]["yearItems"].as_array().cloned().unwrap_or_default();
    let seconds = |year: i64| {
        items.iter().find(|item| item["year"].as_i64() == Some(year)).and_then(|item| item["playDuration"].as_i64()).unwrap_or(0)
    };
    let mut years: Vec<i64> = items.iter().filter_map(|item| item["year"].as_i64()).chain(reports.iter().copied()).collect();
    years.sort_unstable_by(|a, b| b.cmp(a));
    years.dedup();
    let longest = years.iter().map(|&year| seconds(year)).max().unwrap_or(0).max(1);
    years
        .into_iter()
        .map(|year| ListenYear {
            year,
            name: year.to_string(),
            hours: grouped(seconds(year) / 3600),
            share: seconds(year) as f64 / longest as f64,
            has_report: reports.contains(&year),
        })
        .collect()
}

/// The songs of `user_record`, each with how often it played.
fn top_songs(records: &Value) -> Vec<Song> {
    let records = records.as_array().map_or(&[][..], Vec::as_slice);
    records
        .iter()
        .take(TOP_SONGS)
        .enumerate()
        .map(|(index, record)| {
            let mut song = convert::song(&record["song"], index, None);
            song.plays = record["playCount"].as_i64().unwrap_or(0);
            song
        })
        .collect()
}

/// The first of the medals of `user_medal`.
fn medals_of(medals: &Value) -> Vec<Medal> {
    let list = medals["data"]["obtainMedals"].as_array().map_or(&[][..], Vec::as_slice);
    list.iter()
        .take(MEDALS)
        .enumerate()
        .filter_map(|(index, medal)| {
            let picture = medal["medalPicUrl"].as_str().filter(|url| !url.is_empty())?;
            Some(Medal {
                id: index as i64,
                name: medal["medalName"].as_str().unwrap_or("").to_string(),
                picture: MediaSource::new(secure(picture)),
            })
        })
        .collect()
}

/// Send `logs` as the web player does: whether NetEase took them.
async fn send_listens(http: &reqwest::Client, cookie: &str, logs: Vec<Value>) -> bool {
    let csrf = cookie.split(';').find_map(|part| part.trim().strip_prefix("__csrf=")).unwrap_or("");
    let form = ncm_api_rs::crypto::weapi(&json!({ "logs": Value::Array(logs).to_string(), "csrf_token": csrf }));
    let answer = http
        .post(format!("{LOG_URL}?csrf_token={csrf}"))
        .header("Cookie", cookie)
        .header("Referer", format!("{LOG_PAGE}/"))
        .header("Origin", LOG_PAGE)
        .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36")
        .form(&form)
        .send()
        .await;
    let Ok(answer) = answer else { return false };
    let text = answer.text().await.unwrap_or_default();
    serde_json::from_str::<Value>(&text).is_ok_and(|said| said["code"].as_i64() == Some(200))
}

/// A call to the room's service: what it answered in `data`, or its code
/// and what to tell the person.
async fn room_call(path: &'static str, body: Value) -> Result<Value, (i64, String)> {
    let crypto = if path == together::STATUS { CryptoType::Weapi } else { CryptoType::Eapi };
    let option = RequestOption { crypto, ..Default::default() };
    match enji_kit::client().request(path, body, option).await {
        Ok(r) if r.body["code"].as_i64() == Some(200) && r.body["data"]["success"].as_bool() != Some(false) => Ok(r.body["data"].clone()),
        Ok(r) => {
            let said = r.body["message"].as_str().or(r.body["data"]["failedMessage"].as_str()).unwrap_or("一起听请求失败");
            Err((r.body["code"].as_i64().unwrap_or(0), said.to_string()))
        }
        Err(NcmError::Api { code, msg }) => Err((code, if msg == "Unknown error" { "一起听请求失败".to_string() } else { msg })),
        Err(NcmError::AuthRequired(_)) => Err((301, "需要重新登录".to_string())),
        Err(_) => Err((0, "网络连接失败".to_string())),
    }
}

/// The songs `ids`, in that order.
async fn songs_of(client: &ApiClient, ids: &[i64]) -> Vec<Song> {
    const PAGE: usize = 250;
    let mut found = HashMap::new();
    for page in ids.chunks(PAGE) {
        let asked = page.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
        if let Ok(r) = client.song_detail(&Query::new().param("ids", &asked)).await {
            for song in convert::songs(&r.body["songs"], &r.body["privileges"]) {
                found.insert(song.id, song);
            }
        }
    }
    renumbered(ids.iter().filter_map(|id| found.remove(id)))
}

/// The songs of the playlist `info`, in its order, with when each went in.
async fn songs_in(client: &ApiClient, info: &Value) -> Result<Vec<Song>, NcmError> {
    // A page of 250 comes back in half the time of a thousand, and a few
    // go at once.
    const PAGE: usize = 250;
    const AT_ONCE: usize = 6;
    let listed: Vec<(i64, i64)> = info["trackIds"]
        .as_array()
        .map(|ids| ids.iter().filter_map(|one| Some((one["id"].as_i64()?, one["at"].as_i64().unwrap_or(0)))).collect())
        .unwrap_or_default();
    let ids: Vec<i64> = listed.iter().map(|(id, _)| *id).collect();
    let mut found = HashMap::new();
    let mut privileges = HashMap::new();
    let mut failure = None;
    for pages in ids.chunks(PAGE * AT_ONCE) {
        let asked: Vec<_> = pages
            .chunks(PAGE)
            .map(|page| {
                let ids = page.iter().map(i64::to_string).collect::<Vec<_>>().join(",");
                let client = client.clone();
                tokio::spawn(async move { client.song_detail(&Query::new().param("ids", &ids)).await })
            })
            .collect();
        for answer in asked {
            match answer.await {
                Ok(Ok(r)) => {
                    for (into, key) in [(&mut found, "songs"), (&mut privileges, "privileges")] {
                        for one in r.body[key].as_array().into_iter().flatten() {
                            if let Some(id) = one["id"].as_i64() {
                                into.insert(id, one.clone());
                            }
                        }
                    }
                }
                Ok(Err(e)) => failure = Some(e),
                Err(_) => {}
            }
        }
    }
    if let Some(e) = failure.filter(|_| found.is_empty()) {
        return Err(e);
    }
    let songs = listed
        .iter()
        .filter_map(|(id, at)| Some((found.get(id)?, privileges.get(id), *at)))
        .enumerate()
        .map(|(i, (one, privilege, at))| Song { added: at, ..convert::song(one, i, privilege) })
        .collect();
    Ok(songs)
}

/// The open playlist `id` showing `info` and its `songs`.
fn show_opened(s: &mut State, id: i64, info: &Value, mut songs: Vec<Song>, uid: i64) {
    s.opened = Some(convert::playlist(info));
    s.opened_description = info["description"].as_str().unwrap_or("").trim().to_string();
    s.opened_owner = info["creator"]["nickname"].as_str().unwrap_or("").to_string();
    s.opened_owner_avatar = convert::cover(info["creator"]["avatarUrl"].as_str().unwrap_or(""), 64);
    s.opened_meta = convert::playlist_meta(info);
    s.opened_owned = uid != 0 && info["creator"]["userId"].as_i64() == Some(uid);
    enji_kit::dress_all(&mut songs);
    arrange(&mut songs, &s.order);
    s.opened_songs = songs;
    s.opened_is_default = s.default_playlist == id;
    s.opened_loading = false;
}

/// What is downloading as the lists show it, and how far the playing song is.
fn downloads_changed(s: &mut State) {
    s.has_downloads = !s.downloads.is_empty();
    let current = s.downloads.iter().find(|download| download.id == s.current_id);
    s.current_downloading = current.is_some();
    s.current_download = current.map_or(0.0, |download| download.progress);
}

fn sort_playlists(s: &mut State) {
    s.liked_list = s.mine.iter().find(|list| list.liked && list.owned).cloned();
    s.created = s.mine.iter().filter(|list| list.owned && !list.liked).cloned().collect();
    s.collected = s.mine.iter().filter(|list| !list.owned).cloned().collect();
    s.has_created = !s.created.is_empty();
    s.has_collected = !s.collected.is_empty();
}

/// `songs` in `order`, numbered again: as the list itself has them, the
/// most played first, by title, by singer, or the latest added first. Ties
/// keep the list's own order.
fn arrange(songs: &mut [Song], order: &SongOrder) {
    let key = |text: &str| text.to_lowercase();
    match order {
        SongOrder::Default => songs.sort_by_key(|song| song.place),
        SongOrder::Plays => songs.sort_by(|a, b| b.plays.cmp(&a.plays).then(a.place.cmp(&b.place))),
        SongOrder::Name => songs.sort_by(|a, b| key(&a.name).cmp(&key(&b.name)).then(a.place.cmp(&b.place))),
        SongOrder::Artist => songs.sort_by(|a, b| {
            key(&a.artists)
                .cmp(&key(&b.artists))
                .then(key(&a.name).cmp(&key(&b.name)))
                .then(a.place.cmp(&b.place))
        }),
        SongOrder::Added => songs.sort_by(|a, b| b.added.cmp(&a.added).then(a.place.cmp(&b.place))),
    }
    for (i, song) in songs.iter_mut().enumerate() {
        song.index = i as i64;
        song.rank = i as i64 + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{grouped, listen_years, medals_of, profile_of, report_address, secure, top_songs};
    use serde_json::{json, Value};
    #[test]
    fn the_profile_reads_what_netease_says_of_the_account() {
        let detail = json!({
            "level": 9,
            "listenSongs": 1991,
            "createDays": 2600,
            "profile": { "signature": " 你好 ", "follows": 12, "followeds": 34 },
        });
        let level = json!({ "data": { "nowPlayCount": 991, "nextPlayCount": 2000, "progress": 0.4955 } });
        let counts = json!({ "createdPlaylistCount": 7, "subPlaylistCount": 10, "artistCount": 45 });
        let vip = json!({ "data": { "redVipLevel": 7, "redVipLevelIcon": "http://p.net/v.png", "associator": { "expireTime": 1_794_412_799_000_i64 } } });
        let profile = profile_of(7, &detail, 11_372_490, &level, &counts, &vip);
        assert_eq!(profile.signature, "你好");
        assert_eq!((profile.level, profile.songs.as_str(), profile.years.as_str()), (9, "1,991", "7.1"));
        assert_eq!((profile.follows.as_str(), profile.fans.as_str()), ("12", "34"));
        assert_eq!(profile.hours, "3,159");
        assert_eq!((profile.level_songs.as_str(), profile.level_goal.as_str()), ("991", "2,000"));
        assert_eq!(profile.vip_badge.url, "https://p.net/v.png");
        assert!(profile.vip_until.starts_with("2026."));
        // Without a membership there is no badge and no date.
        let none = profile_of(7, &detail, 0, &Value::Null, &Value::Null, &Value::Null);
        assert_eq!((none.vip_badge.url.as_str(), none.vip_until.as_str()), ("", ""));
    }

    #[test]
    fn numbers_read_in_thousands() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1,000");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }

    #[test]
    fn years_come_from_the_listening_and_the_reports() {
        let report = json!({ "data": { "yearItems": [
            { "year": 2024, "playDuration": 7200 },
            { "year": 2025, "playDuration": 3600 },
        ] } });
        let years = listen_years(&report, &[2026, 2025]);
        let names: Vec<&str> = years.iter().map(|year| year.name.as_str()).collect();
        assert_eq!(names, ["2026", "2025", "2024"]);
        assert_eq!(years[1].hours, "1");
        assert_eq!(years[1].share, 0.5);
        assert_eq!(years[2].share, 1.0);
        assert_eq!(years.iter().map(|year| year.has_report).collect::<Vec<_>>(), [true, true, false]);
        // Nothing read at all is no years, not a fault.
        assert!(listen_years(&Value::Null, &[]).is_empty());
    }

    #[test]
    fn the_ranking_keeps_how_often_each_song_played() {
        let records: Vec<Value> = (0..20).map(|n| json!({ "playCount": 100 - n, "song": { "id": n + 1, "name": "a" } })).collect();
        let songs = top_songs(&Value::Array(records));
        assert_eq!(songs.len(), super::TOP_SONGS);
        assert_eq!((songs[0].id, songs[0].plays), (1, 100));
        assert!(top_songs(&Value::Null).is_empty());
    }

    #[test]
    fn medals_without_a_picture_are_left_out() {
        let medals = json!({ "data": { "obtainMedals": [
            { "medalName": "a", "medalPicUrl": "http://p.net/a.png" },
            { "medalName": "b" },
        ] } });
        let found = medals_of(&medals);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].picture.url, "https://p.net/a.png");
        assert_eq!(secure("https://p.net/a.png"), "https://p.net/a.png");
    }

    #[test]
    fn a_report_has_its_year_in_the_address() {
        assert_eq!(report_address(2026), "https://y.music.163.com/st/year2026/?full_screen=true");
    }

    use super::{SongOrder, arrange};
    use bern::media::MediaSource;
    use enji_protocol::enji::Song;

    fn song(place: i64, name: &str, artists: &str, plays: i64, added: i64) -> Song {
        Song {
            id: place + 1,
            index: place,
            rank: place + 1,
            name: name.into(),
            artists: artists.into(),
            album: String::new(),
            cover: MediaSource::new(""),
            artwork: MediaSource::new(""),
            seconds: 0.0,
            duration: String::new(),
            unavailable: false,
            album_id: 0,
            artist_id: 0,
            file: String::new(),
            downloaded: false,
            plays,
            added,
            place,
        }
    }

    #[test]
    fn songs_order_by_plays_title_singer_and_date_then_back() {
        let mut songs = vec![
            song(0, "b", "Zed", 1, 300),
            song(1, "A", "amy", 7, 100),
            song(2, "c", "Amy", 7, 200),
        ];
        let names = |songs: &[Song]| songs.iter().map(|song| song.name.clone()).collect::<Vec<_>>();
        arrange(&mut songs, &SongOrder::Plays);
        assert_eq!(names(&songs), ["A", "c", "b"], "ties keep the list's order");
        assert_eq!(songs.iter().map(|song| song.index).collect::<Vec<_>>(), [0, 1, 2]);
        arrange(&mut songs, &SongOrder::Name);
        assert_eq!(names(&songs), ["A", "b", "c"], "case does not count");
        arrange(&mut songs, &SongOrder::Artist);
        assert_eq!(names(&songs), ["A", "c", "b"]);
        arrange(&mut songs, &SongOrder::Added);
        assert_eq!(names(&songs), ["b", "c", "A"], "the latest first");
        arrange(&mut songs, &SongOrder::Default);
        assert_eq!(names(&songs), ["b", "A", "c"]);
        assert_eq!(songs[2].rank, 3);
    }
}

#[cfg(test)]
mod song_url_tests {
    use super::retried;
    use ncm_api_rs::NcmError;
    use std::{cell::Cell, time::Duration};

    const WAITS: [Duration; 3] = [Duration::ZERO; 3];

    fn refused() -> NcmError {
        NcmError::InvalidParam("Unknown error".into())
    }

    #[tokio::test]
    async fn a_refusal_is_tried_again_until_it_passes() {
        let calls = Cell::new(0);
        let answer = retried(&WAITS, || {
            calls.set(calls.get() + 1);
            let call = calls.get();
            async move { if call < 3 { Err(refused()) } else { Ok(call) } }
        })
        .await;
        assert_eq!(answer.unwrap(), 3);
    }

    #[tokio::test]
    async fn a_refusal_that_lasts_comes_back_after_every_try() {
        let calls = Cell::new(0);
        let answer: Result<(), _> = retried(&WAITS, || {
            calls.set(calls.get() + 1);
            async { Err(refused()) }
        })
        .await;
        assert!(matches!(answer, Err(NcmError::InvalidParam(_))));
        assert_eq!(calls.get(), WAITS.len() + 1);
    }

    #[tokio::test]
    async fn other_failures_are_not_tried_again() {
        let calls = Cell::new(0);
        let answer: Result<(), _> = retried(&WAITS, || {
            calls.set(calls.get() + 1);
            async { Err(NcmError::Timeout("slow".into())) }
        })
        .await;
        assert!(matches!(answer, Err(NcmError::Timeout(_))));
        assert_eq!(calls.get(), 1);
    }
}
