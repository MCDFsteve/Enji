//! Readings supplied with a song, aligned without changing its written lyrics.

use std::collections::HashMap;

/// A reading above a half-open range of Unicode scalar positions in the original.
#[derive(Debug, Clone, PartialEq)]
pub struct Ruby {
    pub start: usize,
    pub end: usize,
    pub reading: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Break {
    None = 0,
    Syllable = 1,
    Phrase = 2,
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

fn norm_ascii(c: char) -> char {
    match c {
        '０'..='９' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        'Ａ'..='Ｚ' => char::from_u32(c as u32 - 0xFEE0)
            .unwrap_or(c)
            .to_ascii_lowercase(),
        'ａ'..='ｚ' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
        _ => c.to_ascii_lowercase(),
    }
}

fn kanji(c: char) -> bool {
    matches!(
        c,
        '\u{3400}'..='\u{4dbf}'
            | '\u{4e00}'..='\u{9fff}'
            | '\u{f900}'..='\u{faff}'
            | '\u{20000}'..='\u{323af}'
            | '々'
            | '〆'
            | '〇'
    )
}

fn kana(c: char) -> bool {
    matches!(c, 'ぁ'..='ゖ' | 'ァ'..='ヶ' | 'ー')
}

fn kept(c: char) -> bool {
    kanji(c) || kana(c) || c.is_alphanumeric()
}

fn small_kana(c: char) -> bool {
    matches!(
        c,
        'ぁ' | 'ぃ' | 'ぅ' | 'ぇ' | 'ぉ' | 'ゃ' | 'ゅ' | 'ょ' | 'ゎ' | 'ゕ' | 'ゖ'
    )
}

fn dependent_start(c: char) -> bool {
    small_kana(c) || matches!(c, 'っ' | 'ん' | 'ー')
}

fn large_kana(c: char) -> char {
    match c {
        'ぁ' => 'あ',
        'ぃ' => 'い',
        'ぅ' => 'う',
        'ぇ' => 'え',
        'ぉ' => 'お',
        'ゃ' => 'や',
        'ゅ' => 'ゆ',
        'ょ' => 'よ',
        'ゎ' => 'わ',
        'っ' => 'つ',
        _ => c,
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
    ("kyi", "きぃ"),
    ("kyu", "きゅ"),
    ("kye", "きぇ"),
    ("kyo", "きょ"),
    ("gya", "ぎゃ"),
    ("gyi", "ぎぃ"),
    ("gyu", "ぎゅ"),
    ("gye", "ぎぇ"),
    ("gyo", "ぎょ"),
    ("sha", "しゃ"),
    ("shu", "しゅ"),
    ("she", "しぇ"),
    ("sho", "しょ"),
    ("sya", "しゃ"),
    ("syi", "しぃ"),
    ("syu", "しゅ"),
    ("sye", "しぇ"),
    ("syo", "しょ"),
    ("cha", "ちゃ"),
    ("chu", "ちゅ"),
    ("che", "ちぇ"),
    ("cho", "ちょ"),
    ("cya", "ちゃ"),
    ("cyi", "ちぃ"),
    ("cyu", "ちゅ"),
    ("cye", "ちぇ"),
    ("cyo", "ちょ"),
    ("tya", "ちゃ"),
    ("tyi", "ちぃ"),
    ("tyu", "ちゅ"),
    ("tye", "ちぇ"),
    ("tyo", "ちょ"),
    ("nya", "にゃ"),
    ("nyi", "にぃ"),
    ("nyu", "にゅ"),
    ("nye", "にぇ"),
    ("nyo", "にょ"),
    ("hya", "ひゃ"),
    ("hyi", "ひぃ"),
    ("hyu", "ひゅ"),
    ("hye", "ひぇ"),
    ("hyo", "ひょ"),
    ("bya", "びゃ"),
    ("byi", "びぃ"),
    ("byu", "びゅ"),
    ("bye", "びぇ"),
    ("byo", "びょ"),
    ("pya", "ぴゃ"),
    ("pyi", "ぴぃ"),
    ("pyu", "ぴゅ"),
    ("pye", "ぴぇ"),
    ("pyo", "ぴょ"),
    ("mya", "みゃ"),
    ("myi", "みぃ"),
    ("myu", "みゅ"),
    ("mye", "みぇ"),
    ("myo", "みょ"),
    ("rya", "りゃ"),
    ("ryi", "りぃ"),
    ("ryu", "りゅ"),
    ("rye", "りぇ"),
    ("ryo", "りょ"),
    ("jya", "じゃ"),
    ("jyi", "じぃ"),
    ("jyu", "じゅ"),
    ("jye", "じぇ"),
    ("jyo", "じょ"),
    ("zya", "じゃ"),
    ("zyi", "じぃ"),
    ("zyu", "じゅ"),
    ("zye", "じぇ"),
    ("zyo", "じょ"),
    ("dya", "ぢゃ"),
    ("dyi", "ぢぃ"),
    ("dyu", "ぢゅ"),
    ("dye", "ぢぇ"),
    ("dyo", "ぢょ"),
    ("fya", "ふゃ"),
    ("fyu", "ふゅ"),
    ("fyo", "ふょ"),
    ("vya", "ゔゃ"),
    ("vyu", "ゔゅ"),
    ("vyo", "ゔょ"),
    ("lya", "ゃ"),
    ("lyu", "ゅ"),
    ("lyo", "ょ"),
    ("lwa", "ゎ"),
    ("xya", "ゃ"),
    ("xyu", "ゅ"),
    ("xyo", "ょ"),
    ("xwa", "ゎ"),
    ("wha", "うぁ"),
    ("whi", "うぃ"),
    ("whe", "うぇ"),
    ("who", "うぉ"),
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
    ("je", "じぇ"),
    ("jo", "じょ"),
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
    ("yi", "い"),
    ("yu", "ゆ"),
    ("ye", "いぇ"),
    ("yo", "よ"),
    ("ra", "ら"),
    ("ri", "り"),
    ("ru", "る"),
    ("re", "れ"),
    ("ro", "ろ"),
    ("wa", "わ"),
    ("wi", "うぃ"),
    ("wu", "う"),
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
        if result.ends_with(c) {
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

fn kana_match_cost(original: char, sung: char, previous: Option<char>) -> Option<i64> {
    if original == sung {
        return Some(0);
    }
    if large_kana(original) == sung {
        return Some(1);
    }
    if matches!(
        (original, sung),
        ('は', 'わ')
            | ('へ', 'え')
            | ('を', 'お')
            | ('ぢ', 'じ')
            | ('づ', 'ず')
            | ('ず', 'づ')
            | ('ゔ', 'ぶ')
    ) {
        return Some(2);
    }
    if original == 'ー' {
        if sung == 'ー' {
            return Some(0);
        }
        if let Some(v) = previous.and_then(vowel)
            && (v == sung || (v == 'お' && sung == 'う') || (v == 'え' && sung == 'い'))
        {
            return Some(1);
        }
    }
    None
}

/// Normalize the supplied reading while recording syllable breaks (spaces and
/// apostrophes) and phrase breaks (commas and other punctuation).
fn parse_roman(text: &str) -> Vec<(char, Break)> {
    let expanded = text
        .replace(['ā', 'â'], "aa")
        .replace(['ī', 'î'], "ii")
        .replace(['ū', 'û'], "uu")
        .replace(['ē', 'ê'], "ee")
        .replace(['ō', 'ô'], "ou");
    let mut out = Vec::new();
    let mut brk = Break::Phrase;
    for raw in expanded.chars() {
        let c = norm_ascii(hiragana(raw));
        if c.is_whitespace() || matches!(c, '\'' | '’') {
            brk = brk.max(Break::Syllable);
        } else if kept(c) {
            out.push((c, brk));
            brk = Break::None;
        } else {
            brk = Break::Phrase;
        }
    }
    out
}

/// Consume one kana syllable from `r[j..]`, returning `(chars, count, next_j)`.
fn kana_step(r: &[(char, Break)], j: usize) -> Option<([char; 2], usize, usize)> {
    let &(c0, _) = r.get(j)?;
    if kana(c0) {
        return Some(([c0, '\0'], 1, j + 1));
    }
    if !c0.is_ascii_lowercase() {
        return None;
    }
    let mut end = j + 1;
    while end < r.len() && r[end].1 == Break::None && r[end].0.is_ascii_lowercase() {
        end += 1;
    }
    let slice_len = end - j;
    let starts = |pat: &str| -> bool {
        slice_len >= pat.len()
            && r[j..j + pat.len()]
                .iter()
                .map(|x| x.0 as u8)
                .eq(pat.bytes())
    };
    if starts("tch") {
        return Some((['っ', '\0'], 1, j + 1));
    }
    if slice_len >= 2 && r[j].0 == r[j + 1].0 && b"bcdfghjkpqrstvwxyz".contains(&(c0 as u8)) {
        return Some((['っ', '\0'], 1, j + 1));
    }
    for &(roman, k) in SYLLABLES {
        if starts(roman) {
            let mut it = k.chars();
            let a = it.next().unwrap();
            let b = it.next().unwrap_or('\0');
            return Some(([a, b], if b == '\0' { 1 } else { 2 }, j + roman.len()));
        }
    }
    if c0 == 'n' {
        if slice_len >= 2 && r[j + 1].0 == 'n' {
            let before_glide_or_vowel =
                slice_len >= 3 && matches!(r[j + 2].0, 'a' | 'i' | 'u' | 'e' | 'o' | 'y');
            if !before_glide_or_vowel {
                return Some((['ん', '\0'], 1, j + 2));
            }
        }
        return Some((['ん', '\0'], 1, j + 1));
    }
    if c0 == 'm' && slice_len >= 2 && matches!(r[j + 1].0, 'b' | 'p') {
        return Some((['ん', '\0'], 1, j + 1));
    }
    None
}

fn kana_slice(r: &[(char, Break)], mut j: usize, end_j: usize) -> String {
    let mut out = String::new();
    while j < end_j {
        let Some((kc, klen, nj)) = kana_step(r, j) else {
            break;
        };
        out.push(kc[0]);
        if klen == 2 {
            out.push(kc[1]);
        }
        j = nj;
    }
    out
}

#[derive(Clone, Copy, Default)]
struct Prosody {
    morae: usize,
    syllables: usize,
    prev_vowel: Option<char>,
    last_kana: char,
}

impl Prosody {
    fn push(&mut self, c: char) {
        if small_kana(c) && self.morae > 0 {
            let mc = large_kana(c);
            self.prev_vowel = vowel(mc);
            self.last_kana = c;
            return;
        }
        self.morae += 1;
        self.last_kana = c;
        if matches!(c, 'ん' | 'っ' | 'ー') {
            if self.syllables == 0 {
                self.syllables = 1;
            }
            self.prev_vowel = None;
            return;
        }
        let v = vowel(c);
        let merges = match (self.prev_vowel, c) {
            (Some('お' | 'う'), 'う') | (Some('え' | 'あ'), 'い') => true,
            (Some(pv), _) if Some(pv) == v && matches!(c, 'あ' | 'い' | 'う' | 'え' | 'お') => {
                true
            }
            _ => false,
        };
        if !merges {
            self.syllables += 1;
        }
        self.prev_vowel = if merges { None } else { v };
    }
}

/// Balanced `(...)` and `（...）` groups that may hold unsung inline readings or
/// backing calls omitted from the supplied romanization.
fn paren_skips(chars: &[char], written: &[(usize, char)]) -> HashMap<usize, usize> {
    let mut skips = HashMap::new();
    if written.is_empty() {
        return skips;
    }
    let mut pos = 0;
    while pos < chars.len() {
        let close_ch = match chars[pos] {
            '(' => Some(')'),
            '（' => Some('）'),
            _ => None,
        };
        if let Some(close_ch) = close_ch {
            let mut depth = 1;
            let mut end = pos + 1;
            while end < chars.len() && depth > 0 {
                if chars[end] == chars[pos] {
                    depth += 1;
                } else if chars[end] == close_ch {
                    depth -= 1;
                }
                end += 1;
            }
            if depth == 0 {
                let first_w = written.iter().position(|(p, _)| *p >= pos);
                let after_w = written
                    .iter()
                    .position(|(p, _)| *p >= end)
                    .unwrap_or(written.len());
                if let Some(fw) = first_w
                    && fw < after_w
                    && (fw > 0 || after_w < written.len())
                {
                    skips.insert(fw, after_w);
                }
                pos = end;
                continue;
            }
        }
        pos += 1;
    }
    skips
}

fn match_same_range(
    r: &[(char, Break)],
    steps: &[Option<([char; 2], usize, usize)>],
    mut a: usize,
    a_end: usize,
    mut b: usize,
) -> Option<usize> {
    let (mut buf_a, mut len_a, mut pos_a) = (['\0'; 2], 0usize, 0usize);
    let (mut buf_b, mut len_b, mut pos_b) = (['\0'; 2], 0usize, 0usize);
    let mut started_b = false;
    loop {
        if pos_a == len_a {
            if a == a_end {
                return (pos_b == len_b).then_some(b);
            }
            let (kc, klen, na) = steps.get(a).copied().flatten()?;
            buf_a = kc;
            len_a = klen;
            pos_a = 0;
            a = na;
        }
        if pos_b == len_b {
            if started_b && b < r.len() && r[b].1 == Break::Phrase {
                return None;
            }
            let (kc, klen, nb) = steps.get(b).copied().flatten()?;
            started_b = true;
            buf_b = kc;
            len_b = klen;
            pos_b = 0;
            b = nb;
        }
        if buf_a[pos_a] != buf_b[pos_b] {
            return None;
        }
        pos_a += 1;
        pos_b += 1;
    }
}

#[derive(Clone, Copy)]
enum Choice {
    None,
    Step {
        ni: usize,
        nj: usize,
    },
    Span {
        ni: usize,
        nj: usize,
    },
    RepeatSpan {
        count: usize,
        span_len: usize,
        first_end_j: usize,
    },
}

struct SpanSpec {
    end: usize,
    next_is_separated: bool,
    known_chars: Option<Vec<char>>,
    repeat_ends: Vec<usize>,
}

struct RawSpan {
    start: usize,
    end: usize,
    reading: String,
    anchored: bool,
}

fn dp_align(
    chars: &[char],
    written: &[(usize, char)],
    r: &[(char, Break)],
    skips: &HashMap<usize, usize>,
    known: &HashMap<String, String>,
    allow_digits: bool,
) -> Vec<RawSpan> {
    if r.is_empty() || written.is_empty() {
        return Vec::new();
    }
    let annotatable = |c: char| kanji(c) || (allow_digits && c.is_ascii_digit());
    let nw = written.len();
    let nr = r.len();
    let steps: Vec<Option<([char; 2], usize, usize)>> = (0..nr).map(|j| kana_step(r, j)).collect();
    const INF: i64 = i64::MAX / 4;
    let mut dp = vec![vec![INF; nr + 1]; nw + 1];
    let mut choice = vec![vec![Choice::None; nr + 1]; nw + 1];
    dp[nw][nr] = 0;

    for i in (0..nw).rev() {
        let (orig_pos, c) = written[i];
        let mut specs: Vec<SpanSpec> = Vec::new();
        if annotatable(c) {
            let mut full_end = i + 1;
            while full_end < nw
                && annotatable(written[full_end].1)
                && written[full_end].0 == written[full_end - 1].0 + 1
            {
                full_end += 1;
            }
            let mut kanji_end = i + 1;
            if kanji(c) {
                while kanji_end < full_end && kanji(written[kanji_end].1) {
                    kanji_end += 1;
                }
            } else {
                kanji_end = full_end;
            }
            let ends = if kanji_end < full_end {
                vec![full_end, kanji_end]
            } else {
                vec![full_end]
            };
            for end in ends {
                let k = end - i;
                let surface: String = chars[orig_pos..written[end - 1].0 + 1].iter().collect();
                let next_is_separated = end < nw && annotatable(written[end].1);
                let mut repeat_ends = Vec::new();
                if end == full_end {
                    let mut probe = end;
                    while probe < nw && annotatable(written[probe].1) {
                        let mut next_e = probe + 1;
                        while next_e < nw
                            && annotatable(written[next_e].1)
                            && written[next_e].0 == written[next_e - 1].0 + 1
                        {
                            next_e += 1;
                        }
                        if next_e - probe == k
                            && chars[written[probe].0..written[next_e - 1].0 + 1]
                                .iter()
                                .copied()
                                .eq(surface.chars())
                        {
                            repeat_ends.push(next_e);
                            probe = next_e;
                        } else {
                            break;
                        }
                    }
                }
                specs.push(SpanSpec {
                    end,
                    next_is_separated,
                    known_chars: known.get(&surface).map(|s| s.chars().collect()),
                    repeat_ends,
                });
            }
        }

        for j in (0..=nr).rev() {
            let mut best = INF;
            let mut best_c = Choice::None;

            if let Some(&ni) = skips.get(&i) {
                let sub = dp[ni][j];
                if sub < INF {
                    let cost = sub + ((ni - i) as i64) * 50_000;
                    if cost < best {
                        best = cost;
                        best_c = Choice::Step { ni, nj: j };
                    }
                }
            }

            if (c.is_ascii_digit() || c.is_ascii_lowercase()) && j < nr && r[j].0 == c {
                let sub = dp[i + 1][j + 1];
                if sub < INF && sub < best {
                    best = sub;
                    best_c = Choice::Step {
                        ni: i + 1,
                        nj: j + 1,
                    };
                }
            }

            if !specs.is_empty() {
                for spec in &specs {
                    let end = spec.end;
                    let k = end - i;
                    let mut prosody = Prosody::default();
                    let mut known_pos = 0usize;
                    let mut known_ok = true;
                    let mut cur = j;
                    let mut first_step = true;
                    while cur < nr
                        && let Some((kc, klen, nj)) = steps[cur]
                    {
                        if !first_step && r[cur].1 == Break::Phrase {
                            break;
                        }
                        if first_step && dependent_start(kc[0]) {
                            break;
                        }
                        first_step = false;
                        prosody.push(kc[0]);
                        if let Some(kc_exp) = &spec.known_chars {
                            if known_pos < kc_exp.len() && kc_exp[known_pos] == kc[0] {
                                known_pos += 1;
                            } else {
                                known_ok = false;
                            }
                        }
                        if klen == 2 {
                            prosody.push(kc[1]);
                            if let Some(kc_exp) = &spec.known_chars {
                                if known_ok
                                    && known_pos < kc_exp.len()
                                    && kc_exp[known_pos] == kc[1]
                                {
                                    known_pos += 1;
                                } else {
                                    known_ok = false;
                                }
                            }
                        }
                        cur = nj;

                        let m = prosody.morae;
                        let s = prosody.syllables.max(1);
                        let mut base_pen: i64 = 0;
                        if m < k {
                            base_pen += ((k - m) as i64) * 100_000;
                        }
                        let diff = (m as i64) * 10 - (k as i64) * 18;
                        base_pen += (diff * diff * 10) / (k as i64);
                        let sdiff = (s as i64) - (k as i64);
                        base_pen += sdiff.abs() * 45;
                        if s < k {
                            base_pen += ((k - s) as i64) * 140;
                        }
                        if let Some(kc_exp) = &spec.known_chars
                            && !(known_ok && known_pos == kc_exp.len())
                        {
                            base_pen += 2_500;
                        }

                        let mut pen = base_pen;
                        if spec.next_is_separated {
                            if prosody.last_kana == 'っ' {
                                pen += 5_000;
                            }
                            if cur < nr
                                && let Some((next_kc, _, _)) = steps[cur]
                            {
                                let last_v = vowel(prosody.last_kana);
                                if last_v == Some('お') && next_kc[0] == 'う' {
                                    pen += 650;
                                } else if (last_v == Some('う') && next_kc[0] == 'う')
                                    || (last_v == Some('え') && next_kc[0] == 'い')
                                {
                                    pen += 500;
                                }
                            }
                        }

                        let sub = dp[end][cur];
                        if sub < INF && sub + pen < best {
                            best = sub + pen;
                            best_c = Choice::Span { ni: end, nj: cur };
                        }

                        if !spec.repeat_ends.is_empty() {
                            let mut rep_cur = cur;
                            for (idx, &rep_end) in spec.repeat_ends.iter().enumerate() {
                                let Some(next_cur) = match_same_range(r, &steps, j, cur, rep_cur)
                                else {
                                    break;
                                };
                                rep_cur = next_cur;
                                let count = idx + 2;
                                let sub_rep = dp[rep_end][rep_cur];
                                if sub_rep < INF {
                                    let rep_total = sub_rep + base_pen * (count as i64)
                                        - ((count as i64 - 1) * 3_000);
                                    if rep_total < best {
                                        best = rep_total;
                                        best_c = Choice::RepeatSpan {
                                            count,
                                            span_len: k,
                                            first_end_j: cur,
                                        };
                                    }
                                }
                            }
                        }
                    }
                }
            } else if kana(c)
                && j < nr
                && let Some((kc, klen, nj)) = steps[j]
            {
                let prev = i.checked_sub(1).map(|p| written[p].1);
                if klen == 1 {
                    if let Some(mc) = kana_match_cost(c, kc[0], prev) {
                        let sub = dp[i + 1][nj];
                        if sub < INF && sub + mc < best {
                            best = sub + mc;
                            best_c = Choice::Step { ni: i + 1, nj };
                        }
                    }
                    if c == 'ゔ' && i + 1 < nw && written[i + 1].0 == orig_pos + 1 {
                        let pair_ok = matches!(
                            (written[i + 1].1, kc[0]),
                            ('ぁ', 'ば') | ('ぃ', 'び') | ('ぇ', 'べ') | ('ぉ', 'ぼ')
                        );
                        if pair_ok {
                            let sub = dp[i + 2][nj];
                            if sub < INF && sub + 2 < best {
                                best = sub + 2;
                                best_c = Choice::Step { ni: i + 2, nj };
                            }
                        }
                    }
                } else {
                    if i + 1 < nw
                        && let (Some(c1), Some(c2)) = (
                            kana_match_cost(c, kc[0], prev),
                            kana_match_cost(written[i + 1].1, kc[1], Some(c)),
                        )
                    {
                        let sub = dp[i + 2][nj];
                        if sub < INF && sub + c1 + c2 < best {
                            best = sub + c1 + c2;
                            best_c = Choice::Step { ni: i + 2, nj };
                        }
                    }
                    let alt_ok = matches!(
                        (c, kc[0], kc[1]),
                        ('ち', 'て', 'ぃ')
                            | ('ぢ', 'で', 'ぃ')
                            | ('つ', 'と', 'ぅ')
                            | ('づ', 'ど', 'ぅ')
                    );
                    if alt_ok {
                        let sub = dp[i + 1][nj];
                        if sub < INF && sub + 3 < best {
                            best = sub + 3;
                            best_c = Choice::Step { ni: i + 1, nj };
                        }
                    }
                }
            }

            dp[i][j] = best;
            choice[i][j] = best_c;
        }
    }

    let mut out = Vec::new();
    if dp[0][0] < INF {
        let (mut i, mut j) = (0, 0);
        while i < nw {
            match choice[i][j] {
                Choice::Step { ni, nj } => {
                    i = ni;
                    j = nj;
                }
                Choice::Span { ni, nj } => {
                    let start = written[i].0;
                    let end = written[ni - 1].0 + 1;
                    let reading = kana_slice(r, j, nj);
                    let left_anchored = i == 0 || !annotatable(written[i - 1].1);
                    let right_anchored = ni == nw || !annotatable(written[ni].1);
                    out.push(RawSpan {
                        start,
                        end,
                        reading,
                        anchored: left_anchored && right_anchored,
                    });
                    i = ni;
                    j = nj;
                }
                Choice::RepeatSpan {
                    count,
                    span_len,
                    first_end_j,
                } => {
                    let reading = kana_slice(r, j, first_end_j);
                    let mut cur_i = i;
                    let mut cur_j = j;
                    for rep in 0..count {
                        let ni = cur_i + span_len;
                        let nj = if rep == 0 {
                            first_end_j
                        } else {
                            match_same_range(r, &steps, j, first_end_j, cur_j).unwrap_or(cur_j)
                        };
                        out.push(RawSpan {
                            start: written[cur_i].0,
                            end: written[ni - 1].0 + 1,
                            reading: reading.clone(),
                            anchored: false,
                        });
                        cur_i = ni;
                        cur_j = nj;
                    }
                    i = cur_i;
                    j = cur_j;
                }
                Choice::None => {
                    out.clear();
                    break;
                }
            }
        }
    }
    out
}

fn annotate_raw(
    original: &str,
    roman: &str,
    known: &HashMap<String, String>,
) -> (Vec<char>, Vec<RawSpan>) {
    let chars: Vec<char> = original.chars().collect();
    let allow_digits = chars.iter().any(|&c| kanji(c) || kana(c));
    let written: Vec<(usize, char)> = chars
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, c)| kept(*c))
        .map(|(i, c)| (i, norm_ascii(hiragana(c))))
        .collect();
    let r = parse_roman(roman);
    let skips = paren_skips(&chars, &written);

    // A pure-kanji line can also be Chinese. A supplied Japanese reading is
    // enough evidence; without it only explicitly written katakana is read.
    let mut raw = dp_align(&chars, &written, &r, &skips, known, allow_digits);
    if raw.iter().any(|span| !span.anchored) {
        let mut local = known.clone();
        let mut added = false;
        for span in &raw {
            if span.anchored && chars[span.start..span.end].iter().any(|&c| kanji(c)) {
                let surf: String = chars[span.start..span.end].iter().collect();
                if let std::collections::hash_map::Entry::Vacant(entry) = local.entry(surf) {
                    entry.insert(span.reading.clone());
                    added = true;
                }
            }
        }
        if added {
            raw = dp_align(&chars, &written, &r, &skips, &local, allow_digits);
        }
    }
    (chars, raw)
}

fn finish_ruby(chars: &[char], raw: Vec<RawSpan>) -> Vec<Ruby> {
    let mut ruby: Vec<Ruby> = raw
        .into_iter()
        .map(|span| Ruby {
            start: span.start,
            end: span.end,
            reading: span.reading,
        })
        .collect();
    let mut i = 0;
    while i < chars.len() {
        if ('ァ'..='ヶ').contains(&chars[i]) {
            let start = i;
            i += 1;
            while i < chars.len() && (('ァ'..='ヶ').contains(&chars[i]) || chars[i] == 'ー') {
                i += 1;
            }
            let reading: String = chars[start..i].iter().copied().map(hiragana).collect();
            let is_dup_inline = start > 0
                && matches!(chars[start - 1], '(' | '（')
                && i < chars.len()
                && matches!(chars[i], ')' | '）')
                && ruby
                    .iter()
                    .any(|prev| prev.end == start - 1 && prev.reading == reading);
            if !is_dup_inline && !ruby.iter().any(|r| r.start < i && start < r.end) {
                ruby.push(Ruby {
                    start,
                    end: i,
                    reading,
                });
            }
        } else {
            i += 1;
        }
    }
    ruby.sort_by_key(|r| r.start);
    ruby
}

/// A complete-line alignment using syllable boundaries and prosodic kanji-span
/// partitioning so consecutive kanji words share readings proportionally.
pub fn annotate(original: &str, roman: &str) -> Vec<Ruby> {
    let (chars, raw) = annotate_raw(original, roman, &HashMap::new());
    finish_ruby(&chars, raw)
}

/// Align all lines of a song together so unambiguous anchored readings in one
/// line reinforce unanchored multi-word splits elsewhere in the same song.
pub fn annotate_lines<'a>(lines: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<Vec<Ruby>> {
    let inputs: Vec<(&'a str, &'a str)> = lines.into_iter().collect();
    let empty = HashMap::new();
    let mut first_pass = Vec::with_capacity(inputs.len());
    let mut counts: HashMap<String, HashMap<String, usize>> = HashMap::new();

    for &(orig, roma) in &inputs {
        let (chars, raw) = annotate_raw(orig, roma, &empty);
        for span in &raw {
            if span.anchored && chars[span.start..span.end].iter().any(|&c| kanji(c)) {
                let surf: String = chars[span.start..span.end].iter().collect();
                *counts
                    .entry(surf)
                    .or_default()
                    .entry(span.reading.clone())
                    .or_insert(0) += 1;
            }
        }
        first_pass.push((chars, raw));
    }

    let mut known = HashMap::new();
    for (surf, map) in counts {
        let total: usize = map.values().sum();
        if let Some((best_rd, &best_cnt)) = map.iter().max_by_key(|(_, c)| *c)
            && best_cnt * 2 > total
        {
            known.insert(surf, best_rd.clone());
        }
    }

    if known.is_empty() {
        return first_pass
            .into_iter()
            .map(|(chars, raw)| finish_ruby(&chars, raw))
            .collect();
    }

    inputs
        .into_iter()
        .zip(first_pass)
        .map(|((orig, roma), (chars, raw))| {
            if !roma.is_empty() && raw.iter().any(|span| !span.anchored) {
                let (_, refined) = annotate_raw(orig, roma, &known);
                finish_ruby(&chars, refined)
            } else {
                finish_ruby(&chars, raw)
            }
        })
        .collect()
}

/// Carry readings between lyric versions only when the original text agrees.
pub fn remap(from: &str, to: &str, ruby: &[Ruby]) -> Option<Vec<Ruby>> {
    let norm = |c: char| match c {
        '（' => '(',
        '）' => ')',
        '！' => '!',
        '？' => '?',
        _ => c,
    };
    let compact = |s: &str| {
        s.chars()
            .enumerate()
            .filter(|(_, c)| !c.is_whitespace())
            .map(|(i, c)| (i, norm(c)))
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
    fn partitions_consecutive_kanji_phrases_without_tearing_syllables() {
        assert_eq!(
            spans(
                "本能に従順忠実 翻弄も重々承知",
                "ho n no u ni ju u ju n chu u ji tsu ho n ro u mo ju u ju u sho u chi"
            ),
            [
                ("本能".into(), "ほんのう".into()),
                ("従順忠実".into(), "じゅうじゅんちゅうじつ".into()),
                ("翻弄".into(), "ほんろう".into()),
                ("重々承知".into(), "じゅうじゅうしょうち".into()),
            ]
        );
        assert_eq!(
            spans(
                "五臓六腑 満身邁進 願望は痩身麗人",
                "go zo u ro ppu ma n shi n ma i shi n ga n bo u wa so u shi n re i ji n"
            ),
            [
                ("五臓六腑".into(), "ごぞうろっぷ".into()),
                ("満身邁進".into(), "まんしんまいしん".into()),
                ("願望".into(), "がんぼう".into()),
                ("痩身麗人".into(), "そうしんれいじん".into()),
            ]
        );
        assert_eq!(
            spans(
                "すかざずに目標 下方修正して",
                "su ka za zu ni mo ku hyo u ka ho u shu u se i shi te"
            ),
            [
                ("目標".into(), "もくひょう".into()),
                ("下方修正".into(), "かほうしゅうせい".into()),
            ]
        );
        assert_eq!(
            spans(
                "柔軟に臨機応変 七変化が勝ち",
                "ju u na n ni ri n ki o u he n shi chi he n ge ga ka chi"
            ),
            [
                ("柔軟".into(), "じゅうなん".into()),
                ("臨機応変".into(), "りんきおうへん".into()),
                ("七変化".into(), "しちへんげ".into()),
                ("勝".into(), "か".into()),
            ]
        );
        assert_eq!(
            spans(
                "僕は雨 君は太陽 手を繋ごう",
                "bo ku wa a me ki mi wa ta i yo u te wo tsu na go u"
            ),
            [
                ("僕".into(), "ぼく".into()),
                ("雨".into(), "あめ".into()),
                ("君".into(), "きみ".into()),
                ("太陽".into(), "たいよう".into()),
                ("手".into(), "て".into()),
                ("繋".into(), "つな".into()),
            ]
        );
    }

    #[test]
    fn avoids_accidental_early_particle_anchors_inside_kanji_readings() {
        assert_eq!(
            spans("素直な声響いて", "su na o na ko e hi bi i te"),
            [
                ("素直".into(), "すなお".into()),
                ("声響".into(), "こえひび".into()),
            ]
        );
        assert_eq!(
            spans("その笑顔が見たい!", "so no e ga o ga mi ta i!"),
            [("笑顔".into(), "えがお".into()), ("見".into(), "み".into()),]
        );
        assert_eq!(
            spans(
                "いざ恋人同士と言われても",
                "i za ko i bi to do u shi to i wa re te mo"
            ),
            [
                ("恋人同士".into(), "こいびとどうし".into()),
                ("言".into(), "い".into()),
            ]
        );
        assert_eq!(
            spans(
                "懐かしい青空を見上げてた",
                "na tsu ka shi i a o zo ra wo mi a ge te ta"
            ),
            [
                ("懐".into(), "なつ".into()),
                ("青空".into(), "あおぞら".into()),
                ("見上".into(), "みあ".into()),
            ]
        );
    }

    #[test]
    fn keeps_repeated_words_and_song_vocabulary_consistent() {
        assert_eq!(
            spans(
                "誰一人 誰一人 幸せにしない",
                "da re hi to ri da re hi to ri shi a wa se ni shi na i"
            ),
            [
                ("誰一人".into(), "だれひとり".into()),
                ("誰一人".into(), "だれひとり".into()),
                ("幸".into(), "しあわ".into()),
            ]
        );
        assert_eq!(
            spans(
                "震える魂 熱く熱くなれ",
                "fu ru e ru ta ma shi i a tsu ku a tsu ku na re"
            ),
            [
                ("震".into(), "ふる".into()),
                ("魂".into(), "たましい".into()),
                ("熱".into(), "あつ".into()),
                ("熱".into(), "あつ".into()),
            ]
        );
        let song = annotate_lines([
            (
                "雲を抜け風下へ私だけ",
                "ku mo wo nu ke ka za shi mo e wa ta shi da ke",
            ),
            ("行く風下 私だけ", "i ku ka za shi mo wa ta shi da ke"),
        ]);
        assert_eq!(
            song[1],
            [
                Ruby {
                    start: 0,
                    end: 1,
                    reading: "い".into()
                },
                Ruby {
                    start: 2,
                    end: 4,
                    reading: "かざしも".into()
                },
                Ruby {
                    start: 5,
                    end: 6,
                    reading: "わたし".into()
                },
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
    fn accepts_gemination_nasal_english_numbers_and_parentheticals() {
        assert_eq!(
            spans(
                "だから不意にピッチ外れるんです",
                "da ka ra fu i ni pi tchi ha zu re ru n de su"
            ),
            [
                ("不意".into(), "ふい".into()),
                ("ピッチ".into(), "ぴっち".into()),
                ("外".into(), "はず".into()),
            ]
        );
        assert_eq!(
            spans("パーティーへ行こう", "pa a thi i e i ko u").last(),
            Some(&("行".into(), "い".into()))
        );
        assert_eq!(
            spans("君とLOVE", "ki mi to LOVE"),
            [("君".into(), "きみ".into())]
        );
        assert_eq!(
            spans(
                "なにもかも愛しい　愛しいI love you",
                "na ni mo ka mo i to shi i i to shi i I love you"
            ),
            [("愛".into(), "いと".into()), ("愛".into(), "いと".into()),]
        );
        assert_eq!(
            spans("wowド派手に踊れ！", "wowdo ha de ni o do re!"),
            [
                ("ド".into(), "ど".into()),
                ("派手".into(), "はで".into()),
                ("踊".into(), "おど".into()),
            ]
        );
        assert_eq!(
            spans("ただ１人だけ 君だけ", "ta da hi to ri da ke ki mi da ke"),
            [
                ("１人".into(), "ひとり".into()),
                ("君".into(), "きみ".into()),
            ]
        );
        assert_eq!(
            spans(
                "ヤバ まさか赤点(Red Point)!? いや ギリクリア",
                "ya ba ma sa ka a ka te n !? i ya gi ri ku ri a"
            ),
            [
                ("ヤバ".into(), "やば".into()),
                ("赤点".into(), "あかてん".into()),
                ("ギリクリア".into(), "ぎりくりあ".into()),
            ]
        );
        assert_eq!(
            spans(
                "最高傑作が　奏でる即興(インプロ)",
                "sa i ko u ke ssa ku ga ka na de ru i n pu ro"
            ),
            [
                ("最高傑作".into(), "さいこうけっさく".into()),
                ("奏".into(), "かな".into()),
                ("即興".into(), "いんぷろ".into()),
            ]
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
