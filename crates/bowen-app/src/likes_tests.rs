use super::*;
use bowen_api::{session::Session, transport::TransportOptions};
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};

async fn take_load(
    liked: &mut LikedSongs,
    rx: &mut mpsc::UnboundedReceiver<Command>,
    session: &Session,
) {
    let Command::LikedLoadDone(generation, revision, result) =
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("unexpected event")
    };
    assert!(liked.finish_load(generation, revision, session.snapshot().revision, result));
}

#[tokio::test]
async fn scans_short_pages_serializes_writes_and_refreshes_after_track_change() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for index in 0..7 {
            let (mut socket, _) = listener.accept().unwrap();
            let mut buffer = [0; 4096];
            let count = socket.read(&mut buffer).unwrap();
            let request = String::from_utf8_lossy(&buffer[..count]);
            let body = match index {
                0 | 4 => {
                    assert!(request.starts_with("GET /api/service/playlist/fond?"));
                    r#"{"code":200,"data":{"id":99,"name":"喜欢"}}"#
                }
                1 => {
                    assert!(request.contains("pn=1") && request.contains("source=5"));
                    r#"{"code":200,"data":{"list":[{"id":12,"name":"第一首"}]}}"#
                }
                2 | 6 => {
                    assert!(request.contains("pn=2"));
                    r#"{"code":200,"data":{"list":[]}}"#
                }
                3 => {
                    assert!(request.starts_with("POST /api/service/playlist/music?"));
                    r#"{"code":200,"data":{}}"#
                }
                _ => {
                    assert!(request.contains("pn=1"));
                    r#"{"code":200,"data":{"list":[{"id":12,"name":"第一首"},{"id":13,"name":"第二首"}]}}"#
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
    let mut liked = LikedSongs::default();
    liked.select(Some(13), transport.clone(), tx.clone());
    take_load(&mut liked, &mut rx, &session).await;
    assert_eq!(liked.view().state, LikeState::Ready(false));
    liked.toggle(13, transport.clone(), tx.clone());
    assert!(liked.has_pending_writes(session.snapshot().revision));
    liked.toggle(13, transport.clone(), tx.clone());
    assert_eq!(liked.view().state, LikeState::Writing);
    liked.select(Some(12), transport.clone(), tx.clone());
    assert_eq!(liked.view().state, LikeState::Loading);
    let Command::LikedWriteDone(revision, id, desired, success) =
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("unexpected event")
    };
    assert!(desired && success);
    assert!(liked.finish_write(
        revision,
        session.snapshot().revision,
        (id, desired, success),
        transport.clone(),
        tx.clone()
    ));
    take_load(&mut liked, &mut rx, &session).await;
    assert_eq!(
        liked.view(),
        LikeView {
            track_id: Some(12),
            state: LikeState::Ready(true)
        }
    );
    liked.select(Some(13), transport, tx);
    assert_eq!(liked.view().state, LikeState::Ready(true));
    server.join().unwrap();
}

#[tokio::test]
async fn failure_and_session_changes_never_produce_a_false_unliked_state() {
    let session = Session::default();
    let transport = Arc::new(
        Transport::new(
            TransportOptions::default(),
            session.clone(),
            "device".into(),
        )
        .unwrap(),
    );
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut liked = LikedSongs::default();
    liked.select(Some(10), transport.clone(), tx.clone());
    assert_eq!(liked.view().state, LikeState::LoginRequired);
    session.set("42".into(), "synthetic".into());
    let revision = session.snapshot().revision;
    liked.key = Some((revision, 10));
    liked.view = LikeView {
        track_id: Some(10),
        state: LikeState::Loading,
    };
    let generation = liked.generation;
    assert!(liked.finish_load(generation, revision, revision, Err(())));
    assert_eq!(liked.view().state, LikeState::Unknown);
    let partial = LikedSnapshot {
        playlist: Some(99),
        ids: HashSet::from([12]),
        complete: false,
    };
    assert_eq!(LikedSongs::state(&partial, 12), LikeState::Ready(true));
    assert_eq!(LikedSongs::state(&partial, 10), LikeState::Unknown);
    session.set("43".into(), "other".into());
    assert!(!liked.finish_load(
        generation,
        revision,
        session.snapshot().revision,
        Ok(partial)
    ));
    liked.reset();
    assert_eq!(liked.view().state, LikeState::Empty);
}

#[tokio::test]
async fn missing_liked_playlist_is_distinct_and_can_be_retried() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for _ in 0..2 {
            let (mut socket, _) = listener.accept().unwrap();
            let mut bytes = [0; 4096];
            let count = socket.read(&mut bytes).unwrap();
            assert!(
                String::from_utf8_lossy(&bytes[..count])
                    .starts_with("GET /api/service/playlist/fond?")
            );
            let body = r#"{"code":200,"data":{}}"#;
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
    let mut liked = LikedSongs::default();
    liked.select(Some(10), transport.clone(), tx.clone());
    take_load(&mut liked, &mut rx, &session).await;
    assert_eq!(liked.view().state, LikeState::NoPlaylist);
    liked.toggle(10, transport.clone(), tx.clone());
    assert_eq!(liked.view().state, LikeState::NoPlaylist);
    liked.refresh(transport, tx, true);
    take_load(&mut liked, &mut rx, &session).await;
    assert_eq!(liked.view().state, LikeState::NoPlaylist);
    server.join().unwrap();
}

#[test]
fn uncertain_write_marks_state_unknown_and_invalidates_cache() {
    let mut liked = LikedSongs {
        key: Some((1, 10)),
        view: LikeView {
            track_id: Some(10),
            state: LikeState::Writing,
        },
        cache: Some(Cache {
            revision: 1,
            snapshot: LikedSnapshot {
                playlist: Some(99),
                ids: HashSet::new(),
                complete: true,
            },
            dirty: false,
        }),
        ..Default::default()
    };
    liked.writes.insert((1, 10));
    let transport = Arc::new(
        Transport::new(
            TransportOptions::default(),
            Session::default(),
            "device".into(),
        )
        .unwrap(),
    );
    let (tx, _) = mpsc::unbounded_channel();
    assert!(liked.finish_write(1, 1, (10, true, false), transport, tx));
    assert_eq!(liked.view().state, LikeState::Unknown);
    assert!(liked.cache.unwrap().dirty);
}

#[tokio::test]
async fn successful_write_after_cache_reset_requests_fresh_state() {
    let session = Session::default();
    session.set("42".into(), "synthetic".into());
    let revision = session.snapshot().revision;
    let transport =
        Arc::new(Transport::new(TransportOptions::default(), session, "device".into()).unwrap());
    let (tx, _) = mpsc::unbounded_channel();
    let mut liked = LikedSongs {
        key: Some((revision, 10)),
        view: LikeView {
            track_id: Some(10),
            state: LikeState::Writing,
        },
        ..Default::default()
    };
    liked.writes.insert((revision, 10));
    assert!(liked.finish_write(revision, revision, (10, true, true), transport, tx));
    assert_eq!(liked.view().state, LikeState::Loading);
    assert!(liked.load.is_some());
    liked.reset();
}
