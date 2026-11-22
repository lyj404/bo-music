use super::*;

#[test]
fn guarded_queue_actions_reject_account_changes_and_cancel_stale_audio() {
    let (mut app, _, session) = controller();
    let old = app.replace(vec![track(1), track(2)], 0).unwrap().unwrap();
    let account = session.snapshot().revision;
    let revision = app.queue().revision();
    let removal = app
        .edit_queue(account, revision, QueueEdit::Remove(0))
        .unwrap();
    assert!(removal.changed && removal.affects_playback);
    let next = removal.ticket.unwrap();
    assert!(matches!(
        app.apply(resolved(old, false)).unwrap(),
        ApplyOutcome::Stale
    ));
    assert!(matches!(
        app.apply(resolved(next, false)).unwrap(),
        ApplyOutcome::Started
    ));
    let before = app.queue().revision();
    session.set("42".into(), "synthetic".into());
    assert!(matches!(
        app.edit_queue(account, before, QueueEdit::Clear),
        Err(CoordinatorError::SessionChanged)
    ));
    assert_eq!(app.queue().items().len(), 1);
    app.poll().unwrap();
    let new_account = session.snapshot().revision;
    let empty_revision = app.queue().revision();
    let new_ticket = app
        .edit_queue(new_account, empty_revision, QueueEdit::PlayNext(track(7)))
        .unwrap()
        .ticket
        .unwrap();
    let clear_revision = app.queue().revision();
    let cleared = app
        .edit_queue(new_account, clear_revision, QueueEdit::Clear)
        .unwrap();
    assert!(cleared.changed && cleared.affects_playback && cleared.ticket.is_none());
    assert!(app.queue().items().is_empty());
    assert!(matches!(
        app.apply(resolved(new_ticket, false)).unwrap(),
        ApplyOutcome::Stale
    ));
}

#[test]
fn stale_resolution_never_replaces_new_selection_or_account() {
    let (mut app, player, session) = controller();
    let first = app.replace(vec![track(1), track(2)], 0).unwrap().unwrap();
    let second = app.next_track().unwrap().unwrap();
    assert_eq!(
        app.apply(resolved(first, false)).unwrap(),
        ApplyOutcome::Stale
    );
    assert_eq!(
        app.apply(resolved(second, false)).unwrap(),
        ApplyOutcome::Started
    );
    assert_eq!(player.0.borrow().loads[0].0, "https://example.test/2");
    let third = app.select(0).unwrap().unwrap();
    session.set("123".into(), "secret".into());
    assert_eq!(
        app.apply(resolved(third, false)).unwrap(),
        ApplyOutcome::Stale
    );
    assert_eq!(player.0.borrow().loads.len(), 1);
    assert!(app.poll().unwrap().is_none());
    assert!(!app.pending);
}

#[test]
fn eof_advances_once_and_single_loop_replays() {
    let (mut app, player, _) = controller();
    let first = app.replace(vec![track(1), track(2)], 0).unwrap().unwrap();
    app.apply(resolved(first, false)).unwrap();
    player.0.borrow_mut().snapshot.completed_count = 1;
    let second = app.poll().unwrap().unwrap();
    assert_eq!(second.track().id, 2);
    assert!(app.poll().unwrap().is_none());
    app.set_mode(PlayMode::SingleLoop);
    app.apply(resolved(second, false)).unwrap();
    player.0.borrow_mut().snapshot.completed_count = 2;
    let replay = app.poll().unwrap().unwrap();
    assert_eq!(replay.track().id, 2);
}

#[test]
fn removing_current_invalidates_pending_ticket_and_selects_next() {
    let (mut app, player, _) = controller();
    let first = app.append(track(1)).unwrap().unwrap();
    assert!(app.append(track(2)).unwrap().is_none());
    let next = app.remove(0).unwrap().unwrap();
    assert_eq!(next.track().id, 2);
    assert_eq!(
        app.apply(resolved(first, false)).unwrap(),
        ApplyOutcome::Stale
    );
    assert_eq!(
        app.apply(resolved(next, false)).unwrap(),
        ApplyOutcome::Started
    );
    assert_eq!(player.0.borrow().loads.len(), 1);
}

