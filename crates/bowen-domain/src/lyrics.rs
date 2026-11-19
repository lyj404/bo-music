//! Lyric parsing and playback timeline, independent of the API and UI.
//! All positions and durations are in milliseconds.

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LyricKind {
    #[default]
    None,
    LineByLine,
    WordByWord,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LyricSyllable {
    pub text: String,
    pub start_ms: u64,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LyricLine {
    pub start_ms: u64,
    pub duration_ms: u64,
    pub text: String,
    pub syllables: Vec<LyricSyllable>,
    pub is_translation: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LyricDocument {
    pub lines: Vec<LyricLine>,
    pub kind: LyricKind,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LyricCursor {
    pub line: Option<usize>,
    pub syllable: Option<usize>,
    pub syllable_progress: f64,
    pub line_progress: f64,
}

impl LyricDocument {
    pub fn without_blank_lines(&self) -> Self {
        Self {
            lines: self
                .lines
                .iter()
                .filter(|line| !line.text.trim().is_empty())
                .cloned()
                .collect(),
            kind: self.kind,
        }
    }

    pub fn without_translations(&self) -> Self {
        Self {
            lines: self
                .lines
                .iter()
                .filter(|line| !line.is_translation)
                .cloned()
                .collect(),
            kind: self.kind,
        }
    }

    pub fn has_translation(&self) -> bool {
        self.lines.iter().any(|line| line.is_translation)
    }

    pub fn line_at(&self, position_ms: u64) -> Option<usize> {
        self.lines
            .partition_point(|line| line.start_ms <= position_ms)
            .checked_sub(1)
    }

    pub fn locate(&self, position_ms: u64) -> LyricCursor {
        let Some(index) = self.line_at(position_ms) else {
            return LyricCursor::default();
        };
        let line = &self.lines[index];
        let syllable = if line.syllables.is_empty() {
            None
        } else {
            Some(
                line.syllables
                    .partition_point(|s| s.start_ms <= position_ms)
                    .saturating_sub(1),
            )
        };
        LyricCursor {
            line: Some(index),
            syllable,
            syllable_progress: syllable
                .map(|i| {
                    progress(
                        position_ms,
                        line.syllables[i].start_ms,
                        line.syllables[i].duration_ms,
                    )
                })
                .unwrap_or(0.0),
            line_progress: progress(position_ms, line.start_ms, line.duration_ms),
        }
    }

    /// Original and adjacent translation lines share one scroll unit.
    pub fn scroll_units(&self) -> Vec<usize> {
        let mut result = vec![0; self.lines.len()];
        let mut leading = Vec::new();
        let mut unit: Option<usize> = None;
        for (index, line) in self.lines.iter().enumerate() {
            if !line.is_translation {
                let next = unit.map_or(0, |value| value + 1);
                unit = Some(next);
                result[index] = next;
                for pending in leading.drain(..) {
                    result[pending] = next;
                }
            } else if let Some(unit) = unit {
                result[index] = unit;
            } else {
                leading.push(index);
            }
        }
        for (unit, pending) in leading.into_iter().enumerate() {
            result[pending] = unit;
        }
        result
    }

    pub fn source_line(&self, index: usize) -> Option<usize> {
        self.lines.get(index)?;
        Some(
            (0..=index)
                .rev()
                .find(|i| !self.lines[*i].is_translation)
                .unwrap_or(index),
        )
    }
}

impl LyricLine {
    pub fn speech_end_ms(&self) -> u64 {
        let end = self.start_ms.saturating_add(self.duration_ms);
        self.syllables
            .last()
            .map(|s| {
                s.start_ms
                    .saturating_add(s.duration_ms)
                    .clamp(self.start_ms, end)
            })
            .unwrap_or(end)
    }
}

fn progress(position: u64, start: u64, duration: u64) -> f64 {
    if position < start {
        return 0.0;
    }
    if duration == 0 {
        return 1.0;
    }
    ((position - start) as f64 / duration as f64).min(1.0)
}

#[derive(Clone)]
struct RawSyllable {
    text: String,
    offset_ms: u64,
    duration_ms: u64,
}

#[derive(Clone)]
struct RawLine {
    start_ms: u64,
    text: String,
    syllables: Vec<RawSyllable>,
    translation: bool,
}

impl RawLine {
    fn timed(&self) -> bool {
        self.syllables.iter().any(|s| s.duration_ms > 0)
    }
}

/// Parse Kuwo's octal factor tag. An invalid or zero factor means line lyrics.
fn factors(text: &str) -> Option<(usize, usize, u64, u64)> {
    let start = text.find("[kuwo:")?;
    let end = start + text[start..].find(']')? + 1;
    let tag = u32::from_str_radix(text[start + 6..end - 1].trim(), 8).ok()?;
    let first = u64::from(tag / 10);
    let second = u64::from(tag % 10);
    (first > 0 && second > 0).then_some((start, end, first, second))
}

fn timestamp(input: &str) -> Option<(u64, usize)> {
    let close = input.find(']')?;
    let tag = input.get(1..close)?;
    if !input.starts_with('[') {
        return None;
    }
    let (minutes, rest) = tag.split_once(':')?;
    if !(1..=3).contains(&minutes.len()) || !minutes.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let split = rest.find(['.', ':']);
    let (seconds, fraction) = match split {
        Some(i) => (&rest[..i], Some(&rest[i + 1..])),
        None => (rest, None),
    };
    if seconds.len() != 2 || !seconds.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let fraction_ms = match fraction {
        None => 0,
        Some(f) if (1..=3).contains(&f.len()) && f.bytes().all(|b| b.is_ascii_digit()) => {
            f.parse::<u64>().ok()? * 10_u64.pow(3 - f.len() as u32)
        }
        _ => return None,
    };
    let ms =
        minutes.parse::<u64>().ok()? * 60_000 + seconds.parse::<u64>().ok()? * 1_000 + fraction_ms;
    Some((ms, close + 1))
}

fn parse_word_tags(
    content: &str,
    start_factor: u64,
    duration_factor: u64,
) -> (String, Vec<RawSyllable>) {
    let mut tags = Vec::new();
    let mut offset = 0;
    while let Some(relative) = content[offset..].find('<') {
        let start = offset + relative;
        let Some(end_relative) = content[start..].find('>') else {
            break;
        };
        let end = start + end_relative + 1;
        let values = &content[start + 1..end - 1];
        let parsed = values.split_once(',').and_then(|(a, b)| {
            if a.starts_with('+') || b.starts_with('+') {
                return None;
            }
            Some((a.parse::<i64>().ok()?, b.parse::<i64>().ok()?))
        });
        if let Some((a, b)) = parsed {
            tags.push((start, end, a, b));
            offset = end;
        } else {
            offset = start + 1;
        }
    }
    let mut text = String::new();
    let mut syllables = Vec::new();
    let mut previous = 0;
    for (index, &(start, end, a, b)) in tags.iter().enumerate() {
        text.push_str(&content[previous..start]);
        previous = end;
        let segment_end = tags.get(index + 1).map_or(content.len(), |next| next.0);
        let segment = &content[end..segment_end];
        if !segment.is_empty() {
            let start_ms = ((i128::from(a) + i128::from(b)).unsigned_abs()
                / (2 * u128::from(start_factor)))
            .min(u128::from(u64::MAX)) as u64;
            let duration_ms = ((i128::from(a) - i128::from(b)).unsigned_abs()
                / (2 * u128::from(duration_factor)))
            .min(u128::from(u64::MAX)) as u64;
            syllables.push(RawSyllable {
                text: segment.into(),
                offset_ms: start_ms,
                duration_ms,
            });
        }
    }
    text.push_str(&content[previous..]);
    (text, syllables)
}

fn mark_translations(raw: &mut [RawLine]) {
    let mut index = 0;
    let mut last_group = (0, 0);
    let mut last_timed = false;
    let mut previous_timed = false;
    while index < raw.len() {
        let mut end = index + 1;
        while end < raw.len() && raw[end].start_ms == raw[index].start_ms {
            end += 1;
        }
        let timed = raw[index..end].iter().any(RawLine::timed);
        let untimed = raw[index..end].iter().any(|line| !line.timed());
        if timed && untimed {
            for line in &mut raw[index..end] {
                line.translation = !line.timed();
            }
        }
        previous_timed = last_timed;
        last_timed = timed;
        last_group = (index, end);
        index = end;
    }
    if !last_timed && previous_timed {
        for line in &mut raw[last_group.0..last_group.1] {
            if !line.timed() && !line.text.trim().is_empty() {
                line.translation = true;
            }
        }
    }
}

/// Parse LRC and Kuwo word timing. Invalid tags are retained as text.
pub fn parse(text: &str, track_duration_ms: Option<u64>) -> LyricDocument {
    let tag = factors(text);
    let stripped = tag.map(|(start, end, _, _)| format!("{}{}", &text[..start], &text[end..]));
    let content = stripped.as_deref().unwrap_or(text);
    let mut raw = Vec::new();
    for line in content.split('\n') {
        let line = line.trim_end_matches('\r');
        let mut offset = 0;
        let mut stamps = Vec::new();
        while let Some((stamp, consumed)) = timestamp(&line[offset..]) {
            stamps.push(stamp);
            offset += consumed;
        }
        if stamps.is_empty() {
            continue;
        }
        let (text, syllables) = match tag {
            Some((_, _, a, b)) => parse_word_tags(&line[offset..], a, b),
            None => (line[offset..].to_owned(), Vec::new()),
        };
        for start_ms in stamps {
            raw.push(RawLine {
                start_ms,
                text: text.clone(),
                syllables: syllables.clone(),
                translation: false,
            });
        }
    }
    if raw.is_empty() {
        return LyricDocument::default();
    }
    // Multiple timestamps on one physical line can otherwise make the timeline non-monotonic.
    raw.sort_by_key(|line| line.start_ms);
    mark_translations(&mut raw);
    let lines = raw
        .iter()
        .enumerate()
        .map(|(index, line)| {
            let end = raw
                .get(index + 1)
                .map(|next| next.start_ms)
                .or(track_duration_ms)
                .unwrap_or(0);
            let duration_ms = end
                .checked_sub(line.start_ms)
                .filter(|duration| *duration > 0)
                .unwrap_or(5000);
            let syllables = if line.syllables.is_empty() {
                vec![LyricSyllable {
                    text: line.text.clone(),
                    start_ms: line.start_ms,
                    duration_ms,
                }]
            } else {
                line.syllables
                    .iter()
                    .map(|s| LyricSyllable {
                        text: s.text.clone(),
                        start_ms: line.start_ms.saturating_add(s.offset_ms),
                        duration_ms: s.duration_ms,
                    })
                    .collect()
            };
            LyricLine {
                start_ms: line.start_ms,
                duration_ms,
                text: line.text.clone(),
                syllables,
                is_translation: line.translation,
            }
        })
        .collect();
    LyricDocument {
        lines,
        kind: if tag.is_some() {
            LyricKind::WordByWord
        } else {
            LyricKind::LineByLine
        },
    }
}

#[cfg(test)]
#[path = "lyrics_tests.rs"]
mod tests;
