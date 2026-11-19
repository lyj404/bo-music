//! Coordinates queue selection, account-bound resolution and a serialized player.
//!
//! Resolution tickets are detached from the coordinator so a UI can run requests
//! concurrently. Only the latest ticket for the current account can be committed.

use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};

use bowen_api::{
    playback::{self, AudioSource, Denial, PlaybackResolution, ResolveError},
    session::Session,
    telemetry::CollectionKind,
    transport::Transport,
};
use bowen_domain::{
    queue::{FinishAction, PlayMode, PlayQueue},
    track::{AudioQuality, Track},
};
use bowen_playback::mpv::{LoadSource, MpvError, MpvPlayer, PlaybackSnapshot, PlaybackState};
use bowen_storage::queue::{QueueError, QueueSnapshot, QueueStore};
use thiserror::Error;

/// The player interface also permits deterministic coordinator tests.
pub trait Player {
    fn load(&self, source: LoadSource) -> Result<(), MpvError>;
    fn refresh_recovery(&self, id: u64, source: LoadSource) -> Result<(), MpvError>;
    fn stop(&self) -> Result<(), MpvError>;
    fn pause(&self, paused: bool) -> Result<(), MpvError>;
    fn seek(&self, seconds: f64) -> Result<(), MpvError>;
    fn volume(&self, percent: f64) -> Result<(), MpvError>;
    fn audio_filter(&self, filter: Option<&str>) -> Result<(), MpvError>;
    fn audio_filter_weights(&self, weights: &str, graph: &str) -> Result<(), MpvError>;
    fn snapshot(&self) -> PlaybackSnapshot;
}

impl Player for MpvPlayer {
    fn load(&self, source: LoadSource) -> Result<(), MpvError> {
        self.load(source)
    }
    fn refresh_recovery(&self, id: u64, source: LoadSource) -> Result<(), MpvError> {
        self.refresh_recovery(id, source)
    }
    fn stop(&self) -> Result<(), MpvError> {
        self.stop()
    }
    fn pause(&self, paused: bool) -> Result<(), MpvError> {
        self.pause(paused)
    }
    fn seek(&self, seconds: f64) -> Result<(), MpvError> {
        self.seek(seconds)
    }
    fn volume(&self, percent: f64) -> Result<(), MpvError> {
        self.volume(percent)
    }
    fn audio_filter(&self, filter: Option<&str>) -> Result<(), MpvError> {
        self.audio_filter(filter)
    }
    fn audio_filter_weights(&self, weights: &str, graph: &str) -> Result<(), MpvError> {
        MpvPlayer::audio_filter_weights(self, weights, graph)
    }
    fn snapshot(&self) -> PlaybackSnapshot {
        self.snapshot()
    }
}

