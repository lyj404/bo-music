use super::*;
use crate::{session::Session, transport::TransportOptions};
use std::{
    io::{Read, Write},
    net::TcpListener,
};

#[tokio::test]
async fn bound_comment_operations_reject_an_old_session_without_network() {
    let session = Session::default();
    session.set("42".into(), "synthetic".into());
    let revision = session.snapshot().revision;
    session.set("43".into(), "other".into());
    let transport = Transport::new(TransportOptions::default(), session, "device".into()).unwrap();
    assert!(matches!(
        list_at_revision(&transport, 1, CommentSort::Latest, 1, revision).await,
        Err(CommentError::Transport(TransportError::SessionChanged))
    ));
    assert!(matches!(
        replies_at_revision(&transport, 1, 10, 1, revision).await,
        Err(CommentError::Transport(TransportError::SessionChanged))
    ));
    let comment = PublishComment {
        music_id: 1,
        content: "测试",
        parent_id: 10,
        reply_id: 11,
        anonymous: true,
    };
    assert!(matches!(
        publish_at_revision(&transport, comment, revision).await,
        Err(CommentError::Transport(TransportError::SessionChanged))
    ));
    assert!(matches!(
        set_liked_at_revision(&transport, 1, 10, true, 0, revision).await,
        Err(CommentError::Transport(TransportError::SessionChanged))
    ));
}

fn server(bodies: Vec<&'static str>) -> (Transport, Session, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let thread = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for body in bodies {
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
    let session = Session::default();
    (
        Transport::new(options, session.clone(), "device".into()).unwrap(),
        session,
        thread,
    )
}

#[tokio::test]
async fn writes_require_login_and_preserve_body_and_operation() {
    let (transport, session, server) = server(vec![
        r#"{"code":200,"data":{"commentId":"88"}}"#,
        r#"{"code":200,"data":{}}"#,
    ]);
    assert!(matches!(
        publish(&transport, 1, "test", 0, 0, false).await,
        Err(CommentError::NotAuthenticated)
    ));
    assert!(matches!(
        publish(&transport, 1, "test", 0, 5, false).await,
        Err(CommentError::InvalidParameter)
    ));
    session.set("42".into(), "example".into());
    assert_eq!(
        publish(&transport, 1, " 你好 ", 0, 0, true).await.unwrap(),
        Some(88)
    );
    set_liked(&transport, 1, 88, false, 70).await.unwrap();
    let requests = server.join().unwrap();
    let body = |i: usize| -> serde_json::Value {
        serde_json::from_str(requests[i].split_once("\r\n\r\n").unwrap().1).unwrap()
    };
    assert_eq!(
        body(0),
        serde_json::json!({"moduleId":1,"moduleType":2,"uid":42,"content":"你好","anonymous":1})
    );
    assert_eq!(
        body(1),
        serde_json::json!({"moduleId":1,"moduleType":2,"uid":42,"commentId":88,"op":2,"parentId":70})
    );
    assert!(requests[0].starts_with("POST /api/comments/v3/publish?"));
    assert!(requests[1].starts_with("POST /api/comments/v3/like?"));
    assert!(requests.iter().all(|request| {
        request
            .to_ascii_lowercase()
            .contains("\r\nplat: android\r\n")
    }));
}

#[test]
fn anonymous_comments_hide_identity_and_normalize_counts() {
    let payload: Payload = serde_json::from_str(r#"{"comment_total":-3,"comment_list":[{"id":"1","nickname":"private","headImg":"https://example.com/a","anonymous":1,"likeCount":-2}]}"#).unwrap();
    let page = map_page(payload);
    assert_eq!(
        (
            page.total_count,
            page.items[0].nickname.as_str(),
            page.items[0].like_count
        ),
        (0, "匿名听友", 0)
    );
    assert!(page.items[0].avatar_url.is_none());
}
