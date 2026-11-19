use super::*;

#[test]
fn detail_view_carries_the_selected_cover_and_clears_it_on_back() {
    let mut browser = Browser {
        selected: Some(Selection::Playlist(Playlist {
            id: 42,
            name: "playlist".into(),
            cover_url: Some("https://example.test/cover.jpg".into()),
            ..Default::default()
        })),
        ..Default::default()
    };
    assert_eq!(
        browser.view().cover_url.as_deref(),
        Some("https://example.test/cover.jpg")
    );
    browser.back();
    assert_eq!(browser.view().cover_url, None);
}

#[test]
fn share_target_preserves_playlist_source_and_rejects_stale_pages() {
    let mut browser = Browser {
        generation: 7,
        selected: Some(Selection::Playlist(Playlist {
            id: 11,
            source_type: 13,
            ..Default::default()
        })),
        ..Default::default()
    };
    assert_eq!(
        browser.share_target(7),
        Some((ShareTarget::Playlist { source: 13 }, 11))
    );
    assert!(browser.view().can_share);
    assert!(browser.share_target(6).is_none());
    browser.selected = Some(Selection::Playlist(Playlist {
        id: 11,
        source_type: 0,
        ..Default::default()
    }));
    assert_eq!(
        browser.share_target(7),
        Some((ShareTarget::Playlist { source: 4 }, 11))
    );
    browser.selected = Some(Selection::Playlist(Playlist::default()));
    assert!(!browser.view().can_share);
    browser.selected = Some(Selection::Album(Album {
        id: 22,
        source_type: 0,
        name: String::new(),
        artist_text: String::new(),
        artist_id: 0,
        artists: vec![],
        cover_url: None,
        music_count: 0,
        artist_cover_url: None,
        release_date: String::new(),
        description: String::new(),
    }));
    assert_eq!(browser.share_target(7), Some((ShareTarget::Album, 22)));
    browser.selected = Some(Selection::Artist(Artist {
        id: 33,
        name: String::new(),
        cover_url: None,
        song_count: 0,
        album_count: 0,
        alias: String::new(),
        fans_count: 0,
        description: String::new(),
    }));
    assert_eq!(browser.share_target(7), Some((ShareTarget::Artist, 33)));
}

