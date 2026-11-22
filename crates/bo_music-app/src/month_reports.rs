use bo_music_api::{account::month_report_at_revision, transport::Transport};
use bo_music_domain::account::{MonthAvailability, MonthPeriod, MonthReport};
use std::{collections::BTreeMap, future::Future, sync::Arc};

pub(crate) type ReportResult = Result<Option<Box<MonthReport>>, ()>;

#[derive(Default)]
pub(crate) struct ReportCache(BTreeMap<MonthPeriod, Option<Box<MonthReport>>>);

impl ReportCache {
    pub(crate) fn get(&self, period: MonthPeriod) -> Option<ReportResult> {
        self.0.get(&period).cloned().map(Ok)
    }

    pub(crate) fn availability(&self, period: MonthPeriod) -> Option<MonthAvailability> {
        self.0.get(&period).map(|entry| {
            if entry.is_some() {
                MonthAvailability::Available
            } else {
                MonthAvailability::Empty
            }
        })
    }

    pub(crate) fn remember(&mut self, period: MonthPeriod, report: &ReportResult) {
        if let Ok(report) = report {
            self.0.insert(period, report.clone());
            if self.0.len() > 72 {
                self.0.pop_first();
            }
        }
    }

    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }
}

pub(crate) fn availability(result: &ReportResult) -> MonthAvailability {
    match result {
        Ok(Some(_)) => MonthAvailability::Available,
        Ok(None) => MonthAvailability::Empty,
        Err(()) => MonthAvailability::Failed,
    }
}

pub(crate) async fn fetch_report(
    transport: &Transport,
    period: MonthPeriod,
    revision: u64,
) -> ReportResult {
    let report = month_report_at_revision(transport, period, revision)
        .await
        .map_err(|_| ())?;
    if report
        .as_ref()
        .is_some_and(|report| report.period != period)
    {
        return Err(());
    }
    Ok(report.map(Box::new))
}

pub(crate) async fn probe_months(
    transport: Arc<Transport>,
    revision: u64,
    periods: Vec<MonthPeriod>,
    report: impl FnMut(MonthPeriod, ReportResult),
) {
    probe(
        periods,
        move |period| {
            let transport = Arc::clone(&transport);
            async move { fetch_report(&transport, period, revision).await }
        },
        report,
    )
    .await;
}

async fn probe<Fut: Future<Output = ReportResult> + Send + 'static>(
    periods: Vec<MonthPeriod>,
    fetch: impl Fn(MonthPeriod) -> Fut,
    mut report: impl FnMut(MonthPeriod, ReportResult),
) {
    let mut periods = periods.into_iter();
    let mut tasks = tokio::task::JoinSet::new();
    let launch = |tasks: &mut tokio::task::JoinSet<_>, period| {
        let request = fetch(period);
        tasks.spawn(async move { (period, request.await) });
    };
    for period in periods.by_ref().take(3) {
        launch(&mut tasks, period);
    }
    while let Some(result) = tasks.join_next().await {
        if let Ok((period, result)) = result {
            report(period, result);
        }
        if let Some(period) = periods.next() {
            launch(&mut tasks, period);
        }
    }
}

#[cfg(test)]
#[path = "month_reports_tests.rs"]
mod tests;
