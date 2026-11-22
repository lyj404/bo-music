use super::*;

#[test]
fn translations_and_timeline_are_position_based() {
    let document = parse(
        "[kuwo:127]\n[00:01.000]<1120,-1120>你<2400,160>好\n[00:01.000]<0,0>Hello\n[00:03.000]<3150,-3150>再见\n[00:05.000]Goodbye",
        None,
    );
    assert_eq!(document.lines[0].syllables[0].duration_ms, 160);
    assert_eq!(document.lines[0].syllables[1].start_ms, 1160);
    assert!(document.lines[1].is_translation);
    assert!(document.lines[3].is_translation);
    assert_eq!(document.scroll_units(), vec![0, 0, 1, 1]);
    assert_eq!(document.source_line(3), Some(2));
    assert_eq!(document.without_translations().lines.len(), 2);
    let clean = document.without_translations();
    assert_eq!(clean.locate(999), LyricCursor::default());
    let cursor = clean.locate(1080);
    assert_eq!((cursor.line, cursor.syllable), (Some(0), Some(0)));
    assert_eq!(cursor.syllable_progress, 0.5);
}

#[test]
fn repeated_timestamps_are_sorted_and_bad_factor_tags_do_not_panic() {
    let document = parse("[00:01.5][00:05.00]重复\n[00:03:50]中间", None);
    assert_eq!(
        document
            .lines
            .iter()
            .map(|line| line.start_ms)
            .collect::<Vec<_>>(),
        vec![1500, 3500, 5000]
    );
    assert_eq!(document.line_at(3500), Some(1));
    let invalid = parse("[kuwo:888]\n[00:01.0]<0,0>text", None);
    assert_eq!(invalid.kind, LyricKind::LineByLine);
    assert_eq!(invalid.lines[0].text, "<0,0>text");
    assert!(parse("[ti:metadata]", None).lines.is_empty());
}
