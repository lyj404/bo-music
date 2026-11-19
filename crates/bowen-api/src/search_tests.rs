use super::*;
use crate::{session::Session, transport::TransportOptions};
use std::{
    io::{Read, Write},
    net::TcpListener,
};

#[test]
fn handles_invalid_audio_and_url_fields_without_panicking() {
    assert_eq!(parse_size("中文"), None);
    assert_eq!(parse_size("1.5Mb"), Some(1_572_864));
    assert_eq!(parse_size("0Mb"), Some(0));
    assert_eq!(parse_size("1.Mb"), None);
    assert!(http_url("https://user:pass@example.com/x").is_none());
    assert_eq!(
        http_url("https://y.gtimg.cn/image_o.jpg").as_deref(),
        Some("https://y.gtimg.cn/image_0.jpg")
    );
    assert!(artist_image("https://example.com/DEFAULT_ARTIST_PIC.PNG").is_none());
    let track: TrackDto = serde_json::from_str(r#"{"id":1,"vid":42,"isMv":0,"payInfo":{"feeType":{"song":"1"}},"audios":[{"level":"p","format":"mp3","bitrate":"320","size":"3Mb"},{"level":"bcms","format":"mgg","bitrate":"1000","size":"1Mb"}]}"#).unwrap();
    let track = track.into_track();
    assert!(track.has_mv && track.requires_purchase && !track.requires_vip);
    assert_eq!(track.audio_variants.len(), 1);
    assert_eq!(track.audio_variants[0].quality, AudioQuality::High);
}

#[tokio::test]
async fn failed_page_leaves_cursor_unchanged() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut bytes = [0; 2048];
        assert!(socket.read(&mut bytes).unwrap() > 0);
        socket.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
    });
    let options = TransportOptions {
        base_url: format!("http://{addr}/api/"),
        ..Default::default()
    };
    let transport = Transport::new(options, Session::default(), "device".into()).unwrap();
    let mut cursor = search_cursor(30);
    assert!(matches!(
        search_tracks(&transport, "晴天", &mut cursor).await,
        Err(SearchError::Transport(TransportError::Http(503)))
    ));
    assert_eq!(cursor.offset(), 0);
    assert!(!cursor.exhausted());
    server.join().unwrap();
}

#[tokio::test]
async fn suggestions_and_hot_words_match_existing_behavior() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let bodies = [
            r#"{"code":200,"data":{"resultList":[{"relword":"周杰伦"},{"relword":""},{"relword":"周杰伦"},{"relword":"周杰伦晴天"},{}]}}"#,
            r#"{"code":200,"data":{"hotWord":[{"key":"晴天","sort":2},{"key":"周杰伦","sort":1},{"key":"","sort":0}]}}"#,
        ];
        let mut requests = Vec::new();
        for body in bodies {
            let (mut socket, _) = listener.accept().unwrap();
            let mut bytes = [0; 4096];
            let length = socket.read(&mut bytes).unwrap();
            requests.push(String::from_utf8_lossy(&bytes[..length]).into_owned());
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
        base_url: format!("http://{addr}/api/"),
        ..Default::default()
    };
    let transport = Transport::new(options, Session::default(), "device".into()).unwrap();
    assert!(matches!(
        suggestions(&transport, "  ").await,
        Err(SearchError::BlankKeyword)
    ));
    assert_eq!(
        suggestions(&transport, "周").await.unwrap(),
        ["周杰伦", "周杰伦晴天"]
    );
    let hot = hot_words(&transport).await.unwrap();
    assert_eq!(hot.iter().map(|word| word.rank).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(
        hot.iter()
            .map(|word| word.keyword.as_str())
            .collect::<Vec<_>>(),
        ["周杰伦", "晴天"]
    );
    let requests = server.join().unwrap();
    assert!(requests[0].starts_with("GET /api/search/tip/v2/list?keyword="));
    assert!(requests[1].starts_with("GET /api/search/topic/word/list?"));
}

#[tokio::test]
async fn category_search_maps_distinct_shapes_and_keeps_short_page() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let bodies = [
            r#"{"code":200,"data":{"total":0,"resultList":[{"id":"2867496601","name":"歌单","source":13,"musicnum":177}]}}"#,
            r#"{"code":200,"data":{"resultList":[{"id":1293,"name":"叶惠美","artist":"周杰伦","musicCount":11}]}}"#,
            r#"{"code":200,"data":{"resultList":[{"artistId":336,"name":"周杰伦","songNum":300,"albumNum":47}]}}"#,
        ];
        let mut requests = Vec::new();
        for body in bodies {
            let (mut socket, _) = listener.accept().unwrap();
            let mut bytes = [0; 4096];
            let length = socket.read(&mut bytes).unwrap();
            requests.push(String::from_utf8_lossy(&bytes[..length]).into_owned());
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
        base_url: format!("http://{addr}/api/"),
        ..Default::default()
    };
    let transport = Transport::new(options, Session::default(), "device".into()).unwrap();
    let mut cursor = search_cursor(30);
    let playlists = search_playlists(&transport, "周", &mut cursor)
        .await
        .unwrap();
    assert_eq!(playlists.items[0].id, 2_867_496_601);
    assert_eq!(
        (
            playlists.items[0].music_count,
            playlists.items[0].source_type
        ),
        (177, 13)
    );
    assert_eq!(cursor.page_number(), 1);
    assert!(!cursor.exhausted());
    let album = search_albums(&transport, "周", &mut search_cursor(30))
        .await
        .unwrap();
    assert_eq!((album.items[0].id, album.items[0].music_count), (1293, 11));
    let artist = search_artists(&transport, "周", &mut search_cursor(30))
        .await
        .unwrap();
    assert_eq!(
        (
            artist.items[0].id,
            artist.items[0].song_count,
            artist.items[0].album_count
        ),
        (336, 300, 47)
    );
    let requests = server.join().unwrap();
    assert!(requests[0].starts_with("GET /api/search/playlist/list?pn=0&rn=30&keyword="));
    assert!(requests[1].starts_with("GET /api/search/album/list?pn=0&rn=30&keyword="));
    assert!(requests[2].starts_with("GET /api/search/artist/list?pn=0&rn=30&keyword="));
}
