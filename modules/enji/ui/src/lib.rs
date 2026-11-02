//! The interface of Enji's core: the player bar and the small player, the
//! playing song with its lyrics following the music, the lists of songs
//! and playlists, the account's pages, the local music, the history and
//! the settings. The shell places them; the modules use them for their
//! own pages.

use bern::prelude::*;
use enji_protocol::enji::{self, Detail, Login, Panel, PlayMode, SignInWay, Source};

/// What a phone's own tab shows: its overview, or one of the lists it
/// leads to.
#[bern::value]
pub enum Shelf {
    #[label(tr! { "zh-Hans" => "我的", "en" => "Mine" })]
    Mine,
    #[label(tr! { "zh-Hans" => "本地音乐", "en" => "Local Music" })]
    Local,
    #[label(tr! { "zh-Hans" => "播放记录", "en" => "History" })]
    History,
}

#[bern::session]
pub struct MainDraft {
    /// The playing song is shown over everything.
    pub now: bool,
    /// The lyric line the lyrics stand at.
    pub line: i64,
    /// The lyrics are being scrolled by hand.
    pub browsing: bool,
    /// On a phone, what its own tab shows.
    pub shelf: Shelf,
    /// A folder chosen to add to the local library.
    pub folder: MediaSource,
    /// A plugin package chosen to install.
    pub plugin_file: MediaSource,
    /// A new playlist's name.
    pub new_name: String,
    /// Other words to find the NetEase song a local song is.
    pub match_words: String,
    /// The account being signed in: a phone number and the code texted to
    /// it, or the password instead; an email address.
    pub phone: String,
    pub sms_code: String,
    pub by_password: bool,
    pub password: String,
    pub email: String,
}







// MARK: - Sidebar

#[component]
pub fn account(app: enji::Client) -> impl View {
    ui! {
        if app.login == Login::In {
            Row(gap: 10, align: center) {
                Image(source: app.avatar, width: 30, height: 30, fit: cover, corner: 15)
                Text(content: app.nickname, style: headline, lines: 1)
                Spacer
                IconButton(id: sign_out, icon: "rectangle.portrait.and.arrow.right", prominence: plain, label: tr! { "zh-Hans" => "退出登录", "en" => "Sign Out" }) => app.sign_out()
            }
        } else {
            Button(id: sign_in, label: tr! { "zh-Hans" => "登录网易云音乐", "en" => "Sign In" }, icon: "person.crop.circle", prominence: prominent, width: fill) => app.sign_in()
        }
    }
}

/// Signing in with the NetEase app: the code on a white card, who scanned
/// it once someone has, and a fresh code once it has run out.
#[component]
pub fn sign_in(app: enji::Client) -> impl View {
    ui! {
        Modal(width: 380) {
            Stack(align: top_trailing) {
                Column(gap: 20, padding_x: 32, padding_y: 32, align: center) {
                    Image(source: "assets/mark.png", width: 44, height: 44, label: tr! { "zh-Hans" => "Enji Music", "en" => "Enji Music" })
                    Text(content: tr! { "zh-Hans" => "登录网易云音乐", "en" => "Sign In to NetEase Cloud Music" }, style: title2, bold: true)
                    Picker(id: sign_in_way, label: tr! { "zh-Hans" => "登录方式", "en" => "Sign In With" }, label_hidden: true, selected: app.sign_in_way, style: segmented) => app.set_sign_in_way(app.sign_in_way)
                    match app.sign_in_way {
                        SignInWay::Code => { sign_in_code(app: app) }
                        SignInWay::Phone => { sign_in_phone(app: app) }
                        SignInWay::Email => { sign_in_email(app: app) }
                    }
                }
                Column(padding: 14) {
                    IconButton(id: close_sign_in, icon: "xmark", prominence: plain, label: tr! { "zh-Hans" => "关闭", "en" => "Close" }) => app.cancel_sign_in()
                }
            }
        }
    }
}

/// The QR code to scan with the NetEase app, and the steps to it.
#[component]
pub fn sign_in_code(app: enji::Client) -> impl View {
    ui! {
        Column(gap: 20, align: center) {
            Stack(variant: qr, appearance: light) {
                Column(padding: 16) {
                    match app.login {
                        Login::Waiting => {
                            Image(source: app.qr, width: 200, height: 200, label: tr! { "zh-Hans" => "登录二维码", "en" => "Sign-in code" }, appear: fade)
                        }
                        Login::Scanned => {
                            Column(gap: 10, width: 200, height: 200, align: center, appear: pop) {
                                Spacer
                                Image(source: app.scanner_avatar, width: 76, height: 76, fit: cover, corner: 38)
                                Text(content: app.scanner, style: headline, lines: 1)
                                Text(content: tr! { "zh-Hans" => "在手机上确认登录", "en" => "Confirm on your phone" }, style: caption, emphasis: secondary)
                                Spacer
                            }
                        }
                        Login::Expired => {
                            Stack {
                                Image(source: app.qr, width: 200, height: 200, blur: 10)
                                Column(gap: 12, align: center) {
                                    Text(content: tr! { "zh-Hans" => "二维码已失效", "en" => "The code has expired" }, style: headline)
                                    Button(id: refresh_code, label: tr! { "zh-Hans" => "刷新", "en" => "Refresh" }, icon: "arrow.clockwise", prominence: prominent) => app.set_sign_in_way(SignInWay::Code)
                                }
                            }
                        }
                        _ => {
                            Column(width: 200, height: 200, align: center) {
                                Spacer
                                Progress(label: tr! { "zh-Hans" => "正在获取二维码", "en" => "Getting the code" })
                                Spacer
                            }
                        }
                    }
                }
            }
            // Across the sheet, so the way to close it stays in its corner.
            Row(gap: 18, align: center) {
                Spacer
                Label(title: tr! { "zh-Hans" => "打开 App", "en" => "Open the app" }, icon: "iphone", style: caption)
                Label(title: tr! { "zh-Hans" => "扫一扫", "en" => "Scan" }, icon: "qrcode.viewfinder", style: caption)
                Label(title: tr! { "zh-Hans" => "确认登录", "en" => "Confirm" }, icon: "checkmark.circle", style: caption)
                Spacer
            }
        }
    }
}

/// A phone number, and the code texted to it or the account's password.
#[component]
pub fn sign_in_phone(app: enji::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Column(gap: 12) {
            TextInput(id: phone, label: tr! { "zh-Hans" => "手机号", "en" => "Phone Number" }, label_hidden: true, value: draft.phone, prompt: tr! { "zh-Hans" => "手机号", "en" => "Phone number" })
            if draft.by_password {
                TextInput(id: phone_password, label: tr! { "zh-Hans" => "密码", "en" => "Password" }, label_hidden: true, kind: secure, value: draft.password, prompt: tr! { "zh-Hans" => "密码", "en" => "Password" }) => app.sign_in_by_password(draft.phone, draft.password)
            } else {
                Row(gap: 8, align: center) {
                    TextInput(id: sms_code, label: tr! { "zh-Hans" => "验证码", "en" => "Code" }, label_hidden: true, value: draft.sms_code, prompt: tr! { "zh-Hans" => "验证码", "en" => "Code" }) => app.sign_in_by_sms(draft.phone, draft.sms_code)
                    if app.sms_wait > 0 {
                        Button(id: sms_waiting, label: tr! { "zh-Hans" => "{n} 秒后重发", "en" => "Resend in {n}s"; n: app.sms_wait }, enabled: false) => app.send_sms_code(draft.phone)
                    } else {
                        Button(id: send_sms, label: tr! { "zh-Hans" => "获取验证码", "en" => "Get Code" }, enabled: draft.phone != "" && !app.signing) => app.send_sms_code(draft.phone)
                    }
                }
            }
            sign_in_error(app: app)
            if app.signing {
                Button(id: phone_signing, label: tr! { "zh-Hans" => "正在登录…", "en" => "Signing In…" }, prominence: prominent, width: fill, height: 36, enabled: false) => app.cancel_sign_in()
            } else if draft.by_password {
                Button(id: phone_password_sign_in, label: tr! { "zh-Hans" => "登录", "en" => "Sign In" }, prominence: prominent, width: fill, height: 36, enabled: draft.phone != "" && draft.password != "", clears: draft.password) => app.sign_in_by_password(draft.phone, draft.password)
            } else {
                Button(id: phone_sign_in, label: tr! { "zh-Hans" => "登录", "en" => "Sign In" }, prominence: prominent, width: fill, height: 36, enabled: draft.phone != "" && draft.sms_code != "") => app.sign_in_by_sms(draft.phone, draft.sms_code)
            }
            Row {
                Spacer
                if draft.by_password {
                    Button(id: use_sms, label: tr! { "zh-Hans" => "用验证码登录", "en" => "Use a Code Instead" }, prominence: plain) => draft.by_password.toggle()
                } else {
                    Button(id: use_password, label: tr! { "zh-Hans" => "用密码登录", "en" => "Use the Password Instead" }, prominence: plain) => draft.by_password.toggle()
                }
                Spacer
            }
        }
    }
}

/// A NetEase email address and its password.
#[component]
pub fn sign_in_email(app: enji::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Column(gap: 12) {
            TextInput(id: email, label: tr! { "zh-Hans" => "邮箱", "en" => "Email" }, label_hidden: true, value: draft.email, prompt: tr! { "zh-Hans" => "网易邮箱", "en" => "NetEase email" })
            TextInput(id: email_password, label: tr! { "zh-Hans" => "密码", "en" => "Password" }, label_hidden: true, kind: secure, value: draft.password, prompt: tr! { "zh-Hans" => "密码", "en" => "Password" }) => app.sign_in_by_email(draft.email, draft.password)
            sign_in_error(app: app)
            if app.signing {
                Button(id: email_signing, label: tr! { "zh-Hans" => "正在登录…", "en" => "Signing In…" }, prominence: prominent, width: fill, height: 36, enabled: false) => app.cancel_sign_in()
            } else {
                Button(id: email_sign_in, label: tr! { "zh-Hans" => "登录", "en" => "Sign In" }, prominence: prominent, width: fill, height: 36, enabled: draft.email != "" && draft.password != "", clears: draft.password) => app.sign_in_by_email(draft.email, draft.password)
            }
        }
    }
}

/// What NetEase refused, while it stands.
#[component]
pub fn sign_in_error(app: enji::Client) -> impl View {
    ui! {
        if app.sign_in_error != "" {
            Text(content: app.sign_in_error, style: footnote, color: status.error)
        }
    }
}

// MARK: - Pages




/// The account's playlists, with a way to make another.
#[component]
pub fn library_page(app: enji::Client) -> impl View {
    ui! {
        ScrollView {
            Column(gap: 20, padding: if window.compact { 18 } else { 32 }) {
                Row(gap: 12, align: center) {
                    Text(content: tr! { "zh-Hans" => "我的歌单", "en" => "My Playlists" }, style: large_title)
                    Spacer
                    if app.login == Login::In {
                        Button(id: new_playlist, label: tr! { "zh-Hans" => "新建歌单", "en" => "New Playlist" }, icon: "plus", shape: capsule, height: 32) => app.begin_naming()
                    }
                }
                if app.login == Login::In {
                    // The songs it likes first, then the lists it made, then
                    // those it saved.
                    if let Some(liked) = app.liked_list {
                        liked_card(app: app, list: liked)
                    }
                    if app.has_created {
                    Column(gap: 14, align: leading) {
                        Text(content: tr! { "zh-Hans" => "创建的歌单", "en" => "Your Playlists" }, style: title3, bold: true)
                        Grid(min_width: 160, gap: 22, cell_ratio: 1, cell_extra: 58) {
                            ForEach(items: app.created, key: id, stagger: 0.03) |list| {
                                playlist_card(app: app, list: list)
                            }
                        }
                    }
                    }
                    if app.has_collected {
                    Column(gap: 14, align: leading) {
                        Text(content: tr! { "zh-Hans" => "收藏的歌单", "en" => "Saved Playlists" }, style: title3, bold: true)
                        Grid(min_width: 160, gap: 22, cell_ratio: 1, cell_extra: 58) {
                            ForEach(items: app.collected, key: id, stagger: 0.03) |list| {
                                playlist_card(app: app, list: list)
                            }
                        }
                    }
                    }
                } else {
                    sign_in_card(app: app)
                }
            }
        }
    }
}

