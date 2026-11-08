//! Readings supplied with a song, aligned without changing its written lyrics.

use std::collections::HashSet;

/// A reading above a half-open range of Unicode scalar positions in the original.
#[derive(Debug, Clone, PartialEq)]
pub struct Ruby {
    pub start: usize,
    pub end: usize,
    pub reading: String,
}

/// The wire format understood by `Text(ruby:)` and floating lyrics.
pub fn encoded(ruby: &[Ruby]) -> String {
    if ruby.is_empty() {
        String::new()
    } else {
        serde_json::Value::Array(
            ruby.iter()
                .map(|r| serde_json::json!([r.start, r.end, r.reading]))
                .collect(),
        )
        .to_string()
    }
}

fn hiragana(c: char) -> char {
    if ('ァ'..='ヶ').contains(&c) {
        char::from_u32(c as u32 - 0x60).unwrap_or(c)
    } else {
        c
    }
}

fn kanji(c: char) -> bool {
    matches!(c, '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}' | '\u{f900}'..='\u{faff}' | '\u{20000}'..='\u{323af}' | '々' | '〆' | '〇')
}

fn kana(c: char) -> bool {
    matches!(c, 'ぁ'..='ゖ' | 'ァ'..='ヶ' | 'ー')
}

fn kept(c: char) -> bool {
    kanji(c) || kana(c) || c.is_alphanumeric()
}

/// Roman syllables are separated by spaces in the service's lyrics. English
/// words occurring verbatim in the original remain literal matching anchors.
fn reading(text: &str, original: &str) -> Vec<char> {
    let english: HashSet<String> = original
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    let mut result = String::new();
    let mut word = String::new();
    let flush = |word: &mut String, result: &mut String| {
        if word.is_empty() {
            return;
        }
        let lower = word.to_ascii_lowercase();
        if english.contains(&lower) {
            result.push_str(&lower);
        } else {
            roman_word(&lower, result);
        }
        word.clear();
    };
    for c in text.chars() {
        if c.is_ascii_alphabetic()
            || matches!(c, 'ā' | 'ī' | 'ū' | 'ē' | 'ō' | 'â' | 'î' | 'û' | 'ê' | 'ô')
        {
            word.push(c);
        } else {
            flush(&mut word, &mut result);
            if kept(c) {
                result.push(hiragana(c));
            }
        }
    }
    flush(&mut word, &mut result);
    result.chars().collect()
}

