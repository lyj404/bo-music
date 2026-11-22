use super::*;

#[tokio::test]
async fn artist_albums_page_and_return_from_album_preserve_navigation() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..5 {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let count = socket.read(&mut request).unwrap();
            let line = String::from_utf8_lossy(&request[..count]);
            let body = if line.starts_with("GET /api/search/artist/list?pn=0&") {
                r#"{"code":200,"data":{"resultList":[{"artistId":31,"name":"歌手甲"}]}}"#
            } else if line.starts_with("GET /api/service/artist/music/31?pn=1&") {
                r#"{"code":200,"data":{"resultList":[{"id":102,"name":"歌手歌曲"}]}}"#
            } else if line.starts_with("GET /api/service/artist/album/31?pn=1&") {
                r#"{"code":200,"data":{"resultList":[{"id":41,"name":"专辑甲"}]}}"#
            } else if line.starts_with("GET /api/service/artist/album/31?pn=2&") {
                r#"{"code":200,"data":{"resultList":[]}}"#
            } else if line.starts_with("GET /api/service/album/music/41?pn=1&") {
                r#"{"code":200,"data":{"resultList":[{"id":103,"name":"专辑歌曲"}]}}"#
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
    let session = Session::default();
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
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut browser = Browser::default();
    browser.search_kind(
        Arc::clone(&transport),
        tx.clone(),
        SearchKind::Artist,
        "测试".into(),
    );
    finish_next(&mut browser, &mut rx, &session).await;
    let BrowseContent::Artists(artists) = browser.view().content else {
        panic!("expected artists")
    };
    browser.open_artist(Arc::clone(&transport), tx.clone(), artists[0].clone());
    finish_next(&mut browser, &mut rx, &session).await;
    assert!(browser.view().artist_page && !browser.view().artist_albums_active);
    browser.show_artist_albums(Arc::clone(&transport), tx.clone());
    finish_next(&mut browser, &mut rx, &session).await;
    let BrowseContent::Albums(albums) = browser.view().content else {
        panic!("expected albums")
    };
    assert_eq!(albums[0].id, 41);
    assert!(browser.view().has_more && browser.view().is_detail);
    browser.next(Arc::clone(&transport), tx.clone());
    finish_next(&mut browser, &mut rx, &session).await;
    assert!(!browser.view().has_more);
    browser.open_album(Arc::clone(&transport), tx.clone(), albums[0].clone());
    finish_next(&mut browser, &mut rx, &session).await;
    assert!(
        matches!(browser.view().content, BrowseContent::Tracks(ref tracks) if tracks[0].id == 103)
    );
    browser.back();
    assert!(browser.view().artist_albums_active && !browser.view().has_more);
    assert!(
        matches!(browser.view().content, BrowseContent::Albums(ref items) if items.len() == 1 && items[0].id == 41)
    );
    browser.show_artist_tracks(transport, tx);
    assert!(
        matches!(browser.view().content, BrowseContent::Tracks(ref tracks) if tracks[0].id == 102)
    );
    browser.back();
    assert!(
        matches!(browser.view().content, BrowseContent::Artists(ref items) if items.len() == 1)
    );
    server.join().unwrap();
}

#[tokio::test]
async fn mine_uses_account_source_deduplicates_and_rejects_stale_results() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let count = socket.read(&mut request).unwrap();
            let line = String::from_utf8_lossy(&request[..count])
                .lines()
                .next()
                .unwrap()
                .to_owned();
            let body = if line.starts_with("GET /api/service/playlist/fond?") {
                r#"{"code":200,"data":{"id":9,"name":"喜欢"}}"#
            } else if line.starts_with("GET /api/service/playlist/userCreate?") {
                r#"{"code":200,"data":{"playLists":[{"id":9,"name":"喜欢"},{"id":10,"name":"自建"}]}}"#
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
    let session = Session::default();
    session.set("42".into(), "synthetic".into());
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
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut browser = Browser::default();
    browser.mine(Arc::clone(&transport), tx);
    finish_next(&mut browser, &mut rx, &session).await;
    let BrowseContent::Playlists(items) = browser.view().content else {
        panic!("expected playlists")
    };
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|item| item.source_type == 5));
    let revision = session.snapshot().revision;
    assert!(browser.is_mine_list(revision));
    assert!(!browser.add_created_playlist(revision, 9, "冲突", false));
    browser.selected = Some(Selection::Playlist(items[0].clone()));
    assert!(!browser.editable_playlist(9, revision));
    assert!(browser.view().editable_playlist.is_none());
    browser.selected = Some(Selection::Playlist(items[1].clone()));
    assert!(browser.editable_playlist(10, revision));
    assert!(!browser.editable_playlist(10, revision + 1));
    assert!(browser.update_edited_playlist(10, revision, "新名称", "新简介", None));
    assert_eq!(browser.view().title, "新名称");
    assert!(browser.remove_deleted_playlist(10, revision));
    assert!(!browser.view().is_detail);
    assert_eq!(browser.playlists.len(), 1);
    let stale_generation = browser.generation;
    let old_revision = session.snapshot().revision;
    session.set("43".into(), "other".into());
    assert!(!browser.finish(
        stale_generation,
        old_revision,
        session.snapshot().revision,
        Ok(BrowseResult::Mine(
            vec![Playlist {
                id: 99,
                ..Default::default()
            }],
            None,
            None
        ))
    ));
    browser.clear();
    assert!(
        matches!(browser.view().content, BrowseContent::Playlists(ref lists) if lists.is_empty())
    );
    server.join().unwrap();
}

