use super::*;

#[test]
fn smtc_timeline_uses_hundred_nanosecond_ticks_without_wrapping() {
    assert_eq!(span(0).Duration, 0);
    assert_eq!(span(1_234).Duration, 12_340_000);
    assert_eq!(span(u64::MAX).Duration, i64::MAX);
}
