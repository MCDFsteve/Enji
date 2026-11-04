//! The shell of Enji Music: the window and its menus, a sidebar of pages
//! beside the chosen one over the player bar, or tabs and a small player on
//! a phone, and the playing song over everything. The core module (`enji`)
//! gives the player, the lists and the settings; For You, Charts and
//! Search come from their own modules, which can be switched off, and show
//! where the shell leaves room for them.

use bern::prelude::*;
use enji_protocol::enji::{self, CloseAction, Detail, Login};
use enji_shell_protocol::shell;
use enji_ui::*;

style! {
    accents {
        enji: "#FF0732",
    }

    motion {
        // A sheet's spring: quick to rise, settling with the faintest
        // overshoot.
        sheet: spring(period: 0.5, damping: 0.86),
        // Back from a beat's pulse: at once and without a bounce, as a
        // speaker's cone.
        beat: spring(period: 0.34, damping: 1.0),
    }

    transition {
        rise: slide(bottom, 16, smooth),
        // The playing page comes up from the bottom edge and goes back
        // down to it, whole.
        sheet: move(bottom, sheet),
    }

    Button.card {
        corner: 12,
        hover: scale(1.03, snappy) + shadow(14, y: 6, opacity: 0.18),
        pressed: scale(0.97, press),
        settle: jelly,
    }

    Button.row {
        corner: 8,
        hover: background(fill.subtle),
        pressed: scale(0.99, press),
        settle: snappy,
    }

    Button.lyric {
        corner: 12,
        hover: background(fill.subtle),
        pressed: scale(0.98, press),
        settle: jelly,
    }

    // The QR code on white, whatever the window's appearance, so phones read it.
    Stack.qr {
        background: color(light: "#FFFFFF", dark: "#FFFFFF"),
        corner: 22,
        shadow: shadow(18, y: 6, opacity: 0.14),
    }

    // The chosen page in the accent, not the system's selection color.
    SidebarItem {
        corner: 8,
        hover: scale(1.03, snappy),
        pressed: scale(0.94, press),
        settle: jelly,
        selected: fill(accent),
    }

    // The small player over the tabs: the song pressed in, nothing more.
    Button.bare {
        pressed: scale(0.97, press),
        settle: snappy,
    }

    // The search field on iOS: a capsule, as the system's own.
    #[target(ios)]
    Stack.search_field {
        background: color(light: "#EEEEF0", dark: "#1C1C1E"),
        corner: 20,
    }

    // Room around the field inside the capsule; Android's field draws its
    // own outline.
    Column.search_inset { padding_x: 14, padding_y: 9 }
    #[target(android)]
    Column.search_inset { padding_x: 0, padding_y: 0 }

    // The button over Android's tabs: round and raised, as the bar below.
    #[target(android)]
    IconButton.round {
        background: color(light: "#FFFFFF", dark: "#2B2930"),
        corner: 22,
        shadow: shadow(10, y: 3, opacity: 0.14),
    }

    // The playing page's close button, at the top trailing corner. On iOS
    // the status bar is there over the page, across the top of an iPad and
    // down the trailing edge of the unfolded iPhone Duo: the button stands
    // left of its icons, the battery's percentage among them. On an Android
    // tablet it stands below them.
    Row.close_now { padding_x: 24, padding_y: 18 }
    #[target(ios)]
    Row.close_now { padding_x: 130, padding_y: 18 }
    #[target(android)]
    Row.close_now { padding_x: 24, padding_y: 44 }

    // The settings' cards, and the cover's place while nothing plays.
    Stack.card {
        background: fill.subtle,
        corner: 14,
    }
    Stack.placeholder {
        background: fill.subtle,
        corner: 8,
    }
    // The account's picture behind who it is, and its level on it.
    Stack.hero {
        corner: 20,
    }
    Stack.level {
        background: fill.subtle,
        corner: 8,
    }
    // What signing in brings, as small tags.
    Stack.tag {
        background: fill.subtle,
        corner: 16,
    }

    // Icons that say what something is, quieter than its words; and the
    // speaker of the playing song in the accent.
    Stack.quiet { foreground: text.secondary }
    Stack.tint { foreground: accent }

    IconButton {
        hover: scale(1.08, snappy),
        pressed: scale(0.84, press),
        settle: jelly,
    }
}

#[bern::value]
pub enum Accent {
    #[label(tr! { "zh-Hans" => "臙脂", "en" => "Enji" })]
    Enji,
}

