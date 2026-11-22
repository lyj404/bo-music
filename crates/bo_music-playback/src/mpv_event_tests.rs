use super::*;

#[derive(Default)]
struct FakeState {
    loads: Vec<LoadSource>,
    paused: bool,
    filters: Vec<String>,
}

struct FakeEngine {
    state: Arc<Mutex<FakeState>>,
    events: Mutex<mpsc::Receiver<NativeEvent>>,
}

impl Engine for FakeEngine {
    fn load(&self, source: LoadSource) -> Result<i64, MpvError> {
        let mut state = self.state.lock().unwrap();
        state.paused = source.initially_paused;
        state.loads.push(source);
        Ok(state.loads.len() as i64)
    }
    fn set_property(&self, name: &str, value: &str) -> Result<(), MpvError> {
        if name == "pause" {
            self.state.lock().unwrap().paused = value == "yes";
        } else if name == "af" {
            self.state.lock().unwrap().filters.push(value.to_owned());
        }
        Ok(())
    }
    fn command(&self, _args: &[&str]) -> Result<(), MpvError> {
        Ok(())
    }
    fn wait_event(&self, timeout: f64) -> NativeEvent {
        self.events
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs_f64(timeout))
            .unwrap_or(NativeEvent::None)
    }
    fn flag(&self, _name: &str) -> Option<bool> {
        Some(self.state.lock().unwrap().paused)
    }
    fn number(&self, name: &str) -> Option<f64> {
        match name {
            "time-pos" => Some(24.0),
            "duration" => Some(100.0),
            _ => None,
        }
    }
}

fn wait_until(predicate: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(4);
    while !predicate() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for playback worker"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn worker_reloads_interrupted_stream_and_preserves_pause_without_finishing() {
    let (event_tx, event_rx) = mpsc::channel();
    let (commands, receiver) = mpsc::channel();
    let snapshot = Arc::new(Mutex::new(PlaybackSnapshot::default()));
    let state = Arc::new(Mutex::new(FakeState::default()));
    let engine = FakeEngine {
        state: Arc::clone(&state),
        events: Mutex::new(event_rx),
    };
    let worker_snapshot = Arc::clone(&snapshot);
    let worker = thread::spawn(move || run(&engine, receiver, &worker_snapshot));
    let player = MpvPlayer {
        commands,
        snapshot,
        worker: Some(worker),
    };
    player.load(remote()).unwrap();
    event_tx.send(NativeEvent::Start(1)).unwrap();
    event_tx.send(NativeEvent::Loaded).unwrap();
    wait_until(|| player.snapshot().position_seconds == 24.0);
    event_tx
        .send(NativeEvent::End {
            id: 1,
            reason: 4,
            error: -1,
        })
        .unwrap();
    wait_until(|| state.lock().unwrap().loads.len() == 2);
    assert_eq!(state.lock().unwrap().loads[1].start_seconds, Some(24.0));
    assert!(player.snapshot().recovering);
    player.pause(true).unwrap();
    event_tx.send(NativeEvent::Start(2)).unwrap();
    event_tx.send(NativeEvent::Loaded).unwrap();
    wait_until(|| player.snapshot().loaded_count == 2);
    let recovered = player.snapshot();
    assert_eq!(recovered.state, PlaybackState::Paused);
    assert!(!recovered.recovering);
    assert_eq!(recovered.completed_count, 0);
    player.stop().unwrap();
    assert_eq!(player.snapshot().state, PlaybackState::Stopped);
}