/// A phone's own tab: the local music and the history at the top, the
/// account's playlists under them; each list opens in its place.
#[component]
pub fn mine_page(app: enji::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        match draft.shelf {
            Shelf::Local => { local_page(app: app) }
            Shelf::History => { history_page(app: app) }
            Shelf::Mine => {
                ScrollView {
                    Column(gap: 22, padding: 18) {
                        Text(content: tr! { "zh-Hans" => "我的", "en" => "Mine" }, style: large_title)
                        Stack(variant: card) {
                            Column(gap: 0) {
                                Button(id: open_local, label: tr! { "zh-Hans" => "本地音乐", "en" => "Local Music" }, variant: row) {
                                    Row(gap: 12, padding_x: 16, padding_y: 13, align: center) {
                                        Column(width: 22, align: center) {
                                            Stack(variant: tint) {
                                                Label(title: tr! { "zh-Hans" => "本地音乐", "en" => "Local Music" }, icon: "music.note", display: icon, font_size: 16)
                                            }
                                        }
                                        Text(content: tr! { "zh-Hans" => "本地音乐", "en" => "Local Music" })
                                        Spacer
                                        Text(content: tr! { "zh-Hans" => "{count} 首", "en" => "{count} songs"; count: app.local_count }, emphasis: secondary)
                                        Stack(variant: quiet) {
                                            Label(title: tr! { "zh-Hans" => "打开", "en" => "Open" }, icon: "chevron.right", display: icon, font_size: 13)
                                        }
                                    }
                                } => draft.shelf.set(Shelf::Local)
                                Column(padding_x: 16) { Divider }
                                Button(id: open_history, label: tr! { "zh-Hans" => "播放记录", "en" => "History" }, variant: row) {
                                    Row(gap: 12, padding_x: 16, padding_y: 13, align: center) {
                                        Column(width: 22, align: center) {
                                            Stack(variant: tint) {
                                                Label(title: tr! { "zh-Hans" => "播放记录", "en" => "History" }, icon: "clock.arrow.circlepath", display: icon, font_size: 16)
                                            }
                                        }
                                        Text(content: tr! { "zh-Hans" => "播放记录", "en" => "History" })
                                        Spacer
                                        Stack(variant: quiet) {
                                            Label(title: tr! { "zh-Hans" => "打开", "en" => "Open" }, icon: "chevron.right", display: icon, font_size: 13)
                                        }
                                    }
                                } => draft.shelf.set(Shelf::History)
                            }
                        }
                        if app.netease {
                            if app.login == Login::In {
                                Row(gap: 12, align: center) {
                                    Text(content: tr! { "zh-Hans" => "我的歌单", "en" => "My Playlists" }, style: title2, bold: true)
                                    Spacer
                                    IconButton(id: new_playlist_phone, icon: "plus", font_size: 18, prominence: plain, label: tr! { "zh-Hans" => "新建歌单", "en" => "New Playlist" }) => app.begin_naming()
                                }
                            }
                            if app.login == Login::In {
                                if let Some(liked) = app.liked_list {
                                    liked_card(app: app, list: liked)
                                }
                                if app.has_created {
                                Column(gap: 14, align: leading) {
                        Text(content: tr! { "zh-Hans" => "创建的歌单", "en" => "Your Playlists" }, style: title3, bold: true)
                        Grid(min_width: 150, gap: 18, cell_ratio: 1, cell_extra: 58) {
                            ForEach(items: app.created, key: id, stagger: 0.03) |list| {
                                playlist_card(app: app, list: list)
                            }
                        }
                    }
                                }
                                if app.has_collected {
                                Column(gap: 14, align: leading) {
                        Text(content: tr! { "zh-Hans" => "收藏的歌单", "en" => "Saved Playlists" }, style: title3, bold: true)
                        Grid(min_width: 150, gap: 18, cell_ratio: 1, cell_extra: 58) {
                            ForEach(items: app.collected, key: id, stagger: 0.03) |list| {
                                playlist_card(app: app, list: list)
                            }
                        }
                    }
                                }
                            } else {
                                sign_in_card(app: app)
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A record sliding out of its sleeve, for a library with no music yet.
#[component]
pub fn sleeve_art() -> impl View {
    ui! {
        Stack(align: center) {
            // The record black in a dark window too, as records are.
            Stack(appearance: light) {
                Canvas(width: 220, height: 160, label: tr! { "zh-Hans" => "唱片", "en" => "Record" }) {
                    Shape(kind: ellipse, x: 92, y: 26, width: 108, height: 108, fill: text.primary, opacity: 0.92)
                    Shape(kind: ellipse, x: 103, y: 37, width: 86, height: 86, stroke: surface.canvas, stroke_width: 1, opacity: 0.16)
                    Shape(kind: ellipse, x: 114, y: 48, width: 64, height: 64, stroke: surface.canvas, stroke_width: 1, opacity: 0.12)
                    Shape(kind: ellipse, x: 130, y: 64, width: 32, height: 32, fill: accent)
                    Shape(kind: ellipse, x: 143, y: 77, width: 6, height: 6, fill: surface.canvas)
                }
            }
            // The sleeve in the accent, lit from the top, its title in white.
            Canvas(width: 220, height: 160) {
                Shape(kind: rounded, x: 28, y: 22, width: 116, height: 116, corner: 16, fill: accent)
                Shape(kind: rounded, x: 28, y: 22, width: 116, height: 116, corner: 16, fill: on_accent, opacity: 0.18, fade: down)
                Shape(kind: capsule, x: 44, y: 104, width: 54, height: 7, fill: on_accent, opacity: 0.8)
                Shape(kind: capsule, x: 44, y: 117, width: 34, height: 7, fill: on_accent, opacity: 0.5)
            }
        }
    }
}

/// A turntable at rest, its arm off the record, for a history with nothing
/// in it.
#[component]
pub fn turntable_art() -> impl View {
    ui! {
        Stack(align: center) {
            Canvas(width: 220, height: 160, label: tr! { "zh-Hans" => "唱机", "en" => "Turntable" }) {
                Shape(kind: rounded, x: 26, y: 16, width: 168, height: 128, corner: 20, fill: surface.raised)
                Shape(kind: rounded, x: 26, y: 16, width: 168, height: 128, corner: 20, stroke: control.border, stroke_width: 1)
                Shape(kind: ellipse, x: 162, y: 28, width: 18, height: 18, fill: fill.strong)
                Shape(kind: capsule, x: 168, y: 38, width: 6, height: 72, fill: text.secondary, opacity: 0.55)
                Shape(kind: rounded, x: 163, y: 104, width: 16, height: 20, corner: 4, fill: text.secondary, opacity: 0.7)
            }
            Stack(appearance: light) {
                Canvas(width: 220, height: 160) {
                    Shape(kind: ellipse, x: 40, y: 26, width: 108, height: 108, fill: text.primary, opacity: 0.92)
                    Shape(kind: ellipse, x: 51, y: 37, width: 86, height: 86, stroke: surface.canvas, stroke_width: 1, opacity: 0.16)
                    Shape(kind: ellipse, x: 62, y: 48, width: 64, height: 64, stroke: surface.canvas, stroke_width: 1, opacity: 0.12)
                    Shape(kind: ellipse, x: 78, y: 64, width: 32, height: 32, fill: accent)
                    Shape(kind: ellipse, x: 91, y: 77, width: 6, height: 6, fill: surface.canvas)
                }
            }
        }
    }
}

/// Covers fanned behind one another, the front one with a heart: what the
/// account brings.
#[component]
pub fn covers_art() -> impl View {
    ui! {
        Stack(align: center) {
            Canvas(width: 240, height: 150, label: tr! { "zh-Hans" => "歌单", "en" => "Playlists" }) {
                Shape(kind: rounded, x: 106, y: 6, width: 100, height: 100, corner: 16, fill: accent, opacity: 0.16)
                Shape(kind: rounded, x: 87, y: 13, width: 106, height: 106, corner: 17, fill: accent, opacity: 0.32)
                Shape(kind: rounded, x: 65, y: 20, width: 110, height: 110, corner: 18, fill: accent)
            }
            Stack(appearance: dark) {
                Label(title: tr! { "zh-Hans" => "喜欢的音乐", "en" => "Liked Songs" }, icon: "heart.fill", display: icon, font_size: 40)
            }
        }
    }
}

/// Signing in, offered where the account's playlists would be: what it
/// brings, and the way in.
#[component]
pub fn sign_in_card(app: enji::Client) -> impl View {
    ui! {
        Stack(variant: card) {
            Row(padding_x: 24, padding_y: 32) {
                Spacer
                Column(gap: 22, align: center) {
                    covers_art()
                    Text(content: tr! { "zh-Hans" => "登录网易云音乐", "en" => "Sign In to NetEase Cloud Music" }, style: title2, bold: true)
                    Flow(gap: 8, align: center) {
                        Stack(variant: tag) {
                            Row(gap: 6, padding_x: 12, padding_y: 7, align: center) {
                                Stack(variant: tint) {
                                    Label(title: tr! { "zh-Hans" => "我喜欢的音乐", "en" => "Liked Songs" }, icon: "heart.fill", display: icon, font_size: 13)
                                }
                                Text(content: tr! { "zh-Hans" => "我喜欢的音乐", "en" => "Liked Songs" }, style: subheadline)
                            }
                        }
                        Stack(variant: tag) {
                            Row(gap: 6, padding_x: 12, padding_y: 7, align: center) {
                                Stack(variant: tint) {
                                    Label(title: tr! { "zh-Hans" => "创建的歌单", "en" => "Your Playlists" }, icon: "music.note.list", display: icon, font_size: 13)
                                }
                                Text(content: tr! { "zh-Hans" => "创建的歌单", "en" => "Your Playlists" }, style: subheadline)
                            }
                        }
                        Stack(variant: tag) {
                            Row(gap: 6, padding_x: 12, padding_y: 7, align: center) {
                                Stack(variant: tint) {
                                    Label(title: tr! { "zh-Hans" => "收藏的歌单", "en" => "Saved Playlists" }, icon: "star.fill", display: icon, font_size: 13)
                                }
                                Text(content: tr! { "zh-Hans" => "收藏的歌单", "en" => "Saved Playlists" }, style: subheadline)
                            }
                        }
                    }
                    Button(id: card_sign_in, label: tr! { "zh-Hans" => "登录", "en" => "Sign In" }, icon: "person.crop.circle", prominence: prominent, shape: capsule, height: 40) => app.sign_in()
                }
                Spacer
            }
        }
    }
}

/// The songs the account likes, wide above its other playlists.
#[component]
pub fn liked_card(app: enji::Client, list: enji::Playlist) -> impl View {
    ui! {
        Stack(variant: card) {
            Row(gap: 8, padding_x: 8, padding_y: 8, align: center) {
                Button(id: open_liked, label: list.name, variant: row) {
                    Row(gap: 18, padding_x: 8, padding_y: 8, align: center) {
                        Image(source: list.cover, width: 88, height: 88, fit: cover, corner: 12, glow: 1, glow_opacity: 0.3)
                        Column(gap: 6, align: leading) {
                            Row(gap: 8, align: center) {
                                Stack(variant: tint) {
                                    Label(title: list.name, icon: "heart.fill", display: icon, font_size: 18)
                                }
                                Text(content: tr! { "zh-Hans" => "我喜欢的音乐", "en" => "Liked Songs" }, style: title2, bold: true)
                            }
                            Text(content: list.detail, style: subheadline, emphasis: secondary)
                        }
                        Spacer
                    }
                } => app.show_playlist(list)
                IconButton(id: play_liked, icon: "play.fill", prominence: prominent, height: 48, font_size: 20, label: tr! { "zh-Hans" => "播放", "en" => "Play" }) => app.play_playlist(list.id)
                Column(width: 8) { Spacer }
            }
        }
    }
}

/// On a phone, the way back from a list of its own tab.
#[component]
pub fn shelf_back(#[session] draft: MainDraft) -> impl View {
    ui! {
        if window.compact {
            Row(padding_x: 4) {
                IconButton(id: shelf_back, icon: "chevron.left", font_size: 17, prominence: plain, label: tr! { "zh-Hans" => "返回", "en" => "Back" }) => draft.shelf.set(Shelf::Mine)
                Spacer
            }
        }
    }
}

/// The music in the local folders: how much there is, a way to play it
/// all and to add a folder, then its songs, the newest first.
#[component]
pub fn local_page(app: enji::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        ScrollView {
            Column(gap: 0, padding_x: if window.compact { 8 } else { 24 }, padding_y: if window.compact { 4 } else { 20 }) {
                shelf_back()
                Row(gap: 12, padding_x: 8, padding_y: 12, align: center) {
                    Column(gap: 6, align: leading) {
                        Text(content: tr! { "zh-Hans" => "本地音乐", "en" => "Local Music" }, style: large_title)
                        if app.has_local && app.local_hours > 0 {
                            Text(content: tr! { "zh-Hans" => "{count} 首 · {hours} 小时 {minutes} 分钟", "en" => "{count} songs · {hours} h {minutes} min"; count: app.local_count, hours: app.local_hours, minutes: app.local_minutes }, style: subheadline, emphasis: secondary)
                        } else if app.has_local {
                            Text(content: tr! { "zh-Hans" => "{count} 首 · {minutes} 分钟", "en" => "{count} songs · {minutes} min"; count: app.local_count, minutes: app.local_minutes }, style: subheadline, emphasis: secondary)
                        }
                    }
                    Spacer
                    if app.scanning && app.scan_total > 0 {
                        Progress(label: tr! { "zh-Hans" => "正在读取 {done}/{total}", "en" => "Reading {done} of {total}"; done: app.scan_done, total: app.scan_total })
                    } else if app.scanning {
                        Progress(label: tr! { "zh-Hans" => "正在读取文件夹", "en" => "Reading the folders" })
                    } else if app.auto_matching {
                        Progress(label: tr! { "zh-Hans" => "正在匹配网易云 {done}/{total}", "en" => "Matching on NetEase {done}/{total}"; done: app.match_done, total: app.match_total })
                    }
                }
                // What is downloading, each with how far along it is.
                if app.has_downloads {
                    Column(gap: 6, padding_x: 8, padding_y: 8) {
                        Text(content: tr! { "zh-Hans" => "正在下载", "en" => "Downloading" }, style: footnote, bold: true, emphasis: secondary)
                        ForEach(items: app.downloads, key: id) |download| {
                            Row(gap: 12, padding_y: 4, align: center) {
                                Column(gap: 2, align: leading) {
                                    Text(content: download.name, lines: 1)
                                    Text(content: download.artists, style: caption, emphasis: secondary, lines: 1)
                                }
                                Spacer
                                Column(width: if window.compact { 96 } else { 200 }) {
                                    Progress(value: download.progress, total: 1, label: tr! { "zh-Hans" => "下载进度", "en" => "Download Progress" })
                                }
                            }
                        }
                    }
                }
                if app.has_local {
                    Row(gap: 10, padding_x: 8, padding_y: 8) {
                        Button(id: play_local, label: tr! { "zh-Hans" => "播放全部", "en" => "Play All" }, icon: "play.fill", prominence: prominent, shape: capsule, height: 36) => app.play(Source::Local, 0)
                        if app.phone_folder == "" {
                            Button(id: add_folder, label: tr! { "zh-Hans" => "添加文件夹", "en" => "Add Folder" }, icon: "plus", shape: capsule, height: 36, pick: draft.folder, files: "folder") => app.add_folder(draft.folder)
                        }
                        Picker(id: local_order, label: tr! { "zh-Hans" => "排序", "en" => "Sort By" }, label_hidden: true, selected: app.order, style: menu, shape: capsule, height: 36) => app.set_order(app.order)
                        IconButton(id: rescan, icon: "arrow.clockwise", font_size: 16, prominence: plain, label: tr! { "zh-Hans" => "重新扫描", "en" => "Scan Again" }) => app.rescan()
                    }
                    song_header(app: app)
                    Column(gap: 2) {
                        ForEach(items: app.local, key: id) |song| {
                            song_row(app: app, song: song, source: Source::Local, full: true, removable: false)
                        }
                    }
                } else if !app.scanning {
                    Row {
                        Spacer
                        Column(gap: 20, padding_y: 64, align: center, appear: rise) {
                            sleeve_art()
                            Text(content: tr! { "zh-Hans" => "添加你的音乐", "en" => "Bring In Your Music" }, style: title2, bold: true)
                            if app.phone_folder != "" {
                                Text(content: tr! { "zh-Hans" => "放进「文件」App 的「我的 iPhone › Enji」", "en" => "Put it in “On My iPhone › Enji” in the Files app" }, style: subheadline, emphasis: secondary)
                                Button(id: rescan_phone, label: tr! { "zh-Hans" => "重新扫描", "en" => "Scan Again" }, icon: "arrow.clockwise", prominence: prominent, shape: capsule, height: 40) => app.rescan()
                            } else {
                                Button(id: add_first_folder, label: tr! { "zh-Hans" => "选择文件夹", "en" => "Choose a Folder" }, icon: "folder", prominence: prominent, shape: capsule, height: 40, pick: draft.folder, files: "folder") => app.add_folder(draft.folder)
                            }
                            Text(content: tr! { "zh-Hans" => "FLAC · ALAC · WAV · AIFF · MP3 · AAC · OGG", "en" => "FLAC · ALAC · WAV · AIFF · MP3 · AAC · OGG" }, style: caption, emphasis: tertiary)
                        }
                        Spacer
                    }
                }
            }
        }
    }
}

/// What was played, the latest first, with when.
#[component]
pub fn history_page(app: enji::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        ScrollView {
            Column(gap: 0, padding_x: if window.compact { 8 } else { 24 }, padding_y: if window.compact { 4 } else { 20 }) {
                shelf_back()
                Row(gap: 12, padding_x: 8, padding_y: 12, align: center) {
                    Text(content: tr! { "zh-Hans" => "播放记录", "en" => "History" }, style: large_title)
                    Spacer
                    if app.has_history {
                        Button(id: clear_history, label: tr! { "zh-Hans" => "清空", "en" => "Clear" }, prominence: plain) => app.clear_history()
                    }
                }
                if app.has_history {
                    Column(gap: 2) {
                        ForEach(items: app.history, key: key) |played| {
                            history_row(app: app, played: played)
                        }
                    }
                } else {
                    Row {
                        Spacer
                        Column(gap: 20, padding_y: 64, align: center, appear: rise) {
                            turntable_art()
                            Text(content: tr! { "zh-Hans" => "还没有听过的歌", "en" => "Nothing Played Yet" }, style: title2, bold: true)
                            Row(gap: 10, align: center) {
                                if app.has_local {
                                    Button(id: play_local_first, label: tr! { "zh-Hans" => "播放本地音乐", "en" => "Play Local Music" }, icon: "play.fill", prominence: prominent, shape: capsule, height: 40) => app.play(Source::Local, 0)
                                }
                                // Where else to find something to play.
                                Slot(id: history_empty)
                            }
                        }
                        Spacer
                    }
                }
            }
        }
    }
}

/// A song of the history: when it last played, and what can be done with
/// it.
#[component]
pub fn history_row(app: enji::Client, played: enji::Played) -> impl View {
    ui! {
        Button(id: played_song, label: played.song.name, variant: row) {
            Row(gap: 14, padding_x: 12, padding_y: 7, align: center) {
                Stack(width: 44, height: 44) {
                    cover_placeholder(side: 44.0)
                    Image(source: played.song.cover, width: 44, height: 44, fit: cover, corner: 6)
                }
                Column(gap: 3, align: leading) {
                    if played.song.id == app.current_id {
                        Text(content: played.song.name, lines: 1, color: accent, bold: true)
                    } else {
                        Text(content: played.song.name, lines: 1)
                    }
                    Text(content: played.song.artists, style: caption, emphasis: secondary, lines: 1)
                }
                Spacer
                Column(width: 96, align: trailing) {
                    Text(content: played.when, style: caption, emphasis: tertiary)
                }
            }
            ContextMenu {
                MenuItem(id: history_next, label: tr! { "zh-Hans" => "下一首播放", "en" => "Play Next" }, icon: "forward.fill") => app.play_next(Source::History, played.key)
                MenuItem(id: history_queue, label: tr! { "zh-Hans" => "加入播放队列", "en" => "Add to Queue" }, icon: "list.bullet") => app.add_to_queue(Source::History, played.key)
                MenuItem(id: history_add, label: tr! { "zh-Hans" => "添加到歌单…", "en" => "Add to Playlist…" }, icon: "plus", enabled: played.song.id > 0 && app.login == Login::In) => app.choose_playlist(Source::History, played.key)
                MenuItem(id: history_artist, label: tr! { "zh-Hans" => "查看歌手", "en" => "Go to Artist" }, icon: "person.crop.circle", enabled: played.song.artist_id > 0) => app.open_artist(played.song.artist_id)
                MenuItem(id: history_album, label: tr! { "zh-Hans" => "查看专辑", "en" => "Go to Album" }, icon: "square.grid.2x2", enabled: played.song.album_id > 0) => app.open_album(played.song.album_id)
                Divider
                MenuItem(id: history_remove, label: tr! { "zh-Hans" => "从播放记录中删除", "en" => "Remove from History" }, icon: "trash", role: destructive) => app.remove_history(played.key)
            }
        } => app.play(Source::History, played.key)
    }
}






/// An album as a card: its cover, name, singer and year.
#[component]
pub fn album_card(app: enji::Client, album: enji::Album) -> impl View {
    ui! {
        Column(gap: 8, appear: rise) {
            Button(id: open_album, label: album.name, variant: card) {
                Image(source: album.cover, aspect: 1, fit: cover, corner: 12, glow: 1)
            } => app.open_album(album.id)
            Text(content: album.name, style: callout, lines: 2)
            Text(content: album.detail, style: caption, emphasis: secondary, lines: 1)
        }
    }
}

/// A singer as a round picture with their name.
#[component]
pub fn artist_card(app: enji::Client, artist: enji::Artist) -> impl View {
    ui! {
        Column(gap: 8, align: center, appear: rise) {
            Button(id: open_artist, label: artist.name, variant: card) {
                Image(source: artist.picture, aspect: 1, fit: cover, corner: 500)
            } => app.open_artist(artist.id)
            Text(content: artist.name, style: callout, lines: 1)
            Text(content: artist.detail, style: caption, emphasis: secondary, lines: 1)
        }
    }
}

/// What is open over the page: the playlist, album or singer opened last.
#[component]
pub fn detail_view(app: enji::Client) -> impl View {
    ui! {
        match app.detail {
            Detail::Album => {
                if let Some(album) = app.album {
                    album_page(app: app, album: album)
                }
            }
            Detail::Artist => {
                if let Some(artist) = app.artist {
                    artist_page(app: app, artist: artist)
                }
            }
            Detail::Playlist => {
                if let Some(list) = app.opened {
                    playlist_page(app: app, list: list)
                }
            }
            Detail::Page => {}
        }
    }
}

/// The way back to what was open before.
#[component]
pub fn back_row(app: enji::Client) -> impl View {
    ui! {
        Row(padding_x: 4) {
            IconButton(id: back, icon: "chevron.left", font_size: 17, prominence: plain, label: tr! { "zh-Hans" => "返回", "en" => "Back" }) => app.back()
            Spacer
        }
    }
}

/// An open album: its cover beside its name, singer and year, then its
/// songs.
#[component]
pub fn album_page(app: enji::Client, album: enji::Album) -> impl View {
    ui! {
        ScrollView {
            Column(gap: 0, padding_x: if window.compact { 8 } else { 24 }, padding_y: if window.compact { 4 } else { 12 }) {
                back_row(app: app)
                if window.compact {
                    Row(padding_x: 10, padding_y: 10) {
                        Spacer
                        Column(gap: 14, align: center) {
                            Image(source: album.cover, width: 200, height: 200, fit: cover, corner: 14, glow: 1, glow_opacity: 0.35)
                            Column(gap: 6, align: center) {
                                Text(content: album.name, style: title2, bold: true, lines: 2)
                                album_singer(app: app, album: album)
                                Text(content: app.album_meta, style: caption, emphasis: secondary)
                            }
                            album_actions(app: app)
                        }
                        Spacer
                    }
                } else {
                    Row(gap: 32, padding_x: 8, padding_y: 16, align: center) {
                        Image(source: album.cover, width: 216, height: 216, fit: cover, corner: 16, glow: 1, glow_opacity: 0.35)
                        Column(gap: 10, align: leading) {
                            Text(content: album.name, style: large_title, bold: true, lines: 2)
                            album_singer(app: app, album: album)
                            Text(content: app.album_meta, style: subheadline, emphasis: secondary)
                            if app.album_description != "" {
                                Text(content: app.album_description, style: footnote, emphasis: secondary, lines: 2)
                            }
                            Column(padding_y: 6) {
                                album_actions(app: app)
                            }
                        }
                        Spacer
                    }
                }
                song_header(app: app)
                if app.album_loading {
                    song_skeleton()
                }
                Column(gap: 2) {
                    ForEach(items: app.album_songs, key: index) |song| {
                        song_row(app: app, song: song, source: Source::Album, full: true, removable: false)
                    }
                }
            }
        }
    }
}

/// Who sang an album, a press opening them.
#[component]
pub fn album_singer(app: enji::Client, album: enji::Album) -> impl View {
    ui! {
        if app.album_artist != "" {
            Button(id: album_singer, label: app.album_artist, prominence: plain, enabled: album.artist_id > 0) => app.open_artist(album.artist_id)
        }
    }
}

#[component]
pub fn album_actions(app: enji::Client) -> impl View {
    ui! {
        Row(gap: 10) {
            Button(id: play_album, label: tr! { "zh-Hans" => "播放全部", "en" => "Play All" }, icon: "play.fill", prominence: prominent, shape: capsule, height: 36, enabled: !app.album_loading) => app.play(Source::Album, 0)
            Button(id: shuffle_album, label: tr! { "zh-Hans" => "随机播放", "en" => "Shuffle" }, icon: "shuffle", shape: capsule, height: 36, enabled: !app.album_loading) => app.shuffle(Source::Album)
        }
    }
}

/// An open singer: their picture beside their name and what they made,
/// their top songs, then their albums.
#[component]
pub fn artist_page(app: enji::Client, artist: enji::Artist) -> impl View {
    ui! {
        ScrollView {
            Column(gap: 0, padding_x: if window.compact { 8 } else { 24 }, padding_y: if window.compact { 4 } else { 12 }) {
                back_row(app: app)
                if window.compact {
                    Row(padding_x: 10, padding_y: 10) {
                        Spacer
                        Column(gap: 12, align: center) {
                            Image(source: artist.picture, width: 160, height: 160, fit: cover, corner: 80)
                            Text(content: artist.name, style: title2, bold: true, lines: 1)
                            Text(content: artist.detail, style: caption, emphasis: secondary)
                            artist_actions(app: app)
                        }
                        Spacer
                    }
                } else {
                    Row(gap: 32, padding_x: 8, padding_y: 16, align: center) {
                        Image(source: artist.picture, width: 200, height: 200, fit: cover, corner: 100)
                        Column(gap: 10, align: leading) {
                            Text(content: artist.name, style: large_title, bold: true, lines: 1)
                            Text(content: artist.detail, style: subheadline, emphasis: secondary)
                            if app.artist_description != "" {
                                Text(content: app.artist_description, style: footnote, emphasis: secondary, lines: 3)
                            }
                            Column(padding_y: 6) {
                                artist_actions(app: app)
                            }
                        }
                        Spacer
                    }
                }
                if app.artist_loading {
                    song_skeleton()
                } else {
                    Column(padding_x: 12, padding_y: 8) {
                        Text(content: tr! { "zh-Hans" => "热门歌曲", "en" => "Top Songs" }, style: title3, bold: true)
                    }
                    song_header(app: app)
                    Column(gap: 2) {
                        ForEach(items: app.artist_songs, key: index) |song| {
                            song_row(app: app, song: song, source: Source::Artist, full: true, removable: false)
                        }
                    }
                    Column(padding_x: 12, padding_y: 20) {
                        Text(content: tr! { "zh-Hans" => "专辑", "en" => "Albums" }, style: title3, bold: true)
                    }
                    Column(padding_x: 12, padding_y: 4) {
                        Grid(min_width: 150, gap: 22, cell_ratio: 1, cell_extra: 58) {
                            ForEach(items: app.artist_albums, key: id, stagger: 0.02) |album| {
                                album_card(app: app, album: album)
                            }
                        }
                    }
                }
            }
        }
    }
}

#[component]
pub fn artist_actions(app: enji::Client) -> impl View {
    ui! {
        Row(gap: 10) {
            Button(id: play_artist, label: tr! { "zh-Hans" => "播放热门歌曲", "en" => "Play Top Songs" }, icon: "play.fill", prominence: prominent, shape: capsule, height: 36, enabled: !app.artist_loading) => app.play(Source::Artist, 0)
            Button(id: shuffle_artist, label: tr! { "zh-Hans" => "随机播放", "en" => "Shuffle" }, icon: "shuffle", shape: capsule, height: 36, enabled: !app.artist_loading) => app.shuffle(Source::Artist)
        }
    }
}

/// An open playlist: its cover beside its name, who made it and what it
/// holds, then its songs.
#[component]
pub fn playlist_page(app: enji::Client, list: enji::Playlist) -> impl View {
    ui! {
        ScrollView {
            Column(gap: 0, padding_x: if window.compact { 8 } else { 24 }, padding_y: if window.compact { 4 } else { 12 }) {
                Row(padding_x: 4) {
                    IconButton(id: back, icon: "chevron.left", font_size: 17, prominence: plain, label: tr! { "zh-Hans" => "返回", "en" => "Back" }) => app.back()
                    Spacer
                }
                if window.compact {
                    Row(padding_x: 10, padding_y: 10) {
                        Spacer
                        Column(gap: 14, align: center) {
                            Image(source: list.cover, width: 200, height: 200, fit: cover, corner: 14, glow: 1, glow_opacity: 0.35)
                            Column(gap: 6, align: center) {
                                Text(content: list.name, style: title2, bold: true, lines: 2)
                                playlist_owner(app: app)
                                Text(content: app.opened_meta, style: caption, emphasis: secondary)
                            }
                            playlist_actions(app: app, list: list)
                        }
                        Spacer
                    }
                } else {
                    Row(gap: 32, padding_x: 8, padding_y: 16, align: center) {
                        Image(source: list.cover, width: 216, height: 216, fit: cover, corner: 16, glow: 1, glow_opacity: 0.35)
                        Column(gap: 10, align: leading) {
                            Text(content: list.name, style: large_title, bold: true, lines: 2)
                            playlist_owner(app: app)
                            Text(content: app.opened_meta, style: subheadline, emphasis: secondary)
                            if app.opened_description != "" {
                                Text(content: app.opened_description, style: footnote, emphasis: secondary, lines: 2)
                            }
                            Column(padding_y: 6) {
                                playlist_actions(app: app, list: list)
                            }
                        }
                        Spacer
                    }
                }
                song_header(app: app)
                if app.opened_loading {
                    song_skeleton()
                }
                // In the scroll view, rows are built as they come into view:
                // a playlist of a thousand songs does not fetch a thousand
                // covers at once.
                Column(gap: 2) {
                    ForEach(items: app.opened_songs, key: index) |song| {
                        song_row(app: app, song: song, source: Source::Playlist, full: true, removable: app.opened_owned)
                    }
                }
            }
        }
    }
}

/// Who made the open playlist, with their picture.
#[component]
pub fn playlist_owner(app: enji::Client) -> impl View {
    ui! {
        if app.opened_owner != "" {
            Row(gap: 8, align: center) {
                Image(source: app.opened_owner_avatar, width: 22, height: 22, fit: cover, corner: 11)
                Text(content: app.opened_owner, style: subheadline, lines: 1)
            }
        }
    }
}

#[component]
pub fn playlist_actions(app: enji::Client, list: enji::Playlist) -> impl View {
    ui! {
        Row(gap: 10, align: center) {
            Button(id: play_all, label: tr! { "zh-Hans" => "播放全部", "en" => "Play All" }, icon: "play.fill", prominence: prominent, shape: capsule, height: 36, enabled: !app.opened_loading) => app.play(Source::Playlist, 0)
            Button(id: shuffle_all, label: tr! { "zh-Hans" => "随机播放", "en" => "Shuffle" }, icon: "shuffle", shape: capsule, height: 36, enabled: !app.opened_loading) => app.shuffle(Source::Playlist)
            Picker(id: playlist_order, label: tr! { "zh-Hans" => "排序", "en" => "Sort By" }, label_hidden: true, selected: app.order, style: menu, shape: capsule, height: 36) => app.set_order(app.order)
            if app.login == Login::In && app.opened_is_default {
                Menu(id: playlist_more_default, label: tr! { "zh-Hans" => "更多", "en" => "More" }, icon: "ellipsis", style: icon, shape: capsule, height: 36) {
                    MenuItem(id: undo_default, label: tr! { "zh-Hans" => "不再直接打开这个歌单", "en" => "Stop Opening This Playlist First" }, icon: "house") => app.set_default_playlist(0)
                }
            } else if app.login == Login::In {
                Menu(id: playlist_more, label: tr! { "zh-Hans" => "更多", "en" => "More" }, icon: "ellipsis", style: icon, shape: capsule, height: 36) {
                    MenuItem(id: make_default, label: tr! { "zh-Hans" => "在「我的歌单」直接打开", "en" => "Open First in My Playlists" }, icon: "house") => app.set_default_playlist(list.id)
                }
            }
        }
    }
}

#[component]
pub fn playlist_card(app: enji::Client, list: enji::Playlist) -> impl View {
    ui! {
        Column(gap: 8, appear: rise) {
            if list.owned {
                Button(id: open_own_list, label: list.name, variant: card) {
                    Image(source: list.cover, aspect: 1, fit: cover, corner: 12, glow: 1)
                    ContextMenu {
                        MenuItem(id: play_own_list, label: tr! { "zh-Hans" => "播放", "en" => "Play" }, icon: "play.fill") => app.play_playlist(list.id)
                        Divider
                        MenuItem(id: delete_list, label: tr! { "zh-Hans" => "删除歌单", "en" => "Delete Playlist" }, icon: "trash", role: destructive) => app.ask_delete_playlist(list.id)
                    }
                } => app.show_playlist(list)
            } else {
                Button(id: open_list, label: list.name, variant: card) {
                    Image(source: list.cover, aspect: 1, fit: cover, corner: 12, glow: 1)
                    ContextMenu {
                        MenuItem(id: play_list, label: tr! { "zh-Hans" => "播放", "en" => "Play" }, icon: "play.fill") => app.play_playlist(list.id)
                    }
                } => app.show_playlist(list)
            }
            Text(content: list.name, style: callout, lines: 2)
            Text(content: list.detail, style: caption, emphasis: secondary, lines: 1)
        }
    }
}

/// A song in a list, and what can be done with it: played next, queued,
/// added to a playlist; out of the account's own open playlist with
/// `removable`.
#[component]
pub fn song_row(app: enji::Client, song: enji::Song, source: Source, full: bool, removable: bool) -> impl View {
    ui! {
        if removable {
            Button(id: own_song, label: song.name, variant: row, enabled: !song.unavailable) {
                song_line(app: app, song: song, full: full, marked: !(source == Source::Local || source == Source::LocalResults))
                ContextMenu {
                    MenuItem(id: own_song_next, label: tr! { "zh-Hans" => "下一首播放", "en" => "Play Next" }, icon: "forward.fill") => app.play_next(source, song.index)
                    MenuItem(id: own_song_queue, label: tr! { "zh-Hans" => "加入播放队列", "en" => "Add to Queue" }, icon: "list.bullet") => app.add_to_queue(source, song.index)
                    MenuItem(id: own_song_add, label: tr! { "zh-Hans" => "添加到歌单…", "en" => "Add to Playlist…" }, icon: "plus", enabled: song.id > 0 && app.login == Login::In) => app.choose_playlist(source, song.index)
                    MenuItem(id: own_song_artist, label: tr! { "zh-Hans" => "查看歌手", "en" => "Go to Artist" }, icon: "person.crop.circle", enabled: song.artist_id > 0) => app.open_artist(song.artist_id)
                    MenuItem(id: own_song_album, label: tr! { "zh-Hans" => "查看专辑", "en" => "Go to Album" }, icon: "square.grid.2x2", enabled: song.album_id > 0) => app.open_album(song.album_id)
                    MenuItem(id: own_song_download, label: tr! { "zh-Hans" => "下载", "en" => "Download" }, icon: "arrow.down.circle", enabled: song.id > 0 && !song.unavailable) => app.download(source, song.index)
                    Divider
                    MenuItem(id: song_remove, label: tr! { "zh-Hans" => "从歌单中删除", "en" => "Remove from Playlist" }, icon: "trash", role: destructive) => app.remove_from_playlist(song.index)
                }
            } => app.play(source, song.index)
        } else if song.file != "" && app.netease {
            Button(id: file_song, label: song.name, variant: row) {
                song_line(app: app, song: song, full: full, marked: !(source == Source::Local || source == Source::LocalResults))
                ContextMenu {
                    MenuItem(id: file_song_next, label: tr! { "zh-Hans" => "下一首播放", "en" => "Play Next" }, icon: "forward.fill") => app.play_next(source, song.index)
                    MenuItem(id: file_song_queue, label: tr! { "zh-Hans" => "加入播放队列", "en" => "Add to Queue" }, icon: "list.bullet") => app.add_to_queue(source, song.index)
                    MenuItem(id: file_song_add, label: tr! { "zh-Hans" => "添加到歌单…", "en" => "Add to Playlist…" }, icon: "plus", enabled: song.id > 0 && app.login == Login::In) => app.choose_playlist(source, song.index)
                    MenuItem(id: file_song_artist, label: tr! { "zh-Hans" => "查看歌手", "en" => "Go to Artist" }, icon: "person.crop.circle", enabled: song.artist_id > 0) => app.open_artist(song.artist_id)
                    MenuItem(id: file_song_album, label: tr! { "zh-Hans" => "查看专辑", "en" => "Go to Album" }, icon: "square.grid.2x2", enabled: song.album_id > 0) => app.open_album(song.album_id)
                    Divider
                    MenuItem(id: file_song_match, label: tr! { "zh-Hans" => "匹配网易云歌曲…", "en" => "Match on NetEase…" }, icon: "link") => app.begin_matching(source, song.index)
                }
            } => app.play(source, song.index)
        } else if !app.netease {
            Button(id: plain_song, label: song.name, variant: row, enabled: !song.unavailable) {
                song_line(app: app, song: song, full: full, marked: !(source == Source::Local || source == Source::LocalResults))
                ContextMenu {
                    MenuItem(id: plain_song_next, label: tr! { "zh-Hans" => "下一首播放", "en" => "Play Next" }, icon: "forward.fill") => app.play_next(source, song.index)
                    MenuItem(id: plain_song_queue, label: tr! { "zh-Hans" => "加入播放队列", "en" => "Add to Queue" }, icon: "list.bullet") => app.add_to_queue(source, song.index)
                }
            } => app.play(source, song.index)
        } else {
            Button(id: song, label: song.name, variant: row, enabled: !song.unavailable) {
                song_line(app: app, song: song, full: full, marked: !(source == Source::Local || source == Source::LocalResults))
                ContextMenu {
                    MenuItem(id: song_next, label: tr! { "zh-Hans" => "下一首播放", "en" => "Play Next" }, icon: "forward.fill") => app.play_next(source, song.index)
                    MenuItem(id: song_queue, label: tr! { "zh-Hans" => "加入播放队列", "en" => "Add to Queue" }, icon: "list.bullet") => app.add_to_queue(source, song.index)
                    MenuItem(id: song_add, label: tr! { "zh-Hans" => "添加到歌单…", "en" => "Add to Playlist…" }, icon: "plus", enabled: song.id > 0 && app.login == Login::In) => app.choose_playlist(source, song.index)
                    MenuItem(id: song_artist, label: tr! { "zh-Hans" => "查看歌手", "en" => "Go to Artist" }, icon: "person.crop.circle", enabled: song.artist_id > 0) => app.open_artist(song.artist_id)
                    MenuItem(id: song_album, label: tr! { "zh-Hans" => "查看专辑", "en" => "Go to Album" }, icon: "square.grid.2x2", enabled: song.album_id > 0) => app.open_album(song.album_id)
                    MenuItem(id: song_download, label: tr! { "zh-Hans" => "下载", "en" => "Download" }, icon: "arrow.down.circle", enabled: song.id > 0 && !song.unavailable) => app.download(source, song.index)
                }
            } => app.play(source, song.index)
        }
    }
}

/// What a song's row shows: its number, or a speaker while it plays; its
/// cover, name and singers, marked when Enji downloaded it or, with
/// `marked`, when a copy is here; with `full` on a wide window its album;
/// its length.
#[component]
pub fn song_line(app: enji::Client, song: enji::Song, full: bool, marked: bool) -> impl View {
    ui! {
        Row(gap: 14, padding_x: 12, padding_y: 7, align: center) {
            if full && !window.compact {
                Column(width: 28, align: center) {
                    if song.id == app.current_id {
                        Stack(variant: tint) {
                            Label(title: tr! { "zh-Hans" => "正在播放", "en" => "Playing" }, icon: "speaker.wave.2", display: icon, font_size: 13)
                        }
                    } else {
                        Text(content: tr! { "zh-Hans" => "{n}", "en" => "{n}"; n: song.rank }, style: callout, emphasis: tertiary)
                    }
                }
            }
            Stack(width: 44, height: 44) {
                cover_placeholder(side: 44.0)
                Image(source: song.cover, width: 44, height: 44, fit: cover, corner: 6)
            }
            Column(gap: 3, align: leading) {
                if song.id == app.current_id {
                    Text(content: song.name, lines: 1, color: accent, bold: true)
                } else {
                    Text(content: song.name, lines: 1)
                }
                Row(gap: 4, align: center) {
                    if song.downloaded {
                        Stack(variant: tint) {
                            Label(title: tr! { "zh-Hans" => "已下载", "en" => "Downloaded" }, icon: "arrow.down.circle", display: icon, font_size: 11)
                        }
                    } else if marked && song.file != "" {
                        Stack(variant: quiet) {
                            Label(title: tr! { "zh-Hans" => "本地", "en" => "On This Device" }, icon: "folder", display: icon, font_size: 11)
                        }
                    }
                    Text(content: song.artists, style: caption, emphasis: secondary, lines: 1)
                }
            }
            Spacer
            if full && !window.compact {
                Column(width: 240, align: leading) {
                    Text(content: song.album, style: subheadline, emphasis: secondary, lines: 1)
                }
            }
            // How many times it was played, where the list counts them.
            if full && app.show_plays {
                Column(width: if window.compact { 36 } else { 52 }, align: trailing) {
                    if song.plays > 0 {
                        Text(content: tr! { "zh-Hans" => "{n}", "en" => "{n}"; n: song.plays }, style: caption, emphasis: secondary)
                    } else {
                        Text(content: tr! { "zh-Hans" => "–", "en" => "–" }, style: caption, emphasis: tertiary)
                    }
                }
            }
            Column(width: 44, align: trailing) {
                Text(content: song.duration, style: caption, emphasis: tertiary)
            }
        }
    }
}

/// Over a list of songs on a wide window: what each column of the rows is.
#[component]
pub fn song_header(app: enji::Client) -> impl View {
    ui! {
        if !window.compact {
            Column(gap: 6, padding_y: 6) {
                Row(gap: 14, padding_x: 12, align: center) {
                    Column(width: 28, align: center) {
                        Text(content: tr! { "zh-Hans" => "#", "en" => "#" }, style: caption, emphasis: tertiary)
                    }
                    Text(content: tr! { "zh-Hans" => "标题", "en" => "Title" }, style: caption, emphasis: tertiary)
                    Spacer
                    Column(width: 240, align: leading) {
                        Text(content: tr! { "zh-Hans" => "专辑", "en" => "Album" }, style: caption, emphasis: tertiary)
                    }
                    if app.show_plays {
                        Column(width: 52, align: trailing) {
                            Text(content: tr! { "zh-Hans" => "播放", "en" => "Plays" }, style: caption, emphasis: tertiary)
                        }
                    }
                    Column(width: 44, align: trailing) {
                        Text(content: tr! { "zh-Hans" => "时长", "en" => "Time" }, style: caption, emphasis: tertiary)
                    }
                }
                Divider
            }
        }
    }
}

/// Rows standing in for songs while they load, each coming up in turn.
#[component]
pub fn song_skeleton() -> impl View {
    ui! {
        Column(gap: 2, stagger: 0.05) {
            Row(gap: 14, padding_x: 12, padding_y: 7, align: center, appear: rise) {
                Canvas(width: 44, height: 44) { Shape(kind: rounded, width: 44, height: 44, corner: 6, fill: text.primary, opacity: 0.07) }
                Column(gap: 8, align: leading) {
                    Canvas(width: 220, height: 12) { Shape(kind: capsule, width: 220, height: 12, fill: text.primary, opacity: 0.09) }
                    Canvas(width: 120, height: 10) { Shape(kind: capsule, width: 120, height: 10, fill: text.primary, opacity: 0.06) }
                }
                Spacer
            }
            Row(gap: 14, padding_x: 12, padding_y: 7, align: center, appear: rise) {
                Canvas(width: 44, height: 44) { Shape(kind: rounded, width: 44, height: 44, corner: 6, fill: text.primary, opacity: 0.07) }
                Column(gap: 8, align: leading) {
                    Canvas(width: 160, height: 12) { Shape(kind: capsule, width: 160, height: 12, fill: text.primary, opacity: 0.09) }
                    Canvas(width: 90, height: 10) { Shape(kind: capsule, width: 90, height: 10, fill: text.primary, opacity: 0.06) }
                }
                Spacer
            }
            Row(gap: 14, padding_x: 12, padding_y: 7, align: center, appear: rise) {
                Canvas(width: 44, height: 44) { Shape(kind: rounded, width: 44, height: 44, corner: 6, fill: text.primary, opacity: 0.07) }
                Column(gap: 8, align: leading) {
                    Canvas(width: 250, height: 12) { Shape(kind: capsule, width: 250, height: 12, fill: text.primary, opacity: 0.09) }
                    Canvas(width: 140, height: 10) { Shape(kind: capsule, width: 140, height: 10, fill: text.primary, opacity: 0.06) }
                }
                Spacer
            }
            Row(gap: 14, padding_x: 12, padding_y: 7, align: center, appear: rise) {
                Canvas(width: 44, height: 44) { Shape(kind: rounded, width: 44, height: 44, corner: 6, fill: text.primary, opacity: 0.07) }
                Column(gap: 8, align: leading) {
                    Canvas(width: 190, height: 12) { Shape(kind: capsule, width: 190, height: 12, fill: text.primary, opacity: 0.09) }
                    Canvas(width: 110, height: 10) { Shape(kind: capsule, width: 110, height: 10, fill: text.primary, opacity: 0.06) }
                }
                Spacer
            }
            Row(gap: 14, padding_x: 12, padding_y: 7, align: center, appear: rise) {
                Canvas(width: 44, height: 44) { Shape(kind: rounded, width: 44, height: 44, corner: 6, fill: text.primary, opacity: 0.07) }
                Column(gap: 8, align: leading) {
                    Canvas(width: 230, height: 12) { Shape(kind: capsule, width: 230, height: 12, fill: text.primary, opacity: 0.09) }
                    Canvas(width: 80, height: 10) { Shape(kind: capsule, width: 80, height: 10, fill: text.primary, opacity: 0.06) }
                }
                Spacer
            }
            Row(gap: 14, padding_x: 12, padding_y: 7, align: center, appear: rise) {
                Canvas(width: 44, height: 44) { Shape(kind: rounded, width: 44, height: 44, corner: 6, fill: text.primary, opacity: 0.07) }
                Column(gap: 8, align: leading) {
                    Canvas(width: 170, height: 12) { Shape(kind: capsule, width: 170, height: 12, fill: text.primary, opacity: 0.09) }
                    Canvas(width: 130, height: 10) { Shape(kind: capsule, width: 130, height: 10, fill: text.primary, opacity: 0.06) }
                }
                Spacer
            }
        }
    }
}

// MARK: - Playing

/// The bar under the pages, there with or without a song: the controls at
/// the leading end, the song with its progress filling the room beside
/// them, the volume at the trailing end.
#[component]
pub fn player_bar(app: enji::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Row(gap: 24, padding_x: 20, padding_y: 10, align: center) {
            Row(gap: 16, align: center) {
                mode_button(app: app)
                IconButton(id: bar_previous, icon: "backward.fill", font_size: 17, prominence: plain, enabled: app.current_id != 0, label: tr! { "zh-Hans" => "上一首", "en" => "Previous" }) => app.previous_song()
                if app.playing {
                    IconButton(id: bar_pause, icon: "pause.fill", font_size: 26, prominence: plain, label: tr! { "zh-Hans" => "暂停", "en" => "Pause" }) => app.toggle()
                } else {
                    IconButton(id: bar_play, icon: "play.fill", font_size: 26, prominence: plain, enabled: app.current_id != 0, label: tr! { "zh-Hans" => "播放", "en" => "Play" }) => app.toggle()
                }
                IconButton(id: bar_next, icon: "forward.fill", font_size: 17, prominence: plain, enabled: app.current_id != 0, label: tr! { "zh-Hans" => "下一首", "en" => "Next" }) => app.next_song()
                PopoverButton(id: bar_queue, label: tr! { "zh-Hans" => "播放列表", "en" => "Queue" }, icon: "list.bullet", font_size: 16, prominence: plain) {
                    queue_list(app: app)
                }
            }
            Row(gap: 12, align: center) {
                if let Some(song) = app.current {
                    Button(id: open_now, label: tr! { "zh-Hans" => "正在播放：{name}", "en" => "Now playing: {name}"; name: song.name }, variant: card) {
                        Stack(width: 44, height: 44) {
                            cover_placeholder(side: 44.0)
                            Image(source: song.cover, width: 44, height: 44, fit: cover, corner: 8)
                        }
                    } => draft.now.set(true)
                } else {
                    cover_placeholder(side: 44.0)
                }
                Column(gap: 4) {
                    Row(gap: 8, align: center) {
                        if let Some(song) = app.current {
                            Text(content: song.name, style: callout, bold: true, lines: 1)
                            Text(content: song.artists, style: callout, emphasis: secondary, lines: 1)
                            if app.preview {
                                Text(content: tr! { "zh-Hans" => "试听", "en" => "Preview" }, style: caption2, color: accent)
                            }
                        } else {
                            Text(content: tr! { "zh-Hans" => "未在播放", "en" => "Not Playing" }, style: callout, emphasis: secondary)
                        }
                        Spacer
                        if app.current_id != 0 {
                            like_button(app: app)
                        }
                        Text(content: tr! { "zh-Hans" => "{elapsed} / {total}", "en" => "{elapsed} / {total}"; elapsed: app.elapsed, total: app.total }, style: caption, emphasis: secondary)
                    }
                    Slider(id: bar_progress, label: tr! { "zh-Hans" => "播放进度", "en" => "Progress" }, label_hidden: true, current: app.progress, min: 0, max: 1) => app.seek(app.progress)
                }
            }
            // A medium window, such as the unfolded iPhone Duo, leaves the
            // song too little room beside a volume: the system's own sets it.
            if !window.medium {
                Row(gap: 6, width: 160, align: center) {
                    if app.volume < 0.01 {
                        IconButton(id: unmute, icon: "speaker.slash.fill", font_size: 15, prominence: plain, label: tr! { "zh-Hans" => "取消静音", "en" => "Unmute" }) => app.toggle_mute()
                    } else {
                        IconButton(id: mute, icon: "speaker.wave.2", font_size: 15, prominence: plain, label: tr! { "zh-Hans" => "静音", "en" => "Mute" }) => app.toggle_mute()
                    }
                    Slider(id: volume, label: tr! { "zh-Hans" => "音量", "en" => "Volume" }, label_hidden: true, current: app.volume, continuous: true, min: 0, max: 1) => app.set_volume(app.volume)
                }
            }
        }
    }
}

/// Where a cover will be once something plays.
#[component]
pub fn cover_placeholder(side: f64) -> impl View {
    ui! {
        Stack(variant: placeholder, width: side, height: side) {
            Stack(variant: quiet) {
                Label(title: tr! { "zh-Hans" => "未在播放", "en" => "Not Playing" }, icon: "music.note", display: icon, font_size: 16)
            }
        }
    }
}

/// The playing song over a phone's tabs, a press on it opens it; while
/// nothing plays, a place for it.
#[component]
pub fn mini_player(app: enji::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Row(gap: 10, padding_x: 12, align: center) {
            if let Some(song) = app.current {
                Button(id: mini_open, label: tr! { "zh-Hans" => "正在播放：{name}", "en" => "Now playing: {name}"; name: song.name }, variant: bare, width: fill) {
                    Row(gap: 10, align: center) {
                        Stack(width: 32, height: 32) {
                            cover_placeholder(side: 32.0)
                            Image(source: song.cover, width: 32, height: 32, fit: cover, corner: 6)
                        }
                        Column(gap: 0, align: leading) {
                            Text(content: song.name, style: subheadline, lines: 1)
                            Text(content: song.artists, style: caption, emphasis: secondary, lines: 1)
                        }
                        Spacer
                    }
                } => draft.now.set(true)
            } else {
                Row(gap: 10, align: center) {
                    cover_placeholder(side: 32.0)
                    Text(content: tr! { "zh-Hans" => "未在播放", "en" => "Not Playing" }, style: subheadline, emphasis: secondary)
                    Spacer
                }
            }
            if app.playing {
                IconButton(id: mini_pause, icon: "pause.fill", font_size: 18, prominence: plain, label: tr! { "zh-Hans" => "暂停", "en" => "Pause" }) => app.toggle()
            } else {
                IconButton(id: mini_play, icon: "play.fill", font_size: 18, prominence: plain, enabled: app.current_id != 0, label: tr! { "zh-Hans" => "播放", "en" => "Play" }) => app.toggle()
            }
            IconButton(id: mini_next, icon: "forward.fill", font_size: 16, prominence: plain, enabled: app.current_id != 0, label: tr! { "zh-Hans" => "下一首", "en" => "Next" }) => app.next_song()
        }
    }
}

/// The playing song filling a phone: the cover large, or in its place the
/// lyrics, comments or details under a small one; the controls below, the
/// switches between them at the bottom.
#[component]
pub fn phone_now_playing(app: enji::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Stack(appearance: dark) {
            if let Some(song) = app.current {
                // The cover blurred to the screen's edges, the controls within
                // the status bar and the home indicator.
                Stack(under_bars: true) {
                    Canvas(label: song.name) {
                        Shape(kind: rect, fill: surface.canvas)
                        Shape(kind: rect, fill_from: song.cover)
                    }
                    Image(source: song.cover, fit: cover, blur: 70)
                    Canvas(label: song.name) {
                        Shape(kind: rect, fill: surface.canvas, opacity: 0.4)
                    }
                }
                Column(gap: 0) {
                    Row(padding_x: 16, padding_y: 4, align: center) {
                        IconButton(id: phone_close_now, icon: "chevron.down", font_size: 20, prominence: plain, label: tr! { "zh-Hans" => "收起", "en" => "Close" }) => draft.now.set(false)
                        Spacer
                    }
                    if app.panel_open {
                        Row(gap: 12, padding_x: 24, padding_y: 8, align: center) {
                            Button(id: phone_cover, label: tr! { "zh-Hans" => "封面", "en" => "Cover" }, variant: bare) {
                                Image(source: song.cover, width: 52, height: 52, fit: cover, corner: 8)
                            } => app.toggle_panel(app.panel)
                            Column(gap: 2, align: leading) {
                                Text(content: song.name, style: headline, lines: 1)
                                Text(content: song.artists, style: subheadline, emphasis: secondary, lines: 1)
                            }
                            Spacer
                            like_button(app: app)
                        }
                        // The rest of the height, between the song and the
                        // controls.
                        match app.panel {
                            Panel::Lyrics => { lyrics(app: app) }
                            Panel::Comments => { comments_panel(app: app) }
                            Panel::Details => { details_panel(app: app) }
                        }
                    } else {
                        Spacer
                        Column(padding_x: 28) {
                            // A spinner on the cover's place until a picture of
                            // it comes, the small one first, then the sharp one.
                            // The place is a square picture as the cover is: on
                            // a screen wider than its height leaves, such as the
                            // unfolded iPhone Duo held upright, the cover is
                            // narrower than the column, and so is its place.
                            // The empty canvas spans the column, so both stand
                            // in its middle.
                            // A plugin's pulses give it a little nudge each.
                            Stack(nudge_key: app.cover_pulse, nudge_scale: 1.04) {
                                Canvas {}
                                Image(source: "assets/cover-place.png", aspect: 1, fit: cover, corner: 18)
                                // Only while a picture is coming: a song
                                // without one keeps the place.
                                if song.artwork.url != "" {
                                    Progress
                                }
                                Image(source: song.cover, aspect: 1, fit: cover, corner: 18)
                                Image(source: song.artwork, aspect: 1, fit: cover, corner: 18, glow: 1.05, glow_opacity: 0.22)
                            }
                        }
                        Spacer
                        Column(gap: 4, padding_x: 28, align: leading) {
                            if app.preview {
                                Text(content: tr! { "zh-Hans" => "试听片段 · 完整版需要黑胶会员", "en" => "Preview · the full song is for members" }, style: caption, color: accent)
                            }
                            Row(gap: 10, align: center) {
                                Text(content: song.name, style: title2, bold: true, lines: 1)
                                Spacer
                                like_button(app: app)
                            }
                            Text(content: song.artists, style: headline, emphasis: secondary, lines: 1)
                            if app.show_format && app.format != "" {
                                Text(content: app.format, style: caption, emphasis: tertiary, lines: 1)
                            }
                            // The sung line, its translation and the next,
                            // while the cover shows in place of the lyrics.
                            if app.cover_lyrics && app.lyric_now != "" {
                                Column(gap: 3, padding_y: 8, align: leading) {
                                    Text(content: app.lyric_now, style: callout, bold: true, lines: 2)
                                    if app.show_translation && app.lyric_now_translation != "" {
                                        Text(content: app.lyric_now_translation, style: footnote, emphasis: secondary, lines: 1)
                                    }
                                    if app.lyric_next != "" {
                                        Text(content: app.lyric_next, style: footnote, emphasis: tertiary, lines: 1)
                                    }
                                }
                            }
                        }
                    }
                    Column(gap: 14, padding_x: 28, padding_y: 18) {
                        Column(gap: 2) {
                            Slider(id: phone_progress, label: tr! { "zh-Hans" => "播放进度", "en" => "Progress" }, label_hidden: true, current: app.progress, min: 0, max: 1) => app.seek(app.progress)
                            Row {
                                Text(content: app.elapsed, style: caption, emphasis: secondary)
                                Spacer
                                Text(content: app.total, style: caption, emphasis: secondary)
                            }
                        }
                        Row(gap: 0, align: center) {
                            Spacer
                            IconButton(id: phone_previous, icon: "backward.fill", font_size: 28, prominence: plain, label: tr! { "zh-Hans" => "上一首", "en" => "Previous" }) => app.previous_song()
                            Spacer
                            if app.playing {
                                IconButton(id: phone_pause, icon: "pause.fill", font_size: 42, prominence: plain, label: tr! { "zh-Hans" => "暂停", "en" => "Pause" }) => app.toggle()
                            } else {
                                IconButton(id: phone_play, icon: "play.fill", font_size: 42, prominence: plain, label: tr! { "zh-Hans" => "播放", "en" => "Play" }) => app.toggle()
                            }
                            Spacer
                            IconButton(id: phone_next, icon: "forward.fill", font_size: 28, prominence: plain, label: tr! { "zh-Hans" => "下一首", "en" => "Next" }) => app.next_song()
                            Spacer
                        }
                        // What follows, and what shows in place of the cover.
                        Row(gap: 0, align: center) {
                            mode_button(app: app)
                            Spacer
                            if app.panel_open && app.panel == Panel::Lyrics {
                                IconButton(id: phone_lyrics_on, icon: "quote.bubble.fill", font_size: 18, prominence: plain, label: tr! { "zh-Hans" => "歌词", "en" => "Lyrics" }) => app.toggle_panel(Panel::Lyrics)
                            } else {
                                IconButton(id: phone_lyrics_off, icon: "quote.bubble", font_size: 18, prominence: plain, label: tr! { "zh-Hans" => "歌词", "en" => "Lyrics" }) => app.toggle_panel(Panel::Lyrics)
                            }
                            if app.current_id > 0 {
                                Spacer
                                if app.panel_open && app.panel == Panel::Comments {
                                    IconButton(id: phone_comments_on, icon: "text.bubble.fill", font_size: 18, prominence: plain, label: tr! { "zh-Hans" => "评论", "en" => "Comments" }) => app.toggle_panel(Panel::Comments)
                                } else {
                                    IconButton(id: phone_comments_off, icon: "text.bubble", font_size: 18, prominence: plain, label: tr! { "zh-Hans" => "评论", "en" => "Comments" }) => app.toggle_panel(Panel::Comments)
                                }
                                Spacer
                                if app.panel_open && app.panel == Panel::Details {
                                    IconButton(id: phone_details_on, icon: "info.circle.fill", font_size: 18, prominence: plain, label: tr! { "zh-Hans" => "详情", "en" => "Details" }) => app.toggle_panel(Panel::Details)
                                } else {
                                    IconButton(id: phone_details_off, icon: "info.circle", font_size: 18, prominence: plain, label: tr! { "zh-Hans" => "详情", "en" => "Details" }) => app.toggle_panel(Panel::Details)
                                }
                                if song.file == "" {
                                    Spacer
                                    IconButton(id: phone_download, icon: "arrow.down.circle", font_size: 18, prominence: plain, label: tr! { "zh-Hans" => "下载", "en" => "Download" }) => app.download_current()
                                }
                            }
                            Spacer
                            PopoverButton(id: phone_queue, label: tr! { "zh-Hans" => "播放列表", "en" => "Queue" }, icon: "list.bullet", font_size: 18, prominence: plain) {
                                queue_list(app: app)
                            }
                        }
                    }
                }
                Watch(value: app.scroll_tick) => draft.line.set(app.scroll_to)
            }
        }
    }
}

/// The playing song over the window: its cover lit under the pointer, the
/// controls under it, and the lyrics beside it standing at the sung line.
#[component]
pub fn now_playing(app: enji::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Stack(appearance: dark, under_bars: true) {
            if let Some(song) = app.current {
                // Opaque while a new cover loads, so the page never shows
                // through.
                Canvas(label: song.name) {
                    Shape(kind: rect, fill: surface.canvas)
                    Shape(kind: rect, fill_from: song.cover)
                }
                // Covering the window whatever its shape.
                Image(source: song.cover, fit: cover, blur: 90)
                Canvas(label: song.name) {
                    Shape(kind: rect, fill: surface.canvas, opacity: 0.4)
                }
                // Each part a layer of its own: a layer is as tall as the
                // window, so the lyrics scroll within it.
                Row(gap: if window.large { 56 } else { 48 }, padding_x: if window.large { 64 } else { 48 }, align: center) {
                    Column(gap: 18, width: 400) {
                        // A spinner on the cover's place until a picture of it
                        // comes: the small cover, often loaded already for the
                        // lists and the player bar, then the sharp one over it.
                        // A plugin's pulses give it a little nudge each.
                        Stack(nudge_key: app.cover_pulse, nudge_scale: 1.04) {
                            Canvas(width: 400, height: 400) {
                                Shape(kind: rounded, corner: 20, fill: fill.subtle)
                            }
                            // Only while a picture is coming: a song without
                            // one keeps the place.
                            if song.artwork.url != "" {
                                Progress
                            } else {
                                Image(source: "assets/cover-place.png", width: 400, height: 400, fit: cover, corner: 20)
                            }
                            Image(source: song.cover, width: 400, height: 400, fit: cover, corner: 20)
                            Image(source: song.artwork, width: 400, height: 400, fit: cover, corner: 20, glow: 1.05, glow_opacity: 0.22)
                        }
                        Column(gap: 4, align: leading) {
                            if app.preview {
                                Text(content: tr! { "zh-Hans" => "试听片段 · 完整版需要黑胶会员", "en" => "Preview · the full song is for members" }, style: caption, color: accent)
                            }
                            Row(gap: 10, align: center) {
                                Text(content: song.name, style: title, lines: 2)
                                Spacer
                                like_button(app: app)
                            }
                            Text(content: song.artists, style: headline, emphasis: secondary, lines: 1)
                            if app.show_format && app.format != "" {
                                Text(content: app.format, style: caption, emphasis: tertiary, lines: 1)
                            }
                        }
                        Row(gap: 10, align: center) {
                            Text(content: app.elapsed, style: caption, emphasis: secondary)
                            Slider(id: now_progress, label: tr! { "zh-Hans" => "播放进度", "en" => "Progress" }, label_hidden: true, current: app.progress, min: 0, max: 1) => app.seek(app.progress)
                            Text(content: app.total, style: caption, emphasis: secondary)
                        }
                        Row(gap: 32, align: center) {
                            Spacer
                            IconButton(id: now_previous, icon: "backward.fill", font_size: 24, prominence: plain, label: tr! { "zh-Hans" => "上一首", "en" => "Previous" }) => app.previous_song()
                            if app.playing {
                                IconButton(id: now_pause, icon: "pause.fill", font_size: 40, prominence: plain, label: tr! { "zh-Hans" => "暂停", "en" => "Pause" }) => app.toggle()
                            } else {
                                IconButton(id: now_play, icon: "play.fill", font_size: 40, prominence: plain, label: tr! { "zh-Hans" => "播放", "en" => "Play" }) => app.toggle()
                            }
                            IconButton(id: now_next, icon: "forward.fill", font_size: 24, prominence: plain, label: tr! { "zh-Hans" => "下一首", "en" => "Next" }) => app.next_song()
                            Spacer
                        }
                        // What follows, and what shows beside the cover.
                        Row(gap: 26, align: center) {
                            Spacer
                            mode_button(app: app)
                            if app.panel == Panel::Lyrics {
                                IconButton(id: lyrics_on, icon: "quote.bubble.fill", font_size: 18, prominence: plain, label: tr! { "zh-Hans" => "歌词", "en" => "Lyrics" }) => app.show_panel(Panel::Lyrics)
                            } else {
                                IconButton(id: lyrics_off, icon: "quote.bubble", font_size: 18, prominence: plain, label: tr! { "zh-Hans" => "歌词", "en" => "Lyrics" }) => app.show_panel(Panel::Lyrics)
                            }
                            if app.current_id > 0 {
                                if app.panel == Panel::Comments {
                                    IconButton(id: comments_on, icon: "text.bubble.fill", font_size: 18, prominence: plain, label: tr! { "zh-Hans" => "评论", "en" => "Comments" }) => app.show_panel(Panel::Comments)
                                } else {
                                    IconButton(id: comments_off, icon: "text.bubble", font_size: 18, prominence: plain, label: tr! { "zh-Hans" => "评论", "en" => "Comments" }) => app.show_panel(Panel::Comments)
                                }
                                if app.panel == Panel::Details {
                                    IconButton(id: details_on, icon: "info.circle.fill", font_size: 18, prominence: plain, label: tr! { "zh-Hans" => "详情", "en" => "Details" }) => app.show_panel(Panel::Details)
                                } else {
                                    IconButton(id: details_off, icon: "info.circle", font_size: 18, prominence: plain, label: tr! { "zh-Hans" => "详情", "en" => "Details" }) => app.show_panel(Panel::Details)
                                }
                                if song.file == "" {
                                    IconButton(id: now_download, icon: "arrow.down.circle", font_size: 18, prominence: plain, label: tr! { "zh-Hans" => "下载", "en" => "Download" }) => app.download_current()
                                }
                            }
                            if app.desktop_lyrics_supported {
                                if app.desktop_lyrics {
                                    IconButton(id: desktop_lyrics_on, icon: "captions.bubble.fill", font_size: 18, prominence: plain, label: tr! { "zh-Hans" => "关闭桌面歌词", "en" => "Hide Desktop Lyrics" }) => app.set_desktop_lyrics(false)
                                } else {
                                    IconButton(id: desktop_lyrics_off, icon: "captions.bubble", font_size: 18, prominence: plain, label: tr! { "zh-Hans" => "桌面歌词", "en" => "Desktop Lyrics" }) => app.set_desktop_lyrics(true)
                                }
                            }
                            PopoverButton(id: now_queue, label: tr! { "zh-Hans" => "播放列表", "en" => "Queue" }, icon: "list.bullet", font_size: 18, prominence: plain) {
                                queue_list(app: app)
                            }
                            Spacer
                        }
                    }
                    // All the width beside the cover, whatever the lines,
                    // which wrap in it, so the cover never moves; as tall as
                    // the window, so lines leave it only at its edges.
                    Column {
                        match app.panel {
                            Panel::Lyrics => { lyrics(app: app) }
                            Panel::Comments => { comments_panel(app: app) }
                            Panel::Details => { details_panel(app: app) }
                        }
                    }
                }
                // Clear of the window's own buttons, which stay over the layer,
                // and on iOS of the status bar (see `Row.close_now`).
                Column(gap: 0) {
                    Row(variant: close_now, padding_x: 24, padding_y: 18) {
                        Spacer
                        IconButton(id: close_now, icon: "chevron.down", font_size: 20, prominence: plain, label: tr! { "zh-Hans" => "收起", "en" => "Close" }) => draft.now.set(false)
                    }
                    Spacer
                }
                Watch(value: app.scroll_tick) => draft.line.set(app.scroll_to)
                Shortcut(key: "escape") => draft.now.set(false)
            }
        }
    }
}

/// The lyrics with the sung line a third of the way down, larger and bold,
/// or as near as the first and last lines let it. Beside it the lines are a
/// little blurred, towards the edges more; scrolled by hand, none is, until
/// the engine brings them back.
#[component]
pub fn lyrics(app: enji::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        if app.has_lyrics {
            ScrollView(position: draft.line, anchor: 0.35, browsing: draft.browsing, indicators: hidden) {
                Column(gap: 4, padding_x: if window.compact { 10 } else { 0 }, padding_y: if window.compact { 40 } else { 88 }) {
                    ForEach(items: app.lyrics, key: key) |line| {
                        Button(id: lyric, label: line.text, variant: lyric) {
                            Column(gap: 4, padding_x: 14, padding_y: 10, blur: line.blur, animate: smooth) {
                                Text(content: line.text, style: title2, font_size: if line.current && window.compact { 26 } else if line.current { 32 } else if window.compact { 21 } else { 26 }, bold: line.current, animate: smooth)
                                if line.translated && app.show_translation {
                                    Text(content: line.translation, font_size: if window.compact { 15 } else if line.current { 18 } else { 16 }, emphasis: secondary, animate: smooth)
                                }
                            }
                        } => app.seek_line(line.key)
                    }
                    // Room under the last lines, so they too can come up to
                    // the sung line's place.
                    Column(height: 460) {
                        Spacer
                    }
                }
            }
            Watch(value: draft.browsing) => app.browse_lyrics(draft.browsing)
        } else if app.buffering {
            Column(gap: 0, padding_y: 88) {
                loading_lines()
                Spacer
            }
        } else {
            // As wide as the lyrics would be, so the cover beside it stays
            // where it is.
            Row(gap: 0) {
                Column(gap: 0, padding_y: 88) {
                    Column(gap: 4, padding_x: 14, padding_y: 10) {
                        Text(content: tr! { "zh-Hans" => "暂无歌词", "en" => "No Lyrics" }, style: title2, font_size: 32, bold: true)
                    }
                    Spacer
                }
                Spacer
            }
        }
    }
}