/// The pages of the window. The modules add For You, Charts and Search,
/// For You first, where the window opens.
#[bern::value(pages = "main")]
pub enum Page {
    #[label(tr! { "zh-Hans" => "我的歌单", "en" => "My Playlists" })]
    Library,
    #[label(tr! { "zh-Hans" => "本地音乐", "en" => "Local Music" })]
    Local,
    #[label(tr! { "zh-Hans" => "播放记录", "en" => "History" })]
    History,
    #[label(tr! { "zh-Hans" => "设置", "en" => "Settings" })]
    Settings,
    /// A TV's tab for the playing song, which shows over the pages elsewhere.
    #[label(tr! { "zh-Hans" => "正在播放", "en" => "Now Playing" })]
    Playing,
}

#[bern::session]
pub struct ShellDraft {
    /// The page chosen in the sidebar or the tabs.
    pub page: Page,
}

#[component]
pub fn app(app: enji::Client, windows: AppWindows) -> impl View {
    ui! {
        WindowSet(handle: AppWindows, primary: main, accent: Accent::Enji, appearance: app.appearance) {
            MenuBar {
                CommandMenu(label: tr! { "zh-Hans" => "播放控制", "en" => "Controls" }) {
                    CommandItem(id: menu_toggle, label: tr! { "zh-Hans" => "播放 / 暂停", "en" => "Play / Pause" }, shortcut: "cmd+return") => app.toggle()
                    CommandItem(id: menu_next, label: tr! { "zh-Hans" => "下一首", "en" => "Next" }, shortcut: "cmd+right") => app.next_song()
                    CommandItem(id: menu_previous, label: tr! { "zh-Hans" => "上一首", "en" => "Previous" }, shortcut: "cmd+left") => app.previous_song()
                    Divider
                    CommandItem(id: menu_mode, label: tr! { "zh-Hans" => "切换播放方式", "en" => "Next Play Mode" }, shortcut: "cmd+r") => app.next_mode()
                    CommandItem(id: menu_like, label: tr! { "zh-Hans" => "喜欢 / 取消喜欢", "en" => "Like / Unlike" }, shortcut: "cmd+l", enabled: app.login == Login::In) => app.toggle_like()
                }
            }
            // Set to stay in the background, closing the window leaves
            // Enji playing here.
            StatusItem(shown: app.on_close == CloseAction::Background, image: "assets/tray.svg", label: tr! { "zh-Hans" => "Enji Music", "en" => "Enji Music" }) {
                CommandItem(id: tray_toggle, label: tr! { "zh-Hans" => "播放 / 暂停", "en" => "Play / Pause" }) => app.toggle()
                CommandItem(id: tray_previous, label: tr! { "zh-Hans" => "上一首", "en" => "Previous" }) => app.previous_song()
                CommandItem(id: tray_next, label: tr! { "zh-Hans" => "下一首", "en" => "Next" }) => app.next_song()
            }
            Window(id: main, title: tr! { "zh-Hans" => "Enji Music", "en" => "Enji Music" }, initial: open, chrome: unified, width: 1200, height: 780, min_width: 960, min_height: 700) {
                main_window(app: app)
            }
        }
    }
}

