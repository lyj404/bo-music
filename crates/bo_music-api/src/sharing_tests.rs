use super::*;
use crate::{session::Session, transport::TransportOptions};
use std::{
    io::{Read, Write},
    net::TcpListener,
};

#[test]
fn share_links_preserve_source_and_encode_uid() {
    assert_eq!(
        link(ShareTarget::Track, 22, "a b&中").unwrap(),
        "https://h5app.kuwo.cn/m/bodian/playMusic.html?uid=a%20b%26%E4%B8%AD&musicId=22&opusId="
    );
    assert_eq!(
        link(ShareTarget::Playlist { source: 13 }, 22, "-1").unwrap(),
        "https://h5app.kuwo.cn/m/bodian/collection.html?uid=-1&playlistId=22&source=13"
    );
    assert!(matches!(
        link(ShareTarget::Album, 0, "-1"),
        Err(ShareError::InvalidId)
    ));
}

#[tokio::test]
async fn share_report_rejects_stale_identity_without_requesting() {
    let session = Session::default();
    let previous = session.snapshot().revision;
    session.set("42".into(), "synthetic".into());
    let transport = Transport::new(
        TransportOptions {
            base_url: "http://127.0.0.1:9/api/".into(),
            ..Default::default()
        },
        session,
        "device".into(),
    )
    .unwrap();
    assert!(matches!(
        report_track_at_revision(&transport, 22, previous).await,
        Err(ShareError::Transport(TransportError::SessionChanged))
    ));
}

#[tokio::test]
async fn share_unsupported_is_a_normal_result() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let thread = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut bytes = [0; 4096];
        let count = socket.read(&mut bytes).unwrap();
        let body = r#"{"code":23006}"#;
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
        String::from_utf8_lossy(&bytes[..count]).into_owned()
    });
    let transport = Transport::new(
        TransportOptions {
            base_url: format!("http://{address}/api/"),
            ..Default::default()
        },
        Session::default(),
        "device".into(),
    )
    .unwrap();
    assert_eq!(
        report_track(&transport, 22).await.unwrap(),
        ShareOutcome::Unsupported
    );
    let request = thread.join().unwrap();
    assert!(request.starts_with(
        "GET /api/service/share/text?shareTo=5&shareSource=0&sourceId=22&playlistType=4&"
    ));
    assert!(request.contains("&sign="));
}
