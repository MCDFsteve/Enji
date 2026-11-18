//! Shared desktop lyrics style state, persistence, and native overlay panel mapping.

use bern::overlay::{Control, Panel, TextEffect, TextStyle};
use enji_protocol::enji::{
    DesktopLyricEffect as Effect, DesktopLyricStyle as Style, DesktopLyricValue as Field,
};
use serde_json::{Value, json};

pub(super) const ID: &str = "lyrics";

pub(super) fn load(saved: &Value) -> Style {
    let number = |key: &str, fallback: f64, min: f64, max: f64| {
        saved[key]
            .as_f64()
            .filter(|n| n.is_finite())
            .unwrap_or(fallback)
            .round()
            .clamp(min, max)
    };
    let rgb = |key: &str| {
        split(
            saved[key]
                .as_u64()
                .filter(|v| *v <= 0xffffff)
                .unwrap_or(0xffffff) as u32,
        )
    };
    let [primary_red, primary_green, primary_blue] = rgb("primary_rgb");
    let [secondary_red, secondary_green, secondary_blue] = rgb("secondary_rgb");
    let phone = cfg!(target_os = "android");
    Style {
        primary_size: number("primary_size", if phone { 20.0 } else { 30.0 }, 10.0, 72.0),
        secondary_size: number(
            "secondary_size",
            if phone { 14.0 } else { 18.0 },
            10.0,
            72.0,
        ),
        primary_red,
        primary_green,
        primary_blue,
        secondary_red,
        secondary_green,
        secondary_blue,
        opacity: number("opacity", 100.0, 0.0, 100.0),
        effect: match saved["effect"].as_str() {
            Some("none") => Effect::None,
            Some("outline") => Effect::Outline,
            _ => Effect::Shadow,
        },
    }
}

pub(super) fn save(s: &Style) -> Value {
    json!({
        "primary_size": s.primary_size,
        "secondary_size": s.secondary_size,
        "primary_rgb": rgb(color(s, true)),
        "secondary_rgb": rgb(color(s, false)),
        "opacity": s.opacity,
        "effect": match s.effect {
            Effect::None => "none",
            Effect::Shadow => "shadow",
            Effect::Outline => "outline",
        },
    })
}

fn split(rgb: u32) -> [f64; 3] {
    [
        (rgb >> 16) as f64,
        ((rgb >> 8) & 255) as f64,
        (rgb & 255) as f64,
    ]
}

fn rgb([r, g, b, _]: [u8; 4]) -> u32 {
    u32::from_be_bytes([0, r, g, b])
}

pub(super) fn color(s: &Style, primary: bool) -> [u8; 4] {
    if primary {
        [
            s.primary_red as u8,
            s.primary_green as u8,
            s.primary_blue as u8,
            255,
        ]
    } else {
        [
            s.secondary_red as u8,
            s.secondary_green as u8,
            s.secondary_blue as u8,
            215,
        ]
    }
}

pub(super) fn text(s: &Style) -> TextStyle {
    TextStyle {
        opacity: s.opacity / 100.0,
        effect: match s.effect {
            Effect::None => TextEffect::None,
            Effect::Shadow => TextEffect::Shadow,
            Effect::Outline => TextEffect::Outline,
        },
    }
}

pub enum Edit {
    Number(Field, f64),
    Color { primary: bool, rgb: u32 },
    Effect(Effect),
}

impl Edit {
    pub(super) fn native(id: &str, value: f64) -> Option<Self> {
        if !value.is_finite() {
            return None;
        }
        Some(match id {
            "primary_size" => Self::Number(Field::PrimarySize, value),
            "secondary_size" => Self::Number(Field::SecondarySize, value),
            "opacity" => Self::Number(Field::Opacity, value),
            "primary_rgb" | "secondary_rgb" if (0.0..=16_777_215.0).contains(&value) => {
                Self::Color {
                    primary: id == "primary_rgb",
                    rgb: value.round() as u32,
                }
            }
            "effect" => Self::Effect(match value.round() as i64 {
                0 => Effect::None,
                1 => Effect::Shadow,
                2 => Effect::Outline,
                _ => return None,
            }),
            _ => return None,
        })
    }