/// Where the lines will be while the song loads: bars as the lyrics are at
/// a song's start, the first the sung one, rising in turn.
#[component]
pub fn loading_lines() -> impl View {
    ui! {
        Column(gap: 4, padding_x: 14, stagger: 0.06) {
            Column(padding_y: 14, appear: rise) {
                Canvas(height: 24) {
                    Shape(kind: capsule, width: 300, height: 24, fill: text.primary, opacity: 0.5)
                }
            }
            Column(padding_y: 14, appear: rise) {
                Canvas(height: 18) {
                    Shape(kind: capsule, width: 360, height: 18, fill: text.primary, opacity: 0.24)
                }
            }
            Column(padding_y: 14, appear: rise) {
                Canvas(height: 18) {
                    Shape(kind: capsule, width: 250, height: 18, fill: text.primary, opacity: 0.24)
                }
            }
            Column(padding_y: 14, appear: rise) {
                Canvas(height: 18) {
                    Shape(kind: capsule, width: 330, height: 18, fill: text.primary, opacity: 0.24)
                }
            }
            Column(padding_y: 14, appear: rise) {
                Canvas(height: 18) {
                    Shape(kind: capsule, width: 280, height: 18, fill: text.primary, opacity: 0.16)
                }
            }
            Column(padding_y: 14, appear: rise) {
                Canvas(height: 18) {
                    Shape(kind: capsule, width: 350, height: 18, fill: text.primary, opacity: 0.16)
                }
            }
            Column(padding_y: 14, appear: rise) {
                Canvas(height: 18) {
                    Shape(kind: capsule, width: 230, height: 18, fill: text.primary, opacity: 0.16)
                }
            }
        }
    }
}

