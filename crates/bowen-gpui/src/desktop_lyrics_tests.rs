use super::*;

#[test]
fn translation_stays_with_the_original_line_until_the_next_original_starts() {
    let line = |start_ms, text: &str, is_translation| LyricLine {
        start_ms,
        duration_ms: 1_000,
        text: text.into(),
        syllables: Vec::new(),
        is_translation,
    };
    let document = LyricDocument {
        lines: vec![
            line(100, "First", false),
            line(150, "第一行", true),
            line(600, "Second", false),
            line(1_000, "Third", false),
        ],
        ..Default::default()
    };
    assert_eq!(lyric_lines(&document, 99), (None, None));
    let pair = lyric_lines(&document, 300);
    assert_eq!(pair.0.unwrap().text, "First");
    assert_eq!(pair.1.unwrap().text, "第一行");
    let pair = lyric_lines(&document, 600);
    assert_eq!(pair.0.unwrap().text, "Second");
    assert_eq!(pair.1.unwrap().text, "Third");
}