#[tokio::test]
async fn playlist_square_retries_the_same_short_page_and_restores_category_after_detail() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for (expected, body) in [
            (
                "/api/service/category/list?",
                r#"{"code":200,"data":{"categories":[{"name":"曲风","subCategories":[{"id":77,"name":"古典"}]}]}}"#,
            ),
            (
                "/api/service/category/77/playlist?pn=1&",
                r#"{"code":200,"data":{"playLists":[{"id":11,"name":"第一张","sourceType":4}]}}"#,
            ),
            ("/api/service/category/77/playlist?pn=2&", r#"{"code":500}"#),
            (
                "/api/service/category/77/playlist?pn=2&",
                r#"{"code":200,"data":{"playLists":[{"id":12,"name":"第二张","sourceType":4}]}}"#,
            ),
            (
                "/api/service/playlist/11/musicList?",
                r#"{"code":200,"data":{"list":[{"id":123,"name":"第一首"}]}}"#,
            ),
        ] {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let count = socket.read(&mut request).unwrap();
            let line = String::from_utf8_lossy(&request[..count]);
            assert!(line.starts_with(&format!("GET {expected}")), "{line}");
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
    browser.playlist_square(Arc::clone(&transport), tx.clone());
    finish_next(&mut browser, &mut rx, &session).await;
    let view = browser.view();
    assert_eq!(view.square.unwrap().groups[0].categories[0].id, 77);
    assert!(!browser.select_playlist_category(
        Arc::clone(&transport),
        tx.clone(),
        77,
        view.generation.wrapping_sub(1)
    ));
    assert!(!browser.select_playlist_category(
        Arc::clone(&transport),
        tx.clone(),
        99,
        view.generation
    ));
    assert!(browser.select_playlist_category(
        Arc::clone(&transport),
        tx.clone(),
        77,
        view.generation
    ));
    finish_next(&mut browser, &mut rx, &session).await;
    browser.next(Arc::clone(&transport), tx.clone());
    finish_next(&mut browser, &mut rx, &session).await;
    let failed = browser.view();
    assert!(failed.error && failed.has_more);
    assert!(matches!(failed.content, BrowseContent::Playlists(items) if items.len() == 1));
    browser.next(Arc::clone(&transport), tx.clone());
    finish_next(&mut browser, &mut rx, &session).await;
    let BrowseContent::Playlists(items) = browser.view().content else {
        panic!("expected playlists");
    };
    assert_eq!(items.len(), 2);
    browser.open(transport, tx, items[0].clone());
    finish_next(&mut browser, &mut rx, &session).await;
    assert!(matches!(browser.view().content, BrowseContent::Tracks(items) if items[0].id == 123));
    browser.back();
    let view = browser.view();
    assert_eq!(view.square.unwrap().category, Some(77));
    assert!(matches!(view.content, BrowseContent::Playlists(items) if items.len() == 2));
    server.join().unwrap();
}

#[tokio::test]
async fn collections_page_mixed_results_and_reject_old_account_responses() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for index in 0..5 {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let count = socket.read(&mut request).unwrap();
            let line = String::from_utf8_lossy(&request[..count]);
            let body = match index {
                0 => {
                    assert!(line.contains("pn=1"));
                    r#"{"code":200,"data":{"playLists":[{"id":21,"name":"专辑","sourceType":6}]}}"#
                }
                1 => {
                    assert!(line.contains("pn=2"));
                    r#"{"code":200,"data":{"playLists":[{"id":11,"name":"歌单","sourceType":4}]}}"#
                }
                2 => {
                    assert!(line.contains("/service/playlist/11/musicList?"));
                    assert!(line.contains("source=4"));
                    r#"{"code":200,"data":{"list":[{"id":101,"name":"歌曲"}]}}"#
                }
                3 => {
                    r#"{"code":200,"data":{"playLists":[{"id":21,"name":"专辑","sourceType":6}]}}"#
                }
                _ => {
                    assert!(line.contains("/service/collect/7/list?"));
                    r#"{"code":200,"data":{"artistList":[{"id":31,"name":"歌手"}]}}"#
                }
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
    browser.collection(transport.clone(), tx.clone(), SearchKind::Playlist);
    finish_next(&mut browser, &mut rx, &session).await;
    assert!(browser.view().has_more);
    assert!(
        matches!(browser.view().content, BrowseContent::Playlists(ref items) if items.is_empty())
    );
    browser.next(transport.clone(), tx.clone());
    finish_next(&mut browser, &mut rx, &session).await;
    assert!(
        matches!(browser.view().content, BrowseContent::Playlists(ref items) if items.len() == 1 && items[0].id == 11)
    );
    let BrowseContent::Playlists(items) = browser.view().content else {
        panic!("expected playlists")
    };
    browser.open(transport.clone(), tx.clone(), items[0].clone());
    finish_next(&mut browser, &mut rx, &session).await;
    assert!(
        matches!(browser.view().content, BrowseContent::Tracks(ref items) if items.len() == 1 && items[0].id == 101)
    );
    browser.back();
    assert!(
        matches!(browser.view().content, BrowseContent::Playlists(ref items) if items.len() == 1 && items[0].id == 11)
    );
    browser.collection(transport.clone(), tx.clone(), SearchKind::Album);
    finish_next(&mut browser, &mut rx, &session).await;
    assert!(
        matches!(browser.view().content, BrowseContent::Albums(ref items) if items.len() == 1 && items[0].id == 21)
    );
    browser.collection(transport.clone(), tx, SearchKind::Artist);
    finish_next(&mut browser, &mut rx, &session).await;
    assert!(!browser.view().has_more);
    assert!(
        matches!(browser.view().content, BrowseContent::Artists(ref items) if items.len() == 1 && items[0].id == 31)
    );
    let generation = browser.generation;
    let old = session.snapshot().revision;
    session.set("43".into(), "other".into());
    assert!(!browser.finish(
        generation,
        old,
        session.snapshot().revision,
        Ok(BrowseResult::Following(Vec::new()))
    ));
    server.join().unwrap();
}

#[tokio::test]
async fn anonymous_collection_can_retry_without_advancing_page() {
    let session = Session::default();
    let transport = Arc::new(
        Transport::new(
            TransportOptions::default(),
            session.clone(),
            "device".into(),
        )
        .unwrap(),
    );
    let (tx, mut rx) = mpsc::unbounded_channel();
    let mut browser = Browser::default();
    browser.collection(transport.clone(), tx.clone(), SearchKind::Playlist);
    finish_next(&mut browser, &mut rx, &session).await;
    assert!(browser.view().error && browser.view().has_more);
    browser.next(transport, tx);
    finish_next(&mut browser, &mut rx, &session).await;
    assert!(browser.view().error && browser.view().has_more);
    assert_eq!(browser.list_cursor.page_number(), 1);
}

#[tokio::test]
async fn collection_write_is_serialized_and_rejects_a_changed_account() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for (index, body) in [
            r#"{"code":200,"data":{"result":[{"id":21,"collect":false}]}}"#,
            r#"{"code":200,"data":{}}"#,
        ]
        .into_iter()
        .enumerate()
        {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buffer = [0; 4096];
            let count = socket.read(&mut buffer).unwrap();
            let request = String::from_utf8_lossy(&buffer[..count]);
            if index == 0 {
                assert!(request.starts_with("GET /api/service/collect/multipleState?"));
            } else {
                assert!(request.starts_with("POST /api/service/collect?"));
            }
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
    let mut browser = Browser {
        selected: Some(Selection::Album(Album {
            id: 21,
            source_type: 6,
            name: "专辑".into(),
            artist_text: String::new(),
            artist_id: 0,
            artists: Vec::new(),
            cover_url: None,
            music_count: 0,
            artist_cover_url: None,
            release_date: String::new(),
            description: String::new(),
        })),
        ..Default::default()
    };
    browser.load_collection_state(transport.clone(), tx.clone());
    let Command::CollectionStateDone(generation, revision, target, state) =
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("unexpected event")
    };
    assert!(browser.finish_collection_state(
        generation,
        revision,
        session.snapshot().revision,
        target,
        state
    ));
    assert_eq!(
        browser.view().collection_state,
        CollectionState::Ready(false)
    );
    browser.write_collection(transport.clone(), tx.clone());
    browser.write_collection(transport.clone(), tx.clone());
    browser.load_collection_state(transport.clone(), tx.clone());
    assert_eq!(browser.view().collection_state, CollectionState::Writing);
    let Command::CollectionWriteDone(revision, target, desired, success) =
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("unexpected event")
    };
    assert!(browser.finish_collection_write(
        revision,
        session.snapshot().revision,
        target,
        desired,
        success
    ));
    assert_eq!(
        browser.view().collection_state,
        CollectionState::Ready(true)
    );
    session.set("43".into(), "other".into());
    browser.write_collection(transport, tx);
    assert_eq!(browser.view().collection_state, CollectionState::Unknown);
    assert!(rx.try_recv().is_err());
    server.join().unwrap();
}

