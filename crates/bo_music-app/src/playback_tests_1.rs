use super::*;

#[test]
fn quality_switch_preserves_position_pause_history_and_default() {
    let (mut app, player, _) = controller();
    app.set_quality(Some(AudioQuality::Lossless));
    let first = app
        .replace(vec![multi_quality_track(1), track(2)], 0)
        .unwrap()
        .unwrap();
    app.apply(resolved(first, false)).unwrap();
    app.take_reports();
    app.take_recent_plays();
    {
        let mut state = player.0.borrow_mut();
        state.snapshot.state = PlaybackState::Paused;
        state.snapshot.position_seconds = 42.5;
        state.snapshot.duration_seconds = Some(100.0);
    }
    let ticket = app.request_quality(AudioQuality::High).unwrap().unwrap();
    assert_eq!(player.0.borrow().loads.len(), 1);
    player.0.borrow_mut().defer_load = true;
    assert_eq!(
        app.apply_quality(quality_result(ticket)).unwrap(),
        QualityOutcome::Loading
    );
    assert_eq!(player.0.borrow().loads[1].1, Some(42.5));
    assert!(player.0.borrow().load_paused[1]);
    assert_eq!(app.snapshot().position_seconds, 42.5);
    assert_eq!(app.snapshot().state, PlaybackState::Paused);
    app.seek(47.0).unwrap();
    app.pause(false).unwrap();
    {
        let mut state = player.0.borrow_mut();
        state.snapshot.loaded_count += 1;
        state.snapshot.state = PlaybackState::Paused;
    }
    app.poll().unwrap();
    assert_eq!(app.take_quality_outcome(), Some(QualityOutcome::Changed));
    assert_eq!(
        app.current_quality().unwrap().requested,
        Some(AudioQuality::High)
    );
    assert_eq!(app.preferred_quality(), AudioQuality::Lossless);
    assert_eq!(player.0.borrow().seeks, vec![47.0]);
    assert_eq!(player.0.borrow().pauses, vec![false]);
    assert!(app.take_reports().is_empty());
    assert!(app.take_recent_plays().is_empty());
    assert_eq!(app.queue().current_index(), Some(0));
}

#[test]
fn failed_quality_load_restores_original_source_at_latest_position() {
    let (mut app, player, _) = controller();
    let first = app
        .replace(vec![multi_quality_track(1)], 0)
        .unwrap()
        .unwrap();
    app.apply(resolved(first, false)).unwrap();
    app.take_reports();
    player.0.borrow_mut().snapshot.position_seconds = 42.0;
    let ticket = app.request_quality(AudioQuality::High).unwrap().unwrap();
    player.0.borrow_mut().defer_load = true;
    assert_eq!(
        app.apply_quality(quality_result(ticket)).unwrap(),
        QualityOutcome::Loading
    );
    app.seek(50.0).unwrap();
    app.pause(true).unwrap();
    player.0.borrow_mut().snapshot.state = PlaybackState::Failed;
    app.poll().unwrap();
    assert_eq!(player.0.borrow().loads[2].0, "https://example.test/1");
    assert_eq!(player.0.borrow().loads[2].1, Some(50.0));
    assert!(player.0.borrow().load_paused[2]);
    {
        let mut state = player.0.borrow_mut();
        state.snapshot.loaded_count += 1;
        state.snapshot.state = PlaybackState::Paused;
    }
    app.poll().unwrap();
    assert_eq!(app.take_quality_outcome(), Some(QualityOutcome::Restored));
    assert_eq!(app.current_quality().unwrap().requested, None);
    assert!(app.take_reports().is_empty());
}

