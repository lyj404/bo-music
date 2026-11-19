use super::*;
use bowen_api::{session::Session, transport::TransportOptions};
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};

async fn page(
    comments: &mut Comments,
    rx: &mut mpsc::UnboundedReceiver<Command>,
    session: &Session,
) {
    let Command::CommentsPageDone(generation, context, parent, result) =
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("unexpected event")
    };
    assert!(comments.finish_page(
        generation,
        context,
        parent,
        session.snapshot().revision,
        result
    ));
}
async fn like(
    comments: &mut Comments,
    rx: &mut mpsc::UnboundedReceiver<Command>,
    session: &Session,
) -> bool {
    let Command::CommentLikeDone(action, success) =
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("unexpected event")
    };
    assert!(comments.finish_like(action, session.snapshot().revision, success));
    success
}

#[tokio::test]
async fn paginates_deduplicates_likes_replies_and_publishes() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for index in 0..9 {
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
            let body = match index {
                0 => {
                    r#"{"code":200,"data":{"comment_list":[{"id":1,"nickname":"甲","content":"第一条","likeCount":3},{"id":2,"nickname":"乙","content":"第二条"}],"comment_total":3,"more":true}}"#
                }
                1 => {
                    r#"{"code":200,"data":{"comment_list":[{"id":2,"content":"重复"},{"id":3,"content":"第三条"}],"comment_total":3,"more":false}}"#
                }
                2 => r#"{"code":200,"data":{}}"#,
                3 | 5 => {
                    r#"{"code":200,"data":{"comment_list":[{"id":11,"nickname":"回复者","content":"回复","likeCount":2}],"comment_total":1,"more":false}}"#
                }
                7 | 8 => {
                    r#"{"code":200,"data":{"comment_list":[{"id":11,"parentId":1,"nickname":"回复者","content":"回复"},{"id":99,"uid":42,"content":"你好"}],"comment_total":2,"more":false}}"#
                }
                4 => "",
                _ => r#"{"code":200,"data":{"commentId":99}}"#,
            };
            let status = if index == 4 {
                "500 Internal Server Error"
            } else {
                "200 OK"
            };
            write!(
                socket,
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
        requests
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
    let mut comments = Comments::default();
    comments.open(Some(123), transport.clone(), tx.clone());
    page(&mut comments, &mut rx, &session).await;
    assert_eq!(comments.view().rows.len(), 2);
    assert!(comments.view().has_more);
    comments.load_more(transport.clone(), tx.clone());
    page(&mut comments, &mut rx, &session).await;
    assert_eq!(
        comments
            .view()
            .rows
            .iter()
            .map(|row| row.id)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert!(!comments.view().has_more);
    let generation = comments.view().generation;
    comments
        .like(1, generation, transport.clone(), tx.clone())
        .unwrap();
    comments
        .like(1, generation, transport.clone(), tx.clone())
        .unwrap();
    assert!(like(&mut comments, &mut rx, &session).await);
    assert!(comments.view().rows[0].is_liked);
    assert_eq!(comments.view().rows[0].like_count, 4);
    comments.open_thread(1, generation, transport.clone(), tx.clone());
    page(&mut comments, &mut rx, &session).await;
    let generation = comments.view().generation;
    assert_eq!(comments.view().root.as_ref().unwrap().id, 1);
    assert_eq!(comments.view().rows[0].parent_id, 1);
    comments
        .like(11, generation, transport.clone(), tx.clone())
        .unwrap();
    assert!(!like(&mut comments, &mut rx, &session).await);
    assert!(comments.view().uncertain_likes.contains(&11));
    assert!(
        comments
            .like(11, generation, transport.clone(), tx.clone())
            .is_err()
    );
    comments.refresh(transport.clone(), tx.clone());
    page(&mut comments, &mut rx, &session).await;
    let generation = comments.view().generation;
    comments.reply_to(11, generation);
    assert_eq!(comments.view().reply_target.as_ref().unwrap().0, 11);
    assert!(
        comments
            .publish(
                generation,
                "x".repeat(1001),
                false,
                transport.clone(),
                tx.clone()
            )
            .is_err()
    );
    comments
        .publish(
            generation,
            " 你好 ".into(),
            true,
            transport.clone(),
            tx.clone(),
        )
        .unwrap();
    assert!(comments.view().posting);
    let Command::CommentPublishDone(context, outcome) =
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("unexpected event")
    };
    assert!(
        outcome == PublishOutcome::Verified
            && comments.finish_publish(
                context,
                session.snapshot().revision,
                outcome,
                transport,
                tx
            )
    );
    page(&mut comments, &mut rx, &session).await;
    assert!(!comments.view().posting);
    assert_eq!(
        comments.view().reply_target.as_ref().map(|target| target.0),
        Some(11)
    );
    assert_eq!(comments.view().root.as_ref().unwrap().reply_count, 2);
    let requests = server.join().unwrap();
    assert!(requests[0].starts_with("GET /api/comments/v3/hot?") && requests[0].contains("pn=1"));
    assert!(requests[1].contains("pn=2"));
    assert!(
        requests[3].starts_with("GET /api/comments/v3/replies?")
            && requests[3].contains("parentId=1")
    );
    let like_payload: serde_json::Value =
        serde_json::from_str(requests[4].split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(like_payload["parentId"], 1);
    let payload: serde_json::Value =
        serde_json::from_str(requests[6].split_once("\r\n\r\n").unwrap().1).unwrap();
    assert_eq!(
        payload,
        serde_json::json!({"moduleId":123,"moduleType":2,"uid":42,"content":"你好","parentId":1,"replyId":11,"anonymous":1})
    );
}

fn mock_responses(responses: Vec<&'static str>) -> (String, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
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
    (format!("http://{address}/api/"), server)
}

#[tokio::test]
async fn accepted_without_readback_is_unverified_and_keeps_composer_context() {
    let (base_url, server) = mock_responses(vec![
        r#"{"code":200,"data":{"commentId":99}}"#,
        r#"{"code":200,"data":{"comment_list":[],"comment_total":0,"more":false}}"#,
        r#"{"code":200,"data":{"comment_list":[],"comment_total":0,"more":false}}"#,
    ]);
    let session = Session::default();
    session.set("42".into(), "synthetic".into());
    let transport = Arc::new(
        Transport::new(
            TransportOptions {
                base_url,
                ..Default::default()
            },
            session.clone(),
            "device".into(),
        )
        .unwrap(),
    );
    let context = CommentContext {
        revision: session.snapshot().revision,
        track: 123,
        generation: 1,
    };
    let mut comments = Comments {
        open: true,
        context: Some(context),
        generation: 1,
        ..Default::default()
    };
    let (tx, mut rx) = mpsc::unbounded_channel();
    comments
        .publish(1, "待确认".into(), false, transport.clone(), tx.clone())
        .unwrap();
    let Command::CommentPublishDone(result_context, outcome) =
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("unexpected event")
    };
    assert_eq!(outcome, PublishOutcome::AcceptedUnverified);
    assert!(comments.finish_publish(
        result_context,
        session.snapshot().revision,
        outcome,
        transport,
        tx
    ));
    page(&mut comments, &mut rx, &session).await;
    assert!(!comments.view().posting);
    assert_eq!(comments.view().track_id, Some(123));
    assert_eq!(comments.view().sort, CommentSort::Latest);
    let requests = server.join().unwrap();
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.starts_with("POST "))
            .count(),
        1
    );
}

