//! The engine of search. The local music is searched at once, among the
//! core's songs; NetEase away from the engine, and `wake` takes in what it
//! found and hands the songs to the core, which plays a song of them by
//! its place. What was searched before is kept in `search.json`.

use bern::CommandError;
use enji_kit::{convert, list, renumbered, store};
use enji_protocol::enji::{self, Song, Source};
use ncm_api_rs::Query;
use search_protocol::search::{self, SearchKind, SearchWord, State};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};

/// How many recent searches are kept.
const RECENT: usize = 12;

/// What NetEase answered.
enum Answer {
    /// What a search found, of the kind it looked for.
    Found(SearchKind, Value),
    /// The most searched words, none when they could not be read.
    Hot(Vec<SearchWord>),
    Failed(SearchKind, String),
}

/// Kept only for the four result lists of the submitted query.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lookup {
    Idle,
    Loading,
    Ready,
    Failed,
}

pub struct SearchEngine {
    state: State,
    /// What NetEase answered, each with the number of the search it
    /// answers, for `wake`.
    inbox: Arc<Mutex<Vec<(u64, Answer)>>>,
    /// Each search gets a new number; what an older one finds is dropped.
    ask: u64,
    lookups: [Lookup; 4],
    /// The core's state was read once, as the engine started.
    started: bool,
    /// The settings want the most searched words.
    wants_hot: bool,
    /// The last `wake` changed nothing.
    quiet: bool,
    /// The local copies and play counts the songs found show.
    dressed: u64,
}

impl Default for SearchEngine {
    fn default() -> Self {
        let recent = recent_words(kept_words());
        Self {
            state: State {
                netease: enji_kit::netease_on(),
                kind: SearchKind::Songs,
                searched: String::new(),
                search_tick: 0,
                searching: false,
                search_failed: false,
                results: Vec::new(),
                album_results: Vec::new(),
                artist_results: Vec::new(),
                playlist_results: Vec::new(),
                local_results: Vec::new(),
                has_local_results: false,
                no_results: false,
                has_recent: !recent.is_empty(),
                recent,
                hot: Vec::new(),
                has_hot: false,
                loading: false,
            },
            inbox: Arc::default(),
            ask: 0,
            lookups: [Lookup::Idle; 4],
            started: false,
            wants_hot: true,
            dressed: enji_kit::dressing(),
            quiet: false,
        }
    }
}

/// The words searched before, as kept; at first those Enji kept with its
/// settings, before search was a module of its own.
fn kept_words() -> Vec<String> {
    let kept = store::read("search.json");
    let words = if kept.is_null() { store::read("settings.json")["recent"].clone() } else { kept["recent"].clone() };
    words
        .as_array()
        .map(|all| all.iter().filter_map(Value::as_str).take(RECENT).map(str::to_string).collect())
        .unwrap_or_default()
}

/// The words searched before, as the search page lists them.
fn recent_words(words: Vec<String>) -> Vec<SearchWord> {
    words
        .into_iter()
        .enumerate()
        .map(|(i, word)| SearchWord { key: i as i64, rank: 0, word, note: String::new(), top: false })
        .collect()
}

/// NetEase's most searched words, the first `limit`.
fn hot_words(list: &Value, limit: usize) -> Vec<SearchWord> {
    list.as_array()
        .map(|all| {
            all.iter()
                .filter_map(|one| one["searchWord"].as_str().filter(|word| !word.is_empty()).map(|word| (one, word)))
                .take(limit)
                .enumerate()
                .map(|(i, (one, word))| SearchWord {
                    key: i as i64,
                    rank: i as i64 + 1,
                    word: word.to_string(),
                    note: one["content"].as_str().unwrap_or("").trim().to_string(),
                    top: i < 3,
                })
                .collect()
        })
        .unwrap_or_default()
}

impl SearchEngine {
    /// Ask NetEase away from the engine; its answer comes back to `wake`.
    fn ask(&self, ask: u64, question: impl Future<Output = Answer> + Send + 'static) {
        let (inbox, waker) = (self.inbox.clone(), bern::wake::waker());
        enji_kit::runtime().spawn(async move {
            let answer = question.await;
            inbox.lock().unwrap_or_else(|e| e.into_inner()).push((ask, answer));
            if let Some(waker) = waker {
                waker.wake();
            }
        });
    }

    /// The most searched words, if they are wanted and not here yet.
    fn follow_hot(&mut self) {
        if self.state.netease && self.wants_hot && !self.state.has_hot && !self.state.loading {
            self.load_hot();
        }
    }

    fn drop_hot(&mut self) {
        self.state.hot.clear();
        self.state.has_hot = false;
        self.state.loading = false;
    }

    fn load_hot(&mut self) {
        self.state.loading = true;
        self.ask(0, async {
            match enji_kit::client().search_hot_detail(&Query::new()).await {
                Ok(r) => Answer::Hot(hot_words(&r.body["data"], 20)),
                Err(_) => Answer::Hot(Vec::new()),
            }
        });
    }