#[test]
fn quality_results_cannot_replace_newer_requests_tracks_or_accounts() {
    let (mut app, player, session) = controller();
    let first = app
        .replace(vec![multi_quality_track(1), multi_quality_track(2)], 0)
        .unwrap()
        .unwrap();
    app.apply(resolved(first, false)).unwrap();
    let old = quality_result(app.request_quality(AudioQuality::High).unwrap().unwrap());
    let newer = quality_result(
        app.request_quality(AudioQuality::Standard)
            .unwrap()
            .unwrap(),
    );
    assert_eq!(app.apply_quality(old).unwrap(), QualityOutcome::Stale);
    let next = app.next_track().unwrap().unwrap();
    app.apply(resolved(next, false)).unwrap();
    assert_eq!(app.apply_quality(newer).unwrap(), QualityOutcome::Stale);
    let old_account = quality_result(app.request_quality(AudioQuality::High).unwrap().unwrap());
    session.set("account".into(), "synthetic".into());
    assert_eq!(
        app.apply_quality(old_account).unwrap(),
        QualityOutcome::Stale
    );
    assert_eq!(player.0.borrow().loads.len(), 2);
}

#[test]
fn choosing_current_quality_cancels_a_pending_switch() {
    let (mut app, player, _) = controller();
    let first = app
        .replace(vec![multi_quality_track(1)], 0)
        .unwrap()
        .unwrap();
    let mut initial = resolved(first, false);
    if let Ok(PlaybackResolution::Playable(source)) = &mut initial.result {
        source.requested_quality = Some(AudioQuality::Lossless);
    }
    app.apply(initial).unwrap();
    let old = quality_result(app.request_quality(AudioQuality::High).unwrap().unwrap());
    assert!(
        app.request_quality(AudioQuality::Lossless)
            .unwrap()
            .is_none()
    );
    assert_eq!(app.apply_quality(old).unwrap(), QualityOutcome::Stale);
    assert_eq!(player.0.borrow().loads.len(), 1);
}

#[test]
fn audition_and_denied_quality_change_leave_original_audio_playing() {
    let (mut app, player, _) = controller();
    let first = app
        .replace(vec![multi_quality_track(1)], 0)
        .unwrap()
        .unwrap();
    app.apply(resolved(first, true)).unwrap();
    assert!(app.request_quality(AudioQuality::High).unwrap().is_none());
    let next = app.play_current().unwrap().unwrap();
    app.apply(resolved(next, false)).unwrap();
    let mut result = quality_result(app.request_quality(AudioQuality::High).unwrap().unwrap());
    result.result = Some(Ok(PlaybackResolution::Denied(Denial::NoPermission)));
    assert_eq!(
        app.apply_quality(result).unwrap(),
        QualityOutcome::Unavailable
    );
    assert_eq!(player.0.borrow().loads.len(), 2);
}

#[test]
fn reauthorization_is_uncached_bounded_and_applies_new_audition_rights() {
    let (mut app, player, _) = controller();
    let ticket = app.replace(vec![track(1)], 0).unwrap().unwrap();
    app.apply(resolved(ticket, false)).unwrap();
    app.take_reports();
    {
        let mut state = player.0.borrow_mut();
        state.snapshot.recovering = true;
        state.snapshot.recovery_id = 3;
        state.snapshot.recovery_failures = 1;
        state.snapshot.position_seconds = 7.0;
    }
    assert!(app.recovery_ticket().is_none());
    player.0.borrow_mut().snapshot.recovery_failures = 2;
    let ticket = app.recovery_ticket().unwrap();
    assert!(app.recovery_ticket().is_none());
    assert_eq!(
        app.apply_recovery(recovery_result(ticket, true)).unwrap(),
        RecoveryOutcome::Refreshed
    );
    assert_eq!(
        player.0.borrow().refreshes[0],
        (3, "https://example.test/1".into(), Some(5.0), Some(10.0))
    );
    assert_eq!(app.audition, Some((5.0, 10.0)));
    assert!(app.take_reports().is_empty());
    assert!(app.recovery_ticket().is_none());
    player.0.borrow_mut().snapshot.recovery_failures = 4;
    let mut failed = recovery_result(app.recovery_ticket().unwrap(), false);
    failed.result = None;
    assert_eq!(
        app.apply_recovery(failed).unwrap(),
        RecoveryOutcome::Unavailable
    );
    assert!(app.recovery_ticket().is_none());
    player.0.borrow_mut().snapshot.recovery_failures = 6;
    assert!(app.recovery_ticket().is_some());
}

