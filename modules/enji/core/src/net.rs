//! The worker: a thread with its own async runtime that talks to NetEase,
//! keeps the play queue and drives the audio thread. It is the only one
//! that writes the shared state; each change wakes the engine, which copies
//! it for the interface.

use crate::{
    audio::{self, Audio, Stream},
    convert, downloads, local, lyrics, plugins,
    shuffle::Shuffle,
    store,
};
use bern::{media::MediaSource, wake::Waker};
use enji_kit::{list, renumbered};
use enji_protocol::enji::{
    Appearance, Detail, Download, Folder, Login, Offer, Plugin, LyricLine, Panel, PlayMode, Played, Playlist, Quality, SignInWay, Song,
    SongOrder, Source, State,
};
use ncm_api_rs::{ApiClient, NcmError, Query};
use rand::Rng;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::Read,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, task::JoinHandle};

pub enum Job {
    Refresh,
    SignIn,
    CancelSignIn,
    SetSignInWay(SignInWay),
    SendSmsCode(String),
    SignInWith(Credentials),
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
        // A phone cannot scan a code it shows.
        sign_in_way: if cfg!(any(target_os = "ios", target_os = "android")) { SignInWay::Phone } else { SignInWay::Code },
        signing: false,
        sign_in_error: String::new(),
        sms_wait: 0,
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
        plugins: Vec::new(),
        has_plugins: false,
        offers: Vec::new(),
        has_offers: false,
        offers_loading: false,
        cover_pulse: 0,
        downloads: Vec::new(),
        has_downloads: false,
        format: String::new(),
        lyric_now: String::new(),
        lyric_now_translation: String::new(),
        lyric_next: String::new(),
        progress: 0.0,
        elapsed: "0:00".into(),
        total: "0:00".into(),
        volume: saved["volume"].as_f64().unwrap_or(0.8),
        lyrics: Vec::new(),
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
        desktop_lyrics: saved["desktop_lyrics"].as_bool().unwrap_or(false),
        desktop_lyrics_locked: saved["desktop_lyrics_locked"].as_bool().unwrap_or(false),
        desktop_lyrics_supported: bern::overlay::supported(),
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
        "desktop_lyrics": state.desktop_lyrics,
        "desktop_lyrics_locked": state.desktop_lyrics_locked,
        "plugins": state.plugins.iter().filter(|plugin| plugin.enabled).map(|plugin| plugin.id.clone()).collect::<Vec<_>>(),
        "appearance": format!("{:?}", state.appearance),
        "netease": state.netease,
        "online_info": state.online_info,
        "order": format!("{:?}", state.order),
        "show_plays": state.show_plays,
        "default_playlist": state.default_playlist,
    }));
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
}

impl Shared {
    fn update(&self, change: impl FnOnce(&mut State)) {
        change(&mut self.state.lock().unwrap_or_else(|e| e.into_inner()));
        if let Some(waker) = &self.waker {
            waker.wake();
        }
    }

    fn read<T>(&self, read: impl FnOnce(&State) -> T) -> T {
        read(&self.state.lock().unwrap_or_else(|e| e.into_inner()))
    }

    fn tell(&self, message: impl Into<String>) {
        let message = message.into();
        self.update(|s| {
            s.notice = message;
            s.notice_key += 1;
            s.has_notice = true;
        });
    }