#[derive(Debug, Error)]
pub enum CoordinatorError {
    #[error(transparent)]
    Player(#[from] MpvError),
    #[error(transparent)]
    Resolve(#[from] ResolveError),
    #[error("invalid audition interval")]
    InvalidAudition,
    #[error("the account changed before its queue was captured")]
    SessionChanged,
    #[error("the queue changed before the action was applied")]
    QueueChanged,
    #[error("track ID must be positive")]
    InvalidTrack,
}

pub enum QueueEdit {
    Append(Track),
    AppendMany(Vec<Track>),
    PlayNext(Track),
    Select(usize),
    Remove(usize),
    Move(usize, usize),
    Clear,
}

pub struct QueueEditOutcome {
    pub changed: bool,
    pub affects_playback: bool,
    pub ticket: Option<ResolutionTicket>,
}

#[derive(Debug, Error)]
pub enum QueuePersistenceError {
    #[error(transparent)]
    Playback(#[from] CoordinatorError),
    #[error(transparent)]
    Storage(#[from] QueueError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApplyOutcome {
    Started,
    Audition,
    Denied(Denial),
    Stale,
    RetryFresh,
}

/// Opaque, account-bound work item. Its URL and credentials are never formatted.
pub struct ResolutionTicket {
    generation: u64,
    revision: u64,
    track: Track,
    quality: Option<AudioQuality>,
    transport: Arc<Transport>,
}

pub struct ResolvedTicket {
    generation: u64,
    revision: u64,
    result: Result<PlaybackResolution, ResolveError>,
    from_prefetch: bool,
    cache_entry: Option<Box<CachedEntry>>,
}

/// An uncached reauthorization bound to a single playback and recovery episode.
pub struct RecoveryTicket {
    ticket: ResolutionTicket,
    recovery_id: u64,
    failures: u32,
}

pub struct RecoveryResult {
    generation: u64,
    revision: u64,
    recovery_id: u64,
    failures: u32,
    result: Option<Result<PlaybackResolution, ResolveError>>,
}

/// A requested quality change is independent of the default quality for future tracks.
pub struct QualityTicket {
    ticket: ResolutionTicket,
    sequence: u64,
}
pub struct QualityResult {
    generation: u64,
    revision: u64,
    sequence: u64,
    result: Option<Result<PlaybackResolution, ResolveError>>,
}

impl QualityTicket {
    pub async fn resolve(self) -> QualityResult {
        let ticket = self.ticket;
        let result = tokio::time::timeout(
            Duration::from_secs(15),
            playback::resolve_for_revision(
                &ticket.transport,
                &ticket.track,
                ticket.quality,
                ticket.revision,
            ),
        )
        .await
        .ok();
        QualityResult {
            generation: ticket.generation,
            revision: ticket.revision,
            sequence: self.sequence,
            result,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CurrentQuality {
    pub requested: Option<AudioQuality>,
    pub format: String,
    pub bitrate_kbps: i32,
    pub downgraded: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QualityOutcome {
    Loading,
    Changed,
    Restored,
    Failed,
    Unavailable,
    Stale,
}

enum QualityPhase {
    Loading(AudioSource),
    Restoring,
}
struct QualityLoad {
    old_source: AudioSource,
    phase: QualityPhase,
    loaded_count: u64,
    started: Instant,
    position: f64,
    duration: Option<f64>,
    paused: bool,
    deferred_seek: Option<f64>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoveryOutcome {
    Refreshed,
    Stale,
    Unavailable,
    Denied(Denial),
}

impl RecoveryTicket {
    pub async fn resolve(self) -> RecoveryResult {
        let ticket = self.ticket;
        let result = tokio::time::timeout(
            Duration::from_secs(15),
            playback::resolve_for_revision(
                &ticket.transport,
                &ticket.track,
                ticket.quality,
                ticket.revision,
            ),
        )
        .await
        .ok();
        RecoveryResult {
            generation: ticket.generation,
            revision: ticket.revision,
            recovery_id: self.recovery_id,
            failures: self.failures,
            result,
        }
    }
}
#[path = "playback_prefetch.rs"]
mod prefetch;
#[path = "playback_quality.rs"]
mod quality;
#[path = "playback_queue.rs"]
mod queue;
use prefetch::{CachedEntry, PREFETCH_TTL, ResolutionKey};
pub(crate) use prefetch::{PrefetchHit, Prefetcher};
/// The captured scope must be used when saving, even if the account changes.
pub struct QueueCapture {
    pub scope: String,
    pub snapshot: QueueSnapshot,
}

impl ResolutionTicket {
    fn key(&self) -> ResolutionKey {
        ResolutionKey {
            revision: self.revision,
            track: self.track.clone(),
            quality: self.quality,
        }
    }
    pub fn track(&self) -> &Track {
        &self.track
    }

    pub async fn resolve(self) -> ResolvedTicket {
        let result = playback::resolve_for_revision(
            &self.transport,
            &self.track,
            self.quality,
            self.revision,
        )
        .await;
        ResolvedTicket {
            generation: self.generation,
            revision: self.revision,
            result,
            from_prefetch: false,
            cache_entry: None,
        }
    }

    pub(crate) async fn resolve_with_prefetch(self, entry: Option<PrefetchHit>) -> ResolvedTicket {
        if let Some(PrefetchHit::Ready { result, started }) = entry.as_ref()
            && started.elapsed() <= PREFETCH_TTL
        {
            return ResolvedTicket {
                generation: self.generation,
                revision: self.revision,
                result: Ok(result.clone()),
                from_prefetch: true,
                cache_entry: None,
            };
        }
        if let Some(PrefetchHit::Pending(mut entry)) = entry
            && entry.started.elapsed() <= PREFETCH_TTL
            && let Ok(cached) = (&mut entry.task).await
            && entry.started.elapsed() <= PREFETCH_TTL
            && let Ok(result) = cached.result
        {
            return ResolvedTicket {
                generation: self.generation,
                revision: self.revision,
                cache_entry: Some(Box::new(CachedEntry {
                    key: entry.key.clone(),
                    started: entry.started,
                    result: Some(result.clone()),
                })),
                result: Ok(result),
                from_prefetch: true,
            };
        }
        self.resolve().await
    }
}

pub struct PlaybackCoordinator<P: Player> {
    transport: Arc<Transport>,
    session: Session,
    player: P,
    queue: PlayQueue,
    generation: u64,
    observed_revision: u64,
    pending: bool,
    active: bool,
    loaded_count: u64,
    start_confirmed: bool,
    prefetched_load: bool,
    recovery_pending: Option<(u64, u32)>,
    recovery_last: Option<(u64, u32)>,
    audition: Option<(f64, f64)>,
    completed_count: u64,
    quality: Option<AudioQuality>,
    current_source: Option<AudioSource>,
    quality_sequence: u64,
    quality_pending: Option<u64>,
    quality_load: Option<QualityLoad>,
    quality_outcome: Option<QualityOutcome>,
    resume: Option<(i64, f64)>,
    reports: Vec<PlaybackReport>,
    recent_plays: Vec<(Track, u64)>,
    origin: Option<PlaybackOrigin>,
    reported_origin: Option<PlaybackOrigin>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaybackOrigin {
    pub kind: CollectionKind,
    pub id: i64,
    pub source_type: i32,
}

#[derive(Debug, PartialEq)]
pub enum PlaybackReport {
    Started {
        music_id: i64,
        revision: u64,
    },
    Ended {
        music_id: i64,
        duration: i32,
        position: i32,
        completed: bool,
        revision: u64,
    },
    Collection {
        origin: PlaybackOrigin,
        revision: u64,
    },
}

impl<P: Player> PlaybackCoordinator<P> {
    pub fn new(transport: Arc<Transport>, player: P) -> Self {
        let session = transport.session();
        let observed_revision = session.snapshot().revision;
        let completed_count = player.snapshot().completed_count;
        Self {
            transport,
            session,
            player,
            queue: PlayQueue::default(),
            generation: 0,
            observed_revision,
            pending: false,
            active: false,
            loaded_count: 0,
            start_confirmed: false,
            prefetched_load: false,
            recovery_pending: None,
            recovery_last: None,
            audition: None,
            completed_count,
            quality: None,
            current_source: None,
            quality_sequence: 0,
            quality_pending: None,
            quality_load: None,
            quality_outcome: None,
            resume: None,
            reports: Vec::new(),
            recent_plays: Vec::new(),
            origin: None,
            reported_origin: None,
        }
    }

    pub fn queue(&self) -> &PlayQueue {
        &self.queue
    }
    pub fn queue_account_revision(&self) -> u64 {
        self.observed_revision
    }

    /// Apply an action only to the identity and queue snapshot displayed by the UI.
    pub fn edit_queue(
        &mut self,
        account_revision: u64,
        queue_revision: u64,
        edit: QueueEdit,
    ) -> Result<QueueEditOutcome, CoordinatorError> {
        let session = self.session.clone();
        session
            .with_revision(account_revision, || {
                if self.observed_revision != account_revision {
                    return Err(CoordinatorError::SessionChanged);
                }
                if self.queue.revision() != queue_revision {
                    return Err(CoordinatorError::QueueChanged);
                }
                let mut changed = false;
                let mut affects_playback = false;
                let mut ticket = None;
                match edit {
                    QueueEdit::Append(track) | QueueEdit::PlayNext(track) if track.id <= 0 => {
                        return Err(CoordinatorError::InvalidTrack);
                    }
                    QueueEdit::Append(track) => {
                        let empty = self.queue.current().is_none();
                        if empty {
                            self.invalidate_inner(Some(account_revision))?;
                        }
                        changed = self.queue.append(track);
                        if changed && empty {
                            affects_playback = true;
                            self.set_origin(None);
                            ticket = self.ticket();
                        }
                    }
                    QueueEdit::AppendMany(tracks) => {
                        let tracks = tracks
                            .into_iter()
                            .filter(|track| track.id > 0)
                            .collect::<Vec<_>>();
                        let empty = self.queue.current().is_none();
                        if empty && !tracks.is_empty() {
                            self.invalidate_inner(Some(account_revision))?;
                        }
                        changed = self.queue.append_range(tracks) > 0;
                        if changed && empty {
                            affects_playback = true;
                            self.set_origin(None);
                            ticket = self.ticket();
                        }
                    }
                    QueueEdit::PlayNext(track) => {
                        let empty = self.queue.current().is_none();
                        if empty {
                            self.invalidate_inner(Some(account_revision))?;
                        }
                        self.queue.insert_next(track);
                        changed = true;
                        if empty {
                            affects_playback = true;
                            self.set_origin(None);
                            ticket = self.ticket();
                        }
                    }
                    QueueEdit::Select(index) if index < self.queue.items().len() => {
                        self.invalidate_inner(Some(account_revision))?;
                        self.queue.move_to(index);
                        changed = true;
                        affects_playback = true;
                        ticket = self.ticket();
                    }
                    QueueEdit::Remove(index) if index < self.queue.items().len() => {
                        let current = self.queue.current_index() == Some(index);
                        if current {
                            self.invalidate_inner(Some(account_revision))?;
                        }
                        changed = self.queue.remove(index);
                        if current {
                            affects_playback = true;
                            ticket = self.ticket();
                            if ticket.is_none() {
                                self.set_origin(None);
                                self.resume = None;
                            }
                        }
                    }
                    QueueEdit::Move(from, to) => changed = self.queue.reorder(from, to),
                    QueueEdit::Clear if !self.queue.items().is_empty() => {
                        self.invalidate_inner(Some(account_revision))?;
                        self.queue.clear();
                        self.set_origin(None);
                        self.resume = None;
                        changed = true;
                        affects_playback = true;
                    }
                    _ => {}
                }
                Ok(QueueEditOutcome {
                    changed,
                    affects_playback,
                    ticket,
                })
            })
            .unwrap_or(Err(CoordinatorError::SessionChanged))
    }
    pub fn snapshot(&self) -> PlaybackSnapshot {
        let mut snapshot = self.player.snapshot();
        if let Some(loading) = &self.quality_load
            && snapshot.state != PlaybackState::Stopped
        {
            snapshot.state = if loading.paused {
                PlaybackState::Paused
            } else {
                PlaybackState::Playing
            };
            snapshot.position_seconds = loading.position;
            snapshot.duration_seconds = loading.duration;
        }
        snapshot
    }

    pub fn take_reports(&mut self) -> Vec<PlaybackReport> {
        std::mem::take(&mut self.reports)
    }
    pub fn take_recent_plays(&mut self) -> Vec<(Track, u64)> {
        std::mem::take(&mut self.recent_plays)
    }

    fn observe_started(&mut self, snapshot: PlaybackSnapshot) {
        if !self.active || self.start_confirmed || snapshot.loaded_count == self.loaded_count {
            return;
        }
        self.start_confirmed = true;
        if let Some(track) = self.queue.current().filter(|track| track.id > 0) {
            self.recent_plays
                .push((track.clone(), self.observed_revision));
            self.reports.push(PlaybackReport::Started {
                music_id: track.id,
                revision: self.observed_revision,
            });
        }
        if let Some(origin) = self.origin
            && self.reported_origin != Some(origin)
        {
            self.reports.push(PlaybackReport::Collection {
                origin,
                revision: self.observed_revision,
            });
            self.reported_origin = Some(origin);
        }
    }

    fn set_origin(&mut self, origin: Option<PlaybackOrigin>) {
        self.origin = origin.filter(|origin| origin.id > 0 && origin.source_type > 0);
        if self.origin.is_none() {
            self.reported_origin = None;
        }
    }

    fn report_end(&mut self, snapshot: PlaybackSnapshot, completed: bool) {
        let Some(track) = self.queue.current() else {
            return;
        };
        if track.id <= 0 || track.duration_seconds <= 0 {
            return;
        }
        let duration = track.duration_seconds;
        let position = if completed {
            duration
        } else if snapshot.position_seconds.is_finite() {
            snapshot
                .position_seconds
                .round_ties_even()
                .clamp(0.0, f64::from(duration)) as i32
        } else {
            0
        };
        self.reports.push(PlaybackReport::Ended {
            music_id: track.id,
            duration,
            position,
            completed,
            revision: self.observed_revision,
        });
    }

    pub fn pause(&mut self, paused: bool) -> Result<(), CoordinatorError> {
        if let Some(loading) = &mut self.quality_load {
            loading.paused = paused;
            return Ok(());
        }
        self.player.pause(paused)?;
        Ok(())
    }

    pub fn volume(&self, percent: f64) -> Result<(), CoordinatorError> {
        self.player.volume(percent)?;
        Ok(())
    }

    pub fn audio_filter(&self, filter: Option<&str>) -> Result<(), CoordinatorError> {
        self.player.audio_filter(filter)?;
        Ok(())
    }
    pub fn audio_filter_weights(&self, weights: &str, graph: &str) -> Result<(), CoordinatorError> {
        self.player.audio_filter_weights(weights, graph)?;
        Ok(())
    }

    pub fn seek(&mut self, seconds: f64) -> Result<(), CoordinatorError> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(MpvError::InvalidArgument.into());
        }
        let position = self
            .audition
            .map_or(seconds, |(start, end)| seconds.clamp(start, end));
        if let Some(loading) = &mut self.quality_load {
            loading.position = position;
            loading.deferred_seek = Some(position);
            return Ok(());
        }
        self.player.seek(position)?;
        Ok(())
    }

    fn invalidate(&mut self) -> Result<(), CoordinatorError> {
        self.invalidate_inner(None)
    }

    fn invalidate_inner(&mut self, locked_revision: Option<u64>) -> Result<(), CoordinatorError> {
        if self.active {
            let snapshot = self.snapshot();
            self.observe_started(snapshot);
            let completed =
                self.audition.is_none() && snapshot.completed_count != self.completed_count;
            if self.start_confirmed {
                self.report_end(snapshot, completed);
            }
        }
        self.generation = self.generation.wrapping_add(1);
        self.pending = false;
        self.active = false;
        self.start_confirmed = false;
        self.prefetched_load = false;
        self.recovery_pending = None;
        self.recovery_last = None;
        self.quality_sequence = self.quality_sequence.wrapping_add(1);
        self.quality_pending = None;
        self.quality_load = None;
        self.quality_outcome = None;
        self.current_source = None;
        self.audition = None;
        self.player.stop()?;
        self.completed_count = self.player.snapshot().completed_count;
        self.observed_revision = match locked_revision {
            Some(revision) => revision,
            None => self.session.snapshot().revision,
        };
        Ok(())
    }

    fn ticket(&mut self) -> Option<ResolutionTicket> {
        let track = self.queue.current()?.clone();
        self.pending = true;
        Some(ResolutionTicket {
            generation: self.generation,
            revision: self.observed_revision,
            track,
            quality: self.quality,
            transport: Arc::clone(&self.transport),
        })
    }

    /// Commit only if neither the selected track nor the session changed during resolution.
    pub fn apply(&mut self, resolved: ResolvedTicket) -> Result<ApplyOutcome, CoordinatorError> {
        if !self.pending
            || resolved.generation != self.generation
            || resolved.revision != self.session.snapshot().revision
        {
            return Ok(ApplyOutcome::Stale);
        }
        self.pending = false;
        let (source, audition, outcome) = match resolved.result? {
            PlaybackResolution::Denied(reason) => return Ok(ApplyOutcome::Denied(reason)),
            PlaybackResolution::Playable(source) => (source, None, ApplyOutcome::Started),
            PlaybackResolution::Audition {
                source,
                start_seconds,
                end_seconds,
            } if start_seconds >= 0 && end_seconds > start_seconds => (
                source,
                Some((f64::from(start_seconds), f64::from(end_seconds))),
                ApplyOutcome::Audition,
            ),
            PlaybackResolution::Audition { .. } => return Err(CoordinatorError::InvalidAudition),
        };
        let load = LoadSource {
            address: source.url.clone(),
            start_seconds: audition.map(|range| range.0).or_else(|| {
                self.resume
                    .filter(|(id, _)| self.queue.current().is_some_and(|track| track.id == *id))
                    .map(|(_, position)| position)
            }),
            end_seconds: audition.map(|range| range.1),
            initially_paused: false,
        };
        // The lock makes the identity check and short native command atomic with
        // respect to account changes; no network operation runs while it is held.
        let loaded_count = self.player.snapshot().loaded_count;
        match self
            .session
            .with_revision(resolved.revision, || self.player.load(load))
        {
            None => Ok(ApplyOutcome::Stale),
            Some(result) => {
                if let Err(error) = result {
                    return if resolved.from_prefetch {
                        Ok(ApplyOutcome::RetryFresh)
                    } else {
                        Err(error.into())
                    };
                }
                self.audition = audition;
                self.current_source = Some(source);
                self.resume = None;
                self.active = true;
                self.loaded_count = loaded_count;
                self.start_confirmed = false;
                self.prefetched_load = resolved.from_prefetch;
                self.observe_started(self.player.snapshot());
                Ok(outcome)
            }
        }
    }

    /// Poll on the application's timer. Returns a ticket when EOF advances the queue.
    pub fn poll(&mut self) -> Result<Option<ResolutionTicket>, CoordinatorError> {
        if self.observed_revision != self.session.snapshot().revision {
            self.ensure_account()?;
            return Ok(None);
        }
        let snapshot = self.player.snapshot();
        self.observe_started(snapshot);
        if let Some(mut attempt) = self.quality_load.take() {
            if snapshot.state == PlaybackState::Failed
                || attempt.started.elapsed() >= Duration::from_secs(25)
            {
                match attempt.phase {
                    QualityPhase::Loading(_) => {
                        if self.restore_quality_source(&mut attempt).is_ok() {
                            self.quality_load = Some(attempt);
                        } else {
                            self.active = false;
                            self.quality_outcome = Some(QualityOutcome::Failed);
                        }
                    }
                    QualityPhase::Restoring => {
                        let _ = self.player.stop();
                        self.active = false;
                        self.quality_outcome = Some(QualityOutcome::Failed);
                    }
                }
                return Ok(None);
            }
            if snapshot.loaded_count != attempt.loaded_count {
                match attempt.phase {
                    QualityPhase::Loading(source) => {
                        self.current_source = Some(source);
                        self.quality_outcome = Some(QualityOutcome::Changed);
                    }
                    QualityPhase::Restoring => {
                        self.current_source = Some(attempt.old_source);
                        self.quality_outcome = Some(QualityOutcome::Restored);
                    }
                }
                if matches!(
                    snapshot.state,
                    PlaybackState::Playing | PlaybackState::Paused
                ) {
                    if let Some(position) = attempt.deferred_seek {
                        self.player.seek(position)?;
                    }
                    self.player.pause(attempt.paused)?;
                }
            } else {
                self.quality_load = Some(attempt);
                return Ok(None);
            }
        }
        if self.active
            && snapshot.state == PlaybackState::Failed
            && self.prefetched_load
            && !self.start_confirmed
        {
            self.active = false;
            self.prefetched_load = false;
            self.generation = self.generation.wrapping_add(1);
            return Ok(self.ticket());
        }
        if self.active && snapshot.state == PlaybackState::Failed {
            self.active = false;
        }
        if !self.active || snapshot.completed_count == self.completed_count {
            return Ok(None);
        }
        self.active = false;
        self.completed_count = snapshot.completed_count;
        if self.start_confirmed {
            self.report_end(snapshot, self.audition.is_none());
        }
        // A preview ending is not completion of a full track and must not
        // silently consume the queue or trigger single-track looping.
        if self.audition.take().is_some() {
            return Ok(None);
        }
        match self.queue.on_finished() {
            FinishAction::Stop => Ok(None),
            FinishAction::PlayNext | FinishAction::ReplayCurrent => {
                self.generation = self.generation.wrapping_add(1);
                self.audition = None;
                Ok(self.ticket())
            }
        }
    }
}

#[cfg(test)]
#[path = "playback_tests.rs"]
mod tests;