#[test]
fn reauthorization_discards_old_track_account_and_recovery_responses() {
    let (mut app, player, session) = controller();
    let ticket = app.replace(vec![track(1), track(2)], 0).unwrap().unwrap();
    app.apply(resolved(ticket, false)).unwrap();
    {
        let mut state = player.0.borrow_mut();
        state.snapshot.recovering = true;
        state.snapshot.recovery_id = 1;
        state.snapshot.recovery_failures = 2;
    }
    let old = recovery_result(app.recovery_ticket().unwrap(), false);
    let next = app.next_track().unwrap().unwrap();
    app.apply(resolved(next, false)).unwrap();
    assert_eq!(app.apply_recovery(old).unwrap(), RecoveryOutcome::Stale);
    player.0.borrow_mut().snapshot.recovery_id = 2;
    let old_epoch = recovery_result(app.recovery_ticket().unwrap(), false);
    player.0.borrow_mut().snapshot.recovery_id = 3;
    let next_epoch = recovery_result(app.recovery_ticket().unwrap(), false);
    assert_eq!(
        app.apply_recovery(old_epoch).unwrap(),
        RecoveryOutcome::Stale
    );
    session.set("new-account".into(), "synthetic".into());
    assert_eq!(
        app.apply_recovery(next_epoch).unwrap(),
        RecoveryOutcome::Stale
    );
    assert!(player.0.borrow().refreshes.is_empty());
}

#[test]
fn reauthorization_denial_stops_playback() {
    let (mut app, player, _) = controller();
    let ticket = app.replace(vec![track(1)], 0).unwrap().unwrap();
    app.apply(resolved(ticket, false)).unwrap();
    {
        let mut state = player.0.borrow_mut();
        state.snapshot.recovering = true;
        state.snapshot.recovery_id = 1;
        state.snapshot.recovery_failures = 2;
    }
    let mut denied = recovery_result(app.recovery_ticket().unwrap(), false);
    denied.result = Some(Ok(PlaybackResolution::Denied(Denial::NotAuthenticated)));
    let stops = player.0.borrow().stops;
    assert_eq!(
        app.apply_recovery(denied).unwrap(),
        RecoveryOutcome::Denied(Denial::NotAuthenticated)
    );
    assert_eq!(player.0.borrow().stops, stops + 1);
    assert!(!app.active);
}

#[tokio::test]
async fn recovery_ticket_requests_fresh_authorization_over_http() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = [0; 4096];
        let count = socket.read(&mut request).unwrap();
        assert!(request[..count].starts_with(b"GET /api/play/music/v2/checkRight?"));
        let body = r#"{"code":200,"data":{"status":7}}"#;
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });
    let session = Session::default();
    let transport = Arc::new(
        Transport::new(
            TransportOptions {
                base_url: format!("http://{address}/api/"),
                ..Default::default()
            },
            session,
            "device".into(),
        )
        .unwrap(),
    );
    let player = MockPlayer::default();
    let mut app = PlaybackCoordinator::new(transport, player.clone());
    let initial = app.replace(vec![track(1)], 0).unwrap().unwrap();
    app.apply(resolved(initial, false)).unwrap();
    {
        let mut state = player.0.borrow_mut();
        state.snapshot.recovering = true;
        state.snapshot.recovery_id = 1;
        state.snapshot.recovery_failures = 2;
    }
    let result = app.recovery_ticket().unwrap().resolve().await;
    assert_eq!(
        app.apply_recovery(result).unwrap(),
        RecoveryOutcome::Denied(Denial::NotAuthenticated)
    );
    server.join().unwrap();
}