/// The heart of the playing song, for an account.
#[component]
pub fn like_button(app: enji::Client) -> impl View {
    ui! {
        if app.login == Login::In && app.current_id > 0 {
            if app.current_liked {
                IconButton(id: unlike, icon: "heart.fill", prominence: plain, label: tr! { "zh-Hans" => "取消喜欢", "en" => "Unlike" }, nudge: app.current_liked) => app.toggle_like()
            } else {
                IconButton(id: like, icon: "heart", prominence: plain, label: tr! { "zh-Hans" => "喜欢", "en" => "Like" }) => app.toggle_like()
            }
        }
    }
}

/// Repeat all, repeat one or shuffle; a press goes to the next.
#[component]
pub fn mode_button(app: enji::Client) -> impl View {
    ui! {
        match app.mode {
            PlayMode::RepeatAll => {
                IconButton(id: repeat_all, icon: "repeat", font_size: 16, prominence: plain, label: tr! { "zh-Hans" => "列表循环", "en" => "Repeat All" }) => app.next_mode()
            }
            PlayMode::RepeatOne => {
                IconButton(id: repeat_one, icon: "repeat.1", font_size: 16, prominence: plain, label: tr! { "zh-Hans" => "单曲循环", "en" => "Repeat One" }) => app.next_mode()
            }
            PlayMode::Shuffle => {
                IconButton(id: shuffle, icon: "shuffle", font_size: 16, prominence: plain, label: tr! { "zh-Hans" => "随机播放", "en" => "Shuffle" }) => app.next_mode()
            }
        }
    }
}

