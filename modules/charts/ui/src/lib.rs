//! The Charts page: NetEase's charts as cards.

use bern::prelude::*;
use charts_protocol::charts;
use enji_protocol::enji::{self, Detail};
use enji_ui::{
    detail_view, detail_view_props, playlist_card, playlist_card_props, watch_playlist_row,
    watch_playlist_row_props,
};

/// The charts, or the one opened over them.
#[component]
pub fn charts_page(app: enji::Client, charts: charts::Client) -> impl View {
    ui! {
        if app.detail != Detail::Page {
            detail_view(app: app)
        } else if window.watch {
            ScrollView {
                Column(gap: 8, padding_x: 14, padding_y: 24) {
                    Row(align: center) {
                        Spacer
                        Text(content: tr! { "zh-Hans" => "排行榜", "en" => "Charts" }, style: headline, bold: true)
                        Spacer
                    }
                    if charts.loading {
                        Progress(label: tr! { "zh-Hans" => "正在加载", "en" => "Loading" })
                    }
                    Column(gap: 4) {
                        ForEach(items: charts.charts, key: id) |list| {
                            watch_playlist_row(app: app, list: list)
                        }
                    }
                }
            }
        } else {
            ScrollView {
                Column(gap: 20, padding: if window.compact { 18 } else { 32 }) {
                    Text(content: tr! { "zh-Hans" => "排行榜", "en" => "Charts" }, style: large_title)
                    if charts.loading {
                        Progress(label: tr! { "zh-Hans" => "正在加载", "en" => "Loading" })
                    }
                    Grid(min_width: 160, gap: 22, cell_ratio: 1, cell_extra: if window.tv { 124 } else { 58 }) {
                        ForEach(items: charts.charts, key: id, stagger: 0.03) |list| {
                            playlist_card(app: app, list: list)
                        }
                    }
                }
            }
        }
    }
}
