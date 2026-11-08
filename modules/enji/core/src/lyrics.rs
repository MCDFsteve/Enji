//! Lyrics as NetEase sends them: timed lines in LRC with the translation as
//! a second LRC at the same times, and for some songs each word timed too.

use serde_json::Value;

mod furigana;
pub use furigana::Ruby;

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub time: f64,
    pub text: String,
    pub translation: Option<String>,
    /// Readings above ranges of the unchanged original text.
    pub ruby: Vec<Ruby>,
    /// When each word starts, if NetEase timed them.
    pub words: Vec<Word>,
}

/// A word of a line.
#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub time: f64,
    /// How long it is sung, in seconds.
    pub length: f64,
    /// How many characters of the line are sung once it is: those of the
    /// words before it and its own.
    pub end: usize,
}

impl Line {
    pub fn ruby(&self) -> String {
        furigana::encoded(&self.ruby)
    }

    /// How the words light up: for each, the second it starts and how many
    /// characters stand lit then, and the same as it ends.
    pub fn path(&self) -> String {
        let mut before = 0;
        let mut points = Vec::with_capacity(self.words.len());
        for word in &self.words {
            points.push(format!(
                "{:.3} {before} {:.3} {}",
                word.time,
                word.time + word.length,
                word.end
            ));
            before = word.end;
        }
        points.join(" ")
    }
}