/// The songs played in turn, the playing one bold; a press plays one.
#[component]
pub fn queue_list(app: enji::Client) -> impl View {
    ui! {
        Column(gap: 8, padding: 12, width: 380, height: 480) {
            Text(content: tr! { "zh-Hans" => "播放列表", "en" => "Queue" }, style: headline)
            ScrollView {
                Column(gap: 2) {
                    ForEach(items: app.queue, key: index) |song| {
                        Button(id: queued, label: song.name, variant: row) {
                            Row(gap: 10, padding_x: 8, padding_y: 6, align: center) {
                                Column(gap: 2, align: leading) {
                                    Text(content: song.name, lines: 1, bold: song.index == app.queue_at)
                                    Text(content: song.artists, style: caption, emphasis: secondary, lines: 1)
                                }
                                Spacer
                                Text(content: song.duration, style: caption, emphasis: tertiary)
                            }
                            ContextMenu {
                                MenuItem(id: unqueue, label: tr! { "zh-Hans" => "从播放队列中删除", "en" => "Remove from Queue" }, icon: "trash", role: destructive, enabled: song.index != app.queue_at) => app.remove_from_queue(song.index)
                            }
                        } => app.play_queued(song.index)
                    }
                }
            }
        }
    }
}

/// The account's playlists to add a song to.
#[component]
pub fn playlist_chooser(app: enji::Client, song: enji::Song) -> impl View {
    ui! {
        Modal(width: 380) {
            Column(gap: 14, padding: 22) {
                Column(gap: 4, align: leading) {
                    Text(content: tr! { "zh-Hans" => "添加到歌单", "en" => "Add to Playlist" }, style: title3, bold: true)
                    Text(content: song.name, style: subheadline, emphasis: secondary, lines: 1)
                }
                Column(height: 320) {
                    ScrollView {
                        Column(gap: 2) {
                            ForEach(items: app.mine, key: id) |list| {
                                if list.owned {
                                    Button(id: choose_list, label: list.name, variant: row) {
                                        Row(gap: 12, padding_x: 8, padding_y: 6, align: center) {
                                            Image(source: list.cover, width: 44, height: 44, fit: cover, corner: 6)
                                            Column(gap: 2, align: leading) {
                                                Text(content: list.name, lines: 1)
                                                Text(content: list.detail, style: caption, emphasis: secondary, lines: 1)
                                            }
                                            Spacer
                                        }
                                    } => app.add_to_playlist(list.id)
                                }
                            }
                        }
                    }
                }
                Row {
                    Spacer
                    Button(id: cancel_choosing, label: tr! { "zh-Hans" => "取消", "en" => "Cancel" }) => app.cancel_choosing()
                }
            }
            Shortcut(key: "escape") => app.cancel_choosing()
        }
    }
}