#[test]
fn collection_write_completion_updates_a_visible_list_after_back_navigation() {
    let mut browser = Browser {
        collection: true,
        ..Default::default()
    };
    browser.playlists.push(Playlist {
        id: 11,
        ..Default::default()
    });
    let target = CollectionTarget::Playlist(11, 4);
    browser.collection_writes.insert((7, target));
    assert!(browser.finish_collection_write(7, 7, target, false, true));
    assert!(browser.playlists.is_empty());
    assert!(!browser.collection_writes.contains(&(7, target)));
    browser.playlists.push(Playlist {
        id: 11,
        ..Default::default()
    });
    browser.collection_writes.insert((7, target));
    assert!(!browser.finish_collection_write(7, 8, target, false, true));
    assert_eq!(browser.playlists.len(), 1);
}

#[test]
fn failed_collection_write_requires_rechecking_state() {
    let target = CollectionTarget::Playlist(11, 4);
    let mut browser = Browser {
        selected: Some(Selection::Playlist(Playlist {
            id: 11,
            source_type: 4,
            ..Default::default()
        })),
        collection_state: CollectionState::Writing,
        collection_state_revision: Some(7),
        ..Default::default()
    };
    browser.collection_writes.insert((7, target));
    assert!(browser.finish_collection_write(7, 7, target, true, false));
    assert_eq!(browser.view().collection_state, CollectionState::Unknown);
}

