//! LRC lyrics as NetEase sends them: timed lines, and the translation as a
//! second LRC with the same times.

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub time: f64,
    pub text: String,
    pub translation: Option<String>,
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
}