#[tokio::test]
async fn next_track_reuses_the_inflight_prefetch() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut request = [0; 4096];
        let count = socket.read(&mut request).unwrap();
        assert!(request[..count].starts_with(b"GET /api/play/music/v2/checkRight?"));
        started_tx.send(()).unwrap();
        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let body = r#"{"code":200,"data":{"status":7}}"#;
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
        listener.set_nonblocking(true).unwrap();
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
    });
    let session = Session::default();
    let transport = Arc::new(
        Transport::new(
            TransportOptions {
                base_url: format!("http://{address}/api/"),
                ..Default::default()
            },
            session,
            "device".into(),
        )
        .unwrap(),
    );
    let mut app = PlaybackCoordinator::new(transport, MockPlayer::default());
    let first = app.replace(vec![track(1), track(2)], 0).unwrap().unwrap();
    app.apply(resolved(first, false)).unwrap();
    let mut prefetch = Prefetcher::default();
    prefetch.reconcile(app.prefetch_candidate());
    tokio::time::timeout(Duration::from_secs(5), started_rx)
        .await
        .unwrap()
        .unwrap();
    let ticket = app.next_track().unwrap().unwrap();
    let cached = prefetch.take_for(&ticket);
    assert!(cached.is_some());
    let task = tokio::spawn(ticket.resolve_with_prefetch(cached));
    release_tx.send(()).unwrap();
    let mut result = tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap();
    assert!(result.from_prefetch);
    prefetch.remember(&mut result, app.session.snapshot().revision);
    assert_eq!(prefetch.cache.len(), 1);
    assert!(matches!(
        app.apply(result).unwrap(),
        ApplyOutcome::Denied(Denial::NotAuthenticated)
    ));
    let replay = app.play_current().unwrap().unwrap();
    let hit = prefetch.take_for(&replay);
    assert!(matches!(hit, Some(PrefetchHit::Ready { .. })));
    let replayed = replay.resolve_with_prefetch(hit).await;
    assert!(replayed.from_prefetch);
    assert!(matches!(
        app.apply(replayed).unwrap(),
        ApplyOutcome::Denied(Denial::NotAuthenticated)
    ));
    server.join().unwrap();
}

#[tokio::test]
async fn cache_retains_eight_results_expires_and_evicts_failed_sources() {
    let (app, _, session) = controller();
    let mut prefetch = Prefetcher::default();
    // Build a synthetic timeline forwards; subtracting from Instant::now() can
    // underflow on a freshly booted Windows runner.
    let started = Instant::now();
    for id in 1..=9 {
        let ticket = ResolutionTicket {
            generation: 0,
            revision: session.snapshot().revision,
            track: track(id),
            quality: None,
            transport: Arc::clone(&app.transport),
        };
        let key = ticket.key();
        let result = resolved(ticket, false);
        let task = tokio::spawn(async move { result });
        prefetch.entry = Some(PrefetchEntry {
            key,
            started: started + Duration::from_secs(id as u64),
            task,
        });
        while !prefetch.entry.as_ref().unwrap().task.is_finished() {
            tokio::task::yield_now().await;
        }
        prefetch.reconcile(None);
    }
    assert_eq!(prefetch.cache.len(), PREFETCH_CAPACITY);
    assert!(!prefetch.cache.iter().any(|entry| entry.key.track.id == 1));
    let ticket = ResolutionTicket {
        generation: 3,
        revision: session.snapshot().revision,
        track: track(3),
        quality: None,
        transport: Arc::clone(&app.transport),
    };
    let hit = prefetch.take_for(&ticket);
    assert!(matches!(hit, Some(PrefetchHit::Ready { .. })));
    assert!(ticket.resolve_with_prefetch(hit).await.from_prefetch);
    assert_eq!(prefetch.cache.len(), PREFETCH_CAPACITY);

    let expired = prefetch
        .cache
        .iter_mut()
        .find(|entry| entry.key.track.id == 4)
        .unwrap();
    expired.started = started;
    let expires_at = started + PREFETCH_TTL;
    prefetch.prune_at(expires_at);
    assert!(prefetch.cache.iter().any(|entry| entry.key.track.id == 4));
    prefetch.prune_at(expires_at + Duration::from_secs(1));
    assert!(!prefetch.cache.iter().any(|entry| entry.key.track.id == 4));
    assert_eq!(prefetch.cache.len(), PREFETCH_CAPACITY - 1);
    assert!(prefetch.cache.iter().any(|entry| entry.key.track.id == 3));
    let retry = ResolutionTicket {
        generation: 4,
        revision: session.snapshot().revision,
        track: track(3),
        quality: None,
        transport: Arc::clone(&app.transport),
    };
    prefetch.forget_for(&retry);
    assert!(prefetch.take_for(&retry).is_none());
    session.set("42".into(), "synthetic".into());
    prefetch.retain_revision(session.snapshot().revision);
    assert!(prefetch.cache.is_empty());
    let stale_ticket = ResolutionTicket {
        generation: 5,
        revision: session.snapshot().revision - 1,
        track: track(8),
        quality: None,
        transport: Arc::clone(&app.transport),
    };
    let stale_key = stale_ticket.key();
    let mut late_result = resolved(stale_ticket, false);
    late_result.cache_entry = Some(Box::new(CachedEntry {
        key: stale_key,
        started: Instant::now(),
        result: Some(late_result.result.as_ref().unwrap().clone()),
    }));
    prefetch.remember(&mut late_result, session.snapshot().revision);
    assert!(prefetch.cache.is_empty());
}