    /// The songs NetEase found, which the core plays by their place.
    fn set_results(&mut self, mut songs: Vec<Song>) {
        enji_kit::dress_all(&mut songs);
        self.state.results = songs;
        enji::calls::set_songs(Source::Search, self.state.results.clone());
    }

    fn set_local_results(&mut self, mut songs: Vec<Song>) {
        enji_kit::dress_all(&mut songs);
        self.state.has_local_results = !songs.is_empty();
        self.state.local_results = songs;
        enji::calls::set_songs(Source::LocalResults, self.state.local_results.clone());
    }

    fn forget_results(&mut self) {
        self.ask += 1;
        self.lookups.fill(Lookup::Idle);
        if !self.state.results.is_empty() {
            self.set_results(Vec::new());
        }
        self.state.album_results.clear();
        self.state.artist_results.clear();
        self.state.playlist_results.clear();
    }

    fn update_status(&mut self) {
        let lookup = self.lookups[self.state.kind.clone() as usize];
        self.state.searching = lookup == Lookup::Loading;
        self.state.search_failed = lookup == Lookup::Failed;
        self.state.no_results = !self.state.searched.is_empty()
            && (!self.state.netease || lookup == Lookup::Ready)
            && match self.state.kind {
                SearchKind::Songs => self.state.results.is_empty() && !self.state.has_local_results,
                SearchKind::Albums => self.state.album_results.is_empty(),
                SearchKind::Artists => self.state.artist_results.is_empty(),
                SearchKind::Playlists => self.state.playlist_results.is_empty(),
            };
    }

    fn load_kind(&mut self) {
        let kind = self.state.kind.clone();
        let lookup = &mut self.lookups[kind.clone() as usize];
        if self.state.netease && !self.state.searched.is_empty() && *lookup == Lookup::Idle {
            *lookup = Lookup::Loading;
            let text = self.state.searched.clone();
            self.ask(self.ask, async move {
                let (code, limit) = match kind {
                    SearchKind::Songs => ("1", "60"),
                    SearchKind::Albums => ("10", "40"),
                    SearchKind::Artists => ("100", "40"),
                    SearchKind::Playlists => ("1000", "40"),
                };
                let query = Query::new().param("keywords", &text).param("type", code).param("limit", limit);
                match enji_kit::client().cloudsearch(&query).await {
                    Ok(r) => Answer::Found(kind, r.body["result"].clone()),
                    Err(e) => Answer::Failed(kind, format!("搜索失败：{e}")),
                }
            });
        }
        self.update_status();
    }
}

impl search::Commands for SearchEngine {
    fn search(&mut self, text: String) -> Result<(), CommandError> {
        let text = text.trim().to_string();
        if text.is_empty() {
            return Ok(());
        }
        self.state.searched = text.clone();
        self.state.search_tick += 1;
        if self.state.recent.first().is_none_or(|word| word.word != text) {
            let mut words: Vec<String> = std::mem::take(&mut self.state.recent)
                .into_iter().map(|entry| entry.word).filter(|word| *word != text).collect();
            words.insert(0, text.clone());
            words.truncate(RECENT);
            self.state.recent = recent_words(words);
            self.state.has_recent = true;
            store::write("search.json", &json!({ "recent": self.state.recent.iter().map(|word| &word.word).collect::<Vec<_>>() }));
        }
        // The local music at once.
        let wanted = text.to_lowercase();
        let local = enji::current_state().map(|core| core.local).unwrap_or_default();
        let found = local.into_iter().filter(|song| {
            [&song.name, &song.artists, &song.album].iter().any(|field| field.to_lowercase().contains(&wanted))
        });
        self.set_local_results(renumbered(found));
        self.forget_results();
        self.load_kind();
        Ok(())
    }

    fn set_kind(&mut self, kind: SearchKind) -> Result<(), CommandError> {
        if self.state.netease {
            self.state.kind = kind;
            self.load_kind();
        }
        Ok(())
    }

    fn retry(&mut self) -> Result<(), CommandError> {
        let lookup = &mut self.lookups[self.state.kind.clone() as usize];
        if *lookup == Lookup::Failed {
            *lookup = Lookup::Idle;
            self.load_kind();
        }
        Ok(())
    }

    fn clear_recent(&mut self) -> Result<(), CommandError> {
        self.state.recent.clear();
        self.state.has_recent = false;
        store::write("search.json", &json!({ "recent": [] }));
        Ok(())
    }

    fn netease_changed(&mut self, on: bool) -> Result<(), CommandError> {
        self.state.netease = on;
        if on {
            self.follow_hot();
        } else {
            self.drop_hot();
            self.state.kind = SearchKind::Songs;
            self.forget_results();
            self.update_status();
        }
        Ok(())
    }

    fn hot_search_changed(&mut self, on: bool) -> Result<(), CommandError> {
        self.wants_hot = on;
        if on {
            self.follow_hot();
        } else {
            self.drop_hot();
        }
        Ok(())
    }
}

impl search::Engine for SearchEngine {
    fn state(&self) -> &State {
        &self.state
    }