    pub(super) fn apply(self, s: &mut Style) {
        match self {
            Self::Effect(effect) => s.effect = effect,
            Self::Color { primary, rgb } => {
                let [r, g, b] = split(rgb);
                if primary {
                    (s.primary_red, s.primary_green, s.primary_blue) = (r, g, b);
                } else {
                    (s.secondary_red, s.secondary_green, s.secondary_blue) = (r, g, b);
                }
            }
            Self::Number(field, value) if value.is_finite() => {
                let (slot, min, max) = match field {
                    Field::PrimarySize => (&mut s.primary_size, 10., 72.),
                    Field::SecondarySize => (&mut s.secondary_size, 10., 72.),
                    Field::Opacity => (&mut s.opacity, 0., 100.),
                    Field::PrimaryRed => (&mut s.primary_red, 0., 255.),
                    Field::PrimaryGreen => (&mut s.primary_green, 0., 255.),
                    Field::PrimaryBlue => (&mut s.primary_blue, 0., 255.),
                    Field::SecondaryRed => (&mut s.secondary_red, 0., 255.),
                    Field::SecondaryGreen => (&mut s.secondary_green, 0., 255.),
                    Field::SecondaryBlue => (&mut s.secondary_blue, 0., 255.),
                };
                *slot = value.round().clamp(min, max);
            }
            Self::Number(_, _) => {}
        }
    }
}

pub(super) fn panel(s: &Style, language: &str) -> Panel {
    let label = |zh: &str, en: &str| {
        if language.starts_with("en") {
            en.to_owned()
        } else {
            zh.to_owned()
        }
    };
    let range = |id: &str, zh, en, min, max, value| Control::Range {
        id: id.into(),
        label: label(zh, en),
        min,
        max,
        step: 1.,
        value,
    };
    let colour = |id: &str, zh, en, primary| Control::Color {
        id: id.into(),
        label: label(zh, en),
        rgb: rgb(color(s, primary)),
    };
    Panel {
        title: label("桌面歌词", "Desktop Lyrics"),
        controls: vec![
            range(
                "primary_size",
                "主行字号",
                "Primary text size",
                10.,
                72.,
                s.primary_size,
            ),
            colour("primary_rgb", "主行颜色", "Primary color", true),
            range(
                "secondary_size",
                "副行字号",
                "Secondary text size",
                10.,
                72.,
                s.secondary_size,
            ),
            colour("secondary_rgb", "副行颜色", "Secondary color", false),
            range(
                "opacity",
                "文字不透明度 (%)",
                "Text opacity (%)",
                0.,
                100.,
                s.opacity,
            ),
            Control::Choice {
                id: "effect".into(),
                label: label("文字效果", "Text effect"),
                options: vec![
                    label("无", "None"),
                    label("阴影", "Shadow"),
                    label("描边", "Outline"),
                ],
                selected: match s.effect {
                    Effect::None => 0,
                    Effect::Shadow => 1,
                    Effect::Outline => 2,
                },
            },
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_and_settings_edits_share_one_saved_style() {
        let mut s = load(&Value::Null);
        assert_eq!(text(&s), TextStyle::default());
        assert_eq!(color(&s, false), [255, 255, 255, 215]);
        Edit::native("primary_rgb", 0x123456 as f64)
            .unwrap()
            .apply(&mut s);
        Edit::Number(Field::PrimaryGreen, 170.).apply(&mut s);
        Edit::native("effect", 2.).unwrap().apply(&mut s);
        Edit::Number(Field::Opacity, 0.).apply(&mut s);
        assert_eq!(color(&s, true), [0x12, 0xaa, 0x56, 255]);
        assert_eq!(
            text(&s),
            TextStyle {
                effect: TextEffect::Outline,
                opacity: 0.
            }
        );
        assert_eq!(load(&save(&s)), s);
    }
}