/// A new playlist's name.
/// The NetEase song a local file is, chosen from a search for its name.
#[component]
pub fn match_dialog(app: enji::Client, song: enji::Song, #[session] draft: MainDraft) -> impl View {
    ui! {
        Modal(width: 460) {
            Column(gap: 14, padding: 22) {
                Column(gap: 4, align: leading) {
                    Text(content: tr! { "zh-Hans" => "匹配网易云歌曲", "en" => "Match on NetEase" }, style: title3, bold: true)
                    Text(content: tr! { "zh-Hans" => "{name} · {artists} · {time}", "en" => "{name} · {artists} · {time}"; name: song.name, artists: song.artists, time: song.duration }, style: subheadline, emphasis: secondary, lines: 1)
                }
                Search(id: match_search, value: draft.match_words, prompt: tr! { "zh-Hans" => "歌名、歌手", "en" => "Title, singer" }) => app.search_matching(draft.match_words)
                Column(height: 360) {
                    if app.match_loading {
                        song_skeleton()
                    } else if !app.has_match_candidates {
                        Column(padding_y: 60, align: center) {
                            Text(content: tr! { "zh-Hans" => "没有找到", "en" => "Nothing found" }, emphasis: secondary)
                        }
                    } else {
                        ScrollView {
                            Column(gap: 2) {
                                ForEach(items: app.match_candidates, key: id) |found| {
                                    Button(id: choose_match, label: found.name, variant: row, clears: draft.match_words) {
                                        Row(gap: 12, padding_x: 8, padding_y: 6, align: center) {
                                            Image(source: found.cover, width: 44, height: 44, fit: cover, corner: 6)
                                            Column(gap: 2, align: leading) {
                                                Text(content: found.name, lines: 1)
                                                if found.album != "" {
                                                    Text(content: tr! { "zh-Hans" => "{artists} · {album}", "en" => "{artists} · {album}"; artists: found.artists, album: found.album }, style: caption, emphasis: secondary, lines: 1)
                                                } else {
                                                    Text(content: found.artists, style: caption, emphasis: secondary, lines: 1)
                                                }
                                            }
                                            Spacer
                                            if found.id == song.id {
                                                Stack(variant: tint) {
                                                    Label(title: tr! { "zh-Hans" => "当前匹配", "en" => "Current match" }, icon: "checkmark", display: icon, font_size: 14)
                                                }
                                            }
                                            Text(content: found.duration, style: caption, emphasis: tertiary)
                                        }
                                    } => app.choose_match(found.id)
                                }
                            }
                        }
                    }
                }
                Row(gap: 10) {
                    Button(id: unmatch, label: tr! { "zh-Hans" => "都不是", "en" => "None of These" }, clears: draft.match_words) => app.unmatch()
                    Spacer
                    Button(id: cancel_matching, label: tr! { "zh-Hans" => "取消", "en" => "Cancel" }, clears: draft.match_words) => app.cancel_matching()
                }
            }
            Shortcut(key: "escape") => app.cancel_matching()
        }
    }
}

