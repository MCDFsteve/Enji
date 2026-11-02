//! The engine of Enji Music. Commands hand jobs to the worker thread, which
//! talks to NetEase and plays the songs; the engine copies the worker's
//! state each time the worker wakes it.

mod audio;
mod covers;
mod downloads;
mod local;
mod lyrics;
mod net;
mod plugins;
mod shuffle;
mod store;

use bern::{media::MediaSource, CommandError};
use enji_kit::{convert, plays};
use enji_protocol::enji::{self, Appearance, Login, Panel, Playlist, Quality, SignInWay, Song, SongOrder, Source, State};
use net::{Credentials, Job};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

pub struct EnjiEngine {
    state: State,
    shared: Arc<Mutex<State>>,
    jobs: Option<mpsc::UnboundedSender<Job>>,
}

impl Default for EnjiEngine {
    fn default() -> Self {
        let state = net::initial_state();
        Self {
            shared: Arc::new(Mutex::new(state.clone())),
            state,
            jobs: None,
        }
    }
}

impl EnjiEngine {
    fn start(&mut self) {
        if self.jobs.is_some() {
            return;
        }
        let (jobs, receive) = mpsc::unbounded_channel();
        let (shared, waker, own) = (self.shared.clone(), bern::wake::waker(), jobs.clone());
        let started = std::thread::Builder::new()
            .name("enji-worker".into())
            .spawn(move || enji_kit::runtime().block_on(net::run(receive, own, shared, waker)));
        if started.is_ok() {
            self.jobs = Some(jobs);
        }
    }

    fn submit(&mut self, job: Job) -> Result<(), CommandError> {
        self.start();
        self.jobs
            .as_ref()
            .ok_or_else(|| CommandError::new("unavailable", "the worker did not start"))?
            .send(job)
            .map_err(|e| CommandError::new("unavailable", e.to_string()))
    }
}

impl enji::Engine for EnjiEngine {
    fn state(&self) -> &State {
        &self.state
    }

    fn wake(&mut self) {
        self.start();
        let (signed_in, netease) = (self.state.login == Login::In, self.state.netease);
        self.state = self.shared.lock().unwrap_or_else(|e| e.into_inner()).clone();
        // The modules follow the account and the NetEase switch.
        if (self.state.login == Login::In) != signed_in {
            enji::emit::account_changed(!signed_in);
        }
        if self.state.netease != netease {
            enji::emit::netease_changed(self.state.netease);
        }
    }
}

