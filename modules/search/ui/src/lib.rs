//! Search: a page of the sidebar on a wide window; on a phone a round
//! button over the tabs, opening it over everything.

use bern::prelude::*;
use enji_protocol::enji::{self, Detail, Source};
use enji_ui::{
    album_card, album_card_props, artist_card, artist_card_props, detail_view, detail_view_props,
    playlist_card, playlist_card_props, song_header, song_header_props, song_row, song_row_props,
    song_skeleton, song_skeleton_props, watch_album_row, watch_album_row_props, watch_artist_row,
    watch_artist_row_props, watch_playlist_row, watch_playlist_row_props,
};
use search_protocol::search::{self, SearchKind};

#[bern::session]
pub struct SearchDraft {
    /// The words in the field.
    pub query: String,
    /// On a phone, search shows over the tabs.
    pub finding: bool,
}

/// Search on a wide window: the field under the page's title, and under it
/// what to search for, or what was found; or what was opened from it.
#[component]
pub fn search_page(
    app: enji::Client,
    search: search::Client,
    #[session] draft: SearchDraft,
) -> impl View {
    ui! {
        if app.detail != Detail::Page {
            detail_view(app: app)
        } else if window.watch {
            watch_search_page(app: app, search: search)
        } else {
            Column(gap: 0) {
                Column(gap: 16, padding_x: 32, padding_y: 24) {
                    Text(content: tr! { "zh-Hans" => "搜索", "en" => "Search" }, style: large_title)
                    Search(id: search_box, value: draft.query, prompt: tr! { "zh-Hans" => "歌曲、歌手、专辑", "en" => "Songs, singers, albums" }, height: 40, font_size: 15) => search.search(draft.query)
                }
                search_body(app: app, search: search)
                Watch(value: search.search_tick) => draft.query.set(search.searched)
            }
        }
    }
}

#[component]
pub fn watch_search_page(
    app: enji::Client,
    search: search::Client,
    #[session] draft: SearchDraft,
) -> impl View {
    ui! {
        ScrollView {
            Column(gap: 6, variant: watch_page) {
                Row(align: center) {
                    Spacer
                    Text(content: tr! { "zh-Hans" => "搜索", "en" => "Search" }, style: headline, bold: true)
                    Spacer
                }
                Column(variant: watch_inset) {
                    Search(id: search_box, value: draft.query, prompt: tr! { "zh-Hans" => "歌曲、歌手、专辑", "en" => "Songs, singers, albums" }) => search.search(draft.query)
                }
                if draft.query == "" {
                    watch_search_home_content(search: search)
                } else if search.searched == "" && !search.searching {
                    watch_search_home_content(search: search)
                } else {
                    watch_search_kinds(search: search)
                    if search.searching {
                        song_skeleton()
                    } else if search.no_results {
                        Column(gap: 8, padding_y: 20, align: center) {
                            Text(content: tr! { "zh-Hans" => "没有找到“{text}”相关的结果", "en" => "Nothing found for “{text}”"; text: search.searched }, style: callout, emphasis: secondary)
                        }
                    } else {
                        watch_search_results_content(app: app, search: search)
                    }
                }
                Watch(value: search.search_tick) => draft.query.set(search.searched)
            }
        }
    }
}

/// The round button over a phone's tabs that opens search.
#[component]
pub fn search_button(#[session] draft: SearchDraft) -> impl View {
    ui! {
        IconButton(id: open_search, icon: "magnifyingglass", height: 44, font_size: 19, variant: round, label: tr! { "zh-Hans" => "搜索", "en" => "Search" }) => draft.finding.set(true)
    }
}

/// Search filling a phone: the field and a way back at the top, what to
/// search for or what was found under them.
#[component]
pub fn phone_search(
    app: enji::Client,
    search: search::Client,
    #[session] draft: SearchDraft,
) -> impl View {
    ui! {
        if draft.finding {
            Stack(appear: rise, disappear: fade) {
                // Opaque to the screen's edges, over the tabs and the player.
                Stack(under_bars: true) {
                    Canvas(label: tr! { "zh-Hans" => "搜索", "en" => "Search" }) {
                        Shape(kind: rect, fill: surface.canvas)
                    }
                }
                if app.detail != Detail::Page {
                    detail_view(app: app)
                } else {
                    Column(gap: 0) {
                        Row(gap: 12, padding_x: 16, padding_y: 8, align: center) {
                            Search(id: phone_search_box, value: draft.query, prompt: tr! { "zh-Hans" => "歌曲、歌手、专辑", "en" => "Songs, singers, albums" }) => search.search(draft.query)
                            Button(id: close_search, label: tr! { "zh-Hans" => "取消", "en" => "Cancel" }, prominence: plain) => draft.finding.set(false)
                        }
                        search_body(app: app, search: search)
                    }
                }
                Watch(value: search.search_tick) => draft.query.set(search.searched)
                Shortcut(key: "escape") => draft.finding.set(false)
            }
        }
    }
}

