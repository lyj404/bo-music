//! Selected-track validation and batch workers.

use super::*;

pub(super) fn selected_tracks(
    source: BatchSource,
    selected: &[(usize, i64)],
    browser: &Browser,
    search: (u64, &[Track]),
    history: (u64, u64, &[Track]),
    current_revision: u64,
) -> Option<Vec<Track>> {
    if selected.is_empty() {
        return None;
    }
    selected
        .iter()
        .map(|&(index, id)| {
            if id <= 0 {
                return None;
            }
            match source {
                BatchSource::Browse(generation) => browser.playable_track(index, id, generation),
                BatchSource::Search(generation) if generation == search.0 => {
                    search.1.get(index).filter(|track| track.id == id).cloned()
                }
                BatchSource::History(generation)
                    if generation == history.0 && history.1 == current_revision =>
                {
                    history.2.get(index).filter(|track| track.id == id).cloned()
                }
                _ => None,
            }
        })
        .collect()
}

pub(super) fn start_batch(
    transport: Arc<Transport>,
    sender: mpsc::UnboundedSender<Command>,
    job: BatchJob,
    picker_generation: Option<u64>,
) -> Arc<AtomicBool> {
    let cancel = Arc::new(AtomicBool::new(false));
    let task_cancel = Arc::clone(&cancel);
    tokio::spawn(async move {
        let revision = job.revision;
        let playlist_id = job.playlist_id;
        let action = if job.add {
            BatchAction::Add
        } else {
            BatchAction::Remove
        };
        let progress_sender = sender.clone();
        let result = batch_playlist::write(&transport, job, &task_cancel, move |progress| {
            let _ = progress_sender.send(Command::BatchProgress(revision, progress));
        })
        .await;
        let _ = sender.send(Command::BatchDone(
            revision,
            playlist_id,
            action,
            picker_generation,
            result,
        ));
    });
    cancel
}

pub(super) fn start_liked_batch(
    transport: Arc<Transport>,
    sender: mpsc::UnboundedSender<Command>,
    ids: Vec<i64>,
    revision: u64,
) -> Arc<AtomicBool> {
    let cancel = Arc::new(AtomicBool::new(false));
    let task_cancel = Arc::clone(&cancel);
    tokio::spawn(async move {
        let ids = batch_playlist::normalize(ids);
        let snapshot = match crate::likes::fetch_for_batch(&transport, revision, &task_cancel).await
        {
            Ok(snapshot) => snapshot,
            Err(()) if task_cancel.load(Ordering::Acquire) => {
                let _ = sender.send(Command::BatchDone(
                    revision,
                    0,
                    BatchAction::Like,
                    None,
                    BatchProgress {
                        total: ids.len(),
                        canceled: true,
                        ..Default::default()
                    },
                ));
                return;
            }
            Err(()) => {
                let _ = sender.send(Command::BatchPreparationFailed(
                    revision,
                    "无法读取喜欢列表，请重试",
                ));
                return;
            }
        };
        if task_cancel.load(Ordering::Acquire) {
            let _ = sender.send(Command::BatchDone(
                revision,
                0,
                BatchAction::Like,
                None,
                BatchProgress {
                    total: ids.len(),
                    canceled: true,
                    ..Default::default()
                },
            ));
            return;
        }
        let Some(playlist_id) = snapshot.playlist else {
            let _ = sender.send(Command::BatchPreparationFailed(
                revision,
                "账号没有喜欢歌单",
            ));
            return;
        };
        if !snapshot.complete {
            let _ = sender.send(Command::BatchPreparationFailed(
                revision,
                "喜欢列表超过扫描上限，无法确定全部歌曲状态",
            ));
            return;
        }
        let targets = ids
            .iter()
            .copied()
            .filter(|id| !snapshot.ids.contains(id))
            .collect::<Vec<_>>();
        let skipped = ids.len() - targets.len();
        let sender_progress = sender.clone();
        let initial = BatchProgress {
            total: ids.len(),
            processed: skipped,
            skipped,
            ..Default::default()
        };
        let _ = sender.send(Command::BatchProgress(revision, initial));
        let adjust = |mut progress: BatchProgress| {
            progress.total += skipped;
            progress.processed += skipped;
            progress.skipped = skipped;
            progress
        };
        let result = batch_playlist::write(
            &transport,
            BatchJob {
                playlist_id,
                track_ids: targets,
                add: true,
                revision,
            },
            &task_cancel,
            move |progress| {
                let _ = sender_progress.send(Command::BatchProgress(revision, adjust(progress)));
            },
        )
        .await;
        let _ = sender.send(Command::BatchDone(
            revision,
            playlist_id,
            BatchAction::Like,
            None,
            adjust(result),
        ));
    });
    cancel
}
