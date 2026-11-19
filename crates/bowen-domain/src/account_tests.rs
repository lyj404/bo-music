use super::MonthPeriod;

#[test]
fn month_picker_infers_previous_year_and_crosses_year_boundary() {
    let january = MonthPeriod::new(2026, 1).unwrap();
    let december = MonthPeriod::new(2025, 12).unwrap();
    assert_eq!(
        MonthPeriod::from_summary("12月听歌", january),
        Some(december)
    );
    assert_eq!(
        MonthPeriod::from_summary("2024年12月", january),
        MonthPeriod::new(2024, 12)
    );
    assert_eq!(MonthPeriod::from_summary("202512", january), Some(december));
    assert_eq!(MonthPeriod::from_summary("第13月", january), None);
    assert_eq!(january.offset(-1), Some(december));
    assert_eq!(december.offset(1), Some(january));
}