/// Under the search field: what to search for until something is; then
/// whether to look for songs, albums, singers or playlists, and rows
/// standing in for what is being found, or what was found.
#[component]
pub fn search_body(
    app: enji::Client,
    search: search::Client,
    #[session] draft: SearchDraft,
) -> impl View {
    ui! {
        if draft.query == "" {
            search_home(search: search)
        } else if search.searched == "" && !search.searching {
            search_home(search: search)
        } else {
            Column(gap: 0) {
                search_kinds(search: search)
                if search.searching {
                    ScrollView {
                        Column(gap: 0, padding_x: if window.compact { 8 } else { 24 }, padding_y: 4) {
                            song_skeleton()
                        }
                    }
                } else if search.no_results {
                    Row {
                        Spacer
                        Column(gap: 12, padding_y: 96, align: center) {
                            Stack(variant: quiet) {
                                Label(title: tr! { "zh-Hans" => "没有结果", "en" => "No Results" }, icon: "magnifyingglass", display: icon, font_size: 40)
                            }
                            Text(content: tr! { "zh-Hans" => "没有找到“{text}”相关的结果", "en" => "Nothing found for “{text}”"; text: search.searched }, style: headline)
                        }
                        Spacer
                    }
                } else {
                    search_results(app: app, search: search)
                }
            }
        }
    }
}

/// Songs, albums, singers or playlists: what the search looks for.
#[component]
pub fn search_kinds(search: search::Client) -> impl View {
    ui! {
        if search.netease {
            if window.watch {
                watch_search_kinds(search: search)
            } else if window.compact {
                Column(padding_x: 16, padding_y: 6) {
                    Picker(id: search_kind, label: tr! { "zh-Hans" => "搜索", "en" => "Search For" }, label_hidden: true, selected: search.kind, style: segmented) => search.set_kind(search.kind)
                }
            } else {
                Row(padding_x: 32, padding_y: 6) {
                    Column(width: 360) {
                        Picker(id: search_kind_wide, label: tr! { "zh-Hans" => "搜索", "en" => "Search For" }, label_hidden: true, selected: search.kind, style: segmented) => search.set_kind(search.kind)
                    }
                    Spacer
                }
            }
        }
    }
}

#[component]
pub fn watch_search_kinds(search: search::Client) -> impl View {
    ui! {
        Column(variant: watch_inset, padding_y: 2) {
            Picker(id: search_kind, label: tr! { "zh-Hans" => "搜索", "en" => "Search For" }, label_hidden: true, selected: search.kind, style: menu) => search.set_kind(search.kind)
        }
    }
}

/// What to search for: the words searched here before, and, while NetEase
/// is on, what is searched most there now.
#[component]
pub fn search_home(search: search::Client) -> impl View {
    ui! {
        if window.watch {
            watch_search_home(search: search)
        } else {
            ScrollView {
                Column(gap: 28, padding_x: if window.compact { 18 } else { 32 }, padding_y: 8) {
                    if search.has_recent {
                        Column(gap: 12) {
                            Row(gap: 8, align: center) {
                                Text(content: tr! { "zh-Hans" => "最近搜索", "en" => "Recent" }, style: title3, bold: true)
                                Spacer
                                IconButton(id: clear_recent, icon: "trash", font_size: 15, prominence: plain, label: tr! { "zh-Hans" => "清除最近搜索", "en" => "Clear Recent Searches" }) => search.clear_recent()
                            }
                            Flow(gap: 8) {
                                ForEach(items: search.recent, key: key) |word| {
                                    Button(id: recent_word, label: word.word, shape: chip) => search.search(word.word)
                                }
                            }
                        }
                    }
                    // What NetEase searches most, while it is on.
                    if search.netease && search.has_hot {
                        Column(gap: 8) {
                            Text(content: tr! { "zh-Hans" => "热搜榜", "en" => "Trending" }, style: title3, bold: true)
                            Grid(min_width: 300, gap: 4, row_gap: 2) {
                                ForEach(items: search.hot, key: key, stagger: 0.02) |word| {
                                    Button(id: hot_word, label: word.word, variant: row) {
                                        Row(gap: 12, padding_x: 10, padding_y: 9, align: center) {
                                            Column(width: 24, align: center) {
                                                if word.top {
                                                    Text(content: tr! { "zh-Hans" => "{n}", "en" => "{n}"; n: word.rank }, style: headline, bold: true, color: accent)
                                                } else {
                                                    Text(content: tr! { "zh-Hans" => "{n}", "en" => "{n}"; n: word.rank }, style: headline, emphasis: tertiary)
                                                }
                                            }
                                            Column(gap: 2, align: leading) {
                                                if word.top {
                                                        Text(content: word.word, bold: true, lines: 1)
                                                    } else {
                                                        Text(content: word.word, lines: 1)
                                                    }
                                                if word.note != "" {
                                                    Text(content: word.note, style: caption, emphasis: secondary, lines: 1)
                                                }
                                            }
                                            Spacer
                                        }
                                    } => search.search(word.word)
                                }
                            }
                        }
                    } else if search.netease && search.loading {
                        song_skeleton()
                    }
                }
            }
        }
    }
}

