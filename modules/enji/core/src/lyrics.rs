//! Lyrics as NetEase sends them: timed lines in LRC with the translation as
//! a second LRC at the same times, and for some songs each word timed too.

use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub time: f64,
    pub text: String,
    pub translation: Option<String>,
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
    /// How far through its characters the singing is at `at`, the word being
    /// sung counted by how much of it has gone: 2.5 is half way through the
    /// third. None for a line that does not know its words.
    pub fn sung(&self, at: f64) -> Option<f64> {
        let (first, _) = self.words.split_first()?;
        if at < first.time {
            return Some(0.0);
        }
        let now = self.words.iter().rposition(|word| word.time <= at)?;
        let word = &self.words[now];
        let before = now.checked_sub(1).map_or(0, |last| self.words[last].end);
        let part = if word.length > 0.0 { ((at - word.time) / word.length).clamp(0.0, 1.0) } else { 1.0 };
        Some(before as f64 + (word.end - before) as f64 * part)
    }
}

/// The lyrics in NetEase's answer `body`: timed by the word if `by_word`
/// and it has them, else by the line.
pub fn of_answer(body: &Value, by_word: bool) -> Vec<Line> {
    let text = |key: &str| body[key]["lyric"].as_str().unwrap_or("");
    let lines = if by_word { word_timed(text("yrc")) } else { Vec::new() };
    if lines.is_empty() {
        return parse(text("lrc"), text("tlyric"));
    }
    let translation = if text("ytlrc").is_empty() { text("tlyric") } else { text("ytlrc") };
    translated(lines, &timed(translation))
}

/// How far a line of a translation may be from the line it belongs to:
/// the two are timed apart, and not to the same moment.
const NEAR: f64 = 1.0;

/// `lines`, each with the translation nearest to it, if any is near.
fn translated(mut lines: Vec<Line>, translation: &[(f64, String)]) -> Vec<Line> {
    for line in &mut lines {
        let gap = |at: f64| (at - line.time).abs();
        let nearest = translation.iter().min_by(|a, b| gap(a.0).total_cmp(&gap(b.0)));
        line.translation = nearest.filter(|(at, _)| gap(*at) <= NEAR).map(|(_, text)| text.clone());
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
        let Some((head, body)) = raw.trim().strip_prefix('[').and_then(|rest| rest.split_once(']')) else { continue };
        let Some(start) = head.split(',').next().and_then(|start| start.parse::<f64>().ok()) else { continue };
        let (mut text, mut words, mut chars) = (String::new(), Vec::new(), 0);
        for (at, length, word) in timed_words(body) {
            chars += word.chars().count();
            text.push_str(word);
            // Some songs count from the line's start.
            let at = if at < start { start + at } else { at };
            words.push(Word { time: at / 1000.0, length: length / 1000.0, end: chars });
        }
        let kept = text.trim_end().chars().count();
        words.iter_mut().for_each(|word| word.end = word.end.min(kept));
        if kept > 0 {
            lines.push(Line { time: start / 1000.0, text: text.trim_end().to_string(), translation: None, words });
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
        let Some(close) = body[open..].find(')').map(|close| open + close) else { break };
        let parts: Vec<&str> = body[open + 1..close].split(',').collect();
        if parts.len() == 3 && parts.iter().all(|part| !part.is_empty() && part.bytes().all(|digit| digit.is_ascii_digit())) {
            let number = |part: &str| part.parse::<f64>().unwrap_or(0.0);
            tags.push((open, close, number(parts[0]), number(parts[1])));
        }
    }
    let ends = tags.iter().skip(1).map(|tag| tag.0).chain(std::iter::once(body.len()));
    tags.iter().zip(ends).map(|(&(_, close, at, length), end)| (at, length, &body[close + 1..end])).collect()
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
            if let Some(last) = lines.last_mut().filter(|last| (last.time - time).abs() < 0.05 && last.translation.is_none()) {
                last.translation = Some(text);
                continue;
            }
        }
        lines.push(Line {
            time,
            translation: translated.iter().find(|(at, _)| (at - time).abs() < 0.05).map(|(_, text)| text.clone()),
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
        assert_eq!((lines[0].text.as_str(), lines[0].translation.as_deref()), ("你好", Some("Hello")));
        assert_eq!((lines[1].text.as_str(), lines[1].translation.as_deref()), ("再见", None));
        // A translation of their own wins: both lines stay lines.
        let lines = parse("[00:01.00]你好\n[00:01.00]Hello\n", "[00:01.00]哈喽\n");
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn lines_take_their_translations_and_tags_are_left_out() {
        let lrc = "[ar:someone]\n[00:01.50]海猫が鳴く頃に\n[00:04.00][00:20.00]もう一度\n[00:06.00]\n";
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
    fn a_line_is_lit_as_far_as_the_singing_has_gone() {
        let lines = super::of_answer(&answer(), true);
        let line = &lines[1];
        assert_eq!(line.sung(4.9), Some(0.0));
        assert_eq!(line.sung(5.0), Some(0.0));
        assert_eq!(line.sung(5.25), Some(0.5));
        assert_eq!(line.sung(5.5), Some(1.0));
        assert_eq!(line.sung(5.75), Some(1.5));
        assert_eq!(line.sung(9.0), Some(2.0));
    }

    #[test]
    fn a_word_of_many_characters_lights_through_them_evenly() {
        let lines = super::of_answer(&answer(), true);
        // "(Rain " is six characters over half a second.
        assert_eq!(lines[0].sung(1.25), Some(3.0));
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
        assert_eq!(lines[0].sung(2.0), None);
        let mut none = answer();
        none["yrc"] = serde_json::json!({});
        assert_eq!(super::of_answer(&none, true)[0].text, "plain line");
    }
}
