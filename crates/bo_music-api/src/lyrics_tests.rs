use super::*;
use crate::{session::Session, transport::TransportOptions};
use std::{
    io::{Read, Write},
    net::TcpListener,
};

fn track() -> Track {
    Track {
        id: 228908,
        title: String::new(),
        artist_text: String::new(),
        artists: vec![],
        album_name: None,
        album_id: 0,
        cover_url: None,
        duration_seconds: 269,
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

#[test]
fn content_decoder_rejects_invalid_base64_and_allows_whitespace() {
    assert_eq!(decode(None).unwrap(), "");
    assert_eq!(decode(Some(" YWJj\n".into())).unwrap(), "abc");
    assert!(matches!(
        decode(Some("%%%".into())),
        Err(LyricError::InvalidContent)
    ));
}

#[tokio::test]
async fn scheduled_lyrics_refuse_stale_session_and_unsafe_endpoint() {
    let session = Session::default();
    let transport = Transport::new(
        TransportOptions::default(),
        session.clone(),
        "device".into(),
    )
    .unwrap();
    let revision = session.snapshot().revision;
    session.set("42".into(), "secret".into());
    assert!(matches!(
        get_at(&transport, &track(), revision, "http://127.0.0.1:1/mobi.s").await,
        Err(LyricError::Transport(TransportError::SessionChanged))
    ));
    assert!(matches!(
        get_at(
            &transport,
            &track(),
            session.snapshot().revision,
            "http://user@127.0.0.1/mobi.s"
        )
        .await,
        Err(LyricError::InvalidEndpoint)
    ));
}

#[tokio::test]
async fn account_switch_during_lyric_request_discards_response() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let session = Session::default();
    session.set("42".into(), "old".into());
    let changed = session.clone();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut bytes = [0; 4096];
        assert!(socket.read(&mut bytes).unwrap() > 0);
        changed.set("43".into(), "new".into());
        let body = r#"{"code":200,"data":{"content":""}}"#;
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });
    let transport = Transport::new(
        TransportOptions::default(),
        session.clone(),
        "device".into(),
    )
    .unwrap();
    let endpoint = reqwest::Url::parse(&format!("http://{address}/mobi.s")).unwrap();
    assert!(matches!(
        get_from(&transport, &endpoint, &track(), session.snapshot().revision).await,
        Err(LyricError::Transport(TransportError::SessionChanged))
    ));
    server.join().unwrap();
    assert_eq!(session.snapshot().uid, "43");
}
