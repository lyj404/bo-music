use super::*;

#[test]
fn qr_login_handles_expiry_cancellation_and_success() {
    use bo_music_storage::credentials::CredentialError;
    use std::sync::Mutex;
    #[derive(Default)]
    struct Memory(
        Mutex<Option<String>>,
        Mutex<std::collections::HashMap<String, String>>,
    );
    impl SecretBackend for Memory {
        fn read(&self) -> Result<Option<String>, CredentialError> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn write(&self, value: &str) -> Result<(), CredentialError> {
            *self.0.lock().unwrap() = Some(value.into());
            Ok(())
        }
        fn delete(&self) -> Result<(), CredentialError> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
        fn read_named(&self, name: &str) -> Result<Option<String>, CredentialError> {
            Ok(self.1.lock().unwrap().get(name).cloned())
        }
        fn write_named(&self, name: &str, value: &str) -> Result<(), CredentialError> {
            self.1.lock().unwrap().insert(name.into(), value.into());
            Ok(())
        }
        fn delete_named(&self, name: &str) -> Result<(), CredentialError> {
            self.1.lock().unwrap().remove(name);
            Ok(())
        }
    }
    fn wait(service: &Service, predicate: impl Fn(&Event) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            if let Ok(event) = service.events.recv_timeout(Duration::from_millis(100)) {
                if let Event::Qr(QrView::Ready(ref url)) = event {
                    assert_ne!(url, "stale");
                }
                if predicate(&event) {
                    return;
                }
            }
        }
        panic!("expected QR event did not arrive");
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let server_stop = Arc::clone(&stop);
    let server = std::thread::spawn(move || {
        let mut challenges = 0;
        let mut success_polls = 0;
        while !server_stop.load(Ordering::Relaxed) {
            let (mut socket, _) = match listener.accept() {
                Ok(value) => value,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(error) => panic!("{error}"),
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut data = [0; 4096];
            let count = socket.read(&mut data).unwrap();
            if count == 0 {
                continue;
            }
            let request = String::from_utf8_lossy(&data[..count]);
            let body = if request.starts_with("GET /api/ucenter/login/qrCode?") {
                challenges += 1;
                match challenges {
                    1 => r#"{"code":200,"data":{"qrCode":"expired"}}"#,
                    2 => r#"{"code":200,"data":{"qrCode":"cancelled"}}"#,
                    3 => r#"{"code":200,"data":{"qrCode":"timeout"}}"#,
                    _ => r#"{"code":200,"data":{"qrCode":"success"}}"#,
                }
            } else if request.starts_with("GET /api/ucenter/login/qrCodeStatus?") {
                if request.contains("qrCode=expired") {
                    r#"{"code":200,"data":{"status":2}}"#
                } else if request.contains("qrCode=cancelled") || request.contains("qrCode=timeout")
                {
                    r#"{"code":200,"data":{"status":1}}"#
                } else {
                    success_polls += 1;
                    if success_polls < 2 {
                        r#"{"code":200,"data":{"status":1}}"#
                    } else {
                        r#"{"code":200,"data":{"status":3}}"#
                    }
                }
            } else if request.starts_with("POST /api/ucenter/users/login?") {
                r#"{"code":200,"data":{"id":42,"bid":42,"token":"synthetic"}}"#
            } else {
                panic!("unexpected request")
            };
            let _ = write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
        }
    });
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("bo_music-qr-test-{}-{unique}", std::process::id()));
    let backend: SharedSecrets = Arc::new(Memory::default());
    let service = Service::start_with(ServiceOptions {
        paths: Some(DataPaths::new(&root)),
        base_url: Some(format!("http://{address}/api/")),
        library: Some(root.join("missing-mpv")),
        credentials: Some(Arc::clone(&backend)),
        qr_interval: Some(Duration::from_millis(50)),
        qr_lifetime: Some(Duration::from_secs(1)),
        ..Default::default()
    });
    wait(
        &service,
        |e| matches!(e, Event::Auth(v) if !v.busy && v.account.is_none()),
    );
    service.commands.send(Command::StartQr).unwrap();
    wait(
        &service,
        |e| matches!(e, Event::Qr(QrView::Ready(url)) if url.contains("expired")),
    );
    wait(&service, |e| matches!(e, Event::Qr(QrView::Expired)));
    service.commands.send(Command::StartQr).unwrap();
    wait(
        &service,
        |e| matches!(e, Event::Qr(QrView::Ready(url)) if url.contains("cancelled")),
    );
    service.commands.send(Command::CancelQr).unwrap();
    wait(&service, |e| matches!(e, Event::Qr(QrView::Idle)));
    service
        .commands
        .send(Command::QrUpdate(2, QrUpdate::Ready("stale".into())))
        .unwrap();
    service.commands.send(Command::StartQr).unwrap();
    wait(
        &service,
        |e| matches!(e, Event::Qr(QrView::Ready(url)) if url.contains("timeout")),
    );
    wait(&service, |e| matches!(e, Event::Qr(QrView::Expired)));
    service.commands.send(Command::StartQr).unwrap();
    wait(
        &service,
        |e| matches!(e, Event::Qr(QrView::Ready(url)) if url.contains("success")),
    );
    wait(
        &service,
        |e| matches!(e, Event::Auth(v) if !v.busy && v.account.as_ref().is_some_and(|a| a.uid == "42")),
    );
    assert!(backend.read().unwrap().is_some());
    drop(service);
    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn shutdown_does_not_wait_for_a_locked_credential_store() {
    use bo_music_storage::credentials::CredentialError;
    use std::sync::Mutex;
    struct Locked {
        entered: std_mpsc::Sender<()>,
        release: Mutex<std_mpsc::Receiver<()>>,
    }
    impl SecretBackend for Locked {
        fn read(&self) -> Result<Option<String>, CredentialError> {
            self.entered.send(()).unwrap();
            self.release.lock().unwrap().recv().unwrap();
            Ok(None)
        }
        fn write(&self, _: &str) -> Result<(), CredentialError> {
            unreachable!()
        }
        fn delete(&self) -> Result<(), CredentialError> {
            unreachable!()
        }
    }
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "bo_music-locked-test-{}-{unique}",
        std::process::id()
    ));
    let (entered_tx, entered_rx) = std_mpsc::channel();
    let (release_tx, release_rx) = std_mpsc::channel();
    let service = Service::start_with(ServiceOptions {
        paths: Some(DataPaths::new(&root)),
        library: Some(root.join("missing-mpv")),
        credentials: Some(Arc::new(Locked {
            entered: entered_tx,
            release: Mutex::new(release_rx),
        })),
        ..Default::default()
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let (done_tx, done_rx) = std_mpsc::channel();
    let closing = std::thread::spawn(move || {
        drop(service);
        let _ = done_tx.send(());
    });
    let closed = done_rx.recv_timeout(Duration::from_secs(2));
    release_tx.send(()).unwrap();
    closing.join().unwrap();
    assert!(
        closed.is_ok(),
        "window close waited for the credential store"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn corrupt_queue_is_not_overwritten_at_shutdown() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "bo_music-corrupt-test-{}-{unique}",
        std::process::id()
    ));
    let queue_file = root.join("accounts/anonymous/queue.json");
    std::fs::create_dir_all(queue_file.parent().unwrap()).unwrap();
    std::fs::write(&queue_file, b"corrupt queue").unwrap();
    let library =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../libmpv/libmpv-2.dll");
    let service = Service::start_with(ServiceOptions {
        paths: Some(DataPaths::new(&root)),
        library: Some(library),
        audio_output: Some("null".into()),
        ..Default::default()
    });
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut warned = false;
    while Instant::now() < deadline {
        if let Ok(Event::Status(message)) = service.events.recv_timeout(Duration::from_millis(250))
            && message.contains("队列无法恢复")
        {
            warned = true;
            break;
        }
    }
    assert!(warned);
    drop(service);
    assert_eq!(std::fs::read(queue_file).unwrap(), b"corrupt queue");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn queue_commands_validate_versions_and_persist_changes() {
    use bo_music_storage::queue::QueueSnapshot;
    fn track(id: i64) -> Track {
        Track {
            id,
            title: format!("歌曲{id}"),
            artist_text: String::new(),
            artists: vec![],
            album_name: None,
            album_id: 0,
            cover_url: Some(format!("https://example.test/{id}.jpg")),
            duration_seconds: 60,
            available_qualities: vec![],
            audio_variants: vec![],
            requires_vip: false,
            requires_purchase: false,
            lyrics: None,
            has_mv: false,
            comment_count: None,
            favorite_count: None,
            share_count: None,
        }
    }
    fn view(service: &Service, predicate: impl Fn(&PlayerView) -> bool) -> PlayerView {
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            if let Ok(Event::Player(view)) = service.events.recv_timeout(Duration::from_millis(200))
                && predicate(&view)
            {
                return view;
            }
        }
        panic!("expected queue view did not arrive")
    }
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "bo_music-queue-edit-{}-{unique}",
        std::process::id()
    ));
    let paths = DataPaths::new(&root);
    QueueStore::new(paths.clone())
        .save(
            "anonymous",
            &QueueSnapshot {
                tracks: vec![track(1)],
                current_index: Some(0),
                position_seconds: 0.0,
            },
        )
        .unwrap();
    let library = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../libmpv/libmpv-2.dll");
    let service = Service::start_with(ServiceOptions {
        paths: Some(paths.clone()),
        library: Some(library),
        audio_output: Some("null".into()),
        base_url: Some("http://127.0.0.1:9/api/".into()),
        lyrics_endpoint: Some("http://127.0.0.1:9/mobi.s".into()),
        ..Default::default()
    });
    let first = view(&service, |view| view.queue.len() == 1);
    assert_eq!(
        first.cover_url.as_deref(),
        Some("https://example.test/1.jpg")
    );
    service
        .commands
        .send(Command::EditQueue(
            first.queue_account_revision,
            first.queue_revision,
            QueueEdit::Append(track(2)),
        ))
        .unwrap();
    let second = view(&service, |view| view.queue.len() == 2);
    service
        .commands
        .send(Command::EditQueue(
            first.queue_account_revision,
            first.queue_revision,
            QueueEdit::Remove(0),
        ))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut rejected = false;
    while Instant::now() < deadline {
        if matches!(service.events.recv_timeout(Duration::from_millis(100)), Ok(Event::Status(message)) if message.contains("队列或账号已变化"))
        {
            rejected = true;
            break;
        }
    }
    assert!(rejected);
    service
        .commands
        .send(Command::EditQueue(
            second.queue_account_revision,
            second.queue_revision,
            QueueEdit::PlayNext(track(3)),
        ))
        .unwrap();
    let third = view(&service, |view| view.queue.len() == 3);
    assert_eq!(third.queue[1], "歌曲3 · ");
    service
        .commands
        .send(Command::EditQueue(
            third.queue_account_revision,
            third.queue_revision,
            QueueEdit::Move(2, 1),
        ))
        .unwrap();
    let reordered = view(&service, |view| {
        view.queue.len() == 3 && view.queue[1] == "歌曲2 · "
    });
    service
        .commands
        .send(Command::EditQueue(
            reordered.queue_account_revision,
            reordered.queue_revision,
            QueueEdit::Remove(2),
        ))
        .unwrap();
    let removed = view(&service, |view| view.queue.len() == 2);
    service
        .commands
        .send(Command::EditQueue(
            removed.queue_account_revision,
            removed.queue_revision,
            QueueEdit::Clear,
        ))
        .unwrap();
    view(&service, |view| view.queue.is_empty());
    drop(service);
    assert!(
        QueueStore::new(paths)
            .load("anonymous")
            .unwrap()
            .tracks
            .is_empty()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn playlist_management_preserves_metadata_and_protects_liked_playlist() {
    use crate::browse::BrowseContent;
    use bo_music_storage::credentials::{CredentialError, SecretBackend};
    use std::sync::Mutex;
    struct Memory;
    impl SecretBackend for Memory {
        fn read(&self) -> Result<Option<String>, CredentialError> {
            Ok(Some(
                r#"{"version":1,"credential":{"uid":"42","token":"synthetic","nickname":"听众"}}"#
                    .into(),
            ))
        }
        fn write(&self, _: &str) -> Result<(), CredentialError> {
            Ok(())
        }
        fn delete(&self) -> Result<(), CredentialError> {
            Ok(())
        }
    }
    fn wait(service: &Service, test: impl Fn(&Event) -> bool) -> Event {
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            if let Ok(event) = service.events.recv_timeout(Duration::from_millis(100))
                && test(&event)
            {
                return event;
            }
        }
        panic!("expected event did not arrive")
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let server_stop = stop.clone();
    let writes = Arc::new(Mutex::new(Vec::<(String, serde_json::Value)>::new()));
    let captured = writes.clone();
    let fail_liked = Arc::new(AtomicBool::new(false));
    let server_fail_liked = Arc::clone(&fail_liked);
    let (request_started, request_seen) = std_mpsc::channel();
    let (release_request, release_signal) = std_mpsc::channel();
    let server = std::thread::spawn(move || {
        while !server_stop.load(Ordering::Relaxed) {
            let (mut socket, _) = match listener.accept() {
                Ok(value) => value,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(_) => panic!("accept failed"),
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut buffer = [0; 4096];
                let count = socket.read(&mut buffer).unwrap_or(0);
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
            let request = String::from_utf8_lossy(&bytes);
            let body = if request.starts_with("GET /api/service/playlist/fond?") {
                if server_fail_liked.swap(false, Ordering::AcqRel) {
                    request_started.send(()).unwrap();
                    release_signal.recv_timeout(Duration::from_secs(5)).unwrap();
                    r#"{"code":500}"#
                } else {
                    r#"{"code":200,"data":{"id":9,"name":"喜欢"}}"#
                }
            } else if request.starts_with("GET /api/service/playlist/userCreate?") {
                r#"{"code":200,"data":{"playLists":[{"id":9,"name":"喜欢"},{"id":10,"name":"自建"}]}}"#
            } else if request.starts_with("GET /api/service/playlist/9/musicList?") {
                r#"{"code":200,"data":{"list":[]}}"#
            } else if request.starts_with("GET /api/service/playlist/10/musicList?") {
                r#"{"code":200,"data":{"list":[{"id":501,"name":"测试歌曲"},{"id":502,"name":"第二首"}]}}"#
            } else if request.starts_with("GET /api/service/playlist/info/10?") {
                r#"{"code":200,"data":{"id":10,"name":"自建","description":"原简介","pic":"https://example.test/original.jpg","categories":[{"id":3,"name":"标签"}]}}"#
            } else if request.starts_with("GET /api/service/category/list?") {
                r#"{"code":200,"data":{"categories":[{"name":"分类","subCategories":[{"id":3,"name":"标签"},{"id":4,"name":"新标签"}]}]}}"#
            } else if request.starts_with("POST /api/service/playlist/uploadPic/10?") {
                assert!(request.contains("image/png"));
                captured
                    .lock()
                    .unwrap()
                    .push(("UPLOAD".into(), serde_json::Value::Null));
                r#"{"code":200,"data":{"url":"https://example.test/new.png"}}"#
            } else if request.starts_with("POST /api/service/playlist/music?")
                || request.starts_with("POST /api/service/playlist/music/delete?")
            {
                let operation = if request.starts_with("POST /api/service/playlist/music/delete?") {
                    "REMOVE"
                } else {
                    "ADD"
                };
                let value =
                    serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
                captured.lock().unwrap().push((operation.into(), value));
                r#"{"code":200,"data":{}}"#
            } else if request.starts_with("POST /api/service/playlist?")
                || request.starts_with("PUT /api/service/playlist?")
                || request.starts_with("DELETE /api/service/playlist?")
            {
                let method = request.split_whitespace().next().unwrap().to_owned();
                let value =
                    serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
                captured.lock().unwrap().push((method.clone(), value));
                if method == "POST" {
                    r#"{"code":200,"data":{"id":11}}"#
                } else {
                    r#"{"code":200,"data":{}}"#
                }
            } else {
                panic!("unexpected local request")
            };
            let _ = write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
        }
    });
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "bo_music-manage-test-{}-{unique}",
        std::process::id()
    ));
    let service = Service::start_with(ServiceOptions {
        paths: Some(DataPaths::new(&root)),
        base_url: Some(format!("http://{address}/api/")),
        library: Some(root.join("missing-mpv.dll")),
        credentials: Some(Arc::new(Memory)),
        ..Default::default()
    });
    wait(
        &service,
        |event| matches!(event, Event::Auth(view) if !view.busy && view.account.is_some()),
    );
    service.commands.send(Command::MyPlaylists).unwrap();
    let Event::Browse(view) = wait(
        &service,
        |event| matches!(event, Event::Browse(view) if view.my_playlists && !view.loading && matches!(&view.content, BrowseContent::Playlists(_))),
    ) else {
        unreachable!()
    };
    let BrowseContent::Playlists(items) = view.content else {
        unreachable!()
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, 10);
    let liked = Playlist {
        id: 9,
        name: "喜欢".into(),
        ..items[0].clone()
    };
    service
        .commands
        .send(Command::CreatePlaylist(" 新歌单 ".into(), true))
        .unwrap();
    wait(
        &service,
        |event| matches!(event, Event::Browse(view) if view.my_playlists && !view.loading && matches!(&view.content, BrowseContent::Playlists(items) if items.iter().any(|item| item.id == 11 && item.is_private))),
    );
    wait(&service, |event| {
        matches!(
            event,
            Event::PlaylistMutationFinished(Some(PlaylistMutationResult::Created { id: 11, .. }))
        )
    });
    service.commands.send(Command::OpenPlaylist(liked)).unwrap();
    let Event::Browse(view) = wait(
        &service,
        |event| matches!(event, Event::Browse(view) if view.selected_playlist_id == Some(9) && !view.loading),
    ) else {
        unreachable!()
    };
    assert!(view.editable_playlist.is_none());
    service.commands.send(Command::DeletePlaylist(9)).unwrap();
    wait(
        &service,
        |event| matches!(event, Event::Status(message) if message.contains("歌单状态已变化")),
    );
    service.commands.send(Command::BrowseBack).unwrap();
    service
        .commands
        .send(Command::OpenPlaylist(items[0].clone()))
        .unwrap();
    let Event::Browse(view) = wait(
        &service,
        |event| matches!(event, Event::Browse(view) if view.selected_playlist_id == Some(10) && !view.loading && view.editable_playlist.is_some()),
    ) else {
        unreachable!()
    };
    let BrowseContent::Tracks(tracks) = view.content else {
        unreachable!()
    };
    let track = tracks[0].clone();
    fail_liked.store(true, Ordering::Release);
    service
        .commands
        .send(Command::LikeSelected(
            BatchSource::Browse(view.generation),
            vec![(0, 501)],
        ))
        .unwrap();
    request_seen.recv_timeout(Duration::from_secs(5)).unwrap();
    service
        .commands
        .send(Command::OpenTrackPicker(track.clone()))
        .unwrap();
    wait(
        &service,
        |event| matches!(event, Event::TrackPicker(Some(picker)) if picker.busy),
    );
    release_request.send(()).unwrap();
    wait(
        &service,
        |event| matches!(event, Event::TrackPicker(Some(picker)) if !picker.busy),
    );
    wait(
        &service,
        |event| matches!(event, Event::Status(message) if message.contains("无法读取喜欢列表")),
    );
    service.commands.send(Command::CloseTrackPicker).unwrap();
    service
        .commands
        .send(Command::StartPlaylistEdit(10))
        .unwrap();
    wait(
        &service,
        |event| matches!(event, Event::PlaylistEditReady(info, Some(groups)) if info.id == 10 && groups[0].categories.len() == 2),
    );
    service
        .commands
        .send(Command::SavePlaylist(
            10,
            "新名称".into(),
            "新简介".into(),
            vec![999],
            None,
        ))
        .unwrap();
    wait(
        &service,
        |event| matches!(event, Event::Status(message) if message.contains("标签已变化")),
    );
    service
        .commands
        .send(Command::LoadPlaylistCover(1, root.join("missing.png")))
        .unwrap();
    wait(&service, |event| {
        matches!(event, Event::PlaylistCoverPreview(1, Err(())))
    });
    let path = root.join("cover.png");
    image::DynamicImage::new_rgb8(8, 6)
        .save_with_format(&path, image::ImageFormat::Png)
        .unwrap();
    service
        .commands
        .send(Command::LoadPlaylistCover(2, path))
        .unwrap();
    let Event::PlaylistCoverPreview(_, Ok(image)) = wait(&service, |event| {
        matches!(event, Event::PlaylistCoverPreview(2, Ok(_)))
    }) else {
        unreachable!()
    };
    service
        .commands
        .send(Command::SavePlaylist(
            10,
            "新名称".into(),
            "新简介".into(),
            vec![3, 4],
            Some(CoverEdit {
                image,
                crop: crate::playlist_cover::CoverCrop::default(),
            }),
        ))
        .unwrap();
    wait(&service, |event| {
        matches!(
            event,
            Event::PlaylistMutationFinished(Some(PlaylistMutationResult::Edited { id: 10, .. }))
        )
    });
    service
        .commands
        .send(Command::OpenTrackPicker(track))
        .unwrap();
    wait(
        &service,
        |event| matches!(event, Event::TrackPicker(Some(view)) if !view.loading && view.track_id == 501 && view.playlists.iter().any(|item| item.id == 10)),
    );
    service
        .commands
        .send(Command::AddTrackToPlaylist(501, 777))
        .unwrap();
    wait(
        &service,
        |event| matches!(event, Event::Status(message) if message.contains("重新选择")),
    );
    service
        .commands
        .send(Command::AddTrackToPlaylist(501, 10))
        .unwrap();
    wait(
        &service,
        |event| matches!(event, Event::Status(message) if message == "已加入歌单"),
    );
    let Event::Browse(view) = wait(
        &service,
        |event| matches!(event, Event::Browse(view) if view.selected_playlist_id == Some(10) && !view.loading && matches!(&view.content, BrowseContent::Tracks(tracks) if !tracks.is_empty())),
    ) else {
        unreachable!()
    };
    service
        .commands
        .send(Command::RemovePlaylistTrack(
            10,
            0,
            501,
            view.generation.wrapping_add(1),
        ))
        .unwrap();
    wait(
        &service,
        |event| matches!(event, Event::Status(message) if message.contains("刷新后重试")),
    );
    service
        .commands
        .send(Command::RemovePlaylistTrack(10, 0, 501, view.generation))
        .unwrap();
    wait(
        &service,
        |event| matches!(event, Event::Status(message) if message == "已从歌单移除"),
    );
    let Event::Browse(view) = wait(
        &service,
        |event| matches!(event, Event::Browse(view) if view.selected_playlist_id == Some(10) && !view.loading),
    ) else {
        unreachable!()
    };
    service
        .commands
        .send(Command::OpenBatchPicker(
            BatchSource::Browse(view.generation.wrapping_add(1)),
            vec![(0, 501), (1, 502)],
        ))
        .unwrap();
    wait(
        &service,
        |event| matches!(event, Event::Status(message) if message.contains("重新选择")),
    );
    service
        .commands
        .send(Command::OpenBatchPicker(
            BatchSource::Browse(view.generation),
            vec![(0, 501), (1, 502)],
        ))
        .unwrap();
    let Event::TrackPicker(Some(picker)) = wait(
        &service,
        |event| matches!(event, Event::TrackPicker(Some(picker)) if !picker.loading && picker.track_count == 2),
    ) else {
        unreachable!()
    };
    service
        .commands
        .send(Command::AddPickedTracks(picker.generation, 10))
        .unwrap();
    wait(
        &service,
        |event| matches!(event, Event::Status(message) if message.contains("加入完成：2 首成功")),
    );
    let Event::Browse(view) = wait(
        &service,
        |event| matches!(event, Event::Browse(view) if view.selected_playlist_id == Some(10) && !view.loading),
    ) else {
        unreachable!()
    };
    service
        .commands
        .send(Command::RemoveSelectedTracks(
            10,
            view.generation,
            vec![(0, 501), (1, 502)],
        ))
        .unwrap();
    wait(
        &service,
        |event| matches!(event, Event::Status(message) if message.contains("移除完成：2 首成功")),
    );
    wait(
        &service,
        |event| matches!(event, Event::Browse(view) if view.selected_playlist_id == Some(10) && !view.loading),
    );
    service.commands.send(Command::DeletePlaylist(10)).unwrap();
    wait(&service, |event| {
        matches!(
            event,
            Event::PlaylistMutationFinished(Some(PlaylistMutationResult::Deleted { id: 10 }))
        )
    });
    drop(service);
    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();
    let writes = writes.lock().unwrap();
    assert_eq!(writes.len(), 10);
    assert_eq!(
        writes[0].1,
        serde_json::json!({"name":"新歌单", "private":true})
    );
    assert_eq!(writes[1].0, "UPLOAD");
    assert_eq!(
        writes[2].1,
        serde_json::json!({"id":10, "name":"新名称", "description":"新简介", "pic":"https://example.test/new.png", "categoryList":[3,4]})
    );
    assert_eq!(
        writes[3],
        (
            "ADD".into(),
            serde_json::json!({"playListId":10,"musicIdList":[501]})
        )
    );
    assert_eq!(
        writes[4],
        (
            "REMOVE".into(),
            serde_json::json!({"playListId":10,"musicIdList":[501]})
        )
    );
    for (index, (operation, id)) in [("ADD", 501), ("ADD", 502), ("REMOVE", 501), ("REMOVE", 502)]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            writes[index + 5],
            (
                operation.into(),
                serde_json::json!({"playListId":10,"musicIdList":[id]})
            )
        );
    }
    assert_eq!(writes[9].1, serde_json::json!({"playlistIds":[10]}));
    drop(writes);
    std::fs::remove_dir_all(root).unwrap();
}