#[test]
fn audition_load_and_seek_are_restricted_to_the_authorized_interval() {
    let (mut app, player, _) = controller();
    let ticket = app.replace(vec![track(1)], 0).unwrap().unwrap();
    assert_eq!(
        app.apply(resolved(ticket, true)).unwrap(),
        ApplyOutcome::Audition
    );
    app.seek(1.0).unwrap();
    app.seek(20.0).unwrap();
    let state = player.0.borrow();
    assert_eq!(state.loads[0].1, Some(5.0));
    assert_eq!(state.loads[0].2, Some(10.0));
    assert_eq!(state.seeks, vec![5.0, 10.0]);
}

#[test]
fn audition_end_does_not_advance_or_loop() {
    let (mut app, player, _) = controller();
    app.set_mode(PlayMode::SingleLoop);
    let ticket = app.replace(vec![track(1), track(2)], 0).unwrap().unwrap();
    app.apply(resolved(ticket, true)).unwrap();
    player.0.borrow_mut().snapshot.completed_count = 1;
    assert!(app.poll().unwrap().is_none());
    assert_eq!(app.queue().current().unwrap().id, 1);
}

#[test]
fn restored_queue_waits_for_play_and_applies_resume_only_to_full_tracks() {
    let (mut app, player, session) = controller();
    app.restore_queue(QueueSnapshot {
        tracks: vec![track(1), track(2)],
        current_index: Some(0),
        position_seconds: 12.0,
    })
    .unwrap();
    assert!(player.0.borrow().loads.is_empty());
    let captured = app.capture_queue().unwrap();
    assert_eq!(captured.scope, "anonymous");
    assert_eq!(captured.snapshot.position_seconds, 12.0);
    let ticket = app.play_current().unwrap().unwrap();
    app.apply(resolved(ticket, false)).unwrap();
    assert_eq!(player.0.borrow().loads[0].1, Some(12.0));
    session.set("42".into(), "secret".into());
    assert!(matches!(
        app.capture_queue(),
        Err(CoordinatorError::SessionChanged)
    ));
    app.poll().unwrap();
    assert!(app.queue().items().is_empty());
}

#[test]
fn restored_near_end_and_audition_do_not_seek_to_saved_position() {
    let (mut app, player, _) = controller();
    app.restore_queue(QueueSnapshot {
        tracks: vec![track(1)],
        current_index: Some(0),
        position_seconds: 25.0,
    })
    .unwrap();
    let ticket = app.play_current().unwrap().unwrap();
    app.apply(resolved(ticket, false)).unwrap();
    assert_eq!(player.0.borrow().loads[0].1, None);
    app.restore_queue(QueueSnapshot {
        tracks: vec![track(1)],
        current_index: Some(0),
        position_seconds: 12.0,
    })
    .unwrap();
    let ticket = app.play_current().unwrap().unwrap();
    app.apply(resolved(ticket, true)).unwrap();
    assert_eq!(player.0.borrow().loads[1].1, Some(5.0));
}

#[test]
fn persistence_uses_the_captured_account_and_restores_without_playing() {
    use bo_music_storage::paths::DataPaths;
    let (mut app, player, session) = controller();
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "bo_music-app-store-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir(&root).unwrap();
    let store = QueueStore::new(DataPaths::new(&root));
    app.restore_queue(QueueSnapshot {
        tracks: vec![track(1)],
        current_index: Some(0),
        position_seconds: 12.0,
    })
    .unwrap();
    app.save_account_queue(&store).unwrap();
    session.set("42".into(), "secret".into());
    assert!(app.save_account_queue(&store).is_err());
    app.restore_account_queue(&store, true).unwrap();
    assert!(app.queue().items().is_empty());
    app.restore_queue(QueueSnapshot {
        tracks: vec![track(2)],
        current_index: Some(0),
        position_seconds: 5.0,
    })
    .unwrap();
    app.save_account_queue(&store).unwrap();
    session.clear();
    app.restore_account_queue(&store, true).unwrap();
    assert_eq!(app.queue().current().unwrap().id, 1);
    assert_eq!(app.capture_queue().unwrap().snapshot.position_seconds, 12.0);
    assert!(player.0.borrow().loads.is_empty());
    std::fs::remove_dir_all(root).unwrap();
}