#[component]
pub fn main_window(app: enji::Client, #[session] nav: ShellDraft, #[session] draft: MainDraft) -> impl View {
    ui! {
        Stack(align: bottom) {
            // While a song plays, its cover blurred behind the whole window,
            // under the title bar too, washed so the pages stay easy to read.
            if let Some(song) = app.current {
                if app.backdrop {
                    Stack(under_bars: true) {
                        Canvas(label: song.name) {
                            Shape(kind: rect, fill: surface.canvas)
                            Shape(kind: rect, fill_from: song.cover)
                        }
                        Image(source: song.cover, fit: cover, blur: 90)
                        Canvas(label: song.name) {
                            Shape(kind: rect, fill: surface.canvas, opacity: 0.6)
                        }
                    }
                }
            }
            // Tabs and a small player on a phone, a sidebar and a bar elsewhere.
            Adaptive(id: shell) {
                Layout(size: compact) { phone_shell(app: app) }
                // A phone on its side is wide but has no height to spare.
                Layout(size: expanded) {
                    if window.short {
                        phone_shell(app: app)
                    } else {
                        desktop_shell(app: app)
                    }
                }
                Layout(size: tv) { tv_shell(app: app) }
            }
            // What modules show over the pages, such as search on a phone.
            Slot(id: overlays)
            // The page the condition shows moves in and out itself: the
            // layer is what appears, not something inside it.
            // Turning the phone changes what is in the layer, not the layer.
            if draft.now && !window.tv && (window.compact || window.short) {
                Stack(appear: sheet, disappear: sheet) {
                    if window.compact {
                        phone_now_playing(app: app)
                    } else {
                        side_now_playing(app: app)
                    }
                }
            }
            if draft.now && !window.short && !window.compact && !window.tv {
                Stack(under_bars: true, appear: sheet, disappear: sheet) {
                    now_playing(app: app)
                }
            }
            if app.login != Login::Out && app.login != Login::In {
                sign_in(app: app)
            }
            if let Some(song) = app.adding {
                playlist_chooser(app: app, song: song)
            }
            if app.naming {
                naming_dialog(app: app)
            }
            if app.plugins_open {
                plugins_dialog(app: app)
            }
            if app.together_open {
                together_dialog(app: app)
            }
            if app.report_url != "" && !window.tv {
                report_dialog(app: app)
            }
            if let Some(list) = app.deleting {
                delete_dialog(app: app, list: list)
            }
            if let Some(song) = app.matching {
                match_dialog(app: app, song: song)
            }
            // On a phone, over the small player and the tabs.
            if app.has_notice {
                Column(padding_x: 24, padding_y: if window.compact || window.short && !window.tv { 124 } else { 24 }) {
                    Toast(content: app.notice, icon: "checkmark.circle.fill", key: app.notice_key)
                }
            }
            if app.has_error {
                Column(padding_x: 24, padding_y: if window.compact || window.short && !window.tv { 124 } else { 24 }) {
                    Toast(content: app.error, key: app.error_key)
                }
            }
            // Roaming that started with the app plays on the playing page. A
            // watch also runs once as it appears, before that has happened.
            Watch(value: app.fm_opened) => draft.now.set(app.fm_opened > 0)
            // A page chosen in the sidebar shows instead of an open playlist;
            // the account's page opens its own playlist, if it has one.
            Watch(value: nav.page) => app.page_changed(nav.page == Page::Library)
        }
    }
}

/// A sidebar of pages, the chosen one beside it over the player bar.
#[component]
pub fn desktop_shell(app: enji::Client, #[session] nav: ShellDraft) -> impl View {
    ui! {
        SplitView {
            Sidebar(id: pages, selection: nav.page, fallback: Page::Local) {
                SidebarHeader(role: title) {
                    Row(gap: 8, align: center) {
                        Image(source: "assets/mark.png", width: 22, height: 22, label: tr! { "zh-Hans" => "Enji Music", "en" => "Enji Music" })
                        Text(content: tr! { "zh-Hans" => "Enji Music", "en" => "Enji Music" }, style: headline, bold: true)
                    }
                }
                Pages(slot: main)
                SidebarItem(value: Page::Local, label: tr! { "zh-Hans" => "本地音乐", "en" => "Local Music" }, icon: "music.note")
                SidebarItem(value: Page::History, label: tr! { "zh-Hans" => "播放记录", "en" => "History" }, icon: "clock.arrow.circlepath")
                if app.netease {
                    SidebarItem(value: Page::Library, label: tr! { "zh-Hans" => "我的歌单", "en" => "My Playlists" }, icon: "music.note.list")
                }
                SidebarItem(value: Page::Settings, label: tr! { "zh-Hans" => "设置", "en" => "Settings" }, icon: "gearshape")
                SidebarFooter {
                    if app.netease {
                        account(app: app)
                    }
                }
            }
            Detail(titled: false) {
                Column(gap: 0) {
                    Stack {
                        match nav.page {
                            Page::Library => { page_view(app: app, page: Page::Library) }
                            Page::Local => { page_view(app: app, page: Page::Local) }
                            Page::History => { page_view(app: app, page: Page::History) }
                            Page::Settings => { page_view(app: app, page: Page::Settings) }
                            _ => { PageContent(slot: main, selected: nav.page) }
                        }
                    }
                    Divider
                    player_bar(app: app)
                }
            }
        }
    }
}

/// The pages in tabs, the playing song in a small player over them, and
/// what modules put at the top trailing corner, such as search.
#[component]
pub fn phone_shell(app: enji::Client, #[session] nav: ShellDraft) -> impl View {
    ui! {
        Stack(align: top_trailing) {
            Tabs(id: phone_tabs, selection: nav.page, fallback: Page::Library, bar: floating) {
                Pages(slot: main)
                Tab(value: Page::Library, label: tr! { "zh-Hans" => "我的", "en" => "Mine" }, icon: "music.note.list") {
                    page_view(app: app, page: Page::Library)
                }
                Tab(value: Page::Settings, label: tr! { "zh-Hans" => "设置", "en" => "Settings" }, icon: "gearshape") {
                    page_view(app: app, page: Page::Settings)
                }
                TabAccessory {
                    mini_player(app: app)
                }
            }
            Column(padding_x: 16, padding_y: 12) {
                Slot(id: phone_actions)
            }
        }
    }
}

