use super::*;
use crate::{session::Session, transport::TransportOptions};
use std::{
    io::{Read, Write},
    net::TcpListener,
};

#[tokio::test]
async fn bound_write_rejects_changed_session_before_network() {
    let session = Session::default();
    session.set("42".into(), "synthetic".into());
    let revision = session.snapshot().revision;
    session.set("43".into(), "other".into());
    let transport = Transport::new(TransportOptions::default(), session, "device".into()).unwrap();
    assert!(matches!(
        set_at_revision(&transport, 21, 6, true, revision).await,
        Err(CollectionError::Transport(TransportError::SessionChanged))
    ));
}

#[tokio::test]
async fn collection_bodies_use_requested_state_and_artist_token_only() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for _ in 0..3 {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut chunk = [0; 2048];
            loop {
                let read = socket.read(&mut chunk).unwrap();
                if read == 0 {
                    break;
                }
                bytes.extend_from_slice(&chunk[..read]);
                if let Some(end) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|n| n.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            requests.push(String::from_utf8_lossy(&bytes).into_owned());
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
    let options = TransportOptions {
        base_url: format!("http://{address}/api/"),
        ..Default::default()
    };
    let transport = Transport::new(options, session.clone(), "device".into()).unwrap();
    assert!(matches!(
        set_album(&transport, 12, true).await,
        Err(CollectionError::NotAuthenticated)
    ));
    session.set("42".into(), "example".into());
    set_playlist(&transport, 10, 13, true).await.unwrap();
    set_album(&transport, 12, false).await.unwrap();
    set_artist(&transport, 11, true).await.unwrap();
    let requests = server.join().unwrap();
    let body = |index: usize| -> serde_json::Value {
        serde_json::from_str(requests[index].split_once("\r\n\r\n").unwrap().1).unwrap()
    };
    assert_eq!(
        body(0),
        serde_json::json!({"source":13, "sourceId":[10], "op":1, "uid":42})
    );
    assert_eq!(
        body(1),
        serde_json::json!({"source":6, "sourceId":[12], "op":2, "uid":42})
    );
    assert_eq!(body(2)["source"], 7);
    assert_eq!(body(2)["token"], "example");
    assert!(
        requests
            .iter()
            .all(|request| request.starts_with("POST /api/service/collect?")
                && request.contains("&sign="))
    );
}

#[tokio::test]
async fn album_state_reports_unknown_and_uses_album_source() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut bytes = [0; 4096];
        let count = socket.read(&mut bytes).unwrap();
        let body = r#"{"code":200,"data":{"result":[{"id":1293,"collect":true}]}}"#;
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
        String::from_utf8_lossy(&bytes[..count]).into_owned()
    });
    let session = Session::default();
    let options = TransportOptions {
        base_url: format!("http://{address}/api/"),
        ..Default::default()
    };
    let transport = Transport::new(options, session.clone(), "device".into()).unwrap();
    assert_eq!(album_state(&transport, 1293).await.unwrap(), None);
    session.set("42".into(), "example".into());
    assert_eq!(album_state(&transport, 1293).await.unwrap(), Some(true));
    assert!(
        server
            .join()
            .unwrap()
            .starts_with("GET /api/service/collect/multipleState?source=6&sourceIds=1293&")
    );
}

#[tokio::test]
async fn mixed_collection_pages_do_not_stop_after_filtering() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for body in [
            r#"{"code":200,"data":{"total":2,"playLists":[{"id":10,"name":"list","sourceType":4}]}}"#,
            r#"{"code":200,"data":{"total":2,"playLists":[{"albumId":20,"name":"album"}]}}"#,
        ] {
            let (mut socket, _) = listener.accept().unwrap();
            let mut bytes = [0; 4096];
            let count = socket.read(&mut bytes).unwrap();
            requests.push(String::from_utf8_lossy(&bytes[..count]).into_owned());
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
    session.set("42".into(), "example".into());
    let transport = Transport::new(
        TransportOptions {
            base_url: format!("http://{address}/api/"),
            ..Default::default()
        },
        session,
        "device".into(),
    )
    .unwrap();
    let mut cursor = crate::catalog::service_cursor(1);
    assert!(
        albums(&transport, &mut cursor)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert!(!cursor.exhausted());
    assert_eq!(
        albums(&transport, &mut cursor).await.unwrap().items[0].id,
        20
    );
    let requests = server.join().unwrap();
    assert!(
        requests[0].starts_with("GET /api/service/collect/4/list?pn=1&rn=1&userId=42&fromUid=42&")
    );
    assert!(requests[1].starts_with("GET /api/service/collect/4/list?pn=2&rn=1&"));
}

#[tokio::test]
async fn followed_artists_use_original_contract() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut bytes = [0; 4096];
        let count = socket.read(&mut bytes).unwrap();
        let body = r#"{"code":200,"data":{"artistList":[{"id":"336","name":"周杰伦","aliasName":"Jay Chou","fansCnt":12,"musicCnt":3,"albumCnt":2}]}}"#;
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
        String::from_utf8_lossy(&bytes[..count]).into_owned()
    });
    let session = Session::default();
    let transport = Transport::new(
        TransportOptions {
            base_url: format!("http://{address}/api/"),
            ..Default::default()
        },
        session.clone(),
        "device".into(),
    )
    .unwrap();
    assert!(matches!(
        followed_artists(&transport).await,
        Err(CollectionError::NotAuthenticated)
    ));
    session.set("42".into(), "example".into());
    let artists = followed_artists(&transport).await.unwrap();
    assert_eq!(
        (
            artists[0].id,
            artists[0].alias.as_str(),
            artists[0].song_count
        ),
        (336, "Jay Chou", 3)
    );
    assert!(
        server
            .join()
            .unwrap()
            .starts_with("GET /api/service/collect/7/list?userId=42&fromUid=42&pn=1&rn=400&")
    );
}