#[component]
pub fn naming_dialog(app: enji::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Modal(width: 360) {
            Column(gap: 16, padding: 22) {
                Text(content: tr! { "zh-Hans" => "新建歌单", "en" => "New Playlist" }, style: title3, bold: true)
                TextInput(id: new_name, label: tr! { "zh-Hans" => "歌单名称", "en" => "Name" }, label_hidden: true, value: draft.new_name, prompt: tr! { "zh-Hans" => "歌单名称", "en" => "Name" }, submit: clear) => app.create_playlist(draft.new_name)
                Row(gap: 10) {
                    Spacer
                    Button(id: cancel_naming, label: tr! { "zh-Hans" => "取消", "en" => "Cancel" }) => app.cancel_naming()
                    Button(id: create_playlist, label: tr! { "zh-Hans" => "新建", "en" => "Create" }, prominence: prominent, enabled: draft.new_name != "", clears: draft.new_name) => app.create_playlist(draft.new_name)
                }
            }
            Shortcut(key: "escape") => app.cancel_naming()
        }
    }
}

/// Deleting one of the account's playlists, which cannot be undone.
#[component]
pub fn delete_dialog(app: enji::Client, list: enji::Playlist) -> impl View {
    ui! {
        Modal(width: 360) {
            Column(gap: 16, padding: 22) {
                Column(gap: 6, align: leading) {
                    Text(content: tr! { "zh-Hans" => "删除歌单「{name}」？", "en" => "Delete “{name}”?"; name: list.name }, style: title3, bold: true)
                    Text(content: tr! { "zh-Hans" => "删除后不能恢复。", "en" => "This cannot be undone." }, style: subheadline, emphasis: secondary)
                }
                Row(gap: 10) {
                    Spacer
                    Button(id: cancel_delete, label: tr! { "zh-Hans" => "取消", "en" => "Cancel" }) => app.cancel_delete()
                    Button(id: confirm_delete, label: tr! { "zh-Hans" => "删除", "en" => "Delete" }, role: destructive, prominence: prominent) => app.confirm_delete()
                }
            }
            Shortcut(key: "escape") => app.cancel_delete()
        }
    }
}

/// The playing song's comments, the most liked first.
#[component]
pub fn comments_panel(app: enji::Client) -> impl View {
    ui! {
        ScrollView(indicators: hidden) {
            Column(gap: 20, padding_x: 14, padding_y: 88) {
                Text(content: tr! { "zh-Hans" => "评论 {count}", "en" => "Comments {count}"; count: app.comments_total }, style: title2, bold: true)
                ForEach(items: app.comments, key: id) |comment| {
                    Row(gap: 12, align: top) {
                        Image(source: comment.avatar, width: 36, height: 36, fit: cover, corner: 18)
                        Column(gap: 4, align: leading) {
                            Row(gap: 8, align: center) {
                                Text(content: comment.user, style: subheadline, emphasis: secondary, lines: 1)
                                Spacer
                                Label(title: comment.likes, icon: "heart", style: caption)
                            }
                            Text(content: comment.content)
                            Text(content: comment.time, style: caption, emphasis: tertiary)
                        }
                    }
                }
                if app.comments_loading {
                    Progress(label: tr! { "zh-Hans" => "正在加载评论", "en" => "Loading comments" })
                } else if app.comments_more {
                    Button(id: more_comments, label: tr! { "zh-Hans" => "更多评论", "en" => "More Comments" }) => app.more_comments()
                }
            }
        }
    }
}

/// The playing song's album and singer.
#[component]
pub fn details_panel(app: enji::Client) -> impl View {
    ui! {
        ScrollView(indicators: hidden) {
            Column(gap: 22, padding_x: 14, padding_y: 88) {
                if let Some(details) = app.details {
                    Row(gap: 16, align: center) {
                        Image(source: details.album_cover, width: 96, height: 96, fit: cover, corner: 10)
                        Column(gap: 4, align: leading) {
                            Text(content: details.album, style: title3, bold: true, lines: 2)
                            Text(content: details.released, style: caption, emphasis: secondary)
                            Text(content: details.company, style: caption, emphasis: secondary)
                        }
                    }
                    if details.album_description != "" {
                        Text(content: details.album_description, style: callout, emphasis: secondary)
                    }
                    Row(gap: 16, align: center) {
                        Image(source: details.artist_picture, width: 64, height: 64, fit: cover, corner: 32)
                        Text(content: details.artist, style: title3, bold: true)
                    }
                    if details.artist_description != "" {
                        Text(content: details.artist_description, style: callout, emphasis: secondary)
                    }
                } else {
                    Progress(label: tr! { "zh-Hans" => "正在加载详情", "en" => "Loading details" })
                }
            }
        }
    }
}

// MARK: - Settings

/// The settings in cards under the account, as wide as reads well on a
/// large window.
#[component]
pub fn settings_page(app: enji::Client) -> impl View {
    ui! {
        ScrollView {
            if window.compact {
                Column(padding_x: 16, padding_y: 12) {
                    settings_list(app: app)
                }
            } else {
                Row(padding_x: 32, padding_y: 28) {
                    Spacer
                    Column(width: 620) {
                        settings_list(app: app)
                    }
                    Spacer
                }
            }
        }
    }
}

