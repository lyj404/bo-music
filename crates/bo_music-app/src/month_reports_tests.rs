use super::*;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

#[tokio::test]
async fn month_probe_limits_concurrency_and_does_not_cache_failures() {
    let running = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let mut cache = ReportCache::default();
    let mut seen = BTreeMap::new();
    let periods: Vec<_> = (1..=12)
        .map(|month| MonthPeriod::new(2026, month).unwrap())
        .collect();
    let failed = periods[4];
    probe(
        periods.clone(),
        |period| {
            let running = Arc::clone(&running);
            let peak = Arc::clone(&peak);
            async move {
                peak.fetch_max(running.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(5)).await;
                running.fetch_sub(1, Ordering::SeqCst);
                if period == failed { Err(()) } else { Ok(None) }
            }
        },
        |period, result| {
            seen.insert(period, availability(&result));
            cache.remember(period, &result);
        },
    )
    .await;
    assert_eq!(peak.load(Ordering::SeqCst), 3);
    assert_eq!(seen.len(), periods.len());
    assert_eq!(seen[&failed], MonthAvailability::Failed);
    assert_eq!(cache.availability(failed), None);
    assert!(matches!(cache.get(periods[0]), Some(Ok(None))));
    cache.clear();
    assert_eq!(cache.availability(periods[0]), None);
}