    fn wake(&mut self) {
        let first = !self.started;
        if first {
            self.started = true;
            if let Some(core) = enji::current_state() {
                self.state.netease = core.netease;
                self.wants_hot = core.hot_search;
            }
            self.follow_hot();
        }
        let arrived: Vec<Answer> = {
            let mut inbox = self.inbox.lock().unwrap_or_else(|e| e.into_inner());
            inbox
                .drain(..)
                .filter(|(ask, answer)| matches!(answer, Answer::Hot(_)) || *ask == self.ask)
                .map(|(_, answer)| answer)
                .collect()
        };
        self.quiet = !first && arrived.is_empty();
        for answer in arrived {
            match answer {
                Answer::Hot(words) => {
                    self.state.loading = false;
                    // Turned off meanwhile: its words are not shown.
                    if self.state.netease && self.wants_hot {
                        self.state.has_hot = !words.is_empty();
                        self.state.hot = words;
                    }
                }
                Answer::Found(kind, result) => {
                    self.lookups[kind.clone() as usize] = Lookup::Ready;
                    match kind {
                        SearchKind::Songs => {
                            self.set_results(convert::songs(&result["songs"], &Value::Null));
                        }
                        SearchKind::Albums => {
                            self.state.album_results = list(&result["albums"], convert::album);
                        }
                        SearchKind::Artists => {
                            self.state.artist_results = list(&result["artists"], convert::artist);
                        }
                        SearchKind::Playlists => {
                            self.state.playlist_results = list(&result["playlists"], convert::playlist);
                        }
                    }
                    self.update_status();
                }
                Answer::Failed(kind, message) => {
                    self.lookups[kind.clone() as usize] = Lookup::Failed;
                    if kind == self.state.kind {
                        self.update_status();
                        enji::calls::show_error(message);
                    }
                }
            }
        }
        // A copy found or a song played since: shown, and played, so.
        if enji_kit::dressing() != self.dressed {
            self.dressed = enji_kit::dressing();
            self.quiet = false;
            if !self.state.results.is_empty() {
                let songs = std::mem::take(&mut self.state.results);
                self.set_results(songs);
            }
            if self.state.has_local_results {
                let songs = std::mem::take(&mut self.state.local_results);
                self.set_local_results(songs);
            }
        }
    }

    fn quiet(&self) -> bool {
        self.quiet
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use search::{Commands, Engine};

    #[test]
    fn switching_categories_reuses_pending_and_empty_results() {
        let mut engine = SearchEngine { started: true, ..Default::default() };
        engine.state.netease = true;
        engine.state.searched = "demo".into();
        engine.lookups[SearchKind::Albums as usize] = Lookup::Loading;
        engine.lookups[SearchKind::Artists as usize] = Lookup::Loading;

        engine.set_kind(SearchKind::Artists).unwrap();
        engine.inbox.lock().unwrap().push((engine.ask, Answer::Found(SearchKind::Albums, json!({ "albums": [] }))));
        engine.wake();
        assert!(engine.state.searching);
        assert!(!engine.state.no_results);

        engine.set_kind(SearchKind::Albums).unwrap();
        assert!(engine.state.no_results);
        assert!(!engine.state.searching);
        engine.set_kind(SearchKind::Artists).unwrap();
        assert!(engine.state.searching);
        assert_eq!(engine.state.search_tick, 0);
    }

    #[test]
    fn a_background_failure_only_affects_its_own_category() {
        let mut engine = SearchEngine { started: true, ..Default::default() };
        engine.state.netease = true;
        engine.state.searched = "demo".into();
        engine.lookups[SearchKind::Songs as usize] = Lookup::Loading;
        engine.inbox.lock().unwrap().push((engine.ask, Answer::Failed(SearchKind::Artists, "offline".into())));
        engine.set_kind(SearchKind::Songs).unwrap();
        engine.wake();
        assert!(engine.state.searching);
        assert!(!engine.state.search_failed);

        engine.set_kind(SearchKind::Artists).unwrap();
        assert!(engine.state.search_failed);
        assert!(!engine.state.no_results);
        engine.forget_results();
        engine.update_status();
        assert!(!engine.state.search_failed);
    }

    #[test]
    fn stale_replies_cannot_hide_local_results_or_change_the_new_query() {
        let mut engine = SearchEngine { started: true, ..Default::default() };
        engine.state.netease = true;
        engine.state.searched = "demo".into();
        engine.state.has_local_results = true;
        let old = engine.ask;
        engine.forget_results();
        engine.lookups[SearchKind::Songs as usize] = Lookup::Loading;
        engine.update_status();
        engine.inbox.lock().unwrap().push((old, Answer::Failed(SearchKind::Songs, "old error".into())));
        engine.wake();
        assert!(engine.state.searching);
        assert!(!engine.state.search_failed);
        assert!(engine.state.has_local_results);

        engine.netease_changed(false).unwrap();
        assert!(!engine.state.searching);
        assert!(!engine.state.no_results);
        assert!(engine.state.has_local_results);
    }
}
