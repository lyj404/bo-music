use super::*;
use crate::{catalog::service_cursor, session::Session, transport::TransportOptions};
use std::{
    io::{Read, Write},
    net::TcpListener,
};

#[tokio::test]
async fn bound_liked_playlist_operations_reject_old_identity_before_network() {
    let session = Session::default();
    session.set("42".into(), "synthetic".into());
    let revision = session.snapshot().revision;
    session.set("43".into(), "other".into());
    let transport = Transport::new(TransportOptions::default(), session, "device".into()).unwrap();
    assert!(matches!(
        liked_at_revision(&transport, revision).await,
        Err(PlaylistError::Transport(TransportError::SessionChanged))
    ));
    assert!(matches!(
        created_at_revision(&transport, revision).await,
        Err(PlaylistError::Transport(TransportError::SessionChanged))
    ));
    assert!(matches!(
        tracks_at_revision(&transport, 99, 5, &mut service_cursor(100), revision).await,
        Err(PlaylistError::Transport(TransportError::SessionChanged))
    ));
    assert!(matches!(
        set_tracks_at_revision(&transport, 99, &[10], true, revision).await,
        Err(PlaylistError::Transport(TransportError::SessionChanged))
    ));
}

#[tokio::test]
async fn bound_playlist_management_rejects_old_identity_before_network() {
    let session = Session::default();
    session.set("42".into(), "synthetic".into());
    let revision = session.snapshot().revision;
    session.set("43".into(), "other".into());
    let transport = Transport::new(TransportOptions::default(), session, "device".into()).unwrap();
    assert!(matches!(
        info_at_revision(&transport, 99, 5, revision).await,
        Err(PlaylistError::Transport(TransportError::SessionChanged))
    ));
    assert!(matches!(
        create_at_revision(&transport, "歌单", false, revision).await,
        Err(PlaylistError::Transport(TransportError::SessionChanged))
    ));
    assert!(matches!(
        delete_at_revision(&transport, 99, revision).await,
        Err(PlaylistError::Transport(TransportError::SessionChanged))
    ));
    assert!(matches!(
        upload_cover_at_revision(&transport, 99, vec![1], "cover.png", "image/png", revision).await,
        Err(PlaylistError::Transport(TransportError::SessionChanged))
    ));
    let edit = PlaylistEdit {
        id: 99,
        name: "歌单",
        description: "简介",
        cover_raw_url: "cover",
        category_ids: &[1, 2],
    };
    assert!(matches!(
        update_at_revision(&transport, edit, revision).await,
        Err(PlaylistError::Transport(TransportError::SessionChanged))
    ));
}

#[tokio::test]
async fn write_tracks_validates_batch_and_sends_numeric_ids() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
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
        let body = r#"{"code":200}"#;
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
        String::from_utf8_lossy(&bytes).into_owned()
    });
    let session = Session::default();
    session.set("42".into(), "example".into());
    let options = TransportOptions {
        base_url: format!("http://{address}/api/"),
        ..Default::default()
    };
    let transport = Transport::new(options, session, "device".into()).unwrap();
    assert!(matches!(
        add_tracks(&transport, 123, &[]).await,
        Err(PlaylistError::InvalidTrackIds)
    ));
    assert!(matches!(
        add_tracks(&transport, 123, &[0]).await,
        Err(PlaylistError::InvalidTrackIds)
    ));
    add_tracks(&transport, 123, &[42, 43]).await.unwrap();
    let request = server.join().unwrap();
    assert!(request.starts_with("POST /api/service/playlist/music?"));
    assert!(request.contains("&sign="));
    let body = request.split_once("\r\n\r\n").unwrap().1;
    let json: serde_json::Value = serde_json::from_str(body).unwrap();
    assert_eq!(json["playListId"], 123);
    assert_eq!(json["musicIdList"], serde_json::json!([42, 43]));
}

#[tokio::test]
async fn create_delete_and_update_preserve_method_and_body() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for body in [
            r#"{"code":200,"data":{"id":123}}"#,
            r#"{"code":200}"#,
            r#"{"code":200}"#,
        ] {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut chunk = [0; 2048];
            loop {
                let count = socket.read(&mut chunk).unwrap();
                if count == 0 {
                    break;
                }
                bytes.extend_from_slice(&chunk[..count]);
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
    let options = TransportOptions {
        base_url: format!("http://{address}/api/"),
        ..Default::default()
    };
    let transport = Transport::new(options, session, "device".into()).unwrap();
    assert!(matches!(
        create(&transport, " ", true).await,
        Err(PlaylistError::BlankName)
    ));
    assert_eq!(create(&transport, " 名称 ", true).await.unwrap(), 123);
    delete(&transport, 123).await.unwrap();
    update(
        &transport,
        PlaylistEdit {
            id: 123,
            name: " 更新 ",
            description: "说明",
            cover_raw_url: "raw-cover",
            category_ids: &[3, 5],
        },
    )
    .await
    .unwrap();
    let requests = server.join().unwrap();
    let body = |index: usize| -> serde_json::Value {
        serde_json::from_str(requests[index].split_once("\r\n\r\n").unwrap().1).unwrap()
    };
    assert!(requests[0].starts_with("POST /api/service/playlist?"));
    assert_eq!(body(0), serde_json::json!({"name":"名称", "private":true}));
    assert!(requests[1].starts_with("DELETE /api/service/playlist?"));
    assert_eq!(body(1), serde_json::json!({"playlistIds":[123]}));
    assert!(requests[2].starts_with("PUT /api/service/playlist?"));
    assert_eq!(
        body(2),
        serde_json::json!({"id":123, "name":"更新", "description":"说明", "pic":"raw-cover", "categoryList":[3,5]})
    );
}

#[tokio::test]
async fn cover_upload_uses_multipart_and_returns_raw_url() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
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
        let body = r#"{"code":200,"data":{"imgUrl":"https://example.com/cover.jpg"}}"#;
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
        String::from_utf8_lossy(&bytes).into_owned()
    });
    let session = Session::default();
    session.set("42".into(), "example".into());
    let options = TransportOptions {
        base_url: format!("http://{address}/api/"),
        ..Default::default()
    };
    let transport = Transport::new(options, session, "device".into()).unwrap();
    assert!(matches!(
        upload_cover(&transport, 1, vec![], "cover.png", "image/png").await,
        Err(PlaylistError::InvalidImage)
    ));
    assert_eq!(
        upload_cover(
            &transport,
            123,
            b"synthetic-image".to_vec(),
            "cover.png",
            "image/png"
        )
        .await
        .unwrap(),
        "https://example.com/cover.jpg"
    );
    let request = server.join().unwrap();
    assert!(request.starts_with("POST /api/service/playlist/uploadPic/123?"));
    assert!(
        request
            .to_ascii_lowercase()
            .contains("content-type: multipart/form-data; boundary=")
    );
    assert!(request.contains("name=\"file\"; filename=\"cover.png\""));
    assert!(request.contains("synthetic-image"));
}