#[test]
fn updated_filter_weights_are_reapplied_to_the_next_track() {
    let (_event_tx, event_rx) = mpsc::channel();
    let (commands, receiver) = mpsc::channel();
    let snapshot = Arc::new(Mutex::new(PlaybackSnapshot::default()));
    let state = Arc::new(Mutex::new(FakeState::default()));
    let engine = FakeEngine {
        state: Arc::clone(&state),
        events: Mutex::new(event_rx),
    };
    let worker_snapshot = Arc::clone(&snapshot);
    let worker = thread::spawn(move || run(&engine, receiver, &worker_snapshot));
    let player = MpvPlayer {
        commands,
        snapshot,
        worker: Some(worker),
    };
    player.audio_filter(Some("old graph")).unwrap();
    player.load(remote()).unwrap();
    player
        .audio_filter_weights("1 0.1", "updated graph")
        .unwrap();
    player.load(remote()).unwrap();
    assert_eq!(
        state.lock().unwrap().filters.last().unwrap(),
        "updated graph"
    );
}

#[test]
fn worker_stop_cancels_a_pending_retry() {
    let (event_tx, event_rx) = mpsc::channel();
    let (commands, receiver) = mpsc::channel();
    let snapshot = Arc::new(Mutex::new(PlaybackSnapshot::default()));
    let state = Arc::new(Mutex::new(FakeState::default()));
    let engine = FakeEngine {
        state: Arc::clone(&state),
        events: Mutex::new(event_rx),
    };
    let worker_snapshot = Arc::clone(&snapshot);
    let worker = thread::spawn(move || run(&engine, receiver, &worker_snapshot));
    let player = MpvPlayer {
        commands,
        snapshot,
        worker: Some(worker),
    };
    player.load(remote()).unwrap();
    event_tx.send(NativeEvent::Start(1)).unwrap();
    event_tx.send(NativeEvent::Loaded).unwrap();
    wait_until(|| player.snapshot().loaded_count == 1);
    event_tx
        .send(NativeEvent::End {
            id: 1,
            reason: 4,
            error: -1,
        })
        .unwrap();
    wait_until(|| state.lock().unwrap().loads.len() == 2);
    event_tx.send(NativeEvent::Start(2)).unwrap();
    event_tx
        .send(NativeEvent::End {
            id: 2,
            reason: 4,
            error: -1,
        })
        .unwrap();
    thread::sleep(Duration::from_millis(150));
    player.stop().unwrap();
    thread::sleep(Duration::from_millis(2200));
    assert_eq!(state.lock().unwrap().loads.len(), 2);
    assert_eq!(player.snapshot().state, PlaybackState::Stopped);
    assert!(!player.snapshot().recovering);
}

#[test]
fn seeking_during_recovery_replaces_inflight_reload_and_ignores_late_events() {
    let (event_tx, event_rx) = mpsc::channel();
    let (commands, receiver) = mpsc::channel();
    let snapshot = Arc::new(Mutex::new(PlaybackSnapshot::default()));
    let state = Arc::new(Mutex::new(FakeState::default()));
    let engine = FakeEngine {
        state: Arc::clone(&state),
        events: Mutex::new(event_rx),
    };
    let worker_snapshot = Arc::clone(&snapshot);
    let worker = thread::spawn(move || run(&engine, receiver, &worker_snapshot));
    let player = MpvPlayer {
        commands,
        snapshot,
        worker: Some(worker),
    };
    player.load(remote()).unwrap();
    event_tx.send(NativeEvent::Start(1)).unwrap();
    event_tx.send(NativeEvent::Loaded).unwrap();
    wait_until(|| player.snapshot().loaded_count == 1);
    event_tx
        .send(NativeEvent::End {
            id: 1,
            reason: 4,
            error: -1,
        })
        .unwrap();
    wait_until(|| state.lock().unwrap().loads.len() == 2);
    player.seek(67.0).unwrap();
    wait_until(|| state.lock().unwrap().loads.len() == 3);
    assert_eq!(state.lock().unwrap().loads[2].start_seconds, Some(67.0));
    event_tx.send(NativeEvent::Start(2)).unwrap();
    event_tx.send(NativeEvent::Loaded).unwrap();
    event_tx
        .send(NativeEvent::End {
            id: 2,
            reason: 4,
            error: -1,
        })
        .unwrap();
    event_tx.send(NativeEvent::Start(3)).unwrap();
    event_tx.send(NativeEvent::Loaded).unwrap();
    wait_until(|| player.snapshot().loaded_count == 2);
    assert!(!player.snapshot().recovering);
    assert_eq!(player.snapshot().completed_count, 0);
}

