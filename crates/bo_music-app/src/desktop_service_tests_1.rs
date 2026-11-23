use super::*;

#[test]
fn service_flushes_the_latest_floating_windows_on_shutdown() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "bo_music-floating-settings-{}-{unique}",
        std::process::id()
    ));
    let paths = DataPaths::new(&root);
    let service = Service::start_with(ServiceOptions {
        paths: Some(paths.clone()),
        library: Some(root.join("missing-libmpv.dll")),
        ..Default::default()
    });
    let mut preferences = FloatingWindows::default();
    assert!(
        matches!(service.events.recv_timeout(Duration::from_secs(5)), Ok(Event::FloatingWindows(value)) if value == preferences)
    );
    preferences.mini_player = Some(FloatingBounds {
        x: -400.,
        y: 60.,
        width: 460.,
        height: 230.,
    });
    service
        .commands
        .send(Command::SaveFloatingWindows(preferences))
        .unwrap();
    preferences.lyric_font_size = 38.;
    service
        .commands
        .send(Command::SaveFloatingWindows(preferences))
        .unwrap();
    drop(service);
    let reopened = SettingsStore::open(paths.config()).unwrap();
    assert_eq!(reopened.floating_windows().unwrap(), preferences);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn batch_selection_binds_search_and_history_to_the_displayed_snapshot() {
    let track = Track {
        id: 7,
        title: "测试".into(),
        artist_text: String::new(),
        artists: vec![],
        album_name: None,
        album_id: 0,
        cover_url: None,
        duration_seconds: 0,
        available_qualities: vec![],
        audio_variants: vec![],
        requires_vip: false,
        requires_purchase: false,
        lyrics: None,
        has_mv: false,
        comment_count: None,
        favorite_count: None,
        share_count: None,
    };
    let browser = Browser::default();
    let tracks = [track];
    let select = |source, revision| {
        selected_tracks(
            source,
            &[(0, 7)],
            &browser,
            (3, &tracks),
            (4, 8, &tracks),
            revision,
        )
    };
    assert!(select(BatchSource::Search(3), 8).is_some());
    assert!(select(BatchSource::Search(2), 8).is_none());
    assert!(select(BatchSource::History(4), 8).is_some());
    assert!(select(BatchSource::History(4), 9).is_none());
    assert!(select(BatchSource::History(3), 8).is_none());
    assert!(
        selected_tracks(
            BatchSource::Search(3),
            &[(0, 99)],
            &browser,
            (3, &tracks),
            (4, 8, &tracks),
            8
        )
        .is_none()
    );
}

#[tokio::test]
async fn batch_likes_skip_existing_and_count_uncertain_writes() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for index in 0..5 {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut buffer = [0; 4096];
                let count = socket.read(&mut buffer).unwrap();
                if count == 0 {
                    break;
                }
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&bytes[..end]);
                    let length = header
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            requests.push(String::from_utf8_lossy(&bytes).into_owned());
            let body = match index {
                0 => r#"{"code":200,"data":{"id":9,"name":"喜欢"}}"#,
                1 => r#"{"code":200,"data":{"list":[{"id":1,"name":"已喜欢"}]}}"#,
                2 => r#"{"code":200,"data":{"list":[]}}"#,
                3 => r#"{"code":200}"#,
                _ => r#"{"code":500}"#,
            };
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
        requests
    });
    let session = Session::default();
    session.set("42".into(), "synthetic".into());
    let revision = session.snapshot().revision;
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
    let (tx, mut rx) = mpsc::unbounded_channel();
    let _cancel = start_liked_batch(transport, tx, vec![1, 2, 3, 2], revision);
    let result = loop {
        match tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap()
        {
            Command::BatchProgress(_, _) => {}
            Command::BatchDone(rev, playlist, BatchAction::Like, None, result) => {
                assert_eq!((rev, playlist), (revision, 9));
                break result;
            }
            _ => panic!("unexpected event"),
        }
    };
    assert_eq!(
        (
            result.total,
            result.processed,
            result.succeeded,
            result.skipped,
            result.unconfirmed
        ),
        (3, 3, 1, 1, 1)
    );
    let requests = server.join().unwrap();
    for (request, id) in requests[3..].iter().zip([2, 3]) {
        let body: serde_json::Value =
            serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(body, serde_json::json!({"playListId":9,"musicIdList":[id]}));
    }
}

