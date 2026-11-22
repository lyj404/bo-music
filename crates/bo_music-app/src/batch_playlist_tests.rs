use super::*;
use bo_music_api::{session::Session, transport::TransportOptions};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::Arc,
    time::Duration,
};

fn server(responses: Vec<&'static str>) -> (String, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let worker = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for body in responses {
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
    (format!("http://{address}/api/"), worker)
}
fn transport(base_url: String, session: Session) -> Transport {
    Transport::new(
        TransportOptions {
            base_url,
            ..Default::default()
        },
        session,
        "device".into(),
    )
    .unwrap()
}

#[tokio::test]
async fn writes_unique_ids_separately_and_reports_uncertain_results() {
    let (base, worker) = server(vec![
        r#"{"code":200}"#,
        r#"{"code":500}"#,
        r#"{"code":200}"#,
    ]);
    let session = Session::default();
    session.set("42".into(), "token".into());
    let revision = session.snapshot().revision;
    let transport = transport(base, session);
    let mut updates = Vec::new();
    let result = write(
        &transport,
        BatchJob {
            playlist_id: 7,
            track_ids: vec![1, -1, 2, 1, 3],
            add: true,
            revision,
        },
        &AtomicBool::new(false),
        |progress| updates.push(progress),
    )
    .await;
    assert_eq!(
        result,
        BatchProgress {
            total: 3,
            processed: 3,
            succeeded: 2,
            skipped: 0,
            unconfirmed: 1,
            canceled: false,
            account_changed: false
        }
    );
    assert_eq!(
        updates
            .iter()
            .map(|update| update.processed)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    let requests = worker.join().unwrap();
    for (index, request) in requests.iter().enumerate() {
        assert!(request.starts_with("POST /api/service/playlist/music?"));
        let payload: serde_json::Value =
            serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(
            payload,
            serde_json::json!({"playListId": 7, "musicIdList": [index + 1]})
        );
    }
}

#[tokio::test]
async fn stops_between_requests_on_cancel_or_account_change() {
    for change_account in [false, true] {
        let (base, worker) = server(vec![r#"{"code":200}"#]);
        let session = Session::default();
        session.set("42".into(), "token".into());
        let revision = session.snapshot().revision;
        let transport = transport(base, session.clone());
        let cancel = Arc::new(AtomicBool::new(false));
        let result = write(
            &transport,
            BatchJob {
                playlist_id: 7,
                track_ids: vec![1, 2],
                add: false,
                revision,
            },
            &cancel,
            |_| {
                if change_account {
                    session.set("43".into(), "other".into());
                } else {
                    cancel.store(true, Ordering::Release);
                }
            },
        )
        .await;
        assert_eq!((result.processed, result.succeeded), (1, 1));
        assert_eq!(result.account_changed, change_account);
        assert_eq!(result.canceled, !change_account);
        let requests = worker.join().unwrap();
        assert!(requests[0].starts_with("POST /api/service/playlist/music/delete?"));
    }
}