#[cfg(windows)]
#[test]
fn real_player_advances_two_local_tracks_to_completion() {
    use bo_music_playback::mpv::MpvOptions;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    struct Fixture(std::path::PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&16036_u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&8000_u32.to_le_bytes());
    wav.extend_from_slice(&16000_u32.to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&16000_u32.to_le_bytes());
    wav.resize(16044, 0);
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let file = Fixture(
        std::env::temp_dir().join(format!("bo_music-app-{}-{unique}.wav", std::process::id())),
    );
    std::fs::write(&file.0, wav).unwrap();

    let library =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../libmpv/libmpv-2.dll");
    let player = MpvPlayer::open(
        library,
        MpvOptions {
            audio_output: Some("null".into()),
        },
    )
    .unwrap();
    let transport = Arc::new(
        Transport::new(
            TransportOptions::default(),
            Session::default(),
            "device".into(),
        )
        .unwrap(),
    );
    let mut app = PlaybackCoordinator::new(transport, player);
    let mut ticket = app.replace(vec![track(1), track(2)], 0).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut played = Vec::new();
    loop {
        if let Some(next) = ticket.take() {
            played.push(next.track().id);
            let mut result = resolved(next, false);
            if let Ok(PlaybackResolution::Playable(source)) = &mut result.result {
                source.url = file.0.to_str().unwrap().to_owned();
            }
            app.apply(result).unwrap();
        }
        let state = app.snapshot();
        assert_ne!(
            state.state,
            PlaybackState::Failed,
            "player failed: {:?}",
            state.last_error
        );
        ticket = app.poll().unwrap();
        if played.len() == 2 && state.completed_count == 2 {
            break;
        }
        assert!(Instant::now() < deadline, "timed out: {state:?}");
        std::thread::sleep(Duration::from_millis(25));
    }
    assert_eq!(played, vec![1, 2]);
    drop(app);
}

