//! The For You page: a greeting, today's songs for the account and the
//! playlists NetEase recommends.

use bern::prelude::*;
use enji_protocol::enji::{self, Detail, Source};
use enji_ui::{detail_view, detail_view_props, playlist_card, playlist_card_props, song_row, song_row_props};
use recommend_protocol::recommend::{self, DayPart};

/// The page, or what was opened from it over it.
#[component]
pub fn recommend_page(app: enji::Client, recommend: recommend::Client) -> impl View {
    ui! {
        if app.detail != Detail::Page {
            detail_view(app: app)
        } else {
            home_page(app: app, recommend: recommend)
        }
    }
}

/// Good morning, afternoon or evening, with the account's name.
#[component]
pub fn greeting(app: enji::Client, recommend: recommend::Client) -> impl View {
    ui! {
        if app.nickname == "" {
            match recommend.day_part {
                DayPart::Morning => { Text(content: tr! { "zh-Hans" => "早上好", "en" => "Good Morning" }, style: large_title) }
                DayPart::Noon => { Text(content: tr! { "zh-Hans" => "中午好", "en" => "Hello" }, style: large_title) }
                DayPart::Afternoon => { Text(content: tr! { "zh-Hans" => "下午好", "en" => "Good Afternoon" }, style: large_title) }
                DayPart::Evening => { Text(content: tr! { "zh-Hans" => "晚上好", "en" => "Good Evening" }, style: large_title) }
            }
        } else {
            match recommend.day_part {
                DayPart::Morning => { Text(content: tr! { "zh-Hans" => "早上好，{name}", "en" => "Good Morning, {name}"; name: app.nickname }, style: large_title) }
                DayPart::Noon => { Text(content: tr! { "zh-Hans" => "中午好，{name}", "en" => "Hello, {name}"; name: app.nickname }, style: large_title) }
                DayPart::Afternoon => { Text(content: tr! { "zh-Hans" => "下午好，{name}", "en" => "Good Afternoon, {name}"; name: app.nickname }, style: large_title) }
                DayPart::Evening => { Text(content: tr! { "zh-Hans" => "晚上好，{name}", "en" => "Good Evening, {name}"; name: app.nickname }, style: large_title) }
            }
        }
    }
}

#[component]
pub fn home_page(app: enji::Client, recommend: recommend::Client) -> impl View {
    ui! {
        ScrollView {
            Column(gap: 20, padding: if window.compact { 18 } else { 32 }) {
                greeting(app: app, recommend: recommend)
                if recommend.has_daily {
                    Row(gap: 12, align: center) {
                        Text(content: tr! { "zh-Hans" => "每日推荐", "en" => "Daily Mix" }, style: title2)
                        Spacer
                        Button(id: play_daily, label: tr! { "zh-Hans" => "播放全部", "en" => "Play All" }, icon: "play.fill", prominence: prominent, shape: capsule, height: 32) => app.play(Source::Daily, 0)
                    }
                    Grid(min_width: 320, gap: 2) {
                        ForEach(items: recommend.daily, key: index) |song| {
                            song_row(app: app, song: song, source: Source::Daily, full: false, removable: false)
                        }
                    }
                }
                Text(content: tr! { "zh-Hans" => "推荐歌单", "en" => "Playlists for You" }, style: title2)
                if recommend.loading {
                    Progress(label: tr! { "zh-Hans" => "正在加载", "en" => "Loading" })
                }
                Grid(min_width: 160, gap: 22, cell_ratio: 1, cell_extra: if window.tv { 124 } else { 58 }) {
                    ForEach(items: recommend.picks, key: id, stagger: 0.03) |list| {
                        playlist_card(app: app, list: list)
                    }
                }
            }
        }
    }
}

/// The way from the empty history to this page.
#[component]
pub fn see_for_you(recommend: recommend::Client) -> impl View {
    ui! {
        if recommend.available {
            Button(id: go_for_you, label: tr! { "zh-Hans" => "去看看推荐", "en" => "See For You" }, icon: "sparkles", shape: capsule, height: 40) => pages.home.show()
        }
    }
}