#[tokio::test]
async fn verification_checks_last_reply_page_and_excludes_existing_ids() {
    let (base_url, server) = mock_responses(vec![
        r#"{"code":200,"data":{"comment_list":[{"id":11,"uid":42,"content":"相同内容"}],"comment_total":31,"more":true}}"#,
        r#"{"code":200,"data":{"comment_list":[{"id":99,"uid":42,"content":"相同内容"}],"comment_total":31,"more":false}}"#,
    ]);
    let session = Session::default();
    session.set("42".into(), "synthetic".into());
    let transport = Transport::new(
        TransportOptions {
            base_url,
            ..Default::default()
        },
        session.clone(),
        "device".into(),
    )
    .unwrap();
    let verification = PublishVerification {
        context: CommentContext {
            revision: session.snapshot().revision,
            track: 123,
            generation: 1,
        },
        parent: 1,
        published_id: None,
        uid: "42".into(),
        text: "相同内容".into(),
        previous_ids: [11].into_iter().collect(),
    };
    assert!(verify_published(&transport, &verification).await);
    let requests = server.join().unwrap();
    assert!(requests[0].contains("pn=1"));
    assert!(requests[1].contains("pn=2"));
}

#[tokio::test]
async fn no_track_is_stable_and_old_context_results_are_ignored() {
    let session = Session::default();
    let transport = Arc::new(
        Transport::new(
            TransportOptions {
                base_url: "http://127.0.0.1:9/api/".into(),
                ..Default::default()
            },
            session.clone(),
            "device".into(),
        )
        .unwrap(),
    );
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut comments = Comments::default();
    comments.open(None, transport.clone(), tx.clone());
    let generation = comments.view().generation;
    comments.select(None, transport.clone(), tx.clone());
    assert_eq!(comments.view().generation, generation);
    comments.select(Some(123), transport.clone(), tx.clone());
    let context = comments.context.unwrap();
    let fetch = comments.fetch_generation;
    session.set("42".into(), "synthetic".into());
    comments.select(Some(123), transport.clone(), tx.clone());
    assert!(!comments.finish_page(
        fetch,
        context,
        None,
        session.snapshot().revision,
        Ok(CommentPage {
            items: Vec::new(),
            total_count: 0,
            has_more: false
        })
    ));
    comments.close();
    assert!(!comments.view().open);
    comments.open(Some(i64::from(i32::MAX) + 1), transport.clone(), tx.clone());
    assert!(!comments.view().has_more);
    comments.refresh(transport.clone(), tx.clone());
    assert!(!comments.view().has_more);
    assert!(
        comments
            .view()
            .error
            .as_deref()
            .is_some_and(|message| message.contains("不支持"))
    );
    assert!(
        comments
            .publish(comments.generation(), "测试".into(), false, transport, tx)
            .is_err()
    );
}