#[cfg(windows)]
#[test]
fn real_mpv_switches_quality_and_rolls_back_failed_stream() {
    use bo_music_playback::mpv::MpvOptions;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::atomic::{AtomicBool, Ordering},
        thread,
    };
    struct Server {
        stop: Arc<AtomicBool>,
        worker: Option<thread::JoinHandle<()>>,
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = Arc::clone(&stop);
    let worker = thread::spawn(move || {
        let size = 8_u32 * 16000;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + size).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&8000_u32.to_le_bytes());
        wav.extend_from_slice(&16000_u32.to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&size.to_le_bytes());
        wav.resize(size as usize + 44, 0);
        while !stopping.load(Ordering::Relaxed) {
            let (mut socket, _) = match listener.accept() {
                Ok(socket) => socket,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(_) => break,
            };
            let _ = socket.set_nonblocking(false);
            let _ = socket.set_read_timeout(Some(Duration::from_secs(2)));
            let mut request = Vec::new();
            let mut buffer = [0; 2048];
            while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                match socket.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => request.extend_from_slice(&buffer[..count]),
                }
            }
            let request = String::from_utf8_lossy(&request);
            if request.starts_with("GET /missing") {
                let _ = socket.write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
                continue;
            }
            let offset = request
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("range: bytes=")
                        .and_then(|range| range.split('-').next())
                        .and_then(|value| value.parse::<usize>().ok())
                })
                .unwrap_or(0)
                .min(wav.len());
            let body = &wav[offset..];
            let header = format!(
                "HTTP/1.1 206 Partial Content\r\nContent-Type: audio/wav\r\nAccept-Ranges: bytes\r\nContent-Range: bytes {}-{}/{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                offset,
                wav.len() - 1,
                wav.len(),
                body.len()
            );
            let _ = socket.write_all(header.as_bytes());
            let _ = socket.write_all(body);
        }
    });
    let _server = Server {
        stop,
        worker: Some(worker),
    };
    let library =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../libmpv/libmpv-2.dll");
    let player = MpvPlayer::open(
        library,
        MpvOptions {
            audio_output: Some("null".into()),
        },
    )
    .unwrap();
    let transport = Arc::new(
        Transport::new(
            TransportOptions::default(),
            Session::default(),
            "device".into(),
        )
        .unwrap(),
    );
    let mut app = PlaybackCoordinator::new(transport, player);
    let first = app
        .replace(vec![multi_quality_track(1)], 0)
        .unwrap()
        .unwrap();
    let mut initial = resolved(first, false);
    if let Ok(PlaybackResolution::Playable(source)) = &mut initial.result {
        source.url = format!("http://{address}/original.wav");
    }
    app.apply(initial).unwrap();
    let deadline = Instant::now() + Duration::from_secs(12);
    while !app.start_confirmed {
        app.poll().unwrap();
        assert!(
            Instant::now() < deadline,
            "initial load: {:?}",
            app.snapshot()
        );
        thread::sleep(Duration::from_millis(20));
    }
    app.pause(true).unwrap();
    while app.snapshot().state != PlaybackState::Paused {
        app.poll().unwrap();
        assert!(Instant::now() < deadline, "pause: {:?}", app.snapshot());
        thread::sleep(Duration::from_millis(20));
    }
    app.seek(2.0).unwrap();
    while app.snapshot().position_seconds < 1.8 {
        app.poll().unwrap();
        assert!(Instant::now() < deadline, "seek: {:?}", app.snapshot());
        thread::sleep(Duration::from_millis(20));
    }
    app.take_reports();
    app.take_recent_plays();
    let ticket = app.request_quality(AudioQuality::High).unwrap().unwrap();
    let mut result = quality_result(ticket);
    if let Some(Ok(PlaybackResolution::Playable(source))) = &mut result.result {
        source.url = format!("http://{address}/new.wav");
    }
    assert_eq!(app.apply_quality(result).unwrap(), QualityOutcome::Loading);
    loop {
        app.poll().unwrap();
        if let Some(outcome) = app.take_quality_outcome() {
            assert_eq!(
                outcome,
                QualityOutcome::Changed,
                "snapshot: {:?}",
                app.snapshot()
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "quality switch timed out: {:?}",
            app.snapshot()
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(app.snapshot().state, PlaybackState::Paused);
    assert!(app.snapshot().position_seconds >= 1.8);
    let ticket = app
        .request_quality(AudioQuality::Standard)
        .unwrap()
        .unwrap();
    let mut result = quality_result(ticket);
    if let Some(Ok(PlaybackResolution::Playable(source))) = &mut result.result {
        source.url = format!("http://{address}/missing.wav");
    }
    assert_eq!(app.apply_quality(result).unwrap(), QualityOutcome::Loading);
    loop {
        app.poll().unwrap();
        if let Some(outcome) = app.take_quality_outcome() {
            assert_eq!(
                outcome,
                QualityOutcome::Restored,
                "snapshot: {:?}",
                app.snapshot()
            );
            break;
        }
        assert!(
            Instant::now() < deadline,
            "quality rollback timed out: {:?}",
            app.snapshot()
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        app.current_quality().unwrap().requested,
        Some(AudioQuality::High)
    );
    assert_eq!(app.snapshot().state, PlaybackState::Paused);
    assert!(app.take_reports().is_empty());
}