#[component]
pub fn settings_list(app: enji::Client, #[session] draft: MainDraft) -> impl View {
    ui! {
        Column(gap: 24) {
            Text(content: tr! { "zh-Hans" => "设置", "en" => "Settings" }, style: large_title)
            if app.netease {
                Stack(variant: card) {
                    if app.login == Login::In {
                        Row(gap: 14, padding: 16, align: center) {
                            Image(source: app.avatar, width: 52, height: 52, fit: cover, corner: 26)
                            Column(gap: 2, align: leading) {
                                Text(content: app.nickname, style: title3, bold: true, lines: 1)
                                Text(content: tr! { "zh-Hans" => "网易云音乐", "en" => "NetEase Cloud Music" }, style: caption, emphasis: secondary)
                            }
                            Spacer
                            Button(id: settings_sign_out, label: tr! { "zh-Hans" => "退出登录", "en" => "Sign Out" }, shape: capsule) => app.sign_out()
                        }
                    } else {
                        Row(gap: 14, padding: 16, align: center) {
                            Image(source: "assets/mark.png", width: 44, height: 44, label: tr! { "zh-Hans" => "Enji Music", "en" => "Enji Music" })
                            Column(gap: 2, align: leading) {
                                Text(content: tr! { "zh-Hans" => "未登录", "en" => "Not Signed In" }, style: headline)
                                Text(content: tr! { "zh-Hans" => "登录后同步歌单、每日推荐和红心", "en" => "Sign in for your playlists, daily mix and likes" }, style: caption, emphasis: secondary)
                            }
                            Spacer
                            Button(id: settings_sign_in, label: tr! { "zh-Hans" => "扫码登录", "en" => "Sign In" }, icon: "qrcode", prominence: prominent, shape: capsule) => app.sign_in()
                        }
                    }
                }
            }
            Column(gap: 8) {
                Column(padding_x: 16) {
                    Text(content: tr! { "zh-Hans" => "可选功能", "en" => "Features" }, style: footnote, bold: true, emphasis: secondary)
                }
                Stack(variant: card) {
                    Column(gap: 0) {
                        Row(gap: 12, padding_x: 16, padding_y: 10, align: center) {
                            Column(width: 22, align: center) {
                                Stack(variant: quiet) {
                                    Label(title: tr! { "zh-Hans" => "网易云音乐", "en" => "NetEase Cloud Music" }, icon: "music.note.list", display: icon, font_size: 15)
                                }
                            }
                            Text(content: tr! { "zh-Hans" => "网易云音乐", "en" => "NetEase Cloud Music" })
                            Spacer
                            Switch(id: netease, label: tr! { "zh-Hans" => "网易云音乐", "en" => "NetEase Cloud Music" }, label_hidden: true, checked: app.netease, style: switch) => app.set_netease(app.netease)
                        }
                        if app.netease {
                            Column(padding_x: 16) { Divider }
                            Row(gap: 12, padding_x: 16, padding_y: 10, align: center) {
                                Column(width: 22, align: center) {
                                    Stack(variant: quiet) {
                                        Label(title: tr! { "zh-Hans" => "在线歌词与封面", "en" => "Online Lyrics and Covers" }, icon: "quote.bubble", display: icon, font_size: 15)
                                    }
                                }
                                Text(content: tr! { "zh-Hans" => "为本地音乐找歌词与封面", "en" => "Lyrics and Covers for Local Music" })
                                Spacer
                                Switch(id: online_info, label: tr! { "zh-Hans" => "为本地音乐找歌词与封面", "en" => "Lyrics and Covers for Local Music" }, label_hidden: true, checked: app.online_info, style: switch) => app.set_online_info(app.online_info)
                            }
                        }
                        // The switches of the modules that can be turned off.
                        Slot(id: feature_switches)
                    }
                }
            }
            Column(gap: 8) {
                Column(padding_x: 16) {
                    Text(content: tr! { "zh-Hans" => "本地音乐", "en" => "Local Music" }, style: footnote, bold: true, emphasis: secondary)
                }
                Stack(variant: card) {
                    Column(gap: 0) {
                        if app.phone_folder != "" {
                            Row(gap: 12, padding_x: 16, padding_y: 12, align: center) {
                                Column(width: 22, align: center) {
                                    Stack(variant: quiet) {
                                        Label(title: tr! { "zh-Hans" => "文件夹", "en" => "Folder" }, icon: "folder", display: icon, font_size: 15)
                                    }
                                }
                                Text(content: tr! { "zh-Hans" => "「文件」App 中的 Enji", "en" => "Enji in the Files App" })
                                Spacer
                                Text(content: tr! { "zh-Hans" => "{count} 首", "en" => "{count} songs"; count: app.local_count }, emphasis: secondary)
                            }
                        } else {
                            ForEach(items: app.folders, key: key) |folder| {
                                Row(gap: 12, padding_x: 16, padding_y: 10, align: center) {
                                    Column(width: 22, align: center) {
                                        Stack(variant: quiet) {
                                            Label(title: folder.name, icon: "folder", display: icon, font_size: 15)
                                        }
                                    }
                                    Column(gap: 2, align: leading) {
                                        Text(content: folder.name, lines: 1)
                                        if folder.path != "" {
                                            Text(content: folder.path, style: caption, emphasis: secondary, lines: 1)
                                        }
                                    }
                                    Spacer
                                    Text(content: tr! { "zh-Hans" => "{count} 首", "en" => "{count} songs"; count: folder.count }, emphasis: secondary)
                                    IconButton(id: remove_folder, icon: "xmark", font_size: 13, prominence: plain, label: tr! { "zh-Hans" => "移除文件夹", "en" => "Remove Folder" }) => app.remove_folder(folder.key)
                                }
                                Column(padding_x: 16) { Divider }
                            }
                            Row(gap: 10, padding_x: 16, padding_y: 10, align: center) {
                                Button(id: settings_add_folder, label: tr! { "zh-Hans" => "添加文件夹", "en" => "Add Folder" }, icon: "plus", shape: capsule, pick: draft.folder, files: "folder") => app.add_folder(draft.folder)
                                Spacer
                                if app.has_folders {
                                    Button(id: settings_rescan, label: tr! { "zh-Hans" => "重新扫描", "en" => "Scan Again" }, icon: "arrow.clockwise", prominence: plain) => app.rescan()
                                }
                            }
                        }
                    }
                }
            }
            Column(gap: 8) {
                Column(padding_x: 16) {
                    Text(content: tr! { "zh-Hans" => "播放", "en" => "Playback" }, style: footnote, bold: true, emphasis: secondary)
                }
                Stack(variant: card) {
                    Column(gap: 0) {
                        Row(gap: 12, padding_x: 16, padding_y: 10, align: center) {
                            Column(width: 22, align: center) {
                                Stack(variant: quiet) {
                                    Label(title: tr! { "zh-Hans" => "音质", "en" => "Quality" }, icon: "music.note", display: icon, font_size: 15)
                                }
                            }
                            Text(content: tr! { "zh-Hans" => "音质", "en" => "Quality" })
                            Spacer
                            Picker(id: quality, label: tr! { "zh-Hans" => "音质", "en" => "Quality" }, label_hidden: true, selected: app.quality, style: menu) => app.set_quality(app.quality)
                        }
                        Column(padding_x: 16) { Divider }
                        Row(gap: 12, padding_x: 16, padding_y: 10, align: center) {
                            Column(width: 22, align: center) {
                                Stack(variant: quiet) {
                                    Label(title: tr! { "zh-Hans" => "歌词翻译", "en" => "Lyric Translations" }, icon: "captions.bubble", display: icon, font_size: 15)
                                }
                            }
                            Text(content: tr! { "zh-Hans" => "歌词翻译", "en" => "Lyric Translations" })
                            Spacer
                            Switch(id: translation, label: tr! { "zh-Hans" => "歌词翻译", "en" => "Lyric Translations" }, label_hidden: true, checked: app.show_translation, style: switch) => app.set_show_translation(app.show_translation)
                        }
                        Column(padding_x: 16) { Divider }
                        Row(gap: 12, padding_x: 16, padding_y: 10, align: center) {
                            Column(width: 22, align: center) {
                                Stack(variant: quiet) {
                                    Label(title: tr! { "zh-Hans" => "音质信息", "en" => "Audio Format" }, icon: "info.circle", display: icon, font_size: 15)
                                }
                            }
                            Text(content: tr! { "zh-Hans" => "播放页显示编码与比特率", "en" => "Codec and Bitrate on the Playing Page" })
                            Spacer
                            Switch(id: show_format, label: tr! { "zh-Hans" => "播放页显示编码与比特率", "en" => "Codec and Bitrate on the Playing Page" }, label_hidden: true, checked: app.show_format, style: switch) => app.set_show_format(app.show_format)
                        }
                        Column(padding_x: 16) { Divider }
                        Row(gap: 12, padding_x: 16, padding_y: 10, align: center) {
                            Column(width: 22, align: center) {
                                Stack(variant: quiet) {
                                    Label(title: tr! { "zh-Hans" => "封面下的歌词", "en" => "Lyrics Under the Cover" }, icon: "quote.bubble", display: icon, font_size: 15)
                                }
                            }
                            Text(content: tr! { "zh-Hans" => "手机竖屏时封面下显示歌词", "en" => "Lyrics Under the Cover on a Phone" })
                            Spacer
                            Switch(id: cover_lyrics, label: tr! { "zh-Hans" => "手机竖屏时封面下显示歌词", "en" => "Lyrics Under the Cover on a Phone" }, label_hidden: true, checked: app.cover_lyrics, style: switch) => app.set_cover_lyrics(app.cover_lyrics)
                        }
                        if app.desktop_lyrics_supported {
                            Column(padding_x: 16) { Divider }
                            Row(gap: 12, padding_x: 16, padding_y: 10, align: center) {
                                Column(width: 22, align: center) {
                                    Stack(variant: quiet) {
                                        Label(title: tr! { "zh-Hans" => "桌面歌词", "en" => "Desktop Lyrics" }, icon: "captions.bubble", display: icon, font_size: 15)
                                    }
                                }
                                Text(content: tr! { "zh-Hans" => "桌面歌词", "en" => "Desktop Lyrics" })
                                Spacer
                                Switch(id: desktop_lyrics, label: tr! { "zh-Hans" => "桌面歌词", "en" => "Desktop Lyrics" }, label_hidden: true, checked: app.desktop_lyrics, style: switch) => app.set_desktop_lyrics(app.desktop_lyrics)
                            }
                            if app.desktop_lyrics {
                                Column(padding_x: 16) { Divider }
                                Row(gap: 12, padding_x: 16, padding_y: 10, align: center) {
                                    Column(width: 22, align: center) {
                                        Stack(variant: quiet) {
                                            Label(title: tr! { "zh-Hans" => "锁定桌面歌词", "en" => "Lock Desktop Lyrics" }, icon: "checkmark.circle", display: icon, font_size: 15)
                                        }
                                    }
                                    Text(content: tr! { "zh-Hans" => "锁定桌面歌词（点击穿透）", "en" => "Lock Desktop Lyrics (Clicks Pass Through)" })
                                    Spacer
                                    Switch(id: desktop_lyrics_locked, label: tr! { "zh-Hans" => "锁定桌面歌词", "en" => "Lock Desktop Lyrics" }, label_hidden: true, checked: app.desktop_lyrics_locked, style: switch) => app.set_desktop_lyrics_locked(app.desktop_lyrics_locked)
                                }
                            }
                        }
                        Column(padding_x: 16) { Divider }
                        Row(gap: 12, padding_x: 16, padding_y: 10, align: center) {
                            Column(width: 22, align: center) {
                                Stack(variant: quiet) {
                                    Label(title: tr! { "zh-Hans" => "播放次数", "en" => "Play Counts" }, icon: "chart.bar", display: icon, font_size: 15)
                                }
                            }
                            Text(content: tr! { "zh-Hans" => "列表中显示播放次数", "en" => "Play Counts in Lists" })
                            Spacer
                            Switch(id: play_counts, label: tr! { "zh-Hans" => "列表中显示播放次数", "en" => "Play Counts in Lists" }, label_hidden: true, checked: app.show_plays, style: switch) => app.set_show_plays(app.show_plays)
                        }
                    }
                }
            }
            Column(gap: 8) {
                Column(padding_x: 16) {
                    Text(content: tr! { "zh-Hans" => "插件", "en" => "Plugins" }, style: footnote, bold: true, emphasis: secondary)
                }
                Stack(variant: card) {
                    Column(gap: 0) {
                        ForEach(items: app.plugins, key: id) |plugin| {
                            Row(gap: 12, padding_x: 16, padding_y: 10, align: center) {
                                Column(gap: 2, align: leading) {
                                    Text(content: tr! { "zh-Hans" => "{name} {version}", "en" => "{name} {version}"; name: plugin.name, version: plugin.version })
                                    Text(content: plugin.description, style: caption, emphasis: secondary, lines: 2)
                                    if !plugin.fits {
                                        Text(content: tr! { "zh-Hans" => "仅支持 {platforms}", "en" => "{platforms} only"; platforms: plugin.platforms }, style: caption, emphasis: tertiary)
                                    }
                                }
                                Spacer
                                Switch(id: plugin_enabled, label: plugin.name, label_hidden: true, checked: plugin.enabled, style: switch) => app.set_plugin_enabled(plugin.id, plugin.enabled)
                                IconButton(id: remove_plugin, icon: "trash", font_size: 15, prominence: plain, label: tr! { "zh-Hans" => "删除插件", "en" => "Remove Plugin" }) => app.remove_plugin(plugin.id)
                            }
                            Column(padding_x: 16) { Divider }
                        }
                        Row(gap: 10, padding_x: 16, padding_y: 10, align: center) {
                            Button(id: import_plugin, label: tr! { "zh-Hans" => "导入插件…", "en" => "Import Plugin…" }, icon: "plus", shape: capsule, pick: draft.plugin_file, files: ".enj") => app.import_plugin(draft.plugin_file)
                            Button(id: plugin_market, label: tr! { "zh-Hans" => "插件商店", "en" => "Plugin Market" }, icon: "square.grid.2x2", shape: capsule) => app.load_offers()
                            Spacer
                            if app.offers_loading {
                                Progress(label: tr! { "zh-Hans" => "正在读取插件商店", "en" => "Reading the market" })
                            }
                        }
                        if app.has_offers {
                            Column(padding_x: 16) { Divider }
                            ForEach(items: app.offers, key: id) |offer| {
                                Row(gap: 12, padding_x: 16, padding_y: 10, align: center) {
                                    Column(gap: 2, align: leading) {
                                        Text(content: tr! { "zh-Hans" => "{name} {version}", "en" => "{name} {version}"; name: offer.name, version: offer.version })
                                        Text(content: offer.description, style: caption, emphasis: secondary, lines: 2)
                                        if !offer.fits {
                                            Text(content: tr! { "zh-Hans" => "仅支持 {platforms}", "en" => "{platforms} only"; platforms: offer.platforms }, style: caption, emphasis: tertiary)
                                        }
                                    }
                                    Spacer
                                    if offer.installed && !offer.newer {
                                        Text(content: tr! { "zh-Hans" => "已安装", "en" => "Installed" }, style: caption, emphasis: secondary)
                                    } else if offer.newer {
                                        Button(id: update_plugin, label: tr! { "zh-Hans" => "更新", "en" => "Update" }, shape: capsule) => app.install_plugin(offer.id)
                                    } else {
                                        Button(id: install_plugin, label: tr! { "zh-Hans" => "安装", "en" => "Install" }, shape: capsule, prominence: prominent) => app.install_plugin(offer.id)
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Column(gap: 8) {
                Column(padding_x: 16) {
                    Text(content: tr! { "zh-Hans" => "外观", "en" => "Appearance" }, style: footnote, bold: true, emphasis: secondary)
                }
                Stack(variant: card) {
                    Column(gap: 0) {
                        Row(gap: 12, padding_x: 16, padding_y: 10, align: center) {
                            Column(width: 22, align: center) {
                                Stack(variant: quiet) {
                                    Label(title: tr! { "zh-Hans" => "外观", "en" => "Appearance" }, icon: "paintpalette", display: icon, font_size: 15)
                                }
                            }
                            Text(content: tr! { "zh-Hans" => "外观", "en" => "Appearance" })
                            Spacer
                            Picker(id: appearance, label: tr! { "zh-Hans" => "外观", "en" => "Appearance" }, label_hidden: true, selected: app.appearance, style: segmented) => app.set_appearance(app.appearance)
                        }
                        Column(padding_x: 16) { Divider }
                        Row(gap: 12, padding_x: 16, padding_y: 10, align: center) {
                            Column(width: 22, align: center) {
                                Stack(variant: quiet) {
                                    Label(title: tr! { "zh-Hans" => "封面背景", "en" => "Cover Backdrop" }, icon: "photo", display: icon, font_size: 15)
                                }
                            }
                            Text(content: tr! { "zh-Hans" => "播放时以封面作背景", "en" => "Cover Behind the Window" })
                            Spacer
                            Switch(id: backdrop, label: tr! { "zh-Hans" => "播放时以封面作背景", "en" => "Cover Behind the Window" }, label_hidden: true, checked: app.backdrop, style: switch) => app.set_backdrop(app.backdrop)
                        }
                        Column(padding_x: 16) { Divider }
                        Row(gap: 12, padding_x: 16, padding_y: 10, align: center) {
                            Column(width: 22, align: center) {
                                Stack(variant: quiet) {
                                    Label(title: tr! { "zh-Hans" => "语言", "en" => "Language" }, icon: "globe", display: icon, font_size: 15)
                                }
                            }
                            Text(content: tr! { "zh-Hans" => "语言", "en" => "Language" })
                            Spacer
                            LanguagePicker(id: language, label: tr! { "zh-Hans" => "语言", "en" => "Language" }, label_hidden: true)
                        }
                    }
                }
            }
            Stack(variant: card) {
                Row(gap: 12, padding: 16, align: center) {
                    Image(source: "assets/mark.png", width: 36, height: 36, label: tr! { "zh-Hans" => "Enji Music", "en" => "Enji Music" })
                    Column(gap: 2, align: leading) {
                        Text(content: tr! { "zh-Hans" => "Enji Music", "en" => "Enji Music" }, style: headline)
                        Text(content: tr! { "zh-Hans" => "版本 0.1.0", "en" => "Version 0.1.0" }, style: caption, emphasis: secondary)
                    }
                    Spacer
                }
            }
        }
    }
}