#[tokio::test]
async fn catalog_details_load_independently_and_stale_responses_are_ignored() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..6 {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let count = socket.read(&mut request).unwrap();
            let line = String::from_utf8_lossy(&request[..count]);
            let body = if line.starts_with("GET /api/search/artist/list?") {
                r#"{"code":200,"data":{"resultList":[{"artistId":31,"name":"歌手"}]}}"#
            } else if line.starts_with("GET /api/service/artist/music/31?") {
                r#"{"code":200,"data":{"resultList":[{"id":123,"name":"歌曲"}]}}"#
            } else if line.starts_with("GET /api/service/artist/31?") {
                r#"{"code":200,"data":{"artistInfo":{"name":"歌手","aliasName":"别名","fansCnt":42,"desc":"歌手简介"}}}"#
            } else if line.starts_with("GET /api/search/album/list?") {
                r#"{"code":200,"data":{"resultList":[{"id":21,"name":"专辑"}]}}"#
            } else if line.starts_with("GET /api/service/album/music/21?") {
                r#"{"code":200,"data":{"resultList":[{"id":124,"name":"专辑歌曲"}]}}"#
            } else if line.starts_with("GET /api/service/album/21?") {
                r#"{"code":200,"data":{"albumInfo":{"id":21,"name":"专辑","showtime":"2020-01-01","info":"专辑简介","musicCount":7}}}"#
            } else {
                panic!("unexpected request");
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
        "歌手".into(),
    );
    finish_next(&mut browser, &mut rx, &session).await;
    let BrowseContent::Artists(artists) = browser.view().content else {
        panic!("expected artist")
    };
    browser.open_artist(Arc::clone(&transport), tx.clone(), artists[0].clone());
    browser.load_details(Arc::clone(&transport), tx.clone());
    let generation = browser.detail_generation;
    assert!(browser.view().detail_loading);
    for _ in 0..2 {
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        match event {
            Command::BrowseDone(generation, revision, result) => {
                assert!(browser.finish(generation, revision, session.snapshot().revision, result))
            }
            Command::DetailDone(generation, revision, result) => {
                assert!(browser.finish_details(
                    generation,
                    revision,
                    session.snapshot().revision,
                    result
                ))
            }
            _ => panic!("unexpected event"),
        }
    }
    let Some(DetailView::Artist(details)) = browser.view().detail else {
        panic!("expected details")
    };
    assert_eq!(
        (
            details.alias.as_str(),
            details.fans_count,
            details.description.as_str()
        ),
        ("别名", 42, "歌手简介")
    );
    assert!(
        matches!(browser.view().content, BrowseContent::Tracks(ref tracks) if tracks[0].id == 123)
    );
    browser.back();
    assert!(!browser.finish_details(
        generation,
        session.snapshot().revision,
        session.snapshot().revision,
        Ok(DetailResult::Artist(Some(details)))
    ));
    assert!(browser.view().detail.is_none());
    browser.search_kind(
        Arc::clone(&transport),
        tx.clone(),
        SearchKind::Album,
        "专辑".into(),
    );
    finish_next(&mut browser, &mut rx, &session).await;
    let BrowseContent::Albums(albums) = browser.view().content else {
        panic!("expected album")
    };
    browser.open_album(Arc::clone(&transport), tx.clone(), albums[0].clone());
    browser.load_details(Arc::clone(&transport), tx);
    for _ in 0..2 {
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();
        match event {
            Command::BrowseDone(generation, revision, result) => {
                assert!(browser.finish(generation, revision, session.snapshot().revision, result))
            }
            Command::DetailDone(generation, revision, result) => {
                assert!(browser.finish_details(
                    generation,
                    revision,
                    session.snapshot().revision,
                    result
                ))
            }
            _ => panic!("unexpected event"),
        }
    }
    let Some(DetailView::Album(details)) = browser.view().detail else {
        panic!("expected album details")
    };
    assert_eq!(
        (
            details.release_date.as_str(),
            details.description.as_str(),
            details.music_count
        ),
        ("2020-01-01", "专辑简介", 7)
    );
    assert!(
        matches!(browser.view().content, BrowseContent::Tracks(ref tracks) if tracks[0].id == 124)
    );
    server.join().unwrap();
}