#[tokio::test]
async fn canceling_batch_likes_before_reading_stops_without_writing() {
    let session = Session::default();
    session.set("42".into(), "synthetic".into());
    let revision = session.snapshot().revision;
    let transport = Arc::new(
        Transport::new(
            TransportOptions {
                base_url: "http://127.0.0.1:9/api/".into(),
                ..Default::default()
            },
            session,
            "device".into(),
        )
        .unwrap(),
    );
    let (tx, mut rx) = mpsc::unbounded_channel();
    let cancel = start_liked_batch(transport, tx, vec![1, 2], revision);
    cancel.store(true, Ordering::Release);
    let Command::BatchDone(_, _, BatchAction::Like, None, result) =
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("unexpected event")
    };
    assert!(result.canceled);
    assert_eq!((result.total, result.processed), (2, 0));
}

#[tokio::test]
async fn batch_likes_do_not_write_when_liked_list_exceeds_scan_limit() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for index in 0..101 {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = [0; 4096];
            let count = socket.read(&mut bytes).unwrap();
            let request = String::from_utf8_lossy(&bytes[..count]);
            assert!(request.starts_with("GET "));
            let body = if index == 0 {
                r#"{"code":200,"data":{"id":9}}"#
            } else {
                r#"{"code":200,"data":{"list":[{"id":1}]}}"#
            };
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
    });
    let session = Session::default();
    session.set("42".into(), "synthetic".into());
    let revision = session.snapshot().revision;
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
    let (tx, mut rx) = mpsc::unbounded_channel();
    let _cancel = start_liked_batch(transport, tx, vec![2], revision);
    let Command::BatchPreparationFailed(rev, message) =
        tokio::time::timeout(Duration::from_secs(20), rx.recv())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("expected scan-limit failure")
    };
    assert_eq!(rev, revision);
    assert!(message.contains("上限"));
    server.join().unwrap();
}