    fn fail(&self, message: impl Into<String>) {
        let message = message.into();
        self.update(|s| {
            s.error = message;
            s.error_key += 1;
            s.has_error = true;
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
    /// What was played, the latest first.
    history: Vec<Entry>,
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
}

/// An account to sign in with, as typed.
pub enum Credentials {
    Sms { phone: String, code: String },
    Password { phone: String, password: String },
    Email { email: String, password: String },
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
    waker: Option<Waker>,
) {
    let shared = Shared { state, waker };
    let volume = shared.read(|s| s.volume) as f32;
    let cookie = store::cookie();
    let mut worker = Worker {
        http: reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko)")
            .build()
            .unwrap_or_default(),
        jobs,
        shared,
        audio: Audio::start(volume),
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
        history: Vec::new(),
        play_opened: None,
        daily: Vec::new(),
        results: Vec::new(),
        local_results: Vec::new(),
        stack: Vec::new(),
        auto_matching: false,
        scanning: false,
        scan_again: false,
        shown_at: Instant::now(),
    };
    worker.load_history();
    if let Some(folder) = store::phone_music_dir() {
        let _ = std::fs::create_dir_all(&folder);
        worker.library.folders = vec![local::Folder::new(local::file_url(&folder))];
        worker.shared.update(|s| s.phone_folder = folder.to_string_lossy().to_string());
    }
    worker.publish_library();
    worker.start_plugins();
    worker.rescan();
    if cookie.is_some() && worker.shared.read(|s| s.netease) {
        worker.fetch_account(false);
    }
    worker.refresh();
    let mut tick = tokio::time::interval(Duration::from_millis(200));
    // Plugins hear the music's loudness and answer more often than that.
    let mut quick = tokio::time::interval(Duration::from_millis(50));
    loop {
        tokio::select! {
            job = receive.recv() => match job {
                Some(job) => worker.handle(job),
                None => break,
            },
            _ = tick.tick() => worker.tick(),
            _ = quick.tick(), if !worker.plugins.is_empty() => worker.plugin_tick(),
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
                let (login, way) = self.shared.read(|s| (s.login.clone(), s.sign_in_way.clone()));
                if login != Login::In {
                    self.sign_in_by(way);
                }
            }
            Job::CancelSignIn => {
                if let Some(task) = self.signing_in.take() {
                    task.abort();
                }
                self.shared.update(|s| {
                    s.login = Login::Out;
                    s.signing = false;
                });
            }
            Job::SetSignInWay(way) => {
                if matches!(self.shared.read(|s| s.login.clone()), Login::Out | Login::In) {
                    self.shared.update(|s| s.sign_in_way = way);
                } else {
                    self.sign_in_by(way);
                }
            }
            Job::SendSmsCode(phone) => self.send_sms_code(&phone),
            Job::SignInWith(credentials) => self.sign_in_with(credentials),
            Job::SignOut => {
                store::forget_cookie();
                enji_kit::set_cookie(None);
                self.uid = 0;
                self.liked.clear();
                self.shared.update(|s| {
                    s.login = Login::Out;
                    s.current_liked = false;
                    s.nickname.clear();
                    s.avatar = MediaSource::new("");
                    s.mine.clear();
                    sort_playlists(s);
                });
            }
            Job::SignedIn(cookie) => {
                if let Err(e) = store::save_cookie(&cookie) {
                    self.shared.fail(format!("无法保存登录状态：{e}"));
                }
                self.signing_in = None;
                self.shared.update(|s| s.signing = false);
                enji_kit::set_cookie(Some(cookie));
                self.fetch_account(true);
            }
            Job::Account(uid) => {
                self.uid = uid;
                self.refresh();
                self.load_likes();
                self.load_plays();
            }
            Job::Counted => self.show_counts(),
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
                    self.set_queue(list, index);
                    self.start(index);
                }
            }
            Job::PlayQueued(index) => {
                if index < self.queue.len() {
                    self.failures = 0;
                    self.start(index);
                }
            }
            Job::NextMode => {
                let mode = self.shared.read(|s| match s.mode {
                    PlayMode::RepeatAll => PlayMode::RepeatOne,
                    PlayMode::RepeatOne => PlayMode::Shuffle,
                    PlayMode::Shuffle => PlayMode::RepeatAll,
                });
                let shuffling = mode == PlayMode::Shuffle;
                self.shared.update(|s| {
                    s.mode = mode;
                    save_settings(s);
                });
                if shuffling && self.queue.len() > 1 {
                    self.shuffle.generate(self.queue.len(), self.at);
                }
                self.publish_queue();
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
            }),
            Job::Downloaded { song, path } => {
                let url = local::file_url(&path);
                self.downloaded.insert(url.clone(), downloads::Origin { id: song.id, album: song.album_id, artist: song.artist_id });
                self.shared.update(|s| {
                    s.downloads.retain(|download| download.id != song.id);
                    s.has_downloads = !s.downloads.is_empty();
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
                    s.has_downloads = !s.downloads.is_empty();
                });
                self.shared.fail(message);
            }
            Job::SetAppearance(appearance) => self.shared.update(|s| {
                s.appearance = appearance;
                save_settings(s);
            }),
            Job::Seek(progress) => {
                let length = self.shared.read(|s| s.current.as_ref().map_or(0.0, |song| song.seconds));
                self.audio.seek(progress.clamp(0.0, 1.0) * length);
                self.show_now_playing(Some(progress.clamp(0.0, 1.0) * length));
            }
            Job::SeekLine(key) => {
                if let Some(line) = self.lines.get(key as usize) {
                    self.audio.seek(line.time);
                    self.show_now_playing(Some(line.time));
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
                self.show_now_playing(Some(0.0));
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
            Source::Artist => s.artist_songs.clone(),
        })
    }

    /// A new queue, `at` playing; Misuzu Music's `setQueue`.
    fn set_queue(&mut self, songs: Vec<Song>, at: usize) {
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
        use bern::overlay::{Line, Overlay};
        let line = self.shared.read(|s| {
            json!({ "text": s.lyric_now, "translation": s.lyric_now_translation, "next": s.lyric_next, "index": s.line })
        });
        self.send_plugins("lyric", line);
        let shown = self.shared.read(|s| {
            let song = s.current.as_ref().filter(|_| s.desktop_lyrics)?;
            let (first, second) = if s.lyric_now.is_empty() {
                (song.name.clone(), song.artists.clone())
            } else if s.show_translation && !s.lyric_now_translation.is_empty() {
                (s.lyric_now.clone(), s.lyric_now_translation.clone())
            } else {
                (s.lyric_now.clone(), s.lyric_next.clone())
            };
            Some((first, second, s.desktop_lyrics_locked))
        });
        let Some((first, second, locked)) = shown else {
            return bern::overlay::hide("lyrics");
        };
        // Phones show it smaller, over another app.
        let (large, small) = if cfg!(target_os = "android") { (20.0, 14.0) } else { (30.0, 18.0) };
        bern::overlay::show(
            "lyrics",
            &Overlay {
                lines: vec![
                    Line { text: first, size: large, bold: true, color: [255, 255, 255, 255] },
                    Line { text: second, size: small, bold: false, color: [255, 255, 255, 215] },
                ],
                place: bern::overlay::Place::Free,
                locked,
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
                    newer: have.is_some_and(|(pack, _)| pack.version != listing.pack.version),
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
        if playing && self.plugins.wants_audio() {
            let (level, bass) = audio::levels();
            let position = self.position;
            self.send_plugins("audio", json!({ "level": level, "bass": bass, "position": position }));
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
                plugins::Call::CoverPulse => self.shared.update(|s| s.cover_pulse += 1),
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
            return;
        }
        self.shared.update(|s| {
            s.downloads.push(Download { id: song.id, name: song.name.clone(), artists: song.artists.clone(), progress: 0.0 });
            s.has_downloads = true;
        });
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
            Detail::Page => None,
        });
        self.shared.update(|s| s.detail = kind.clone());
        if shown != Some(id) {
            match kind {
                Detail::Playlist => self.open(id, None),
                Detail::Album => self.load_album(id),
                Detail::Artist => self.load_artist(id),
                Detail::Page => {}
            }
        }
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
        let name = self.shared.read(|s| s.mine.iter().find(|list| list.id == id).map(|list| list.name.clone()).unwrap_or_default());
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

    /// Open the playlist `id`, showing what is `known` of it until it
    /// arrives.
    fn open(&self, id: i64, known: Option<Playlist>) {
        let client = enji_kit::client();
        let shared = self.shared.clone();
        let (jobs, uid) = (self.jobs.clone(), self.uid);
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
        });
        tokio::spawn(async move {
            let id_text = param(id);
            let detail = client.playlist_detail(&Query::new().param("id", &id_text)).await;
            // When each song went into the list.
            let added: std::collections::HashMap<i64, i64> = detail
                .as_ref()
                .ok()
                .and_then(|d| d.body["playlist"]["trackIds"].as_array().cloned())
                .map(|ids| ids.iter().filter_map(|one| Some((one["id"].as_i64()?, one["at"].as_i64().unwrap_or(0)))).collect())
                .unwrap_or_default();
            // All of its songs, a thousand at a time: NetEase gives no more
            // in one answer.
            const PAGE: usize = 1000;
            let mut songs = Vec::new();
            let mut privileges = Vec::new();
            let mut failure = None;
            loop {
                let page = client
                    .playlist_track_all(
                        &Query::new()
                            .param("id", &id_text)
                            .param("limit", &PAGE.to_string())
                            .param("offset", &songs.len().to_string()),
                    )
                    .await;
                match page {
                    Ok(r) => {
                        let got = r.body["songs"].as_array().cloned().unwrap_or_default();
                        let count = got.len();
                        songs.extend(got);
                        privileges.extend(r.body["privileges"].as_array().cloned().unwrap_or_default());
                        if count < PAGE {
                            break;
                        }
                    }
                    Err(e) => {
                        failure = Some(e);
                        break;
                    }
                }
            }
            let tracks = match failure {
                Some(e) if songs.is_empty() => Err(e),
                _ => Ok(serde_json::json!({ "songs": songs, "privileges": privileges })),
            };
            let still_open = |s: &State| s.opened.as_ref().is_some_and(|p| p.id == id);
            match tracks {
                Ok(body) => shared.update(|s| {
                    if !still_open(s) {
                        return;
                    }
                    if let Ok(d) = &detail {
                        let info = &d.body["playlist"];
                        s.opened = Some(convert::playlist(info));
                        s.opened_description = info["description"].as_str().unwrap_or("").trim().to_string();
                        s.opened_owner = info["creator"]["nickname"].as_str().unwrap_or("").to_string();
                        s.opened_owner_avatar = convert::cover(info["creator"]["avatarUrl"].as_str().unwrap_or(""), 64);
                        s.opened_meta = convert::playlist_meta(info);
                        s.opened_owned = uid != 0 && info["creator"]["userId"].as_i64() == Some(uid);
                    }
                    let mut songs = convert::songs(&body["songs"], &body["privileges"]);
                    for song in &mut songs {
                        song.added = added.get(&song.id).copied().unwrap_or(0);
                    }
                    enji_kit::dress_all(&mut songs);
                    arrange(&mut songs, &s.order);
                    s.opened_songs = songs;
                    s.opened_is_default = s.default_playlist == id;
                    s.opened_loading = false;
                    let _ = jobs.send(Job::Opened(id));
                }),
                Err(e) => {
                    shared.update(|s| s.opened_loading = false);
                    shared.fail(format!("歌单加载失败：{e}"));
                }
            }
        });
    }

    // MARK: Account

    /// Open the sign-in sheet `way`: the QR code starts at once, a phone
    /// number or an email address waits for the account.
    fn sign_in_by(&mut self, way: SignInWay) {
        if let Some(task) = self.signing_in.take() {
            task.abort();
        }
        self.shared.update(|s| {
            s.sign_in_way = way.clone();
            s.sign_in_error.clear();
            s.signing = false;
        });
        match way {
            SignInWay::Code => self.show_code(),
            SignInWay::Phone | SignInWay::Email => self.shared.update(|s| s.login = Login::Form),
        }
    }

    /// Text a sign-in code to the phone, then count down to when another
    /// may go.
    fn send_sms_code(&mut self, typed: &str) {
        if self.shared.read(|s| s.sms_wait > 0 || s.signing) {
            return;
        }
        let Some((country, phone)) = phone_number(typed) else {
            return self.shared.update(|s| s.sign_in_error = "手机号不对".into());
        };
        let shared = self.shared.clone();
        let client = enji_kit::client();
        shared.update(|s| {
            s.sign_in_error.clear();
            s.sms_wait = 60;
        });
        tokio::spawn(async move {
            let sent = client.captcha_sent(&Query::new().param("phone", &phone).param("ctcode", &country)).await;
            let refused = match sent {
                Ok(r) if r.body["code"].as_i64() == Some(200) => None,
                Ok(r) => Some(refusal(&r.body, "验证码没发出去")),
                Err(e) => Some(failure(&e, "验证码没发出去")),
            };
            if let Some(message) = refused {
                return shared.update(|s| {
                    s.sms_wait = 0;
                    s.sign_in_error = message;
                });
            }
            for left in (0..60).rev() {
                tokio::time::sleep(Duration::from_secs(1)).await;
                shared.update(|s| s.sms_wait = left);
            }
        });
    }

    /// Sign in with a phone number and a texted code or the password, or
    /// with an email address and the password.
    fn sign_in_with(&mut self, credentials: Credentials) {
        if self.shared.read(|s| s.signing) {
            return;
        }
        let query = match &credentials {
            Credentials::Sms { phone, code } | Credentials::Password { phone, password: code } => {
                let Some((country, phone)) = phone_number(phone) else {
                    return self.shared.update(|s| s.sign_in_error = "手机号不对".into());
                };
                let query = Query::new().param("phone", &phone).param("countrycode", &country);
                match credentials {
                    Credentials::Sms { .. } => query.param("captcha", code.trim()),
                    _ => query.param("password", code),
                }
            }
            Credentials::Email { email, password } => Query::new().param("email", email.trim()).param("password", password),
        };
        let email = matches!(credentials, Credentials::Email { .. });
        let (client, shared, jobs) = (enji_kit::client(), self.shared.clone(), self.jobs.clone());
        shared.update(|s| {
            s.signing = true;
            s.sign_in_error.clear();
        });
        self.signing_in = Some(tokio::spawn(async move {
            let answer = if email { client.login(&query).await } else { client.login_cellphone(&query).await };
            let refused = match answer {
                Ok(r) if r.body["code"].as_i64() == Some(200) => {
                    let cookie = cookie_of(&r.cookie);
                    if cookie.contains("MUSIC_U=") {
                        let _ = jobs.send(Job::SignedIn(cookie));
                        return;
                    }
                    "没拿到登录凭据，再试一次".to_string()
                }
                Ok(r) => refusal(&r.body, "登录失败"),
                Err(e) => failure(&e, "登录失败"),
            };
            shared.update(|s| {
                s.signing = false;
                s.sign_in_error = refused;
            });
        }));
    }

    fn show_code(&mut self) {
        let client = enji_kit::client();
        let shared = self.shared.clone();
        let jobs = self.jobs.clone();
        shared.update(|s| {
            s.login = Login::Loading;
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

    fn start(&mut self, index: usize) {
        // Its covers from this device when they were fetched ahead.
        let Some(song) = self.queue.get(index).cloned().map(crate::covers::localized) else { return };
        self.at = index;
        // The queue is shown from the playing song on.
        self.publish_queue();
        self.token += 1;
        self.position = 0.0;
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
            s.current_liked = liked;
            s.queue_at = index as i64;
            s.comments.clear();
            s.comments_total = 0;
            s.comments_more = false;
            s.details = None;
            s.preview = false;
            s.playing = false;
            s.buffering = true;
            s.progress = 0.0;
            s.elapsed = "0:00".into();
            s.total = song.duration.clone();
            s.lyrics.clear();
            s.has_lyrics = false;
            s.line = 0;
            s.browsing = false;
            s.format.clear();
            sung(s);
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
        tokio::spawn(async move {
            let id = param(song.id);
            let url = async {
                let r = client
                    .song_url_v1(&Query::new().param("id", &id).param("level", level(quality)))
                    .await
                    .map_err(|e| format!("取不到《{}》的音源：{e}", song.name))?;
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
            let lines = lyric
                .map(|r| {
                    lyrics::parse(
                        r.body["lrc"]["lyric"].as_str().unwrap_or(""),
                        r.body["tlyric"]["lyric"].as_str().unwrap_or(""),
                    )
                })
                .unwrap_or_default();
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
        tokio::spawn(async move {
            if let Ok(r) = client.lyric_new(&Query::new().param("id", &param(id))).await {
                let lines = lyrics::parse(
                    r.body["lrc"]["lyric"].as_str().unwrap_or(""),
                    r.body["tlyric"]["lyric"].as_str().unwrap_or(""),
                );
                let _ = jobs.send(Job::LocalInfo { token, lines, cover: String::new() });
            }
        });
    }

    /// The same song on NetEase, found by name and length, for the lyrics
    /// and cover a local file lacks.
    fn look_up(&self, song: Song, want_lyrics: bool) {
        let (client, jobs, token) = (enji_kit::client(), self.jobs.clone(), self.token);
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
                        lines = lyrics::parse(
                            r.body["lrc"]["lyric"].as_str().unwrap_or(""),
                            r.body["tlyric"]["lyric"].as_str().unwrap_or(""),
                        );
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
        let mode = self.shared.read(|s| s.mode.clone());
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
        self.paused = !self.paused;
        if self.paused {
            self.audio.pause();
        } else {
            self.audio.resume();
        }
        let playing = !self.paused;
        self.shared.update(|s| s.playing = playing);
        self.show_now_playing(None);
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
        if status.ended == Some(self.token) {
            return self.advance(true, true);
        }
        if (status.position - self.position).abs() < 0.05 {
            return;
        }
        self.position = status.position;
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
        let line_changed = self.shared.read(|s| s.line != line);
        self.shared.update(|s| {
            let length = s.current.as_ref().map_or(0.0, |song| song.seconds);
            s.progress = if length > 0.0 { (position / length).clamp(0.0, 1.0) } else { 0.0 };
            s.elapsed = convert::clock(position);
            if s.line != line {
                s.line = line;
                dress(&mut s.lyrics, line, s.browsing);
                sung(s);
                // Browsed lyrics stay where the person put them.
                if !s.browsing {
                    s.scroll_to = line;
                    s.scroll_tick += 1;
                }
            }
        });
        if line_changed {
            self.desktop_lyrics();
        }
    }
}

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

/// A phone number as typed, as its country code and digits: "13800000000"
/// is in mainland China, "+852 61234567" in Hong Kong.
fn phone_number(typed: &str) -> Option<(String, String)> {
    let typed = typed.trim();
    let (country, rest) = match typed.strip_prefix('+') {
        Some(rest) => {
            let split = rest.find(|c: char| !c.is_ascii_digit())?;
            (rest[..split].to_string(), &rest[split..])
        }
        None => ("86".to_string(), typed),
    };
    let digits: String = rest.chars().filter(char::is_ascii_digit).collect();
    let usable = !country.is_empty() && digits.len() >= 5 && rest.chars().all(|c| c.is_ascii_digit() || " -".contains(c));
    usable.then_some((country, digits))
}

/// Why a request failed, after `fallback`; for a sign-in NetEase wants
/// verified first, the ways that need none.
fn failure(error: &NcmError, fallback: &str) -> String {
    let said = match error {
        NcmError::Api { msg, .. } | NcmError::InvalidParam(msg) | NcmError::RateLimited(msg) | NcmError::AuthRequired(msg) => msg.clone(),
        NcmError::Http(_) | NcmError::Timeout(_) => "连不上网易云".to_string(),
        other => other.to_string(),
    };
    if said.contains("验证") && !said.contains("验证码错误") {
        return format!("{fallback}：网易云要先验证身份，改用短信验证码或扫码登录");
    }
    format!("{fallback}：{said}")
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
    use super::{SongOrder, arrange, phone_number};
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

    #[test]
    fn phone_numbers_carry_their_country() {
        let pair = |a: &str, b: &str| Some((a.to_string(), b.to_string()));
        assert_eq!(phone_number("13800000000"), pair("86", "13800000000"));
        assert_eq!(phone_number(" 138 0000-0000 "), pair("86", "13800000000"));
        assert_eq!(phone_number("+852 6123 4567"), pair("852", "61234567"));
        assert_eq!(phone_number("+1-4155550100"), pair("1", "4155550100"));
        assert_eq!(phone_number("+8613800000000"), None);
        assert_eq!(phone_number("abc"), None);
        assert_eq!(phone_number("1234"), None);
    }
}