#[test]
fn refreshed_recovery_is_epoch_bound_and_enforces_new_preview_range() {
    let (event_tx, event_rx) = mpsc::channel();
    let (commands, receiver) = mpsc::channel();
    let snapshot = Arc::new(Mutex::new(PlaybackSnapshot::default()));
    let state = Arc::new(Mutex::new(FakeState::default()));
    let engine = FakeEngine {
        state: Arc::clone(&state),
        events: Mutex::new(event_rx),
    };
    let worker_snapshot = Arc::clone(&snapshot);
    let worker = thread::spawn(move || run(&engine, receiver, &worker_snapshot));
    let player = MpvPlayer {
        commands,
        snapshot,
        worker: Some(worker),
    };
    player.load(remote()).unwrap();
    event_tx.send(NativeEvent::Start(1)).unwrap();
    event_tx.send(NativeEvent::Loaded).unwrap();
    wait_until(|| player.snapshot().position_seconds == 24.0);
    event_tx
        .send(NativeEvent::End {
            id: 1,
            reason: 4,
            error: -1,
        })
        .unwrap();
    wait_until(|| state.lock().unwrap().loads.len() == 2);
    let id = player.snapshot().recovery_id;
    let fresh = || LoadSource {
        address: "https://example.invalid/fresh".into(),
        start_seconds: Some(5.0),
        end_seconds: Some(10.0),
        initially_paused: false,
    };
    assert!(matches!(
        player.refresh_recovery(id.wrapping_add(1), fresh()),
        Err(MpvError::StaleRecovery)
    ));
    player.pause(true).unwrap();
    player.refresh_recovery(id, fresh()).unwrap();
    wait_until(|| state.lock().unwrap().loads.len() == 3);
    {
        let state = state.lock().unwrap();
        let load = &state.loads[2];
        assert_eq!(load.address, "https://example.invalid/fresh");
        assert_eq!(load.end_seconds, Some(10.0));
        assert!(load.start_seconds.unwrap() < 10.0);
        assert!(load.initially_paused);
    }
    event_tx.send(NativeEvent::Start(3)).unwrap();
    event_tx.send(NativeEvent::Loaded).unwrap();
    wait_until(|| !player.snapshot().recovering);
    assert!(matches!(
        player.refresh_recovery(id, fresh()),
        Err(MpvError::StaleRecovery)
    ));
}

fn remote() -> LoadSource {
    LoadSource {
        address: "https://example.invalid/audio".into(),
        start_seconds: None,
        end_seconds: None,
        initially_paused: false,
    }
}

#[test]
fn recovery_requires_a_loaded_network_stream_and_preserves_position_and_pause() {
    let now = Instant::now();
    let mut recovery = Recovery::default();
    recovery.reset(Some(remote()));
    let mut state = PlaybackSnapshot {
        state: PlaybackState::Paused,
        position_seconds: 42.5,
        ..Default::default()
    };
    assert!(!recovery.should_recover_end(4, -1, state));
    recovery.loaded(now, 42.5);
    assert!(recovery.should_recover_end(4, -1, state));
    recovery.begin(now);
    let source = recovery.retry_source(state).unwrap();
    assert_eq!(source.start_seconds, Some(42.5));
    assert!(source.initially_paused);
    state.position_seconds = 75.0;
    assert_eq!(
        recovery.retry_source(state).unwrap().start_seconds,
        Some(75.0)
    );
    recovery.reset(Some(LoadSource {
        address: "local.wav".into(),
        ..remote()
    }));
    recovery.loaded(now, 0.0);
    assert!(!recovery.should_recover_end(4, -1, state));
}