#[tokio::test]
async fn failed_prefetch_falls_back_to_fresh_resolution() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for body in [r#"{"code":500}"#, r#"{"code":200,"data":{"status":7}}"#] {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let count = socket.read(&mut request).unwrap();
            assert!(request[..count].starts_with(b"GET /api/play/music/v2/checkRight?"));
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
    });
    let transport = Arc::new(
        Transport::new(
            TransportOptions {
                base_url: format!("http://{address}/api/"),
                ..Default::default()
            },
            Session::default(),
            "device".into(),
        )
        .unwrap(),
    );
    let mut app = PlaybackCoordinator::new(transport, MockPlayer::default());
    let first = app.replace(vec![track(1), track(2)], 0).unwrap().unwrap();
    app.apply(resolved(first, false)).unwrap();
    let mut prefetch = Prefetcher::default();
    prefetch.reconcile(app.prefetch_candidate());
    tokio::time::timeout(Duration::from_secs(5), async {
        while !prefetch.entry.as_ref().unwrap().task.is_finished() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let ticket = app.next_track().unwrap().unwrap();
    let entry = prefetch.take_for(&ticket);
    let result = tokio::time::timeout(Duration::from_secs(5), ticket.resolve_with_prefetch(entry))
        .await
        .unwrap();
    assert!(!result.from_prefetch);
    assert!(matches!(
        app.apply(result).unwrap(),
        ApplyOutcome::Denied(Denial::NotAuthenticated)
    ));
    server.join().unwrap();
}

#[tokio::test]
async fn changed_quality_or_queue_target_cancels_prefetch() {
    let (mut app, _, _) = controller();
    let ticket = app
        .replace(vec![track(1), track(2), track(3)], 0)
        .unwrap()
        .unwrap();
    app.apply(resolved(ticket, false)).unwrap();
    let candidate = app.prefetch_candidate().unwrap();
    let task = tokio::spawn(std::future::pending::<ResolvedTicket>());
    let abort = task.abort_handle();
    let mut prefetch = Prefetcher {
        entry: Some(PrefetchEntry {
            key: candidate.key(),
            started: Instant::now(),
            task,
        }),
        ..Default::default()
    };
    app.set_quality(Some(AudioQuality::High));
    let next = app.next_track().unwrap().unwrap();
    assert!(prefetch.take_for(&next).is_none());
    tokio::task::yield_now().await;
    assert!(abort.is_finished());

    app.apply(resolved(next, false)).unwrap();
    let candidate = app.prefetch_candidate().unwrap();
    assert_eq!(candidate.track().id, 3);
    let task = tokio::spawn(std::future::pending::<ResolvedTicket>());
    let abort = task.abort_handle();
    prefetch.entry = Some(PrefetchEntry {
        key: candidate.key(),
        started: Instant::now(),
        task,
    });
    let other = app.select(0).unwrap().unwrap();
    assert!(prefetch.take_for(&other).is_none());
    tokio::task::yield_now().await;
    assert!(abort.is_finished());
}

#[tokio::test]
async fn expired_or_wrong_account_prefetch_is_cancelled() {
    let (mut app, _, session) = controller();
    let first = app.replace(vec![track(1), track(2)], 0).unwrap().unwrap();
    app.apply(resolved(first, false)).unwrap();
    let candidate = app.prefetch_candidate().unwrap();
    let key = candidate.key();
    let task = tokio::spawn(std::future::pending::<ResolvedTicket>());
    let abort = task.abort_handle();
    let started = Instant::now();
    let mut prefetch = Prefetcher {
        entry: Some(PrefetchEntry { key, started, task }),
        ..Default::default()
    };
    let ticket = app.next_track().unwrap().unwrap();
    assert!(
        prefetch
            .take_for_at(&ticket, started + PREFETCH_TTL + Duration::from_secs(1))
            .is_none()
    );
    tokio::task::yield_now().await;
    assert!(abort.is_finished());

    app.apply(resolved(ticket, false)).unwrap();
    let ticket = app.play_current().unwrap().unwrap();
    let key = ticket.key();
    let task = tokio::spawn(std::future::pending::<ResolvedTicket>());
    let abort = task.abort_handle();
    prefetch.entry = Some(PrefetchEntry {
        key,
        started: Instant::now(),
        task,
    });
    session.set("42".into(), "synthetic".into());
    app.poll().unwrap();
    assert!(app.prefetch_candidate().is_none());
    prefetch.reconcile(app.prefetch_candidate());
    tokio::task::yield_now().await;
    assert!(abort.is_finished());
}

#[test]
fn cached_load_failure_retries_only_once_with_a_fresh_ticket() {
    let (mut app, player, _) = controller();
    player.0.borrow_mut().defer_load = true;
    let ticket = app.replace(vec![track(1)], 0).unwrap().unwrap();
    let mut cached = resolved(ticket, false);
    cached.from_prefetch = true;
    app.apply(cached).unwrap();
    player.0.borrow_mut().snapshot.state = PlaybackState::Failed;
    let retry = app
        .poll()
        .unwrap()
        .expect("cached URL should be resolved again");
    assert!(app.is_resolving());
    assert!(app.take_reports().is_empty());
    app.apply(resolved(retry, false)).unwrap();
    player.0.borrow_mut().snapshot.state = PlaybackState::Failed;
    assert!(app.poll().unwrap().is_none());
    assert!(app.take_reports().is_empty());

    let ticket = app.play_current().unwrap().unwrap();
    let mut cached = resolved(ticket, false);
    cached.from_prefetch = true;
    player.0.borrow_mut().fail_load = true;
    assert_eq!(app.apply(cached).unwrap(), ApplyOutcome::RetryFresh);
}

#[test]
fn history_waits_for_loaded_event_and_handles_fast_completion() {
    let (mut app, player, session) = controller();
    player.0.borrow_mut().defer_load = true;
    let origin = PlaybackOrigin {
        kind: CollectionKind::Album,
        id: 20,
        source_type: 6,
    };
    let revision = session.snapshot().revision;
    let ticket = app
        .replace_with_origin(vec![track(1)], 0, Some(origin))
        .unwrap()
        .unwrap();
    app.apply(resolved(ticket, false)).unwrap();
    assert!(app.take_reports().is_empty());
    assert!(app.take_recent_plays().is_empty());
    player.0.borrow_mut().snapshot.state = PlaybackState::Failed;
    app.poll().unwrap();
    assert!(app.take_reports().is_empty());
    assert!(app.take_recent_plays().is_empty());

    let ticket = app.play_current().unwrap().unwrap();
    app.apply(resolved(ticket, false)).unwrap();
    {
        let mut state = player.0.borrow_mut();
        state.snapshot.loaded_count += 1;
        state.snapshot.completed_count += 1;
        state.snapshot.state = PlaybackState::Stopped;
    }
    app.poll().unwrap();
    assert_eq!(
        app.take_reports(),
        vec![
            PlaybackReport::Started {
                music_id: 1,
                revision
            },
            PlaybackReport::Collection { origin, revision },
            PlaybackReport::Ended {
                music_id: 1,
                duration: 30,
                position: 30,
                completed: true,
                revision
            },
        ]
    );
    assert_eq!(app.take_recent_plays()[0].0.id, 1);
    app.poll().unwrap();
    assert!(app.take_reports().is_empty());
}

#[test]
fn collection_origin_is_reported_once_and_never_leaks_across_accounts() {
    let (mut app, _, session) = controller();
    let playlist = PlaybackOrigin {
        kind: CollectionKind::Playlist,
        id: 10,
        source_type: 13,
    };
    let album = PlaybackOrigin {
        kind: CollectionKind::Album,
        id: 20,
        source_type: 6,
    };
    let revision = session.snapshot().revision;
    let ticket = app
        .replace_with_origin(vec![track(1), track(2)], 0, Some(playlist))
        .unwrap()
        .unwrap();
    app.apply(resolved(ticket, false)).unwrap();
    let events = app.take_reports();
    assert!(events.contains(&PlaybackReport::Collection {
        origin: playlist,
        revision
    }));
    let next = app.next_track().unwrap().unwrap();
    app.take_reports();
    app.apply(resolved(next, false)).unwrap();
    assert!(
        !app.take_reports()
            .iter()
            .any(|event| matches!(event, PlaybackReport::Collection { .. }))
    );
    let ticket = app
        .enqueue_and_play_with_origin(track(3), Some(album))
        .unwrap()
        .unwrap();
    app.take_reports();
    app.apply(resolved(ticket, false)).unwrap();
    assert!(app.take_reports().contains(&PlaybackReport::Collection {
        origin: album,
        revision
    }));
    let ticket = app.replace(vec![track(4)], 0).unwrap().unwrap();
    app.take_reports();
    app.apply(resolved(ticket, false)).unwrap();
    assert!(
        !app.take_reports()
            .iter()
            .any(|event| matches!(event, PlaybackReport::Collection { .. }))
    );
    let ticket = app
        .replace_with_origin(vec![track(5)], 0, Some(album))
        .unwrap()
        .unwrap();
    app.take_reports();
    app.apply(resolved(ticket, false)).unwrap();
    assert!(app.take_reports().contains(&PlaybackReport::Collection {
        origin: album,
        revision
    }));
    session.set("42".into(), "synthetic".into());
    app.poll().unwrap();
    app.take_reports();
    let revision = session.snapshot().revision;
    let ticket = app
        .replace_with_origin(vec![track(6)], 0, Some(album))
        .unwrap()
        .unwrap();
    app.apply(resolved(ticket, false)).unwrap();
    assert!(app.take_reports().contains(&PlaybackReport::Collection {
        origin: album,
        revision
    }));
}

#[test]
fn reports_started_switched_completed_and_audition_once() {
    let (mut app, player, session) = controller();
    let revision = session.snapshot().revision;
    let first = app.replace(vec![track(1), track(2)], 0).unwrap().unwrap();
    app.apply(resolved(first, false)).unwrap();
    assert_eq!(
        app.take_reports(),
        vec![PlaybackReport::Started {
            music_id: 1,
            revision
        }]
    );
    player.0.borrow_mut().snapshot.position_seconds = 8.5;
    let second = app.next_track().unwrap().unwrap();
    assert_eq!(
        app.take_reports(),
        vec![PlaybackReport::Ended {
            music_id: 1,
            duration: 30,
            position: 8,
            completed: false,
            revision
        }]
    );
    app.apply(resolved(second, false)).unwrap();
    assert_eq!(
        app.take_reports(),
        vec![PlaybackReport::Started {
            music_id: 2,
            revision
        }]
    );
    player.0.borrow_mut().snapshot.completed_count += 1;
    app.poll().unwrap();
    assert_eq!(
        app.take_reports(),
        vec![PlaybackReport::Ended {
            music_id: 2,
            duration: 30,
            position: 30,
            completed: true,
            revision
        }]
    );
    app.poll().unwrap();
    assert!(app.take_reports().is_empty());
    let preview = app.play_current().unwrap().unwrap();
    app.apply(resolved(preview, true)).unwrap();
    app.take_reports();
    {
        let mut state = player.0.borrow_mut();
        state.snapshot.position_seconds = 10.0;
        state.snapshot.completed_count += 1;
    }
    app.poll().unwrap();
    assert_eq!(
        app.take_reports(),
        vec![PlaybackReport::Ended {
            music_id: 2,
            duration: 30,
            position: 10,
            completed: false,
            revision
        }]
    );
}

#[test]
fn enqueue_and_play_preserves_queue_and_reuses_existing_tracks() {
    let (mut app, _, _) = controller();
    app.replace(vec![track(1), track(2)], 0).unwrap();
    let third = app.enqueue_and_play(track(3)).unwrap().unwrap();
    assert_eq!(
        app.queue()
            .items()
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        vec![1, 3, 2]
    );
    assert_eq!(app.queue().current().unwrap().id, 3);
    let second = app.enqueue_and_play(track(2)).unwrap().unwrap();
    assert_eq!(app.queue().items().len(), 3);
    assert_eq!(app.queue().current().unwrap().id, 2);
    assert!(matches!(
        app.apply(resolved(third, false)).unwrap(),
        ApplyOutcome::Stale
    ));
    assert!(matches!(
        app.apply(resolved(second, false)).unwrap(),
        ApplyOutcome::Started
    ));
}

#[test]
fn guarded_batch_queue_addition_deduplicates_and_starts_only_when_empty() {
    let (mut app, _, session) = controller();
    let account = session.snapshot().revision;
    let revision = app.queue().revision();
    let outcome = app
        .edit_queue(
            account,
            revision,
            QueueEdit::AppendMany(vec![track(1), track(1), track(2)]),
        )
        .unwrap();
    assert!(outcome.changed && outcome.affects_playback && outcome.ticket.is_some());
    assert_eq!(
        app.queue()
            .items()
            .iter()
            .map(|track| track.id)
            .collect::<Vec<_>>(),
        [1, 2]
    );
    let revision = app.queue().revision();
    let outcome = app
        .edit_queue(
            account,
            revision,
            QueueEdit::AppendMany(vec![track(2), track(3)]),
        )
        .unwrap();
    assert!(outcome.changed && !outcome.affects_playback && outcome.ticket.is_none());
    assert_eq!(app.queue().current().unwrap().id, 1);
    assert!(matches!(
        app.edit_queue(account, revision, QueueEdit::AppendMany(vec![track(4)])),
        Err(CoordinatorError::QueueChanged)
    ));
    assert_eq!(app.queue().items().len(), 3);
}

#[test]
fn guarded_queue_edits_keep_current_track_and_respect_shuffle() {
    for mode in [PlayMode::Sequential, PlayMode::Shuffle] {
        let (mut app, _, session) = controller();
        let pending = app
            .replace(vec![track(1), track(2), track(3)], 1)
            .unwrap()
            .unwrap();
        app.set_mode(mode);
        let account = session.snapshot().revision;
        let before = app.queue().revision();
        let appended = app
            .edit_queue(account, before, QueueEdit::Append(track(4)))
            .unwrap();
        assert!(appended.changed && !appended.affects_playback && appended.ticket.is_none());
        assert_eq!(app.queue().current().unwrap().id, 2);
        let revision = app.queue().revision();
        assert!(
            !app.edit_queue(account, revision, QueueEdit::Append(track(4)))
                .unwrap()
                .changed
        );
        assert_eq!(app.queue().revision(), revision);
        let next = app
            .edit_queue(account, revision, QueueEdit::PlayNext(track(5)))
            .unwrap();
        assert!(next.changed && !next.affects_playback);
        assert_eq!(app.queue().current().unwrap().id, 2);
        assert_eq!(app.queue().peek_next().unwrap().id, 5);
        assert!(matches!(
            app.edit_queue(account, revision, QueueEdit::Clear),
            Err(CoordinatorError::QueueChanged)
        ));
        let current_revision = app.queue().revision();
        let changed = app
            .edit_queue(account, current_revision, QueueEdit::Move(0, 1))
            .unwrap()
            .changed;
        assert_eq!(changed, mode != PlayMode::Shuffle);
        assert_eq!(app.queue().current().unwrap().id, 2);
        assert!(matches!(
            app.apply(resolved(pending, false)).unwrap(),
            ApplyOutcome::Started
        ));
    }
}
