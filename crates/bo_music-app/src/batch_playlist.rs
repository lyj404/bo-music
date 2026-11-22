//! Sequential playlist mutations, with cooperative cancellation between writes.
use bo_music_api::{playlist, transport::Transport};
use std::{
    collections::HashSet,
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BatchProgress {
    pub total: usize,
    pub processed: usize,
    pub succeeded: usize,
    pub skipped: usize,
    pub unconfirmed: usize,
    pub canceled: bool,
    pub account_changed: bool,
}

/// Dedupe in the displayed order; repeated IDs must not trigger duplicate writes.
pub fn normalize(ids: impl IntoIterator<Item = i64>) -> Vec<i64> {
    let mut seen = HashSet::new();
    ids.into_iter()
        .filter(|id| *id > 0 && seen.insert(*id))
        .collect()
}

pub struct BatchJob {
    pub playlist_id: i64,
    pub track_ids: Vec<i64>,
    pub add: bool,
    pub revision: u64,
}

/// Never abort an in-flight non-idempotent request or automatically retry one.
pub async fn write(
    transport: &Transport,
    job: BatchJob,
    cancel: &AtomicBool,
    mut report: impl FnMut(BatchProgress),
) -> BatchProgress {
    let ids = normalize(job.track_ids);
    let mut progress = BatchProgress {
        total: ids.len(),
        ..Default::default()
    };
    for id in ids {
        if cancel.load(Ordering::Acquire) {
            progress.canceled = true;
            break;
        }
        let identity = transport.session().snapshot();
        if identity.revision != job.revision || !identity.is_authenticated() {
            progress.account_changed = true;
            break;
        }
        let result = playlist::set_tracks_at_revision(
            transport,
            job.playlist_id,
            &[id],
            job.add,
            job.revision,
        )
        .await;
        progress.processed += 1;
        if result.is_ok() {
            progress.succeeded += 1;
        } else {
            progress.unconfirmed += 1;
        }
        report(progress);
        let identity = transport.session().snapshot();
        if identity.revision != job.revision || !identity.is_authenticated() {
            progress.account_changed = true;
            break;
        }
    }
    progress
}

#[cfg(test)]
#[path = "batch_playlist_tests.rs"]
mod tests;