#[tokio::test]
async fn playback_reports_are_dispatched_without_rebinding_accounts() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for _ in 0..3 {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let count = socket.read(&mut buffer).unwrap();
                if count == 0 {
                    break;
                }
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            requests.push(String::from_utf8(bytes).unwrap());
            let body = r#"{"code":200}"#;
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
        requests
    });
    let session = Session::default();
    session.set("42".into(), "synthetic".into());
    let revision = session.snapshot().revision;
    let transport = Arc::new(
        Transport::new(
            TransportOptions {
                base_url: format!("http://{address}/api/"),
                ..Default::default()
            },
            session.clone(),
            "device".into(),
        )
        .unwrap(),
    );
    let mut tasks = tokio::task::JoinSet::new();
    schedule_reports(
        &transport,
        vec![
            PlaybackReport::Started {
                music_id: 22,
                revision,
            },
            PlaybackReport::Ended {
                music_id: 22,
                duration: 100,
                position: 30,
                completed: false,
                revision,
            },
            PlaybackReport::Collection {
                origin: crate::playback::PlaybackOrigin {
                    kind: telemetry::CollectionKind::Playlist,
                    id: 10,
                    source_type: 13,
                },
                revision,
            },
        ],
        &mut tasks,
    );
    while tasks.join_next().await.is_some() {}
    let requests = server.join().unwrap();
    assert!(requests.iter().any(
        |request| request.starts_with("POST /api/playlist/history/add?")
            && request.ends_with(r#"{"sourceType":1,"items":[{"sourceId":22}]}"#)
    ));
    assert!(requests.iter().any(|request| request.starts_with("POST /api/service/music/record?") && request.ends_with(r#"{"musicList":[{"id":22,"src":false,"currentDur":100,"currentSpt":30,"pSrc":""}],"operate":5}"#)));
    assert!(requests.iter().any(|request| {
        request.starts_with("POST /api/playlist/history/add?")
            && request.ends_with(r#"{"sourceType":2,"items":[{"sourceId":10,"playlistType":13}]}"#)
    }));
    session.set("43".into(), "new".into());
    schedule_reports(
        &transport,
        vec![PlaybackReport::Started {
            music_id: 22,
            revision,
        }],
        &mut tasks,
    );
    assert!(tasks.is_empty());
}

#[test]
fn phone_numbers_follow_original_client_normalization() {
    assert_eq!(
        normalize_mobile(" +86 138-0000-0000"),
        Some("13800000000".into())
    );
    assert_eq!(
        normalize_mobile("8613800000000"),
        Some("13800000000".into())
    );
    assert_eq!(normalize_mobile("12800000000"), None);
    assert_eq!(normalize_mobile("1380000000"), None);
    assert_eq!(normalize_mobile("138000000000"), None);
}

#[test]
fn searches_resolves_plays_and_saves_queue_with_local_services() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let server_stop = Arc::clone(&stop);
    let server = std::thread::spawn(move || {
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
        while !server_stop.load(Ordering::Relaxed) {
            let (mut socket, _) = match listener.accept() {
                Ok(value) => value,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(error) => panic!("accept failed: {error}"),
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let count = match socket.read(&mut buffer) {
                    Ok(count) => count,
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::ConnectionAborted
                                | std::io::ErrorKind::ConnectionReset
                                | std::io::ErrorKind::TimedOut
                                | std::io::ErrorKind::WouldBlock
                        ) =>
                    {
                        break;
                    }
                    Err(error) => panic!("local server request failed: {error}"),
                };
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..count]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&request[..end]);
                    let length = header
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            if !request.windows(4).any(|part| part == b"\r\n\r\n") {
                continue;
            }
            let request = String::from_utf8_lossy(&request);
            let body = if request.starts_with("GET /api/search/tip/v2/list?") {
                assert!(request.contains("keyword=latest&"));
                br#"{"code":200,"data":{"resultList":[{"relword":"latest suggestion"}]}}"#.to_vec()
            } else if request.starts_with("GET /api/search/topic/word/list?") {
                br#"{"code":200,"data":{"hotWord":[{"key":"second","sort":2},{"key":"first","sort":1}]}}"#.to_vec()
            } else if request.starts_with("GET /api/search/music/list") && request.contains("pn=2&")
            {
                br#"{"code":200,"data":{"resultList":[]}}"#.to_vec()
            } else if request.starts_with("GET /api/search/music/list") && request.contains("pn=1&")
            {
                r#"{"code":200,"data":{"resultList":[{"id":124,"name":"第二首","artist":"测试歌手","duration":1,"audios":[{"level":"s","format":"mp3","bitrate":"128","size":"1Mb"}]}]}}"#.as_bytes().to_vec()
            } else if request.starts_with("GET /api/search/music/list") {
                r#"{"code":200,"data":{"resultList":[{"id":123,"name":"测试歌曲","artist":"测试歌手","duration":1,"audios":[{"level":"s","format":"mp3","bitrate":"128","size":"1Mb"}]}]}}"#.as_bytes().to_vec()
            } else if request.starts_with("GET /api/play/music/v2/checkRight") {
                br#"{"code":200,"data":{"status":4}}"#.to_vec()
            } else if request.starts_with("GET /api/play/music/v2/audioUrl") {
                format!(r#"{{"code":200,"data":{{"audioUrl":"http://{address}/audio.wav","format":"mp3","bitrate":128}}}}"#).into_bytes()
            } else if request.starts_with("GET /mobi.s?") {
                assert!(!request.to_ascii_lowercase().contains("\r\nuid:"));
                // Base64-encoded "[00:00.00]bo_music".
                br#"{"code":200,"data":{"content":"WzAwOjAwLjAwXWJvX211c2lj"}}"#.to_vec()
            } else if request.starts_with("GET /audio.wav") {
                wav.clone()
            } else {
                panic!("unexpected local request");
            };
            if let Err(error) = write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            ) {
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::ConnectionAborted
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::BrokenPipe
                ) {
                    continue;
                }
                panic!("local server response failed: {error}");
            }
            let _ = socket.write_all(&body);
        }
    });
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "bo_music-service-test-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir(&root).unwrap();
    let paths = DataPaths::new(&root);
    let library =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../libmpv/libmpv-2.dll");
    let service = Service::start_with(ServiceOptions {
        paths: Some(paths.clone()),
        base_url: Some(format!("http://{address}/api/")),
        library: Some(library),
        audio_output: Some("null".into()),
        lyrics_endpoint: Some(format!("http://{address}/mobi.s")),
        credentials: None,
        qr_interval: None,
        qr_lifetime: None,
        video_library: None,
        startup_paths: None,
    });
    for (generation, query) in [(1, "old"), (2, "latest")] {
        service
            .commands
            .send(Command::SearchSuggestions(generation, query.into()))
            .unwrap();
    }
    service.commands.send(Command::LoadSearchHotWords).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut saw_suggestions = false;
    let mut saw_hot_words = false;
    while Instant::now() < deadline && !(saw_suggestions && saw_hot_words) {
        match service.events.recv_timeout(Duration::from_millis(100)) {
            Ok(Event::SearchSuggestions(generation, words)) => {
                assert_eq!(generation, 2);
                assert_eq!(words, ["latest suggestion"]);
                saw_suggestions = true;
            }
            Ok(Event::SearchHotWords(words)) => {
                assert_eq!(words.unwrap(), ["first", "second"]);
                saw_hot_words = true;
            }
            _ => {}
        }
    }
    assert!(saw_suggestions && saw_hot_words);
    service.commands.send(Command::VolumeDelta(-10)).unwrap();
    service.commands.send(Command::CycleMode).unwrap();
    let settings_deadline = Instant::now() + Duration::from_secs(5);
    let mut changed = false;
    while Instant::now() < settings_deadline {
        if let Ok(Event::Player(view)) = service.events.recv_timeout(Duration::from_millis(250))
            && view.volume == 90
            && view.mode == PlayMode::ListLoop
        {
            changed = true;
            break;
        }
    }
    assert!(changed, "volume or mode did not update");
    for expected in [0, 90] {
        service.commands.send(Command::ToggleMute).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            assert!(
                Instant::now() < deadline,
                "mute did not update the effective volume"
            );
            if matches!(service.events.recv_timeout(Duration::from_millis(200)), Ok(Event::Player(view)) if view.volume == expected)
            {
                break;
            }
        }
    }
    service
        .commands
        .send(Command::SetPreferredQuality(AudioQuality::Standard))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut quality_updated = false;
    while Instant::now() < deadline {
        if matches!(service.events.recv_timeout(Duration::from_millis(250)),
            Ok(Event::Player(view)) if view.preferred_quality == AudioQuality::Standard)
        {
            quality_updated = true;
            break;
        }
    }
    assert!(quality_updated, "preferred quality did not update");
    assert_eq!(
        SettingsStore::open(paths.config())
            .unwrap()
            .preferred_quality()
            .unwrap(),
        AudioQuality::Standard
    );
    service
        .commands
        .send(Command::AudioEffect(AudioEffectAction::Select(
            "galaxy.clear".into(),
        )))
        .unwrap();
    service
        .commands
        .send(Command::AudioEffect(AudioEffectAction::AdjustBand(0, 2.0)))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut updated = false;
    while Instant::now() < deadline {
        if matches!(service.events.recv_timeout(Duration::from_millis(250)),
            Ok(Event::Player(view)) if view.audio_effects.enabled && view.audio_effects.preset_id == "custom.gpui"
                && view.audio_effects.custom_presets.first().is_some_and(|preset| preset.bands[0] == 2.0))
        {
            updated = true;
            break;
        }
    }
    assert!(updated, "audio effect did not update");
    let persisted = SettingsStore::open(paths.config())
        .unwrap()
        .audio_effects()
        .unwrap();
    assert_eq!(persisted.custom_presets[0].bands[0], 2.0);
    service
        .commands
        .send(Command::AudioEffect(AudioEffectAction::SelectAmbient(
            "ambient.rain".into(),
        )))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut ambient_updated = false;
    while Instant::now() < deadline {
        if matches!(service.events.recv_timeout(Duration::from_millis(250)),
            Ok(Event::Player(view)) if view.audio_effects.ambient_id == "ambient.rain")
        {
            ambient_updated = true;
            break;
        }
    }
    assert!(ambient_updated, "ambient sound did not update");
    assert!(paths.root().join("ambient/v9/ambient.rain.ogg").is_file());
    for _ in 0..3 {
        service.commands.send(Command::CycleMode).unwrap();
    }
    service
        .commands
        .send(Command::Search("测试".into()))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut search_started = false;
    let mut search_phase = 0;
    let mut started = false;
    let mut completed = false;
    let mut lyrics_ready = false;
    while Instant::now() < deadline {
        match service.events.recv_timeout(Duration::from_millis(250)) {
            Ok(Event::Search(search)) if !search.loading && !search.tracks.is_empty() => {
                assert!(search.tracks.iter().all(|track| track.id != 999));
                match search_phase {
                    0 => {
                        assert_eq!(search.tracks[0].id, 123);
                        assert!(search.has_more);
                        // A previous search may finish after the current search.
                        let mut stale = search.tracks[0].clone();
                        stale.id = 999;
                        service
                            .commands
                            .send(Command::SearchDone(0, search_cursor(30), Ok(vec![stale])))
                            .unwrap();
                        service.commands.send(Command::SearchNext).unwrap();
                        search_phase = 1;
                    }
                    1 if search.tracks.len() == 2 => {
                        assert_eq!(search.tracks[1].id, 124);
                        assert!(search.has_more);
                        service.commands.send(Command::SearchNext).unwrap();
                        search_phase = 2;
                    }
                    2 if !search.has_more => {
                        service
                            .commands
                            .send(Command::Play(search.tracks, 0))
                            .unwrap();
                        search_started = true;
                        search_phase = 3;
                    }
                    _ => {}
                }
            }
            Ok(Event::Status(message)) if message == "正在播放" => started = true,
            Ok(Event::Lyrics(LyricsView::Ready(document))) => {
                assert_eq!(document.lines[0].text, "bo_music");
                lyrics_ready = true;
                if completed {
                    break;
                }
            }
            Ok(Event::Player(view)) if started && view.state == PlaybackState::Stopped => {
                completed = true;
                if lyrics_ready {
                    break;
                }
            }
            Ok(Event::Lyrics(LyricsView::Failed)) => panic!("lyrics request failed"),
            Ok(Event::Status(message))
                if message.contains("失败") || message.contains("不可用") =>
            {
                panic!("{message}")
            }
            _ => {}
        }
    }
    assert!(
        search_started && started && completed && lyrics_ready,
        "search={search_started}, started={started}, completed={completed}, lyrics={lyrics_ready}"
    );
    service.commands.send(Command::ShowHistory).unwrap();
    let history_deadline = Instant::now() + Duration::from_secs(8);
    let mut history_found = false;
    while Instant::now() < history_deadline {
        if let Ok(Event::History(view)) = service.events.recv_timeout(Duration::from_millis(200))
            && !view.loading
            && view.tracks.iter().any(|track| track.id == 123)
        {
            history_found = true;
            break;
        }
    }
    assert!(
        history_found,
        "played track was not recorded in recent history"
    );
    drop(service);
    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();
    let queue = QueueStore::new(paths).load("anonymous").unwrap();
    assert_eq!(queue.tracks[0].id, 123);
    let settings = SettingsStore::open(root.join("config.json")).unwrap();
    assert_eq!(settings.volume().unwrap(), 90.0);
    assert_eq!(settings.playback().unwrap().mode, PlayMode::Sequential);
    std::fs::remove_dir_all(root).unwrap();
}