#[component]
pub fn watch_search_home_content(search: search::Client) -> impl View {
    ui! {
        Column(gap: 10, padding_y: 4) {
            if search.has_recent {
                Column(gap: 6) {
                    Row(gap: 8, align: center) {
                        Text(content: tr! { "zh-Hans" => "最近搜索", "en" => "Recent" }, style: callout, bold: true)
                        Spacer
                        IconButton(id: clear_recent, icon: "trash", font_size: 15, prominence: plain, label: tr! { "zh-Hans" => "清除最近搜索", "en" => "Clear Recent Searches" }) => search.clear_recent()
                    }
                    Flow(gap: 6) {
                        ForEach(items: search.recent, key: key) |word| {
                            Button(id: recent_word, label: word.word, shape: chip) => search.search(word.word)
                        }
                    }
                }
            }
            if search.netease && search.has_hot {
                Column(gap: 4) {
                    Text(content: tr! { "zh-Hans" => "热搜榜", "en" => "Trending" }, style: callout, bold: true)
                    Column(gap: 2) {
                        ForEach(items: search.hot, key: key) |word| {
                            Button(id: hot_word, label: word.word, variant: row) {
                                Row(gap: 8, padding_x: 8, padding_y: 6, align: center) {
                                    Column(width: 22, align: center) {
                                        if word.top {
                                            Text(content: tr! { "zh-Hans" => "{n}", "en" => "{n}"; n: word.rank }, style: callout, bold: true, color: accent)
                                        } else {
                                            Text(content: tr! { "zh-Hans" => "{n}", "en" => "{n}"; n: word.rank }, style: callout, emphasis: tertiary)
                                        }
                                    }
                                    Text(content: word.word, lines: 1)
                                    Spacer
                                }
                            } => search.search(word.word)
                        }
                    }
                }
            } else if search.netease && search.loading {
                song_skeleton()
            }
        }
    }
}

#[component]
pub fn watch_search_results_content(app: enji::Client, search: search::Client) -> impl View {
    ui! {
        match search.kind {
            SearchKind::Songs => {
                Column(gap: 2) {
                    if search.has_local_results {
                        Text(content: tr! { "zh-Hans" => "本地音乐", "en" => "Local Music" }, style: caption, bold: true)
                        ForEach(items: search.local_results, key: index) |song| {
                            song_row(app: app, song: song, source: Source::LocalResults, full: true, removable: false)
                        }
                        if search.netease {
                            Text(content: tr! { "zh-Hans" => "网易云音乐", "en" => "NetEase Cloud Music" }, style: caption, bold: true)
                        }
                    }
                    if search.netease {
                        ForEach(items: search.results, key: index) |song| {
                            song_row(app: app, song: song, source: Source::Search, full: true, removable: false)
                        }
                    }
                }
            }
            SearchKind::Albums => {
                Column(gap: 4) {
                    ForEach(items: search.album_results, key: id) |album| {
                        watch_album_row(app: app, album: album)
                    }
                }
            }
            SearchKind::Artists => {
                Column(gap: 4) {
                    ForEach(items: search.artist_results, key: id) |artist| {
                        watch_artist_row(app: app, artist: artist)
                    }
                }
            }
            SearchKind::Playlists => {
                Column(gap: 4) {
                    ForEach(items: search.playlist_results, key: id) |list| {
                        watch_playlist_row(app: app, list: list)
                    }
                }
            }
        }
    }
}