impl enji::Commands for EnjiEngine {
    fn refresh(&mut self) -> Result<(), CommandError> {
        self.submit(Job::Refresh)
    }
    fn sign_in(&mut self) -> Result<(), CommandError> {
        self.submit(Job::SignIn)
    }
    fn cancel_sign_in(&mut self) -> Result<(), CommandError> {
        self.submit(Job::CancelSignIn)
    }
    fn set_sign_in_way(&mut self, way: SignInWay) -> Result<(), CommandError> {
        self.submit(Job::SetSignInWay(way))
    }
    fn send_sms_code(&mut self, phone: String) -> Result<(), CommandError> {
        self.submit(Job::SendSmsCode(phone))
    }
    fn sign_in_by_sms(&mut self, phone: String, code: String) -> Result<(), CommandError> {
        self.submit(Job::SignInWith(Credentials::Sms { phone, code }))
    }
    fn sign_in_by_password(&mut self, phone: String, password: String) -> Result<(), CommandError> {
        self.submit(Job::SignInWith(Credentials::Password { phone, password }))
    }
    fn sign_in_by_email(&mut self, email: String, password: String) -> Result<(), CommandError> {
        self.submit(Job::SignInWith(Credentials::Email { email, password }))
    }
    fn sign_out(&mut self) -> Result<(), CommandError> {
        self.submit(Job::SignOut)
    }
    fn open_playlist(&mut self, id: i64) -> Result<(), CommandError> {
        self.submit(Job::Open(id))
    }
    fn show_playlist(&mut self, list: Playlist) -> Result<(), CommandError> {
        self.submit(Job::Show(list))
    }
    fn close_playlist(&mut self) -> Result<(), CommandError> {
        self.submit(Job::Close)
    }
    fn add_folder(&mut self, folder: MediaSource) -> Result<(), CommandError> {
        match bern::files::list(&folder.url) {
            Ok(_) => self.submit(Job::AddFolder(folder.url)),
            Err(_) => self.submit(Job::RefuseFolder),
        }
    }
    fn remove_folder(&mut self, key: i64) -> Result<(), CommandError> {
        self.submit(Job::RemoveFolder(place(key)?))
    }
    fn rescan(&mut self) -> Result<(), CommandError> {
        self.submit(Job::Rescan)
    }
    fn begin_matching(&mut self, source: Source, index: i64) -> Result<(), CommandError> {
        self.submit(Job::BeginMatching(source, place(index)?))
    }
    fn cancel_matching(&mut self) -> Result<(), CommandError> {
        self.submit(Job::CancelMatching)
    }
    fn search_matching(&mut self, text: String) -> Result<(), CommandError> {
        self.submit(Job::SearchMatching(text))
    }
    fn choose_match(&mut self, id: i64) -> Result<(), CommandError> {
        self.submit(Job::ChooseMatch(id))
    }
    fn unmatch(&mut self) -> Result<(), CommandError> {
        self.submit(Job::Unmatch)
    }
    fn play_next(&mut self, source: Source, index: i64) -> Result<(), CommandError> {
        self.submit(Job::PlayNext(source, place(index)?))
    }
    fn add_to_queue(&mut self, source: Source, index: i64) -> Result<(), CommandError> {
        self.submit(Job::AddToQueue(source, place(index)?))
    }
    fn remove_from_queue(&mut self, index: i64) -> Result<(), CommandError> {
        self.submit(Job::RemoveFromQueue(place(index)?))
    }
    fn remove_history(&mut self, key: i64) -> Result<(), CommandError> {
        self.submit(Job::RemoveHistory(place(key)?))
    }
    fn clear_history(&mut self) -> Result<(), CommandError> {
        self.submit(Job::ClearHistory)
    }
    fn choose_playlist(&mut self, source: Source, index: i64) -> Result<(), CommandError> {
        self.submit(Job::ChoosePlaylist(source, place(index)?))
    }
    fn cancel_choosing(&mut self) -> Result<(), CommandError> {
        self.submit(Job::CancelChoosing)
    }
    fn add_to_playlist(&mut self, id: i64) -> Result<(), CommandError> {
        self.submit(Job::AddToPlaylist(id))
    }
    fn remove_from_playlist(&mut self, index: i64) -> Result<(), CommandError> {
        self.submit(Job::RemoveFromPlaylist(place(index)?))
    }
    fn begin_naming(&mut self) -> Result<(), CommandError> {
        self.submit(Job::Naming(true))
    }
    fn cancel_naming(&mut self) -> Result<(), CommandError> {
        self.submit(Job::Naming(false))
    }
    fn create_playlist(&mut self, name: String) -> Result<(), CommandError> {
        if name.trim().is_empty() {
            return Err(CommandError::new("empty_name", "a playlist needs a name"));
        }
        self.submit(Job::CreatePlaylist(name.trim().to_string()))
    }
    fn ask_delete_playlist(&mut self, id: i64) -> Result<(), CommandError> {
        self.submit(Job::AskDelete(id))
    }
    fn confirm_delete(&mut self) -> Result<(), CommandError> {
        self.submit(Job::ConfirmDelete)
    }
    fn cancel_delete(&mut self) -> Result<(), CommandError> {
        self.submit(Job::CancelDelete)
    }
    fn play_playlist(&mut self, id: i64) -> Result<(), CommandError> {
        self.submit(Job::PlayPlaylist(id))
    }
    fn set_netease(&mut self, on: bool) -> Result<(), CommandError> {
        self.submit(Job::SetNetease(on))
    }
    fn set_online_info(&mut self, on: bool) -> Result<(), CommandError> {
        self.submit(Job::SetOnlineInfo(on))
    }
    fn play(&mut self, source: Source, index: i64) -> Result<(), CommandError> {
        if index < 0 {
            return Err(CommandError::new("invalid_index", "the index starts at 0"));
        }
        self.submit(Job::Play(source, index as usize))
    }
    fn toggle(&mut self) -> Result<(), CommandError> {
        self.submit(Job::Toggle)
    }
    fn next_song(&mut self) -> Result<(), CommandError> {
        self.submit(Job::Next)
    }
    fn previous_song(&mut self) -> Result<(), CommandError> {
        self.submit(Job::Previous)
    }
    fn seek(&mut self, progress: f64) -> Result<(), CommandError> {
        self.submit(Job::Seek(progress))
    }
    fn seek_line(&mut self, key: i64) -> Result<(), CommandError> {
        self.submit(Job::SeekLine(key))
    }
    fn set_volume(&mut self, volume: f64) -> Result<(), CommandError> {
        self.submit(Job::Volume(volume))
    }
    fn toggle_mute(&mut self) -> Result<(), CommandError> {
        self.submit(Job::ToggleMute)
    }
    fn browse_lyrics(&mut self, active: bool) -> Result<(), CommandError> {
        self.submit(Job::Browse(active))
    }
    fn play_queued(&mut self, index: i64) -> Result<(), CommandError> {
        if index < 0 {
            return Err(CommandError::new("invalid_index", "the index starts at 0"));
        }
        self.submit(Job::PlayQueued(index as usize))
    }
    fn next_mode(&mut self) -> Result<(), CommandError> {
        self.submit(Job::NextMode)
    }
    fn show_panel(&mut self, panel: Panel) -> Result<(), CommandError> {
        self.submit(Job::ShowPanel(panel))
    }
    fn toggle_panel(&mut self, panel: Panel) -> Result<(), CommandError> {
        self.submit(Job::TogglePanel(panel))
    }
    fn more_comments(&mut self) -> Result<(), CommandError> {
        self.submit(Job::MoreComments)
    }
    fn set_quality(&mut self, quality: Quality) -> Result<(), CommandError> {
        self.submit(Job::SetQuality(quality))
    }
    fn set_order(&mut self, order: SongOrder) -> Result<(), CommandError> {
        self.submit(Job::SetOrder(order))
    }
    fn set_show_plays(&mut self, shown: bool) -> Result<(), CommandError> {
        self.submit(Job::SetShowPlays(shown))
    }
    fn set_default_playlist(&mut self, id: i64) -> Result<(), CommandError> {
        self.submit(Job::SetDefaultPlaylist(id))
    }
    fn page_changed(&mut self, library: bool) -> Result<(), CommandError> {
        self.submit(Job::PageChanged(library))
    }
    fn set_show_translation(&mut self, shown: bool) -> Result<(), CommandError> {
        self.submit(Job::SetTranslation(shown))
    }
    fn set_backdrop(&mut self, shown: bool) -> Result<(), CommandError> {
        self.submit(Job::SetBackdrop(shown))
    }
    fn set_show_format(&mut self, shown: bool) -> Result<(), CommandError> {
        self.submit(Job::SetShowFormat(shown))
    }
    fn download(&mut self, source: Source, index: i64) -> Result<(), CommandError> {
        self.submit(Job::Download(source, place(index)?))
    }
    fn download_current(&mut self) -> Result<(), CommandError> {
        self.submit(Job::DownloadCurrent)
    }
    fn set_plugin_enabled(&mut self, id: String, on: bool) -> Result<(), CommandError> {
        self.submit(Job::SetPluginEnabled(id, on))
    }
    fn remove_plugin(&mut self, id: String) -> Result<(), CommandError> {
        self.submit(Job::RemovePlugin(id))
    }
    fn load_offers(&mut self) -> Result<(), CommandError> {
        self.submit(Job::LoadOffers)
    }
    fn install_plugin(&mut self, id: String) -> Result<(), CommandError> {
        self.submit(Job::InstallPlugin(id))
    }
    fn import_plugin(&mut self, file: MediaSource) -> Result<(), CommandError> {
        self.submit(Job::ImportPlugin(file.url))
    }
    fn set_cover_lyrics(&mut self, shown: bool) -> Result<(), CommandError> {
        self.submit(Job::SetCoverLyrics(shown))
    }
    fn set_desktop_lyrics(&mut self, shown: bool) -> Result<(), CommandError> {
        self.submit(Job::SetDesktopLyrics(shown))
    }
    fn set_desktop_lyrics_locked(&mut self, locked: bool) -> Result<(), CommandError> {
        self.submit(Job::SetDesktopLyricsLocked(locked))
    }
    fn set_appearance(&mut self, appearance: Appearance) -> Result<(), CommandError> {
        self.submit(Job::SetAppearance(appearance))
    }
    fn toggle_like(&mut self) -> Result<(), CommandError> {
        self.submit(Job::ToggleLike)
    }
    fn shuffle(&mut self, source: Source) -> Result<(), CommandError> {
        self.submit(Job::Shuffle(source))
    }
    fn open_album(&mut self, id: i64) -> Result<(), CommandError> {
        self.submit(Job::OpenAlbum(id))
    }
    fn open_artist(&mut self, id: i64) -> Result<(), CommandError> {
        self.submit(Job::OpenArtist(id))
    }
    fn back(&mut self) -> Result<(), CommandError> {
        self.submit(Job::Back)
    }
    fn dismiss_error(&mut self) -> Result<(), CommandError> {
        self.submit(Job::DismissError)
    }
    fn set_songs(&mut self, source: Source, songs: Vec<Song>) -> Result<(), CommandError> {
        self.submit(Job::SetSongs(source, songs))
    }
    fn show_error(&mut self, message: String) -> Result<(), CommandError> {
        self.submit(Job::Fail(message))
    }
}

/// A place in a list, which starts at 0.
fn place(index: i64) -> Result<usize, CommandError> {
    usize::try_from(index).map_err(|_| CommandError::new("invalid_index", "the index starts at 0"))
}