#[tokio::test]
async fn charts_retry_open_page_play_all_and_back() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for index in 0..4 {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let count = socket.read(&mut request).unwrap();
            let line = String::from_utf8_lossy(&request[..count]);
            let body = match index {
                0 => {
                    assert!(line.starts_with("GET /api/service/home/bangNew?"));
                    r#"{"code":500,"msg":"failed"}"#
                }
                1 => {
                    assert!(line.starts_with("GET /api/service/home/bangNew?"));
                    r#"{"code":200,"data":[{"moduleName":"热门榜","bangList":[{"id":16,"name":"新歌榜","pubStr":"每日更新","musics":[{"id":123,"name":"预览"}]}]}]}"#
                }
                2 => {
                    assert!(line.starts_with("GET /api/service/bang/16/musics?pn=1&"));
                    r#"{"code":200,"data":{"musics":[{"id":123,"name":"第一首"}],"total":2}}"#
                }
                3 => {
                    assert!(line.starts_with("GET /api/service/bang/16/musics?pn=2&"));
                    r#"{"code":200,"data":{"musics":[],"total":2}}"#
                }
                _ => unreachable!(),
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
    browser.charts(Arc::clone(&transport), tx.clone());
    finish_next(&mut browser, &mut rx, &session).await;
    assert!(browser.view().error && browser.view().has_more);
    browser.next(Arc::clone(&transport), tx.clone());
    finish_next(&mut browser, &mut rx, &session).await;
    let BrowseContent::Charts(sections) = browser.view().content else {
        panic!("expected charts")
    };
    assert_eq!(sections[0].title, "热门榜");
    assert_eq!(sections[0].charts[0].preview_tracks[0].title, "预览");
    browser.open_chart(
        Arc::clone(&transport),
        tx.clone(),
        sections[0].charts[0].clone(),
    );
    finish_next(&mut browser, &mut rx, &session).await;
    assert!(
        matches!(browser.view().content, BrowseContent::Tracks(ref tracks) if tracks[0].id == 123)
    );
    assert!(browser.view().has_more);
    browser.play_all(Arc::clone(&transport), tx);
    finish_next(&mut browser, &mut rx, &session).await;
    assert_eq!(browser.take_play_all().unwrap()[0].id, 123);
    assert!(!browser.view().has_more);
    browser.back();
    assert!(
        matches!(browser.view().content, BrowseContent::Charts(ref groups) if groups[0].charts[0].id == 16)
    );
    server.join().unwrap();
}

#[tokio::test]
async fn playlist_search_detail_pagination_and_back_preserve_state() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut paths = Vec::new();
        for _ in 0..4 {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let count = socket.read(&mut request).unwrap();
            let line = String::from_utf8_lossy(&request[..count])
                .lines()
                .next()
                .unwrap()
                .to_owned();
            let body = if line.starts_with("GET /api/search/playlist/list?pn=0&") {
                r#"{"code":200,"data":{"resultList":[{"id":11,"name":"歌单甲","musicnum":2,"source":4}]}}"#
            } else if line.starts_with("GET /api/search/playlist/list?pn=1&") {
                r#"{"code":200,"data":{"resultList":[]}}"#
            } else if line.starts_with("GET /api/service/playlist/11/musicList?pn=1&") {
                assert!(line.contains("source=4"));
                r#"{"code":200,"data":{"list":[{"id":123,"name":"歌曲甲","artist":"歌手"}]}}"#
            } else if line.starts_with("GET /api/service/playlist/11/musicList?pn=2&") {
                r#"{"code":200,"data":{"list":[]}}"#
            } else {
                panic!("unexpected request")
            };
            paths.push(line);
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
        paths
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
    browser.search(Arc::clone(&transport), tx.clone(), "测试".into());
    finish_next(&mut browser, &mut rx, &session).await;
    let BrowseContent::Playlists(items) = browser.view().content else {
        panic!("expected playlists")
    };
    assert_eq!(items[0].id, 11);
    assert!(browser.view().has_more); // A short page does not mean end of results.
    browser.next(Arc::clone(&transport), tx.clone());
    finish_next(&mut browser, &mut rx, &session).await;
    assert!(!browser.view().has_more);
    browser.open(Arc::clone(&transport), tx.clone(), items[0].clone());
    finish_next(&mut browser, &mut rx, &session).await;
    let BrowseContent::Tracks(tracks) = browser.view().content else {
        panic!("expected tracks")
    };
    assert_eq!(tracks[0].id, 123);
    let generation = browser.view().generation;
    assert_eq!(browser.playable_track(0, 123, generation).unwrap().id, 123);
    assert!(browser.playable_track(0, 999, generation).is_none());
    assert_eq!(
        browser.playback_origin(),
        Some(PlaybackOrigin {
            kind: CollectionKind::Playlist,
            id: 11,
            source_type: 4,
        })
    );
    assert!(browser.view().has_more);
    browser.next(Arc::clone(&transport), tx);
    finish_next(&mut browser, &mut rx, &session).await;
    assert!(!browser.view().has_more);
    browser.back();
    assert!(browser.playable_track(0, 123, generation).is_none());
    assert_eq!(browser.playback_origin(), None);
    assert!(
        matches!(browser.view().content, BrowseContent::Playlists(ref lists) if lists.len() == 1)
    );
    assert!(!browser.view().has_more);
    assert_eq!(server.join().unwrap().len(), 4);
}

#[tokio::test]
async fn album_and_artist_search_open_and_page_their_tracks() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..6 {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let count = socket.read(&mut request).unwrap();
            let line = String::from_utf8_lossy(&request[..count]);
            let body = if line.starts_with("GET /api/search/album/list?pn=0&") {
                r#"{"code":200,"data":{"resultList":[{"id":21,"name":"专辑甲","artist":"歌手甲","sourceType":6}]}}"#
            } else if line.starts_with("GET /api/service/album/music/21?pn=1&") {
                r#"{"code":200,"data":{"resultList":[{"id":101,"name":"专辑歌曲"}]}}"#
            } else if line.starts_with("GET /api/service/album/music/21?pn=2&") {
                r#"{"code":200,"data":{"resultList":[]}}"#
            } else if line.starts_with("GET /api/search/artist/list?pn=0&") {
                r#"{"code":200,"data":{"resultList":[{"artistId":31,"name":"歌手甲"}]}}"#
            } else if line.starts_with("GET /api/service/artist/music/31?pn=1&") {
                r#"{"code":200,"data":{"resultList":[{"id":102,"name":"歌手歌曲"}]}}"#
            } else if line.starts_with("GET /api/service/artist/music/31?pn=2&") {
                r#"{"code":200,"data":{"resultList":[]}}"#
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
        SearchKind::Album,
        "测试".into(),
    );
    finish_next(&mut browser, &mut rx, &session).await;
    let BrowseContent::Albums(albums) = browser.view().content else {
        panic!("expected albums")
    };
    assert_eq!(albums[0].id, 21);
    browser.open_album(Arc::clone(&transport), tx.clone(), albums[0].clone());
    finish_next(&mut browser, &mut rx, &session).await;
    assert_eq!(
        browser.playback_origin(),
        Some(PlaybackOrigin {
            kind: CollectionKind::Album,
            id: 21,
            source_type: 6,
        })
    );
    assert!(
        matches!(browser.view().content, BrowseContent::Tracks(ref tracks) if tracks[0].id == 101)
    );
    browser.next(Arc::clone(&transport), tx.clone());
    finish_next(&mut browser, &mut rx, &session).await;
    assert!(!browser.view().has_more);
    browser.back();
    assert!(
        matches!(browser.view().content, BrowseContent::Albums(ref albums) if albums.len() == 1)
    );
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
    assert_eq!(artists[0].id, 31);
    browser.open_artist(Arc::clone(&transport), tx.clone(), artists[0].clone());
    finish_next(&mut browser, &mut rx, &session).await;
    assert_eq!(browser.playback_origin(), None);
    assert!(
        matches!(browser.view().content, BrowseContent::Tracks(ref tracks) if tracks[0].id == 102)
    );
    browser.play_all(transport, tx);
    finish_next(&mut browser, &mut rx, &session).await;
    assert_eq!(browser.take_play_all().unwrap()[0].id, 102);
    assert!(!browser.view().has_more);
    server.join().unwrap();
}
