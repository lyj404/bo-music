use super::*;
use crate::{session::Session, transport::TransportOptions};
use std::{
    io::{Read, Write},
    net::TcpListener,
};

#[test]
fn switch_thresholds_match_existing_player() {
    assert_eq!(PlayOperation::for_position(0, 1), None);
    assert_eq!(
        PlayOperation::for_position(100, 29),
        Some(PlayOperation::Switch)
    );
    assert_eq!(
        PlayOperation::for_position(100, 30),
        Some(PlayOperation::InProgress)
    );
    assert_eq!(
        PlayOperation::for_position(100, 90),
        Some(PlayOperation::InProgress)
    );
    assert_eq!(
        PlayOperation::for_position(100, 91),
        Some(PlayOperation::Complete)
    );
    assert_eq!(
        PlayOperation::for_position(100, 500),
        Some(PlayOperation::Complete)
    );
}

#[tokio::test]
async fn reports_do_not_use_a_new_account_after_switching() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let session = Session::default();
    session.set("42".into(), "old".into());
    let revision = session.snapshot().revision;
    let transport = Transport::new(
        TransportOptions {
            base_url: format!("http://{}/api/", listener.local_addr().unwrap()),
            ..Default::default()
        },
        session.clone(),
        "device".into(),
    )
    .unwrap();
    session.set("43".into(), "new".into());
    assert!(matches!(
        collection_history_for_revision(&transport, CollectionKind::Album, 9, 6, Some(revision))
            .await,
        Err(TelemetryError::Transport(TransportError::SessionChanged))
    ));
    assert!(matches!(
        song_history_for_revision(&transport, 22, Some(revision)).await,
        Err(TelemetryError::Transport(TransportError::SessionChanged))
    ));
    assert!(matches!(
        play_record_for_revision(
            &transport,
            22,
            100,
            30,
            PlayOperation::InProgress,
            Some(revision)
        )
        .await,
        Err(TelemetryError::Transport(TransportError::SessionChanged))
    ));
    assert!(
        matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
    );
}

#[tokio::test]
async fn writes_are_signed_and_use_original_json_shapes() {
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
        song_history(&transport, 1).await,
        Err(TelemetryError::NotAuthenticated)
    ));
    session.set("42".into(), "example".into());
    song_history(&transport, 22).await.unwrap();
    collection_history(&transport, CollectionKind::Album, 9, 6)
        .await
        .unwrap();
    play_record(&transport, 22, 100, 500, PlayOperation::Complete)
        .await
        .unwrap();
    let requests = server.join().unwrap();
    let body = |i: usize| -> serde_json::Value {
        serde_json::from_str(requests[i].split_once("\r\n\r\n").unwrap().1).unwrap()
    };
    assert_eq!(
        body(0),
        serde_json::json!({"sourceType":1,"items":[{"sourceId":22}]})
    );
    assert_eq!(
        body(1),
        serde_json::json!({"sourceType":3,"items":[{"sourceId":9,"playlistType":6}]})
    );
    assert_eq!(
        body(2),
        serde_json::json!({"musicList":[{"id":22,"src":false,"currentDur":100,"currentSpt":100,"pSrc":""}],"operate":0})
    );
    assert!(requests.iter().all(|request| request.contains("&sign=")));
}
