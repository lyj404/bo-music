//! Thread-owned libmpv audio backend. URLs are never logged or returned in errors.
use std::{
    path::Path,
    sync::{Arc, Mutex, mpsc},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};
use thiserror::Error;

#[path = "video.rs"]
pub mod video;

#[derive(Debug, Error)]
pub enum MpvError {
    #[error("could not load libmpv or a required symbol")]
    Library,
    #[error("libmpv client API 1.108+ or 2.x is required")]
    UnsupportedVersion,
    #[error("libmpv did not return a playlist entry ID")]
    MissingEntryId,
    #[error("could not start the playback worker")]
    Worker,
    #[error("libmpv could not create a player")]
    Create,
    #[error("libmpv {operation} failed with code {code}")]
    Native { operation: &'static str, code: i32 },
    #[error("invalid playback argument")]
    InvalidArgument,
    #[error("playback worker has stopped")]
    Closed,
    #[error("the recovery attempt has already ended")]
    StaleRecovery,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PlaybackState {
    #[default]
    Idle,
    Loading,
    Playing,
    Paused,
    Stopped,
    Failed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlaybackSnapshot {
    pub state: PlaybackState,
    /// A previously playing network stream is being reconnected.
    pub recovering: bool,
    /// Identifies a recovery episode, independently of ordinary snapshot revisions.
    pub recovery_id: u64,
    /// Failed reload attempts in the current episode.
    pub recovery_failures: u32,
    /// Reapplying the requested filter failed when loading a track.
    pub audio_filter_failed: bool,
    pub position_seconds: f64,
    pub duration_seconds: Option<f64>,
    /// Incremented only after the current entry reports a successful file load.
    pub loaded_count: u64,
    /// Incremented only on a natural end, never on stop or replacement.
    pub completed_count: u64,
    pub last_error: Option<i32>,
    pub revision: u64,
}

#[derive(Clone, Debug, Default)]
pub struct MpvOptions {
    /// `None` lets libmpv select the system audio output. `Some("null")` is useful for tests.
    pub audio_output: Option<String>,
}

/// Stream addresses may contain credentials, so this intentionally has no Debug implementation.
#[derive(Clone)]
pub struct LoadSource {
    pub address: String,
    pub start_seconds: Option<f64>,
    pub end_seconds: Option<f64>,
    pub initially_paused: bool,
}

enum Command {
    Load(LoadSource),
    RefreshRecovery(u64, LoadSource),
    Pause(bool),
    Seek(f64),
    Volume(f64),
    AudioFilter(String),
    AudioFilterWeights(String, String),
    Stop,
    Shutdown,
}
type Reply = mpsc::Sender<Result<(), MpvError>>;
struct Message {
    command: Command,
    reply: Reply,
}

/// Owns one mpv instance. Commands are serialized on its native thread.
pub struct MpvPlayer {
    commands: mpsc::Sender<Message>,
    snapshot: Arc<Mutex<PlaybackSnapshot>>,
    worker: Option<JoinHandle<()>>,
}

impl MpvPlayer {
    /// `library` must name a trusted local libmpv binary; the library stays loaded until shutdown.
    pub fn open(library: impl AsRef<Path>, options: MpvOptions) -> Result<Self, MpvError> {
        let path = library.as_ref().to_owned();
        let (commands, receiver) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let snapshot = Arc::new(Mutex::new(PlaybackSnapshot::default()));
        let worker_snapshot = Arc::clone(&snapshot);
        let worker = thread::Builder::new()
            .name("bo_music-mpv".into())
            .spawn(move || {
                let engine = match NativeMpv::open(path, &options) {
                    Ok(engine) => {
                        let _ = ready_tx.send(Ok(()));
                        engine
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                        return;
                    }
                };
                run(&engine, receiver, &worker_snapshot);
            })
            .map_err(|_| MpvError::Worker)?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                commands,
                snapshot,
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(_) => {
                let _ = worker.join();
                Err(MpvError::Worker)
            }
        }
    }

    /// A successful load means mpv accepted the command; `snapshot` reports loading and completion.
    pub fn load(&self, source: LoadSource) -> Result<(), MpvError> {
        validate_source(&source)?;
        self.send(Command::Load(source))
    }
    /// Replace the URL and access range only while the same recovery is still active.
    pub fn refresh_recovery(&self, id: u64, source: LoadSource) -> Result<(), MpvError> {
        validate_source(&source)?;
        if !source.address.starts_with("https://") && !source.address.starts_with("http://") {
            return Err(MpvError::InvalidArgument);
        }
        self.send(Command::RefreshRecovery(id, source))
    }
    pub fn pause(&self, paused: bool) -> Result<(), MpvError> {
        self.send(Command::Pause(paused))
    }
    pub fn seek(&self, seconds: f64) -> Result<(), MpvError> {
        if !seconds.is_finite() || seconds < 0.0 {
            return Err(MpvError::InvalidArgument);
        }
        self.send(Command::Seek(seconds))
    }
    pub fn volume(&self, percent: f64) -> Result<(), MpvError> {
        if !percent.is_finite() || !(0.0..=100.0).contains(&percent) {
            return Err(MpvError::InvalidArgument);
        }
        self.send(Command::Volume(percent))
    }
    /// Applies a generated audio filter without changing the current track.
    pub fn audio_filter(&self, filter: Option<&str>) -> Result<(), MpvError> {
        let filter = filter.unwrap_or_default();
        if filter.contains('\0') || filter.len() > 16384 {
            return Err(MpvError::InvalidArgument);
        }
        self.send(Command::AudioFilter(filter.to_owned()))
    }
    /// Updates amix in place; `graph` is retained for subsequent rollback.
    pub fn audio_filter_weights(&self, weights: &str, graph: &str) -> Result<(), MpvError> {
        if weights.contains('\0')
            || graph.contains('\0')
            || weights.len() > 128
            || graph.len() > 16384
        {
            return Err(MpvError::InvalidArgument);
        }
        self.send(Command::AudioFilterWeights(
            weights.to_owned(),
            graph.to_owned(),
        ))
    }
    pub fn stop(&self) -> Result<(), MpvError> {
        self.send(Command::Stop)
    }
    pub fn snapshot(&self) -> PlaybackSnapshot {
        *self
            .snapshot
            .lock()
            .expect("playback snapshot lock poisoned")
    }

    fn send(&self, command: Command) -> Result<(), MpvError> {
        let (tx, rx) = mpsc::channel();
        self.commands
            .send(Message { command, reply: tx })
            .map_err(|_| MpvError::Closed)?;
        rx.recv().map_err(|_| MpvError::Closed)?
    }
}

fn validate_source(source: &LoadSource) -> Result<(), MpvError> {
    if source.address.is_empty()
        || source.address.contains('\0')
        || source
            .start_seconds
            .is_some_and(|v| !v.is_finite() || v < 0.0)
        || source
            .end_seconds
            .is_some_and(|v| !v.is_finite() || v <= 0.0)
        || matches!((source.start_seconds, source.end_seconds), (Some(start), Some(end)) if end <= start)
    {
        return Err(MpvError::InvalidArgument);
    }
    Ok(())
}

impl Drop for MpvPlayer {
    fn drop(&mut self) {
        let _ = self.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn update(snapshot: &Mutex<PlaybackSnapshot>, f: impl FnOnce(&mut PlaybackSnapshot)) {
    let mut state = snapshot.lock().expect("playback snapshot lock poisoned");
    f(&mut state);
    state.revision = state.revision.wrapping_add(1);
}

#[derive(Default)]
struct EventTracker {
    current: Option<i64>,
    started: Option<i64>,
}

impl EventTracker {
    fn event(
        &mut self,
        event: NativeEvent,
        paused: bool,
        snapshot: &Mutex<PlaybackSnapshot>,
    ) -> bool {
        match event {
            NativeEvent::Start(id) => self.started = Some(id),
            NativeEvent::Loaded if self.current.is_some() && self.started == self.current => {
                update(snapshot, |s| {
                    s.loaded_count = s.loaded_count.wrapping_add(1);
                    s.state = if paused {
                        PlaybackState::Paused
                    } else {
                        PlaybackState::Playing
                    };
                });
            }
            NativeEvent::End { id, reason, error } => {
                if self.started == Some(id) {
                    self.started = None;
                }
                if self.current == Some(id) {
                    self.current = None;
                    update(snapshot, |s| {
                        if error < 0 || reason == 4 {
                            s.state = PlaybackState::Failed;
                            s.last_error = Some(error);
                        } else {
                            s.state = PlaybackState::Stopped;
                            if reason == 0 {
                                s.completed_count = s.completed_count.wrapping_add(1);
                            }
                        }
                    });
                }
            }
            NativeEvent::Overflow => {
                self.current = None;
                self.started = None;
                update(snapshot, |s| s.state = PlaybackState::Failed);
            }
            NativeEvent::Shutdown => {
                update(snapshot, |s| s.state = PlaybackState::Failed);
                return false;
            }
            NativeEvent::None | NativeEvent::Loaded => {}
        }
        true
    }
}

const STALL_TIMEOUT: Duration = Duration::from_secs(10);
const RECOVERY_LOAD_TIMEOUT: Duration = Duration::from_secs(15);

/// Recovery is confined to the native worker so replacement and stop cancel it synchronously.
#[derive(Default)]
struct Recovery {
    source: Option<LoadSource>,
    loaded: bool,
    recovering: bool,
    waiting_since: Option<Instant>,
    retry_at: Option<Instant>,
    failures: u32,
    truncated: u8,
    progress: Option<(f64, Instant)>,
}

impl Recovery {
    fn reset(&mut self, source: Option<LoadSource>) {
        *self = Self {
            source,
            ..Self::default()
        };
    }

    fn is_network(&self) -> bool {
        self.source.as_ref().is_some_and(|source| {
            source.address.starts_with("https://") || source.address.starts_with("http://")
        })
    }

    fn begin(&mut self, now: Instant) {
        self.recovering = true;
        self.waiting_since = None;
        self.retry_at = Some(now);
        self.progress = None;
    }

    fn failed_attempt(&mut self, now: Instant) {
        let delay = 2_u64.saturating_pow(self.failures.min(4) + 1).min(30);
        self.failures = self.failures.saturating_add(1);
        self.waiting_since = None;
        self.retry_at = Some(now + Duration::from_secs(delay));
    }

    fn loaded(&mut self, now: Instant, position: f64) {
        self.loaded = true;
        self.recovering = false;
        self.waiting_since = None;
        self.retry_at = None;
        self.failures = 0;
        self.progress = Some((position, now));
    }

    fn observe_progress(&mut self, now: Instant, position: f64, paused: bool) -> bool {
        if paused || !self.loaded || !self.is_network() || self.recovering {
            self.progress = Some((position, now));
            return false;
        }
        let Some((previous, since)) = self.progress else {
            self.progress = Some((position, now));
            return false;
        };
        if (position - previous).abs() >= 0.05 {
            self.progress = Some((position, now));
            false
        } else {
            now.duration_since(since) >= STALL_TIMEOUT
        }
    }

    fn should_recover_end(&mut self, reason: i32, error: i32, state: PlaybackSnapshot) -> bool {
        if !self.loaded || !self.is_network() {
            return false;
        }
        if error < 0 || reason == 4 || self.recovering {
            return true;
        }
        let truncated = reason == 0
            && self.truncated < 3
            && self
                .source
                .as_ref()
                .is_some_and(|source| source.end_seconds.is_none())
            && state.position_seconds > 0.0
            && state
                .duration_seconds
                .is_some_and(|duration| duration - state.position_seconds > 3.0);
        if truncated {
            self.truncated += 1;
        }
        truncated
    }

    fn retry_source(&self, state: PlaybackSnapshot) -> Option<LoadSource> {
        let mut source = self.source.clone()?;
        let mut position = state
            .position_seconds
            .max(source.start_seconds.unwrap_or(0.0));
        if let Some(end) = source.end_seconds {
            position = position.min((end - 0.01).max(0.0));
        }
        source.start_seconds = Some(position);
        source.initially_paused = state.state == PlaybackState::Paused;
        Some(source)
    }
}

trait Engine {
    fn load(&self, source: LoadSource) -> Result<i64, MpvError>;
    fn set_property(&self, name: &str, value: &str) -> Result<(), MpvError>;
    fn command(&self, args: &[&str]) -> Result<(), MpvError>;
    fn wait_event(&self, timeout: f64) -> NativeEvent;
    fn flag(&self, name: &str) -> Option<bool>;
    fn number(&self, name: &str) -> Option<f64>;
}

impl Engine for NativeMpv {
    fn load(&self, source: LoadSource) -> Result<i64, MpvError> {
        NativeMpv::load(self, source)
    }
    fn set_property(&self, name: &str, value: &str) -> Result<(), MpvError> {
        NativeMpv::set_property(self, name, value)
    }
    fn command(&self, args: &[&str]) -> Result<(), MpvError> {
        NativeMpv::command(self, args)
    }
    fn wait_event(&self, timeout: f64) -> NativeEvent {
        NativeMpv::wait_event(self, timeout)
    }
    fn flag(&self, name: &str) -> Option<bool> {
        NativeMpv::flag(self, name)
    }
    fn number(&self, name: &str) -> Option<f64> {
        NativeMpv::number(self, name)
    }
}

fn apply_filter_for_load(engine: &impl Engine, filter: &str, snapshot: &Mutex<PlaybackSnapshot>) {
    if filter.is_empty() {
        return;
    }
    let failed = engine.set_property("af", filter).is_err();
    if failed {
        let _ = engine.set_property("af", "");
    }
    update(snapshot, |state| state.audio_filter_failed = failed);
}

fn run(
    engine: &impl Engine,
    commands: mpsc::Receiver<Message>,
    snapshot: &Mutex<PlaybackSnapshot>,
) {
    let mut last_poll = Instant::now();
    let mut tracker = EventTracker::default();
    let mut recovery = Recovery::default();
    let mut audio_filter = String::new();
    loop {
        while let Ok(message) = commands.try_recv() {
            let shutdown = matches!(message.command, Command::Shutdown);
            let result = match message.command {
                Command::Load(source) => {
                    recovery.reset(None);
                    update(snapshot, |s| s.recovering = false);
                    apply_filter_for_load(engine, &audio_filter, snapshot);
                    let start = source.start_seconds.unwrap_or(0.0);
                    let result = engine.load(source.clone());
                    if let Ok(id) = result {
                        recovery.reset(Some(source));
                        tracker.current = Some(id);
                        update(snapshot, |s| {
                            s.state = PlaybackState::Loading;
                            s.recovering = false;
                            s.position_seconds = start;
                            s.duration_seconds = None;
                            s.last_error = None;
                        });
                    } else if matches!(result, Err(MpvError::MissingEntryId)) {
                        tracker.current = None;
                        update(snapshot, |s| s.state = PlaybackState::Failed);
                    }
                    result.map(|_| ())
                }
                Command::RefreshRecovery(id, source) => {
                    let state = *snapshot.lock().expect("playback snapshot lock poisoned");
                    if !recovery.recovering || !state.recovering || state.recovery_id != id {
                        Err(MpvError::StaleRecovery)
                    } else {
                        let start = source.start_seconds.unwrap_or(0.0);
                        let end = source.end_seconds;
                        recovery.source = Some(source);
                        recovery.retry_at = Some(Instant::now());
                        recovery.waiting_since = None;
                        tracker.current = None;
                        tracker.started = None;
                        update(snapshot, |s| {
                            s.position_seconds = s.position_seconds.max(start);
                            if let Some(end) = end {
                                s.position_seconds =
                                    s.position_seconds.min((end - 0.01).max(start));
                            }
                        });
                        Ok(())
                    }
                }
                Command::Pause(value) => {
                    let result = engine.set_property("pause", if value { "yes" } else { "no" });
                    if result.is_ok() && recovery.recovering {
                        update(snapshot, |s| {
                            s.state = if value {
                                PlaybackState::Paused
                            } else {
                                PlaybackState::Playing
                            }
                        });
                    }
                    recovery.progress = None;
                    result
                }
                Command::Seek(value) => {
                    recovery.progress = None;
                    if recovery.recovering {
                        update(snapshot, |s| s.position_seconds = value);
                        tracker.current = None;
                        tracker.started = None;
                        recovery.retry_at = Some(Instant::now());
                        recovery.waiting_since = None;
                        Ok(())
                    } else {
                        engine.set_property("time-pos", &value.to_string())
                    }
                }
                Command::Volume(value) => engine.set_property("volume", &value.to_string()),
                Command::AudioFilter(value) => {
                    let result = engine.set_property("af", &value);
                    if result.is_ok() {
                        audio_filter = value;
                        update(snapshot, |state| state.audio_filter_failed = false);
                    } else if engine.set_property("af", &audio_filter).is_err() {
                        let _ = engine.set_property("af", "");
                    }
                    result
                }
                Command::AudioFilterWeights(weights, graph) => {
                    let result =
                        engine.command(&["af-command", "bo_musicFx", "weights", &weights, "amix"]);
                    if result.is_ok() {
                        audio_filter = graph;
                    }
                    result
                }
                Command::Stop => {
                    recovery.reset(None);
                    update(snapshot, |s| s.recovering = false);
                    let result = engine.command(&["stop"]);
                    if result.is_ok() {
                        tracker.current = None;
                        update(snapshot, |s| {
                            s.state = PlaybackState::Stopped;
                            s.recovering = false;
                            s.position_seconds = 0.0;
                            s.duration_seconds = None;
                        });
                    }
                    result
                }
                Command::Shutdown => Ok(()),
            };
            let _ = message.reply.send(result);
            if shutdown {
                return;
            }
        }
        let event = engine.wait_event(0.05);
        if matches!(event, NativeEvent::Overflow) {
            let _ = engine.command(&["stop"]);
        }
        if matches!(event, NativeEvent::Loaded)
            && recovery.recovering
            && tracker.current.is_some()
            && tracker.started == tracker.current
        {
            let state = *snapshot.lock().expect("playback snapshot lock poisoned");
            let _ = engine.set_property(
                "pause",
                if state.state == PlaybackState::Paused {
                    "yes"
                } else {
                    "no"
                },
            );
        }
        let paused = if matches!(event, NativeEvent::Loaded) {
            engine.flag("pause").unwrap_or(false)
        } else {
            false
        };
        let now = Instant::now();
        if let NativeEvent::End { id, reason, error } = event {
            let state = *snapshot.lock().expect("playback snapshot lock poisoned");
            if tracker.current == Some(id) && recovery.should_recover_end(reason, error, state) {
                tracker.current = None;
                tracker.started = None;
                if recovery.recovering {
                    recovery.failed_attempt(now);
                    update(snapshot, |s| s.recovery_failures = recovery.failures);
                } else {
                    recovery.begin(now);
                    update(snapshot, |s| {
                        s.recovering = true;
                        s.recovery_id = s.recovery_id.wrapping_add(1);
                        s.recovery_failures = 0;
                    });
                }
                continue;
            }
        }
        if matches!(event, NativeEvent::Loaded)
            && tracker.current.is_some()
            && tracker.started == tracker.current
        {
            let state = *snapshot.lock().expect("playback snapshot lock poisoned");
            recovery.loaded(now, state.position_seconds);
            update(snapshot, |s| s.recovering = false);
        }
        if matches!(event, NativeEvent::Overflow | NativeEvent::Shutdown) {
            recovery.reset(None);
            update(snapshot, |s| s.recovering = false);
        }
        if !tracker.event(event, paused, snapshot) {
            return;
        }
        if recovery.recovering {
            if recovery
                .waiting_since
                .is_some_and(|since| now.duration_since(since) >= RECOVERY_LOAD_TIMEOUT)
            {
                tracker.current = None;
                tracker.started = None;
                recovery.failed_attempt(now);
                update(snapshot, |s| s.recovery_failures = recovery.failures);
            }
            if recovery.retry_at.is_some_and(|at| now >= at) {
                let state = *snapshot.lock().expect("playback snapshot lock poisoned");
                if let Some(source) = recovery.retry_source(state) {
                    apply_filter_for_load(engine, &audio_filter, snapshot);
                    match engine.load(source) {
                        Ok(id) => {
                            tracker.current = Some(id);
                            tracker.started = None;
                            recovery.retry_at = None;
                            recovery.waiting_since = Some(now);
                        }
                        Err(_) => {
                            recovery.failed_attempt(now);
                            update(snapshot, |s| s.recovery_failures = recovery.failures);
                        }
                    }
                }
            }
        }
        if last_poll.elapsed() >= Duration::from_millis(200) {
            last_poll = Instant::now();
            let state = *snapshot.lock().expect("playback snapshot lock poisoned");
            if !state.recovering
                && matches!(state.state, PlaybackState::Playing | PlaybackState::Paused)
            {
                let position = engine
                    .number("time-pos")
                    .filter(|n| n.is_finite() && *n >= 0.0);
                let duration = engine
                    .number("duration")
                    .filter(|n| n.is_finite() && *n > 0.0);
                let paused = engine.flag("pause");
                update(snapshot, |s| {
                    if let Some(position) = position {
                        s.position_seconds = position;
                    }
                    if duration.is_some() {
                        s.duration_seconds = duration;
                    }
                    if let Some(paused) = paused {
                        s.state = if paused {
                            PlaybackState::Paused
                        } else {
                            PlaybackState::Playing
                        };
                    }
                });
                if recovery.observe_progress(
                    now,
                    position.unwrap_or(state.position_seconds),
                    paused.unwrap_or(state.state == PlaybackState::Paused),
                ) {
                    recovery.begin(now);
                    update(snapshot, |s| {
                        s.recovering = true;
                        s.recovery_id = s.recovery_id.wrapping_add(1);
                        s.recovery_failures = 0;
                    });
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "mpv_event_tests.rs"]
mod event_tests;

#[path = "mpv_native.rs"]
mod native;
use native::{NativeEvent, NativeMpv};
#[cfg(all(test, windows))]
#[path = "mpv_tests.rs"]
mod tests;