fn roman_word(word: &str, out: &mut String) {
    let word = word
        .replace(['ā', 'â'], "aa")
        .replace(['ī', 'î'], "ii")
        .replace(['ū', 'û'], "uu")
        .replace(['ē', 'ê'], "ee")
        .replace(['ō', 'ô'], "ou");
    let mut rest = word.as_str();
    while !rest.is_empty() {
        let bytes = rest.as_bytes();
        if bytes.len() > 1 && bytes[0] == bytes[1] && b"bcdfghjkpqrstvwxyz".contains(&bytes[0]) {
            out.push('っ');
            rest = &rest[1..];
            continue;
        }
        if let Some((roman, kana)) = SYLLABLES.iter().find(|(r, _)| rest.starts_with(*r)) {
            out.push_str(kana);
            rest = &rest[roman.len()..];
        } else if rest.starts_with('n') {
            out.push('ん');
            rest = &rest[1..];
        } else {
            let c = rest.chars().next().unwrap();
            out.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
}

// Longest spellings precede their prefixes; both Hepburn and common keyboard
// spellings are accepted. No dictionary or network request is involved.
const SYLLABLES: &[(&str, &str)] = &[
    ("xtsu", "っ"),
    ("ltsu", "っ"),
    ("xtu", "っ"),
    ("ltu", "っ"),
    ("kya", "きゃ"),
    ("kyu", "きゅ"),
    ("kyo", "きょ"),
    ("gya", "ぎゃ"),
    ("gyu", "ぎゅ"),
    ("gyo", "ぎょ"),
    ("sha", "しゃ"),
    ("shu", "しゅ"),
    ("sho", "しょ"),
    ("she", "しぇ"),
    ("sya", "しゃ"),
    ("syu", "しゅ"),
    ("syo", "しょ"),
    ("cha", "ちゃ"),
    ("chu", "ちゅ"),
    ("cho", "ちょ"),
    ("che", "ちぇ"),
    ("cya", "ちゃ"),
    ("cyu", "ちゅ"),
    ("cyo", "ちょ"),
    ("tya", "ちゃ"),
    ("tyu", "ちゅ"),
    ("tyo", "ちょ"),
    ("nya", "にゃ"),
    ("nyu", "にゅ"),
    ("nyo", "にょ"),
    ("hya", "ひゃ"),
    ("hyu", "ひゅ"),
    ("hyo", "ひょ"),
    ("bya", "びゃ"),
    ("byu", "びゅ"),
    ("byo", "びょ"),
    ("pya", "ぴゃ"),
    ("pyu", "ぴゅ"),
    ("pyo", "ぴょ"),
    ("mya", "みゃ"),
    ("myu", "みゅ"),
    ("myo", "みょ"),
    ("rya", "りゃ"),
    ("ryu", "りゅ"),
    ("ryo", "りょ"),
    ("jya", "じゃ"),
    ("jyu", "じゅ"),
    ("jyo", "じょ"),
    ("zya", "じゃ"),
    ("zyu", "じゅ"),
    ("zyo", "じょ"),
    ("shi", "し"),
    ("chi", "ち"),
    ("tsu", "つ"),
    ("dzu", "づ"),
    ("tsa", "つぁ"),
    ("tsi", "つぃ"),
    ("tse", "つぇ"),
    ("tso", "つぉ"),
    ("thi", "てぃ"),
    ("thu", "てゅ"),
    ("dhi", "でぃ"),
    ("dhu", "でゅ"),
    ("kwa", "くぁ"),
    ("kwi", "くぃ"),
    ("kwe", "くぇ"),
    ("kwo", "くぉ"),
    ("gwa", "ぐぁ"),
    ("gwi", "ぐぃ"),
    ("gwe", "ぐぇ"),
    ("gwo", "ぐぉ"),
    ("ja", "じゃ"),
    ("ju", "じゅ"),
    ("jo", "じょ"),
    ("je", "じぇ"),
    ("ka", "か"),
    ("ki", "き"),
    ("ku", "く"),
    ("ke", "け"),
    ("ko", "こ"),
    ("ga", "が"),
    ("gi", "ぎ"),
    ("gu", "ぐ"),
    ("ge", "げ"),
    ("go", "ご"),
    ("sa", "さ"),
    ("si", "し"),
    ("su", "す"),
    ("se", "せ"),
    ("so", "そ"),
    ("za", "ざ"),
    ("zi", "じ"),
    ("zu", "ず"),
    ("ze", "ぜ"),
    ("zo", "ぞ"),
    ("ji", "じ"),
    ("ta", "た"),
    ("ti", "てぃ"),
    ("tu", "とぅ"),
    ("te", "て"),
    ("to", "と"),
    ("da", "だ"),
    ("di", "でぃ"),
    ("du", "どぅ"),
    ("de", "で"),
    ("do", "ど"),
    ("na", "な"),
    ("ni", "に"),
    ("nu", "ぬ"),
    ("ne", "ね"),
    ("no", "の"),
    ("ha", "は"),
    ("hi", "ひ"),
    ("hu", "ふ"),
    ("he", "へ"),
    ("ho", "ほ"),
    ("fu", "ふ"),
    ("ba", "ば"),
    ("bi", "び"),
    ("bu", "ぶ"),
    ("be", "べ"),
    ("bo", "ぼ"),
    ("pa", "ぱ"),
    ("pi", "ぴ"),
    ("pu", "ぷ"),
    ("pe", "ぺ"),
    ("po", "ぽ"),
    ("ma", "ま"),
    ("mi", "み"),
    ("mu", "む"),
    ("me", "め"),
    ("mo", "も"),
    ("ya", "や"),
    ("yu", "ゆ"),
    ("yo", "よ"),
    ("ra", "ら"),
    ("ri", "り"),
    ("ru", "る"),
    ("re", "れ"),
    ("ro", "ろ"),
    ("wa", "わ"),
    ("wi", "うぃ"),
    ("we", "うぇ"),
    ("wo", "を"),
    ("fa", "ふぁ"),
    ("fi", "ふぃ"),
    ("fe", "ふぇ"),
    ("fo", "ふぉ"),
    ("va", "ゔぁ"),
    ("vi", "ゔぃ"),
    ("vu", "ゔ"),
    ("ve", "ゔぇ"),
    ("vo", "ゔぉ"),
    ("xa", "ぁ"),
    ("xi", "ぃ"),
    ("xu", "ぅ"),
    ("xe", "ぇ"),
    ("xo", "ぉ"),
    ("la", "ぁ"),
    ("li", "ぃ"),
    ("lu", "ぅ"),
    ("le", "ぇ"),
    ("lo", "ぉ"),
    ("a", "あ"),
    ("i", "い"),
    ("u", "う"),
    ("e", "え"),
    ("o", "お"),
];

fn vowel(c: char) -> Option<char> {
    for (letters, result) in SYLLABLES {
        if result.chars().last() == Some(c) {
            return match letters.as_bytes().last()? {
                b'a' => Some('あ'),
                b'i' => Some('い'),
                b'u' => Some('う'),
                b'e' => Some('え'),
                b'o' => Some('お'),
                _ => None,
            };
        }
    }
    None
}

fn matches(original: char, sung: char, previous: Option<char>) -> bool {
    original == sung
        || matches!(
            (original, sung),
            ('は', 'わ') | ('へ', 'え') | ('を', 'お') | ('ぢ', 'じ') | ('づ', 'ず')
        )
        || (original == 'ー' && (sung == 'ー' || previous.and_then(vowel) == Some(sung)))
}

/// A complete-line alignment. The memo table bounds repeated anchors without
/// greedily taking the first occurrence of a kana in a reading.
pub fn annotate(original: &str, roman: &str) -> Vec<Ruby> {
    let chars: Vec<char> = original.chars().collect();
    let written: Vec<(usize, char)> = chars
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, c)| kept(*c))
        .map(|(i, c)| (i, hiragana(c).to_ascii_lowercase()))
        .collect();
    let sung = reading(roman, original);
    let mut failed = HashSet::new();
    let mut ruby = Vec::new();
    fn align(
        written: &[(usize, char)],
        sung: &[char],
        i: usize,
        j: usize,
        failed: &mut HashSet<(usize, usize)>,
        ruby: &mut Vec<Ruby>,
    ) -> bool {
        if i == written.len() {
            return j == sung.len();
        }
        if failed.contains(&(i, j)) {
            return false;
        }
        let c = written[i].1;
        if kanji(c) {
            let mut end = i + 1;
            while end < written.len()
                && kanji(written[end].1)
                && written[end].0 == written[end - 1].0 + 1
            {
                end += 1;
            }
            for after in j + 1..=sung.len() {
                // A roman typo must not become a purported Japanese reading.
                if sung[j..after].iter().any(|c| !kana(*c)) {
                    break;
                }
                ruby.push(Ruby {
                    start: written[i].0,
                    end: written[end - 1].0 + 1,
                    reading: sung[j..after].iter().collect(),
                });
                if align(written, sung, end, after, failed, ruby) {
                    return true;
                }
                ruby.pop();
            }
        } else if let Some(&sung_char) = sung.get(j) {
            let previous = i.checked_sub(1).map(|i| written[i].1);
            if matches(c, sung_char, previous) && align(written, sung, i + 1, j + 1, failed, ruby) {
                return true;
            }
            let spelling: &[char] = match c {
                'ち' => &['て', 'ぃ'],
                'ぢ' => &['で', 'ぃ'],
                'つ' => &['と', 'ぅ'],
                'づ' => &['ど', 'ぅ'],
                _ => &[],
            };
            if !spelling.is_empty()
                && sung[j..].starts_with(spelling)
                && align(written, sung, i + 1, j + spelling.len(), failed, ruby)
            {
                return true;
            }
        }
        failed.insert((i, j));
        false
    }
    // A pure-kanji line can also be Chinese. A supplied Japanese reading is
    // enough evidence; without it only explicitly written katakana is read.
    if sung.is_empty() || !align(&written, &sung, 0, 0, &mut failed, &mut ruby) {
        ruby.clear();
    }
    let mut i = 0;
    while i < chars.len() {
        if ('ァ'..='ヶ').contains(&chars[i]) {
            let start = i;
            i += 1;
            while i < chars.len() && (('ァ'..='ヶ').contains(&chars[i]) || chars[i] == 'ー') {
                i += 1;
            }
            ruby.push(Ruby {
                start,
                end: i,
                reading: chars[start..i].iter().copied().map(hiragana).collect(),
            });
        } else {
            i += 1;
        }
    }
    ruby.sort_by_key(|r| r.start);
    ruby
}