/// The lyrics in NetEase's answer `body`: timed by the word if `by_word`
/// and it has them, else by the line.
pub fn of_answer(body: &Value, by_word: bool) -> Vec<Line> {
    let text = |key: &str| body[key]["lyric"].as_str().unwrap_or("");
    let mut plain = parse(text("lrc"), text("tlyric"));
    let roman = timed(text("romalrc"));
    for line in &mut plain {
        if let Some(roman) = nearest(&roman, line.time) {
            line.ruby = furigana::annotate(&line.text, roman);
        }
    }
    let lines = if by_word {
        word_timed(text("yrc"))
    } else {
        Vec::new()
    };
    if lines.is_empty() {
        return plain;
    }
    let translation = if text("ytlrc").is_empty() {
        text("tlyric")
    } else {
        text("ytlrc")
    };
    let mut lines = translated(lines, &timed(translation));
    let roman = timed(text("yromalrc"));
    for line in &mut lines {
        if let Some(roman) = nearest(&roman, line.time) {
            line.ruby = furigana::annotate(&line.text, roman);
        } else if let Some(mapped) = plain
            .iter()
            .filter(|other| (other.time - line.time).abs() <= NEAR)
            .filter_map(|other| {
                furigana::remap(&other.text, &line.text, &other.ruby)
                    .map(|ruby| ((other.time - line.time).abs(), ruby))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
        {
            line.ruby = mapped.1;
        }
    }
    lines
}

fn nearest(lines: &[(f64, String)], time: f64) -> Option<&str> {
    lines
        .iter()
        .min_by(|a, b| (a.0 - time).abs().total_cmp(&(b.0 - time).abs()))
        .filter(|(at, _)| (at - time).abs() <= NEAR)
        .map(|(_, text)| text.as_str())
}

/// How far a line of a translation may be from the line it belongs to:
/// the two are timed apart, and not to the same moment.
const NEAR: f64 = 1.0;

/// `lines`, each with the translation nearest to it, if any is near.
fn translated(mut lines: Vec<Line>, translation: &[(f64, String)]) -> Vec<Line> {
    for line in &mut lines {
        let gap = |at: f64| (at - line.time).abs();
        let nearest = translation
            .iter()
            .min_by(|a, b| gap(a.0).total_cmp(&gap(b.0)));
        line.translation = nearest
            .filter(|(at, _)| gap(*at) <= NEAR)
            .map(|(_, text)| text.clone());
    }
    lines
}

/// The sung lines of NetEase's word-timed format, each like
/// `[16210,3460](16210,670,0)Rain(16880,410,0) falls`: the line's start and
/// length, then each word's start, length and a zero before its text, all
/// in milliseconds. Credits are lines of JSON, and left out.
fn word_timed(yrc: &str) -> Vec<Line> {
    let mut lines = Vec::new();
    for raw in yrc.lines() {
        let Some((head, body)) = raw
            .trim()
            .strip_prefix('[')
            .and_then(|rest| rest.split_once(']'))
        else {
            continue;
        };
        let Some(start) = head
            .split(',')
            .next()
            .and_then(|start| start.parse::<f64>().ok())
        else {
            continue;
        };
        let (mut text, mut words, mut chars) = (String::new(), Vec::new(), 0);
        for (at, length, word) in timed_words(body) {
            chars += word.chars().count();
            text.push_str(word);
            // Some songs count from the line's start.
            let at = if at < start { start + at } else { at };
            words.push(Word {
                time: at / 1000.0,
                length: length / 1000.0,
                end: chars,
            });
        }
        let kept = text.trim_end().chars().count();
        words
            .iter_mut()
            .for_each(|word| word.end = word.end.min(kept));
        if kept > 0 {
            let text = text.trim_end().to_string();
            let ruby = furigana::annotate(&text, "");
            lines.push(Line {
                time: start / 1000.0,
                text,
                translation: None,
                words,
                ruby,
            });
        }
    }
    lines.sort_by(|a, b| a.time.total_cmp(&b.time));
    lines
}

/// Each word of a line's `body` with when it starts and how long it is. A
/// word may hold brackets of its own, so a tag is only what reads as one.
fn timed_words(body: &str) -> Vec<(f64, f64, &str)> {
    let mut tags = Vec::new();
    for (open, _) in body.match_indices('(') {
        let Some(close) = body[open..].find(')').map(|close| open + close) else {
            break;
        };
        let parts: Vec<&str> = body[open + 1..close].split(',').collect();
        if parts.len() == 3
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.bytes().all(|digit| digit.is_ascii_digit()))
        {
            let number = |part: &str| part.parse::<f64>().unwrap_or(0.0);
            tags.push((open, close, number(parts[0]), number(parts[1])));
        }
    }
    let ends = tags
        .iter()
        .skip(1)
        .map(|tag| tag.0)
        .chain(std::iter::once(body.len()));
    tags.iter()
        .zip(ends)
        .map(|(&(_, close, at, length), end)| (at, length, &body[close + 1..end]))
        .collect()
}

/// The sung lines of `lrc` in order, each with the line of `translation`
/// at the same time. Credits, tags and empty lines are left out.
pub fn parse(lrc: &str, translation: &str) -> Vec<Line> {
    let translated = timed(translation);
    let mut lines: Vec<Line> = Vec::new();
    for (time, text) in timed(lrc) {
        // Without a translation of their own, a second line at the same
        // moment is the first one's translation, as bilingual LRC files
        // write it.
        if translated.is_empty() {
            if let Some(last) = lines
                .last_mut()
                .filter(|last| (last.time - time).abs() < 0.05 && last.translation.is_none())
            {
                last.translation = Some(text);
                continue;
            }
        }
        lines.push(Line {
            time,
            translation: translated
                .iter()
                .find(|(at, _)| (at - time).abs() < 0.05)
                .map(|(_, text)| text.clone()),
            ruby: furigana::annotate(&text, ""),
            text,
            words: Vec::new(),
        });
    }
    lines
}

fn timed(lrc: &str) -> Vec<(f64, String)> {
    let mut lines = Vec::new();
    for raw in lrc.lines() {
        let mut rest = raw.trim();
        let mut times = Vec::new();
        while let Some(tag) = rest.strip_prefix('[') {
            let Some(end) = tag.find(']') else { break };
            match time(&tag[..end]) {
                Some(at) => times.push(at),
                // `[ar:…]` and other tags: not a sung line.
                None => {
                    times.clear();
                    break;
                }
            }
            rest = tag[end + 1..].trim();
        }
        if !times.is_empty() && !rest.is_empty() {
            lines.extend(times.into_iter().map(|at| (at, rest.to_string())));
        }
    }
    lines.sort_by(|a, b| a.0.total_cmp(&b.0));
    lines
}

/// `mm:ss.xx`, `mm:ss:xx` or `mm:ss`, in seconds.
fn time(tag: &str) -> Option<f64> {
    let (minutes, seconds) = tag.split_once(':')?;
    let minutes: f64 = minutes.trim().parse().ok()?;
    let seconds: f64 = match seconds.split_once(':') {
        Some((whole, part)) => format!("{whole}.{part}").parse().ok()?,
        None => seconds.trim().parse().ok()?,
    };
    Some(minutes * 60.0 + seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_line_at_the_same_moment_is_its_translation() {
        let lines = parse("[00:01.00]你好\n[00:01.00]Hello\n[00:03.00]再见\n", "");
        assert_eq!(lines.len(), 2);
        assert_eq!(
            (lines[0].text.as_str(), lines[0].translation.as_deref()),
            ("你好", Some("Hello"))
        );
        assert_eq!(
            (lines[1].text.as_str(), lines[1].translation.as_deref()),
            ("再见", None)
        );
        // A translation of their own wins: both lines stay lines.
        let lines = parse("[00:01.00]你好\n[00:01.00]Hello\n", "[00:01.00]哈喽\n");
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn lines_take_their_translations_and_tags_are_left_out() {
        let lrc =
            "[ar:someone]\n[00:01.50]海猫が鳴く頃に\n[00:04.00][00:20.00]もう一度\n[00:06.00]\n";
        let tlyric = "[00:01.50]在海猫鸣叫之时\n[00:04.00]再一次";
        let lines = parse(lrc, tlyric);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].text, "海猫が鳴く頃に");
        assert_eq!(lines[0].translation.as_deref(), Some("在海猫鸣叫之时"));
        assert_eq!(lines[1].time, 4.0);
        assert_eq!(lines[2].time, 20.0);
        assert_eq!(lines[2].translation, None);
    }

    fn answer() -> serde_json::Value {
        serde_json::json!({
            "lrc": { "lyric": "[00:01.000]plain line\n" },
            "tlyric": { "lyric": "[00:01.200]第一行\n[00:30.000]远处的翻译\n" },
            "yrc": { "lyric": concat!(
                "{\"t\":0,\"c\":[{\"tx\":\"作词: \"}]}\n",
                "[1000,2000](1000,500,0)(Rain (1500,500,0)falls) (2000,1000,0)down \n",
                "[5000,1000](5000,500,0)你(5500,500,0)好\n",
            ) },
        })
    }

    #[test]
    fn words_are_timed_and_brackets_of_their_own_stay() {
        let lines = super::of_answer(&answer(), true);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "(Rain falls) down");
        assert_eq!(lines[0].time, 1.0);
        let ends: Vec<usize> = lines[0].words.iter().map(|word| word.end).collect();
        assert_eq!(ends, [6, 13, 17]);
    }

    #[test]
    fn a_lines_path_goes_through_each_word() {
        let lines = super::of_answer(&answer(), true);
        assert_eq!(lines[1].path(), "5.000 0 5.500 1 5.500 1 6.000 2");
        // "(Rain " is six characters over half a second.
        assert!(lines[0].path().starts_with("1.000 0 1.500 6 "));
    }

    #[test]
    fn the_nearest_translation_goes_with_a_line_and_no_far_one() {
        let lines = super::of_answer(&answer(), true);
        assert_eq!(lines[0].translation.as_deref(), Some("第一行"));
        assert_eq!(lines[1].translation, None);
    }

    #[test]
    fn without_word_times_the_lines_are_as_before() {
        let lines = super::of_answer(&answer(), false);
        assert_eq!(lines[0].text, "plain line");
        assert!(lines[0].words.is_empty());
        assert_eq!(lines[0].path(), "");
        let mut none = answer();
        none["yrc"] = serde_json::json!({});
        assert_eq!(super::of_answer(&none, true)[0].text, "plain line");
    }

    #[test]
    fn roman_readings_follow_the_selected_lyric_version() {
        let body = serde_json::json!({
            "lrc": { "lyric": "[00:00.696]開け放たれた この部屋には誰もいない" },
            "romalrc": { "lyric": "[00:00.696]a ke ha na ta re ta ko no he ya ni wa da re mo i na i" },
            "yrc": { "lyric": "[750,9500](750,440,0)開(1190,320,0)け放たれた この部屋には誰もいない" },
            "yromalrc": { "lyric": "[00:00.750]a ke ha na ta re ta ko no he ya ni wa da re mo i na i" }
        });
        let word = of_answer(&body, true);
        let plain = of_answer(&body, false);
        assert_eq!(word[0].ruby, plain[0].ruby);
        assert_eq!(word[0].ruby[0].reading, "あ");
        assert_eq!(word[0].words[0].end, 1);
        assert_eq!(word[0].time, 0.75);
        assert!(word[0].ruby().starts_with("[[0,1,"));
        let mut fallback = body;
        fallback["yromalrc"] = serde_json::json!({});
        assert_eq!(of_answer(&fallback, true)[0].ruby, plain[0].ruby);
        fallback["yrc"]["lyric"] = serde_json::json!("[750,1000](750,1000,0)違う歌詞");
        assert!(of_answer(&fallback, true)[0].ruby.is_empty());
    }
}