#[tokio::test]
async fn mine_keeps_available_playlists_when_one_request_fails() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let count = socket.read(&mut request).unwrap();
            let line = String::from_utf8_lossy(&request[..count]);
            let (status, body) = if line.starts_with("GET /api/service/playlist/fond?") {
                ("500 Internal Server Error", "")
            } else {
                assert!(line.starts_with("GET /api/service/playlist/userCreate?"));
                (
                    "200 OK",
                    r#"{"code":200,"data":{"playLists":[{"id":10,"name":"自建"}]}}"#,
                )
            };
            write!(
                socket,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
    });
    let session = Session::default();
    session.set("42".into(), "synthetic".into());
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
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut browser = Browser::default();
    browser.mine(transport, tx);
    finish_next(&mut browser, &mut rx, &session).await;
    let view = browser.view();
    assert!(
        matches!(view.content, BrowseContent::Playlists(ref items) if items.len() == 1 && items[0].id == 10)
    );
    assert!(view.message.contains("部分"));
    browser.selected = Some(Selection::Playlist(Playlist {
        id: 10,
        source_type: 5,
        ..Default::default()
    }));
    assert!(!browser.editable_playlist(10, session.snapshot().revision));
    assert!(view.has_more);
    server.join().unwrap();
}

#[tokio::test]
async fn play_all_loads_remaining_pages_and_keeps_partial_results_on_failure() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for attempt in 0..3 {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let count = socket.read(&mut request).unwrap();
            let line = String::from_utf8_lossy(&request[..count]);
            let (status, body) = match attempt {
                0 => {
                    assert!(line.starts_with("GET /api/service/playlist/11/musicList?pn=2&"));
                    (
                        "200 OK",
                        r#"{"code":200,"data":{"list":[{"id":124,"name":"第二首"}]}}"#,
                    )
                }
                1 => {
                    assert!(line.starts_with("GET /api/service/playlist/11/musicList?pn=3&"));
                    ("500 Internal Server Error", "")
                }
                _ => {
                    assert!(line.starts_with("GET /api/service/playlist/11/musicList?pn=3&"));
                    ("200 OK", r#"{"code":200,"data":{"list":[]}}"#)
                }
            };
            write!(
                socket,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
    });
    let session = Session::default();
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
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut browser = Browser {
        selected: Some(Selection::Playlist(Playlist {
            id: 11,
            name: "歌单".into(),
            source_type: 4,
            ..Default::default()
        })),
        ..Default::default()
    };
    browser.tracks.push(track(123));
    browser.track_cursor.advance(1);
    browser.play_all(Arc::clone(&transport), tx.clone());
    finish_next(&mut browser, &mut rx, &session).await;
    let partial = browser.take_play_all().unwrap();
    assert_eq!(
        partial.iter().map(|item| item.id).collect::<Vec<_>>(),
        vec![123, 124]
    );
    assert!(browser.view().error && browser.view().has_more);
    browser.play_all(transport, tx);
    finish_next(&mut browser, &mut rx, &session).await;
    assert_eq!(browser.take_play_all().unwrap().len(), 2);
    assert!(!browser.view().has_more);
    server.join().unwrap();
}

#[tokio::test]
async fn cancelling_play_all_discards_late_pages() {
    let session = Session::default();
    let transport = Arc::new(
        Transport::new(
            TransportOptions::default(),
            session.clone(),
            "device".into(),
        )
        .unwrap(),
    );
    let (tx, _) = mpsc::unbounded_channel();
    let mut browser = Browser {
        selected: Some(Selection::Playlist(Playlist {
            id: 11,
            source_type: 4,
            ..Default::default()
        })),
        ..Default::default()
    };
    browser.tracks.push(track(123));
    browser.track_cursor.advance(1);
    browser.play_all(transport, tx);
    let generation = browser.generation;
    let revision = session.snapshot().revision;
    assert!(browser.cancel_play_all());
    assert!(!browser.finish(
        generation,
        revision,
        revision,
        Ok(BrowseResult::AllTracks(
            service_cursor(30),
            vec![track(999)],
            None
        ))
    ));
    assert!(browser.take_play_all().is_none());
    assert!(
        matches!(browser.view().content, BrowseContent::Tracks(ref tracks) if tracks.len() == 1 && tracks[0].id == 123)
    );
}

#[tokio::test]
async fn switching_search_category_discards_old_results() {
    let session = Session::default();
    let transport = Arc::new(
        Transport::new(
            TransportOptions::default(),
            session.clone(),
            "device".into(),
        )
        .unwrap(),
    );
    let (tx, _) = mpsc::unbounded_channel();
    let mut browser = Browser::default();
    browser.search_kind(
        Arc::clone(&transport),
        tx.clone(),
        SearchKind::Album,
        "测试".into(),
    );
    let old_generation = browser.generation;
    let revision = session.snapshot().revision;
    browser.search_kind(transport, tx, SearchKind::Artist, "测试".into());
    assert!(!browser.finish(
        old_generation,
        revision,
        revision,
        Ok(BrowseResult::Albums(search_cursor(30), Vec::new()))
    ));
    assert!(matches!(browser.view().content, BrowseContent::Artists(_)));
    assert!(browser.view().loading);
    browser.cancel_pending();
}
