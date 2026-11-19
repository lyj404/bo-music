use super::*;

#[test]
fn phone_login_restores_and_isolates_account_queues() {
    use bowen_storage::{
        credentials::{CredentialError, SecretBackend},
        queue::QueueSnapshot,
    };
    use std::sync::Mutex;
    #[derive(Default)]
    struct MemorySecrets(
        Mutex<Option<String>>,
        Mutex<std::collections::HashMap<String, String>>,
    );
    impl SecretBackend for MemorySecrets {
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
    fn track(id: i64, title: &str) -> Track {
        Track {
            id,
            title: title.into(),
            artist_text: "歌手".into(),
            artists: vec![],
            album_name: None,
            album_id: 0,
            cover_url: None,
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
    fn await_event(service: &Service, predicate: impl Fn(&Event) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            if let Ok(event) = service.events.recv_timeout(Duration::from_millis(200))
                && predicate(&event)
            {
                return;
            }
        }
        panic!("expected service event did not arrive");
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let server_stop = Arc::clone(&stop);
    let server = std::thread::spawn(move || {
        while !server_stop.load(Ordering::Relaxed) {
            let (mut socket, _) = match listener.accept() {
                Ok(value) => value,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
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
            let request = &data[..count];
            let body = if request.starts_with(b"GET /api/ucenter/code/sendsms?") {
                r#"{"code":200}"#
            } else if request.starts_with(b"POST /api/ucenter/users/login?") {
                r#"{"code":200,"data":{"id":42,"bid":42,"token":"synthetic","userInfo":{"id":42,"nickname":"听众"}}}"#
            } else if request.starts_with(b"GET /api/search/music/list?") {
                r#"{"code":11012}"#
            } else if request.starts_with(b"GET /api/service/playlist/fond?") {
                r#"{"code":200,"data":{}}"#
            } else if request.starts_with(b"GET /mobi.s?") {
                r#"{"code":200,"data":{"content":"WzAwOjAwLjAwXUJvd2Vu"}}"#
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
        std::env::temp_dir().join(format!("bowen-auth-test-{}-{unique}", std::process::id()));
    let paths = DataPaths::new(&root);
    let queues = QueueStore::new(paths.clone());
    for (scope, id, title) in [
        ("anonymous", 1, "匿名曲目"),
        ("42", 2, "账号曲目"),
        ("43", 3, "另一个账号曲目"),
    ] {
        queues
            .save(
                scope,
                &QueueSnapshot {
                    tracks: vec![track(id, title)],
                    current_index: Some(0),
                    position_seconds: 0.0,
                },
            )
            .unwrap();
    }
    let history = HistoryStore::new(paths.clone());
    history.record("anonymous", &track(11, "匿名历史")).unwrap();
    history.record("42", &track(22, "账号历史")).unwrap();
    let backend: SharedSecrets = Arc::new(MemorySecrets::default());
    let saved = CredentialStore::new(Arc::clone(&backend));
    saved
        .save(bowen_storage::credentials::Credential {
            uid: "43".into(),
            token: "other-synthetic".into(),
            nickname: Some("另一个".into()),
        })
        .unwrap();
    saved
        .save(bowen_storage::credentials::Credential {
            uid: "44".into(),
            token: "remove-synthetic".into(),
            nickname: Some("待移除".into()),
        })
        .unwrap();
    saved.clear().unwrap();
    let options = || ServiceOptions {
        paths: Some(paths.clone()),
        base_url: Some(format!("http://{address}/api/")),
        library: Some(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../libmpv/libmpv-2.dll"),
        ),
        audio_output: Some("null".into()),
        lyrics_endpoint: Some(format!("http://{address}/mobi.s")),
        credentials: Some(Arc::clone(&backend)),
        qr_interval: None,
        qr_lifetime: None,
        video_library: None,
        startup_paths: None,
    };
    let service = Service::start_with(options());
    await_event(
        &service,
        |e| matches!(e, Event::Auth(v) if !v.busy && v.account.is_none()),
    );
    service.commands.send(Command::ShowHistory).unwrap();
    await_event(
        &service,
        |e| matches!(e, Event::History(v) if !v.loading && v.tracks.len() == 1 && v.tracks[0].id == 11),
    );
    service
        .commands
        .send(Command::RequestSms("+86 138 0000 0000".into()))
        .unwrap();
    await_event(
        &service,
        |e| matches!(e, Event::Auth(v) if v.message.contains("已发送")),
    );
    service
        .commands
        .send(Command::RequestSms("13800000000".into()))
        .unwrap();
    await_event(
        &service,
        |e| matches!(e, Event::Auth(v) if v.message.contains("过于频繁")),
    );
    service
        .commands
        .send(Command::LoginPhone("13800000000".into(), "123456".into()))
        .unwrap();
    await_event(
        &service,
        |e| matches!(e, Event::Auth(v) if !v.busy && v.account.as_ref().is_some_and(|a| a.uid == "42")),
    );
    await_event(
        &service,
        |e| matches!(e, Event::Player(v) if v.title == "账号曲目"),
    );
    service.commands.send(Command::ShowHistory).unwrap();
    await_event(
        &service,
        |e| matches!(e, Event::History(v) if !v.loading && v.tracks.len() == 1 && v.tracks[0].id == 22),
    );
    service
        .commands
        .send(Command::SwitchAccount("43".into()))
        .unwrap();
    await_event(
        &service,
        |e| matches!(e, Event::Auth(v) if !v.busy && v.account.as_ref().is_some_and(|account| account.uid == "43")),
    );
    await_event(
        &service,
        |e| matches!(e, Event::Player(view) if view.title == "另一个账号曲目"),
    );
    service
        .commands
        .send(Command::SwitchAccount("42".into()))
        .unwrap();
    await_event(
        &service,
        |e| matches!(e, Event::Auth(v) if !v.busy && v.account.as_ref().is_some_and(|account| account.uid == "42")),
    );
    await_event(
        &service,
        |e| matches!(e, Event::Player(view) if view.title == "账号曲目"),
    );
    service
        .commands
        .send(Command::ForgetAccount("44".into()))
        .unwrap();
    await_event(
        &service,
        |e| matches!(e, Event::Auth(view) if !view.busy && view.message.contains("已移除")),
    );
    await_event(
        &service,
        |e| matches!(e, Event::Accounts(accounts) if accounts.entries.len() == 2 && accounts.entries.iter().all(|entry| entry.uid != "44")),
    );
    drop(service);
    let service = Service::start_with(options());
    await_event(
        &service,
        |e| matches!(e, Event::Auth(v) if !v.busy && v.account.as_ref().is_some_and(|a| a.uid == "42")),
    );
    await_event(
        &service,
        |e| matches!(e, Event::Player(v) if v.title == "账号曲目"),
    );
    service.commands.send(Command::SignOut).unwrap();
    await_event(
        &service,
        |e| matches!(e, Event::Auth(v) if !v.busy && v.account.is_none()),
    );
    await_event(
        &service,
        |e| matches!(e, Event::Player(v) if v.title == "匿名曲目"),
    );
    service.commands.send(Command::ShowHistory).unwrap();
    await_event(
        &service,
        |e| matches!(e, Event::History(v) if !v.loading && v.tracks.len() == 1 && v.tracks[0].id == 11),
    );
    service
        .commands
        .send(Command::LoginPhone("13800000000".into(), "123456".into()))
        .unwrap();
    await_event(
        &service,
        |e| matches!(e, Event::Auth(v) if !v.busy && v.account.is_some()),
    );
    service
        .commands
        .send(Command::Search("test".into()))
        .unwrap();
    await_event(
        &service,
        |e| matches!(e, Event::Auth(v) if !v.busy && v.account.is_none() && v.message.contains("已失效")),
    );
    drop(service);
    assert_eq!(
        queues.load("anonymous").unwrap().tracks[0].title,
        "匿名曲目"
    );
    assert_eq!(queues.load("42").unwrap().tracks[0].title, "账号曲目");
    assert_eq!(queues.load("43").unwrap().tracks[0].title, "另一个账号曲目");
    let saved = CredentialStore::new(Arc::clone(&backend));
    assert!(saved.load().unwrap().is_none());
    assert!(
        saved
            .remembered()
            .unwrap()
            .iter()
            .any(|entry| entry.uid == "42" && entry.stale)
    );
    assert!(
        saved
            .remembered()
            .unwrap()
            .iter()
            .any(|entry| entry.uid == "43" && !entry.stale)
    );
    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn library_opens_a_validated_album_through_the_desktop_service() {
    use crate::browse::BrowseContent;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..4 {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut buffer = [0; 4096];
            let count = socket.read(&mut buffer).unwrap();
            let request = String::from_utf8_lossy(&buffer[..count]);
            let body = if request.starts_with("GET /api/play/music/library/navigation?") {
                r#"{"code":200,"data":[{"id":"2005","ptypeName":"流行","childList":[{"id":"8101","name":"华语"}]}]}"#
            } else if request.starts_with("GET /api/play/music/library/albums?") {
                r#"{"code":200,"data":{"list":[{"id":11,"name":"乐库专辑"}]}}"#
            } else if request.starts_with("GET /api/service/album/music/11?") {
                r#"{"code":200,"data":{"resultList":[{"id":123,"name":"专辑歌曲"}]}}"#
            } else if request.starts_with("GET /api/service/album/11?") {
                r#"{"code":200,"data":{"albumInfo":{"id":11,"name":"乐库专辑"}}}"#
            } else {
                panic!("unexpected request")
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
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("bowen-library-{}-{unique}", std::process::id()));
    let service = Service::start_with(ServiceOptions {
        paths: Some(DataPaths::new(&root)),
        base_url: Some(format!("http://{address}/api/")),
        library: Some(root.join("missing-mpv.dll")),
        ..Default::default()
    });
    fn wait(service: &Service, predicate: impl Fn(&Event) -> bool) -> Event {
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            if let Ok(event) = service.events.recv_timeout(Duration::from_millis(100))
                && predicate(&event)
            {
                return event;
            }
        }
        panic!("expected event did not arrive")
    }
    service.commands.send(Command::LoadLibrary).unwrap();
    let Event::Library(view) = wait(
        &service,
        |event| matches!(event, Event::Library(view) if !view.loading && view.groups.len() == 1),
    ) else {
        unreachable!()
    };
    service
        .commands
        .send(Command::LibrarySelectGroup(0, view.generation))
        .unwrap();
    let Event::Library(view) = wait(
        &service,
        |event| matches!(event, Event::Library(view) if !view.loading && view.albums.len() == 1),
    ) else {
        unreachable!()
    };
    service
        .commands
        .send(Command::LibraryOpenAlbum(
            Some(0),
            11,
            view.generation.wrapping_add(1),
        ))
        .unwrap();
    wait(
        &service,
        |event| matches!(event, Event::Status(message) if message.contains("变化")),
    );
    service
        .commands
        .send(Command::LibraryOpenAlbum(Some(0), 11, view.generation))
        .unwrap();
    wait(&service, |event| matches!(event, Event::LibraryAlbumOpened));
    wait(
        &service,
        |event| matches!(event, Event::Browse(view) if !view.loading && matches!(&view.content, BrowseContent::Tracks(tracks) if tracks.first().is_some_and(|track| track.id == 123))),
    );
    drop(service);
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn discovery_opens_a_validated_playlist_through_the_desktop_service() {
    use crate::browse::BrowseContent;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for _ in 0..3 {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut buffer = [0; 4096];
            let count = socket.read(&mut buffer).unwrap();
            let request = String::from_utf8_lossy(&buffer[..count]);
            let body = if request.starts_with("GET /api/service/home/index?") {
                r#"{"code":200,"data":{"moduleList":[{"id":3,"type":5,"name":"精选"}]}}"#
            } else if request.starts_with("GET /api/service/home/module?") {
                r#"{"code":200,"data":{"moduleName":"精选","songList":[{"id":99,"name":"发现歌单","sourceType":7}]}}"#
            } else if request.starts_with("GET /api/service/playlist/99/musicList?") {
                r#"{"code":200,"data":{"list":[{"id":123,"name":"发现歌曲"}]}}"#
            } else {
                panic!("unexpected request")
            };
            requests.push(request.into_owned());
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
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("bowen-home-{}-{unique}", std::process::id()));
    let service = Service::start_with(ServiceOptions {
        paths: Some(DataPaths::new(&root)),
        base_url: Some(format!("http://{address}/api/")),
        library: Some(root.join("missing-mpv.dll")),
        ..Default::default()
    });
    fn wait(service: &Service, predicate: impl Fn(&Event) -> bool) -> Event {
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            if let Ok(event) = service.events.recv_timeout(Duration::from_millis(100))
                && predicate(&event)
            {
                return event;
            }
        }
        panic!("expected event did not arrive")
    }
    service.commands.send(Command::LoadHome).unwrap();
    let Event::Home(view) = wait(
        &service,
        |event| matches!(event, Event::Home(view) if !view.loading && view.items.len() == 1),
    ) else {
        unreachable!()
    };
    service
        .commands
        .send(Command::HomeOpenPlaylist(
            0,
            0,
            99,
            view.generation.wrapping_add(1),
        ))
        .unwrap();
    wait(
        &service,
        |event| matches!(event, Event::Status(message) if message.contains("变化")),
    );
    service
        .commands
        .send(Command::HomeOpenPlaylist(0, 0, 99, view.generation))
        .unwrap();
    wait(&service, |event| matches!(event, Event::HomePlaylistOpened));
    wait(
        &service,
        |event| matches!(event, Event::Browse(view) if !view.loading && matches!(&view.content, BrowseContent::Tracks(tracks) if tracks.first().is_some_and(|track| track.id == 123))),
    );
    drop(service);
    let requests = server.join().unwrap();
    assert!(requests[2].contains("source=7"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn service_opens_playlist_search_and_returns_to_results() {
    use crate::browse::BrowseContent;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..12 {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let count = socket.read(&mut request).unwrap();
            let line = String::from_utf8_lossy(&request[..count]);
            let body = if line.starts_with("GET /api/search/comprehensive/v2/list?") {
                r#"{"code":200,"data":{"content":[{"musicpage":[{"id":99,"name":"歌曲"}],"songlistpage":[{"id":12,"name":"综合歌单","source":13}],"albumpage":[{"id":22,"name":"专辑"}],"artistpage":[{"artistId":32,"name":"歌手"}]}]}}"#
            } else if line.starts_with("GET /api/service/playlist/12/musicList?") {
                assert!(line.contains("source=13"));
                r#"{"code":200,"data":{"list":[{"id":128,"name":"综合歌曲"}]}}"#
            } else if line.starts_with("GET /api/search/playlist/list?") {
                r#"{"code":200,"data":{"resultList":[{"id":11,"name":"歌单甲","source":4}]}}"#
            } else if line.starts_with("GET /api/service/playlist/11/musicList?") {
                r#"{"code":200,"data":{"list":[{"id":123,"name":"歌曲甲"}]}}"#
            } else if line.starts_with("GET /api/search/album/list?") {
                r#"{"code":200,"data":{"resultList":[{"id":21,"name":"专辑甲"}]}}"#
            } else if line.starts_with("GET /api/service/album/music/21?") {
                r#"{"code":200,"data":{"resultList":[{"id":124,"name":"专辑歌曲"}]}}"#
            } else if line.starts_with("GET /api/search/artist/list?") {
                r#"{"code":200,"data":{"resultList":[{"artistId":31,"name":"歌手甲"}]}}"#
            } else if line.starts_with("GET /api/service/artist/music/31?") {
                r#"{"code":200,"data":{"resultList":[{"id":125,"name":"歌手歌曲"}]}}"#
            } else if line.starts_with("GET /api/service/artist/album/31?") {
                r#"{"code":200,"data":{"resultList":[{"id":41,"name":"歌手专辑"}]}}"#
            } else if line.starts_with("GET /api/service/album/music/41?") {
                r#"{"code":200,"data":{"resultList":[{"id":126,"name":"专辑内歌曲"}]}}"#
            } else if line.starts_with("GET /api/service/home/bangNew?") {
                r#"{"code":200,"data":[{"moduleName":"榜单分组","bangList":[{"id":16,"name":"新歌榜"}]}]}"#
            } else if line.starts_with("GET /api/service/bang/16/musics?") {
                r#"{"code":200,"data":{"musics":[{"id":127,"name":"榜单歌曲"}]}}"#
            } else {
                panic!("unexpected request")
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
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("bowen-browse-test-{}-{unique}", std::process::id()));
    let service = Service::start_with(ServiceOptions {
        paths: Some(DataPaths::new(&root)),
        base_url: Some(format!("http://{address}/api/")),
        library: Some(root.join("missing-mpv")),
        ..Default::default()
    });
    service
        .commands
        .send(Command::SearchPlaylists("测试".into()))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut phase = 0;
    while Instant::now() < deadline {
        if let Ok(Event::Browse(view)) = service.events.recv_timeout(Duration::from_millis(200))
            && !view.loading
        {
            match (phase, view.content) {
                (0, BrowseContent::Playlists(items)) if !items.is_empty() => {
                    service
                        .commands
                        .send(Command::OpenPlaylist(items[0].clone()))
                        .unwrap();
                    phase = 1;
                }
                (1, BrowseContent::Tracks(tracks)) if !tracks.is_empty() => {
                    assert_eq!(tracks[0].id, 123);
                    service.commands.send(Command::BrowseBack).unwrap();
                    phase = 2;
                }
                (2, BrowseContent::Playlists(items)) if items.len() == 1 => {
                    service
                        .commands
                        .send(Command::SearchAlbums("测试".into()))
                        .unwrap();
                    phase = 3;
                }
                (3, BrowseContent::Albums(items)) if !items.is_empty() => {
                    service
                        .commands
                        .send(Command::OpenAlbum(items[0].clone()))
                        .unwrap();
                    phase = 4;
                }
                (4, BrowseContent::Tracks(items)) if !items.is_empty() => {
                    assert_eq!(items[0].id, 124);
                    service
                        .commands
                        .send(Command::SearchArtists("测试".into()))
                        .unwrap();
                    phase = 5;
                }
                (5, BrowseContent::Artists(items)) if !items.is_empty() => {
                    service
                        .commands
                        .send(Command::OpenArtist(items[0].clone()))
                        .unwrap();
                    phase = 6;
                }
                (6, BrowseContent::Tracks(items)) if !items.is_empty() => {
                    assert_eq!(items[0].id, 125);
                    service.commands.send(Command::ArtistAlbums).unwrap();
                    phase = 7;
                }
                (7, BrowseContent::Albums(items)) if !items.is_empty() => {
                    service
                        .commands
                        .send(Command::OpenAlbum(items[0].clone()))
                        .unwrap();
                    phase = 8;
                }
                (8, BrowseContent::Tracks(items)) if !items.is_empty() => {
                    assert_eq!(items[0].id, 126);
                    service.commands.send(Command::BrowseBack).unwrap();
                    phase = 9;
                }
                (9, BrowseContent::Albums(items)) if !items.is_empty() && view.artist_page => {
                    service.commands.send(Command::ArtistTracks).unwrap();
                    phase = 10;
                }
                (10, BrowseContent::Tracks(items)) if !items.is_empty() => {
                    assert_eq!(items[0].id, 125);
                    service.commands.send(Command::Charts).unwrap();
                    phase = 11;
                }
                (11, BrowseContent::Charts(sections)) if !sections.is_empty() => {
                    service
                        .commands
                        .send(Command::OpenChart(sections[0].charts[0].clone()))
                        .unwrap();
                    phase = 12;
                }
                (12, BrowseContent::Tracks(items)) if !items.is_empty() => {
                    assert_eq!(items[0].id, 127);
                    service.commands.send(Command::BrowseBack).unwrap();
                    phase = 13;
                }
                (13, BrowseContent::Charts(sections)) if !sections.is_empty() => {
                    assert_eq!(sections[0].charts[0].id, 16);
                    phase = 14;
                    break;
                }
                _ => {}
            }
        }
    }
    assert_eq!(phase, 14);
    service
        .commands
        .send(Command::SearchOverview(40, "测试".into()))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut saw_opened = false;
    let mut saw_tracks = false;
    while Instant::now() < deadline && !(saw_opened && saw_tracks) {
        match service.events.recv_timeout(Duration::from_millis(200)) {
            Ok(Event::SearchOverview(40, Ok(sections))) => {
                assert!(matches!(
                    sections.as_slice(),
                    [
                        SearchSection::Tracks(_),
                        SearchSection::Playlists(_),
                        SearchSection::Albums(_),
                        SearchSection::Artists(_),
                    ]
                ));
                service
                    .commands
                    .send(Command::SelectSearchOverview(39, 1, 0))
                    .unwrap();
                service
                    .commands
                    .send(Command::SelectSearchOverview(40, 1, 0))
                    .unwrap();
            }
            Ok(Event::SearchOverviewOpened(generation)) => {
                assert_eq!(generation, 40);
                saw_opened = true;
            }
            Ok(Event::Browse(view)) if !view.loading && view.selected_playlist_id == Some(12) => {
                assert!(
                    matches!(view.content, BrowseContent::Tracks(tracks) if tracks[0].id == 128)
                );
                saw_tracks = true;
            }
            _ => {}
        }
    }
    assert!(saw_opened && saw_tracks);
    drop(service);
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn service_play_all_loads_remaining_playlist_pages_into_queue() {
    use crate::browse::BrowseContent;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let server_stop = Arc::clone(&stop);
    let server = std::thread::spawn(move || {
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
            let mut request = [0; 4096];
            let count = socket.read(&mut request).unwrap();
            if count == 0 {
                continue;
            }
            let line = String::from_utf8_lossy(&request[..count]);
            let body = if line.starts_with("GET /api/search/playlist/list?") {
                r#"{"code":200,"data":{"resultList":[{"id":11,"name":"歌单","source":4}]}}"#
            } else if line.starts_with("GET /api/service/playlist/11/musicList?pn=1&") {
                r#"{"code":200,"data":{"list":[{"id":123,"name":"第一首"}]}}"#
            } else if line.starts_with("GET /api/service/playlist/11/musicList?pn=2&") {
                r#"{"code":200,"data":{"list":[{"id":124,"name":"第二首"}]}}"#
            } else if line.starts_with("GET /api/service/playlist/11/musicList?pn=3&") {
                r#"{"code":200,"data":{"list":[]}}"#
            } else if line.starts_with("GET /api/play/music/v2/checkRight?") {
                r#"{"code":200,"data":{"status":7}}"#
            } else if line.starts_with("GET /mobi.s?") {
                r#"{"code":200,"data":{"content":"WzAwOjAwLjAwXUJvd2Vu"}}"#
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
    let root = std::env::temp_dir().join(format!(
        "bowen-play-all-test-{}-{unique}",
        std::process::id()
    ));
    let service = Service::start_with(ServiceOptions {
        paths: Some(DataPaths::new(&root)),
        base_url: Some(format!("http://{address}/api/")),
        library: Some(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../libmpv/libmpv-2.dll"),
        ),
        audio_output: Some("null".into()),
        lyrics_endpoint: Some(format!("http://{address}/mobi.s")),
        ..Default::default()
    });
    service
        .commands
        .send(Command::SearchPlaylists("测试".into()))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut phase = 0;
    while Instant::now() < deadline {
        match service.events.recv_timeout(Duration::from_millis(200)) {
            Ok(Event::Browse(view)) if !view.loading => match (phase, view.content) {
                (0, BrowseContent::Playlists(items)) if !items.is_empty() => {
                    service
                        .commands
                        .send(Command::OpenPlaylist(items[0].clone()))
                        .unwrap();
                    phase = 1;
                }
                (1, BrowseContent::Tracks(tracks)) if !tracks.is_empty() => {
                    service.commands.send(Command::PlayAllPlaylist).unwrap();
                    phase = 2;
                }
                _ => {}
            },
            Ok(Event::Player(view)) if phase == 2 && view.queue.len() == 2 => {
                phase = 3;
                break;
            }
            _ => {}
        }
    }
    assert_eq!(phase, 3);
    drop(service);
    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
