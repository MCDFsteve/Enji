//! Shared desktop lyrics style state and persistence.

use bern::overlay::{TextEffect, TextStyle};
use enji_protocol::enji::{
    DesktopLyricEffect as Effect, DesktopLyricStyle as Style, DesktopLyricValue as Field,
};
use serde_json::{Value, json};

pub(super) const OVERLAY_ID: &str = "lyrics";
pub(super) const SETTINGS_WINDOW_ID: &str = "desktop_lyrics_settings";

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
        saved[key]
            .as_i64()
            .filter(|v| (0..=0xffffff).contains(v))
            .unwrap_or(0xffffff)
    };
    let phone = cfg!(target_os = "android");
    Style {
        primary_size: number("primary_size", if phone { 20.0 } else { 30.0 }, 10.0, 72.0),
        secondary_size: number(
            "secondary_size",
            if phone { 14.0 } else { 18.0 },
            10.0,
            72.0,
        ),
        primary_rgb: rgb("primary_rgb"),
        secondary_rgb: rgb("secondary_rgb"),
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
        "primary_rgb": s.primary_rgb.clamp(0, 0xffffff),
        "secondary_rgb": s.secondary_rgb.clamp(0, 0xffffff),
        "opacity": s.opacity,
        "effect": match s.effect {
            Effect::None => "none",
            Effect::Shadow => "shadow",
            Effect::Outline => "outline",
        },
    })
}

pub(super) fn color(s: &Style, primary: bool) -> [u8; 4] {
    let raw = if primary { s.primary_rgb } else { s.secondary_rgb }.clamp(0, 0xffffff) as u32;
    let [_, r, g, b] = raw.to_be_bytes();
    [r, g, b, if primary { 255 } else { 215 }]
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
    Color { primary: bool, rgb: i64 },
    Effect(Effect),
}

impl Edit {
    pub(super) fn apply(self, s: &mut Style) {
        match self {
            Self::Effect(effect) => s.effect = effect,
            Self::Color { primary, rgb } => {
                let clamped = rgb.clamp(0, 0xffffff);
                if primary {
                    s.primary_rgb = clamped;
                } else {
                    s.secondary_rgb = clamped;
                }
            }
            Self::Number(field, value) if value.is_finite() => {
                let (slot, min, max) = match field {
                    Field::PrimarySize => (&mut s.primary_size, 10., 72.),
                    Field::SecondarySize => (&mut s.secondary_size, 10., 72.),
                    Field::Opacity => (&mut s.opacity, 0., 100.),
                };
                *slot = value.round().clamp(min, max);
            }
            Self::Number(_, _) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_edits_save_and_restore_cleanly() {
        let mut s = load(&Value::Null);
        assert_eq!(text(&s), TextStyle::default());
        assert_eq!(color(&s, false), [255, 255, 255, 215]);
        Edit::Color {
            primary: true,
            rgb: 0x12aa56,
        }
        .apply(&mut s);
        Edit::Effect(Effect::Outline).apply(&mut s);
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