#[component]
pub fn watch_search_home(search: search::Client) -> impl View {
    ui! {
        ScrollView {
            Column(gap: 12, padding_x: 14, padding_y: 8) {
                if search.has_recent {
                    Column(gap: 8) {
                        Row(gap: 8, align: center) {
                            Text(content: tr! { "zh-Hans" => "最近搜索", "en" => "Recent" }, style: callout, bold: true)
                            Spacer
                            IconButton(id: clear_recent, icon: "trash", font_size: 15, prominence: plain, label: tr! { "zh-Hans" => "清除最近搜索", "en" => "Clear Recent Searches" }) => search.clear_recent()
                        }
                        Flow(gap: 6) {
                            ForEach(items: search.recent, key: key) |word| {
                                Button(id: recent_word, label: word.word, shape: chip) => search.search(word.word)
                            }
                        }
                    }
                }
                if search.netease && search.has_hot {
                    Column(gap: 6) {
                        Text(content: tr! { "zh-Hans" => "热搜榜", "en" => "Trending" }, style: callout, bold: true)
                        Column(gap: 2) {
                            ForEach(items: search.hot, key: key) |word| {
                                Button(id: hot_word, label: word.word, variant: row) {
                                    Row(gap: 8, padding_x: 8, padding_y: 6, align: center) {
                                        Column(width: 22, align: center) {
                                            if word.top {
                                                Text(content: tr! { "zh-Hans" => "{n}", "en" => "{n}"; n: word.rank }, style: callout, bold: true, color: accent)
                                            } else {
                                                Text(content: tr! { "zh-Hans" => "{n}", "en" => "{n}"; n: word.rank }, style: callout, emphasis: tertiary)
                                            }
                                        }
                                        Text(content: word.word, lines: 1)
                                        Spacer
                                    }
                                } => search.search(word.word)
                            }
                        }
                    }
                } else if search.netease && search.loading {
                    song_skeleton()
                }
            }
        }
    }
}

/// What a search found: songs, local ones first; or albums, singers or
/// playlists as cards.
#[component]
pub fn search_results(app: enji::Client, search: search::Client) -> impl View {
    ui! {
        match search.kind {
            SearchKind::Songs => {
                ScrollView {
                    Column(gap: 0, padding_x: if window.watch { 12 } else if window.compact { 8 } else { 24 }, padding_y: 4) {
                        if search.has_local_results {
                            Column(padding_x: 12, padding_y: 8) {
                                Text(content: tr! { "zh-Hans" => "本地音乐", "en" => "Local Music" }, style: title3, bold: true)
                            }
                            song_header(app: app)
                            Column(gap: 2) {
                                ForEach(items: search.local_results, key: index) |song| {
                                    song_row(app: app, song: song, source: Source::LocalResults, full: true, removable: false)
                                }
                            }
                            if search.netease {
                                Column(padding_x: 12, padding_y: 8) {
                                    Text(content: tr! { "zh-Hans" => "网易云音乐", "en" => "NetEase Cloud Music" }, style: title3, bold: true)
                                }
                            }
                        }
                        if search.netease {
                            song_header(app: app)
                            Column(gap: 2) {
                                ForEach(items: search.results, key: index) |song| {
                                    song_row(app: app, song: song, source: Source::Search, full: true, removable: false)
                                }
                            }
                        }
                    }
                }
            }
            SearchKind::Albums => {
                ScrollView {
                    if window.watch {
                        Column(gap: 4, padding_x: 14, padding_y: 6) {
                            ForEach(items: search.album_results, key: id) |album| {
                                watch_album_row(app: app, album: album)
                            }
                        }
                    } else {
                        Column(padding: if window.compact { 18 } else { 32 }) {
                            Grid(min_width: 150, gap: 22, cell_ratio: 1, cell_extra: if window.tv { 124 } else { 58 }) {
                                ForEach(items: search.album_results, key: id, stagger: 0.02) |album| {
                                    album_card(app: app, album: album)
                                }
                            }
                        }
                    }
                }
            }
            SearchKind::Artists => {
                ScrollView {
                    if window.watch {
                        Column(gap: 4, padding_x: 14, padding_y: 6) {
                            ForEach(items: search.artist_results, key: id) |artist| {
                                watch_artist_row(app: app, artist: artist)
                            }
                        }
                    } else {
                        Column(padding: if window.compact { 18 } else { 32 }) {
                            Grid(min_width: if window.compact { 96 } else { 130 }, gap: if window.compact { 14 } else { 22 }, cell_ratio: 1, cell_extra: 50) {
                                ForEach(items: search.artist_results, key: id, stagger: 0.02) |artist| {
                                    artist_card(app: app, artist: artist)
                                }
                            }
                        }
                    }
                }
            }
            SearchKind::Playlists => {
                ScrollView {
                    if window.watch {
                        Column(gap: 4, padding_x: 14, padding_y: 6) {
                            ForEach(items: search.playlist_results, key: id) |list| {
                                watch_playlist_row(app: app, list: list)
                            }
                        }
                    } else {
                        Column(padding: if window.compact { 18 } else { 32 }) {
                            Grid(min_width: 150, gap: 22, cell_ratio: 1, cell_extra: if window.tv { 124 } else { 58 }) {
                                ForEach(items: search.playlist_results, key: id, stagger: 0.02) |list| {
                                    playlist_card(app: app, list: list)
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