/// The pages in tabs across the top, for a remote: the playing song is one
/// of them, and the rest stand under it.
#[component]
pub fn tv_shell(app: enji::Client, #[session] nav: ShellDraft) -> impl View {
    ui! {
        Tabs(id: tv_tabs, selection: nav.page, fallback: Page::Library) {
            Pages(slot: main, all: true)
            Tab(value: Page::Library, label: tr! { "zh-Hans" => "我的", "en" => "Mine" }, icon: "music.note.list") {
                page_view(app: app, page: Page::Library)
            }
            Tab(value: Page::Playing, label: tr! { "zh-Hans" => "正在播放", "en" => "Now Playing" }, icon: "play.fill") {
                tv_now_playing(app: app)
            }
            Tab(value: Page::Settings, label: tr! { "zh-Hans" => "设置", "en" => "Settings" }, icon: "gearshape") {
                page_view(app: app, page: Page::Settings)
            }
        }
    }
}

/// One of the core's pages, or the playlist opened from it; with NetEase
/// off, the local music in place of the account's page.
#[component]
pub fn page_view(app: enji::Client, page: Page) -> impl View {
    ui! {
        if app.detail != Detail::Page {
            detail_view(app: app)
        } else {
            match page {
                Page::Library => {
                    if window.compact || window.short && !window.tv {
                        mine_page(app: app)
                    } else if app.netease || window.tv {
                        library_page(app: app)
                    } else {
                        local_page(app: app)
                    }
                }
                // A TV has no folders to choose music from.
                Page::Local => {
                    if !window.tv {
                        local_page(app: app)
                    }
                }
                Page::History => { history_page(app: app) }
                Page::Settings => { settings_page(app: app) }
                _ => {}
            }
        }
    }
}

/// The switches of the optional modules, in the settings' features.
#[component]
pub fn module_switches(app: enji::Client, shell: shell::Client) -> impl View {
    ui! {
        Column(gap: 0) {
            if build.modules.recommend {
                if app.netease {
                    Column(padding_x: 16) { Divider }
                    Row(gap: 12, padding_x: 16, padding_y: 10, align: center) {
                        Column(width: 22, align: center) {
                            Stack(variant: quiet) {
                                Label(title: tr! { "zh-Hans" => "推荐", "en" => "For You" }, icon: "sparkles", display: icon, font_size: 15)
                            }
                        }
                        Text(content: tr! { "zh-Hans" => "推荐", "en" => "For You" })
                        Spacer
                        Switch(id: module_recommend, label: tr! { "zh-Hans" => "推荐", "en" => "For You" }, label_hidden: true, checked: shell.modules.recommend, style: switch) => shell.set_module("recommend", shell.modules.recommend)
                    }
                }
            }
            if build.modules.charts {
                if app.netease {
                    Column(padding_x: 16) { Divider }
                    Row(gap: 12, padding_x: 16, padding_y: 10, align: center) {
                        Column(width: 22, align: center) {
                            Stack(variant: quiet) {
                                Label(title: tr! { "zh-Hans" => "排行榜", "en" => "Charts" }, icon: "chart.bar", display: icon, font_size: 15)
                            }
                        }
                        Text(content: tr! { "zh-Hans" => "排行榜", "en" => "Charts" })
                        Spacer
                        Switch(id: module_charts, label: tr! { "zh-Hans" => "排行榜", "en" => "Charts" }, label_hidden: true, checked: shell.modules.charts, style: switch) => shell.set_module("charts", shell.modules.charts)
                    }
                }
            }
            if build.modules.search {
                Column(padding_x: 16) { Divider }
                Row(gap: 12, padding_x: 16, padding_y: 10, align: center) {
                    Column(width: 22, align: center) {
                        Stack(variant: quiet) {
                            Label(title: tr! { "zh-Hans" => "搜索", "en" => "Search" }, icon: "magnifyingglass", display: icon, font_size: 15)
                        }
                    }
                    Text(content: tr! { "zh-Hans" => "搜索", "en" => "Search" })
                    Spacer
                    Switch(id: module_search, label: tr! { "zh-Hans" => "搜索", "en" => "Search" }, label_hidden: true, checked: shell.modules.search, style: switch) => shell.set_module("search", shell.modules.search)
                }
            }
        }
    }
}