/// Carry readings between lyric versions only when the original text agrees.
pub fn remap(from: &str, to: &str, ruby: &[Ruby]) -> Option<Vec<Ruby>> {
    let compact = |s: &str| {
        s.chars()
            .enumerate()
            .filter(|(_, c)| !c.is_whitespace())
            .collect::<Vec<_>>()
    };
    let a = compact(from);
    let b = compact(to);
    if !a.iter().map(|(_, c)| c).eq(b.iter().map(|(_, c)| c)) {
        return None;
    }
    ruby.iter()
        .map(|r| {
            let start = a.iter().position(|(i, _)| *i == r.start)?;
            let end = a.iter().position(|(i, _)| *i + 1 == r.end)?;
            Some(Ruby {
                start: b[start].0,
                end: b[end].0 + 1,
                reading: r.reading.clone(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans(text: &str, roman: &str) -> Vec<(String, String)> {
        let chars: Vec<char> = text.chars().collect();
        annotate(text, roman)
            .into_iter()
            .map(|r| (chars[r.start..r.end].iter().collect(), r.reading))
            .collect()
    }

    #[test]
    fn aligns_the_supplied_song_reading() {
        assert_eq!(
            spans(
                "無敵の笑顔で荒らすメディア",
                "mu te ki no e ga o de a ra su me di a"
            ),
            [
                ("無敵".into(), "むてき".into()),
                ("笑顔".into(), "えがお".into()),
                ("荒".into(), "あ".into()),
                ("メディア".into(), "めでぃあ".into())
            ]
        );
    }

    #[test]
    fn preserves_particles_and_searches_past_repeated_anchors() {
        assert_eq!(
            spans("この部屋には", "ko no he ya ni wa"),
            [("部屋".into(), "へや".into())]
        );
        assert_eq!(spans("君へ", "ki mi e"), [("君".into(), "きみ".into())]);
        assert_eq!(spans("君を", "ki mi o"), [("君".into(), "きみ".into())]);
        assert_eq!(spans("祝い", "i wa i"), [("祝".into(), "いわ".into())]);
        assert_eq!(spans("今日", "kyo u"), [("今日".into(), "きょう".into())]);
    }

    #[test]
    fn accepts_gemination_nasal_and_long_vowels() {
        assert_eq!(
            reading("ga kko u shi n yo u", ""),
            "がっこうしんよう".chars().collect::<Vec<_>>()
        );
        assert_eq!(
            spans("パーティーへ行こう", "pa a thi i e i ko u").last(),
            Some(&("行".into(), "い".into()))
        );
        assert_eq!(
            spans("君とLOVE", "ki mi to LOVE"),
            [("君".into(), "きみ".into())]
        );
    }

    #[test]
    fn ranges_are_original_character_positions() {
        let result = annotate("🎵 今日", "kyo u");
        assert_eq!(
            result,
            [Ruby {
                start: 2,
                end: 4,
                reading: "きょう".into()
            }]
        );
        assert_eq!(
            remap("🎵 今日", "🎵今 日", &result),
            Some(vec![Ruby {
                start: 1,
                end: 4,
                reading: "きょう".into()
            }])
        );
        assert_eq!(remap("今日", "明日", &result), None);
        assert!(annotate("今日", "broken input").is_empty());
    }
}
