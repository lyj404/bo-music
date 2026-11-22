use super::*;
use crate::{session::Session, transport::TransportOptions};
use std::{
    io::{Read, Write},
    net::TcpListener,
};

fn sample_track() -> Track {
    Track {
        id: 1,
        title: "Test track".into(),
        artist_text: String::new(),
        artists: Vec::new(),
        album_name: None,
        album_id: 0,
        cover_url: None,
        duration_seconds: 60,
        available_qualities: vec![AudioQuality::Standard],
        audio_variants: Vec::new(),
        requires_vip: false,
        requires_purchase: false,
        lyrics: None,
        has_mv: false,
        comment_count: None,
        favorite_count: None,
        share_count: None,
    }
}

fn server(bodies: Vec<&'static str>) -> (Transport, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let thread = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for body in bodies {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let length = socket.read(&mut buffer).unwrap();
                if length == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..length]);
                if let Some(header_end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&request[..header_end]);
                    let length = head
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|n| n.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if request.len() >= header_end + 4 + length {
                        break;
                    }
                }
            }
            requests.push(String::from_utf8_lossy(&request).into_owned());
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
    let options = TransportOptions {
        base_url: format!("http://{address}/api/"),
        ..Default::default()
    };
    (
        Transport::new(options, Session::default(), "device".into()).unwrap(),
        thread,
    )
}

#[tokio::test]
async fn denied_rights_does_not_try_to_fetch_audio() {
    let (transport, server) = server(vec![r#"{"code":200,"data":{"status":7}}"#]);
    assert!(matches!(
        resolve(&transport, &sample_track(), None).await.unwrap(),
        PlaybackResolution::Denied(Denial::NotAuthenticated)
    ));
    assert_eq!(server.join().unwrap().len(), 1);
}

#[tokio::test]
async fn encrypted_response_is_not_given_to_player() {
    let (transport, server) = server(vec![
        r#"{"code":200,"data":{"status":4}}"#,
        r#"{"code":200,"data":{"audioUrl":"https://example.com/audio","format":"mflac","bitrate":2000}}"#,
    ]);
    assert!(matches!(
        resolve(&transport, &sample_track(), None).await.unwrap(),
        PlaybackResolution::Denied(Denial::NoUsableQuality)
    ));
    assert_eq!(server.join().unwrap().len(), 2);
}

#[tokio::test]
async fn unsupported_variant_is_not_requested() {
    let (transport, server) = server(vec![r#"{"code":200,"data":{"status":4}}"#]);
    let mut track = sample_track();
    track.available_qualities.clear();
    track.audio_variants = vec![AudioVariant {
        quality: AudioQuality::Lossless,
        level: "zply".into(),
        format: "flac".into(),
        bitrate_kbps: 2000,
        size_bytes: 0,
    }];
    assert!(matches!(
        resolve(&transport, &track, None).await.unwrap(),
        PlaybackResolution::Denied(Denial::NoUsableQuality)
    ));
    assert_eq!(server.join().unwrap().len(), 1);
}

#[tokio::test]
async fn legacy_quality_only_snapshot_uses_matching_fallback_bitrate() {
    let (transport, server) = server(vec![
        r#"{"code":200,"data":{"status":4}}"#,
        r#"{"code":200,"data":{"audioUrl":"https://example.com/audio","format":"mp3","bitrate":128}}"#,
    ]);
    let mut track = sample_track();
    track.audio_variants.clear();
    track.available_qualities = vec![AudioQuality::Lossless, AudioQuality::High];
    let result = resolve(&transport, &track, Some(AudioQuality::High))
        .await
        .unwrap();
    assert!(matches!(result, PlaybackResolution::Playable(source)
        if source.requested_quality == Some(AudioQuality::High)
            && source.requested_variant.is_none() && source.downgraded));
    let requests = server.join().unwrap();
    assert!(requests[1].contains("br=320kmp3"));
}

#[tokio::test]
async fn stale_scheduled_revision_is_rejected_before_network_io() {
    let session = Session::default();
    let transport = Transport::new(
        TransportOptions {
            base_url: "http://127.0.0.1:1/api/".into(),
            ..Default::default()
        },
        session.clone(),
        "device".into(),
    )
    .unwrap();
    let revision = session.snapshot().revision;
    session.set("new-user".into(), "token".into());
    assert!(matches!(
        resolve_for_revision(&transport, &sample_track(), None, revision).await,
        Err(ResolveError::Transport(TransportError::SessionChanged))
    ));
}