#[test]
fn recovery_retries_with_bounded_backoff_and_stop_cancels_it() {
    let now = Instant::now();
    let mut recovery = Recovery::default();
    recovery.reset(Some(remote()));
    recovery.loaded(now, 1.0);
    recovery.begin(now);
    for delay in [2, 4, 8, 16, 30, 30] {
        recovery.failed_attempt(now);
        assert_eq!(recovery.retry_at, Some(now + Duration::from_secs(delay)));
    }
    recovery.reset(None);
    assert!(recovery.retry_source(PlaybackSnapshot::default()).is_none());
    assert!(!recovery.recovering);
    assert!(recovery.retry_at.is_none());
}

#[test]
fn stalled_stream_and_truncated_ends_are_bounded() {
    let now = Instant::now();
    let mut recovery = Recovery::default();
    recovery.reset(Some(remote()));
    recovery.loaded(now, 20.0);
    assert!(!recovery.observe_progress(now + Duration::from_secs(9), 20.0, false));
    assert!(recovery.observe_progress(now + Duration::from_secs(10), 20.0, false));
    assert!(!recovery.observe_progress(now + Duration::from_secs(11), 20.0, true));
    assert!(!recovery.observe_progress(now + Duration::from_secs(12), 21.0, false));
    let state = PlaybackSnapshot {
        position_seconds: 20.0,
        duration_seconds: Some(100.0),
        ..Default::default()
    };
    for _ in 0..3 {
        assert!(recovery.should_recover_end(0, 0, state));
    }
    assert!(!recovery.should_recover_end(0, 0, state));
    recovery.reset(Some(LoadSource {
        end_seconds: Some(30.0),
        ..remote()
    }));
    recovery.loaded(now, 20.0);
    assert!(!recovery.should_recover_end(0, 0, state));
    let retry = recovery
        .retry_source(PlaybackSnapshot {
            position_seconds: 35.0,
            ..state
        })
        .unwrap();
    assert!(retry.start_seconds.unwrap() < 30.0);
}

#[test]
fn late_events_from_replaced_or_stopped_entries_are_ignored() {
    let snapshot = Mutex::new(PlaybackSnapshot {
        state: PlaybackState::Loading,
        ..Default::default()
    });
    let mut tracker = EventTracker {
        current: Some(2),
        started: None,
    };
    tracker.event(NativeEvent::Start(1), false, &snapshot);
    tracker.event(NativeEvent::Loaded, false, &snapshot);
    tracker.event(
        NativeEvent::End {
            id: 1,
            reason: 4,
            error: -1,
        },
        false,
        &snapshot,
    );
    assert_eq!(snapshot.lock().unwrap().state, PlaybackState::Loading);
    assert_eq!(snapshot.lock().unwrap().loaded_count, 0);
    tracker.event(NativeEvent::Start(2), false, &snapshot);
    tracker.event(NativeEvent::Loaded, true, &snapshot);
    assert_eq!(snapshot.lock().unwrap().state, PlaybackState::Paused);
    assert_eq!(snapshot.lock().unwrap().loaded_count, 1);
    tracker.current = None;
    tracker.event(
        NativeEvent::End {
            id: 2,
            reason: 0,
            error: 0,
        },
        false,
        &snapshot,
    );
    assert_eq!(snapshot.lock().unwrap().completed_count, 0);
}

#[test]
fn current_eof_is_counted_once_and_overflow_fails_closed() {
    let snapshot = Mutex::new(PlaybackSnapshot::default());
    let mut tracker = EventTracker {
        current: Some(3),
        started: Some(3),
    };
    tracker.event(
        NativeEvent::End {
            id: 3,
            reason: 0,
            error: 0,
        },
        false,
        &snapshot,
    );
    tracker.event(
        NativeEvent::End {
            id: 3,
            reason: 0,
            error: 0,
        },
        false,
        &snapshot,
    );
    assert_eq!(snapshot.lock().unwrap().completed_count, 1);
    tracker.current = Some(4);
    tracker.event(NativeEvent::Overflow, false, &snapshot);
    assert_eq!(snapshot.lock().unwrap().state, PlaybackState::Failed);
    assert!(tracker.current.is_none());
}
