use super::*;

#[test]
fn comments_follow_current_track_and_publish_reply() {
    use bowen_storage::{
        credentials::{CredentialError, SecretBackend},
        queue::QueueSnapshot,
    };
    use std::sync::Mutex;
    struct Memory;
    impl SecretBackend for Memory {
        fn read(&self) -> Result<Option<String>, CredentialError> {
            Ok(Some(
                r#"{"version":1,"credential":{"uid":"42","token":"synthetic","nickname":"听众"}}"#
                    .into(),
            ))
        }
        fn write(&self, _: &str) -> Result<(), CredentialError> {
            Ok(())
        }
        fn delete(&self) -> Result<(), CredentialError> {
            Ok(())
        }
    }
    fn track(id: i64) -> Track {
        Track {
            id,
            title: format!("歌曲{id}"),
            artist_text: String::new(),
            artists: vec![],
            album_name: None,
            album_id: 0,
            cover_url: None,
            duration_seconds: 60,
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
    fn wait(service: &Service, test: impl Fn(&Event) -> bool) -> Event {
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            if let Ok(event) = service.events.recv_timeout(Duration::from_millis(100))
                && test(&event)
            {
                return event;
            }
        }
        panic!("expected event did not arrive")
    }
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let server_stop = stop.clone();
    let writes = Arc::new(Mutex::new(Vec::<(String, serde_json::Value)>::new()));
    let captured = writes.clone();
    let share_requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let captured_shares = share_requests.clone();
    let server = std::thread::spawn(move || {
        let mut published = false;
        while !server_stop.load(Ordering::Relaxed) {
            let (mut socket, _) = match listener.accept() {
                Ok(value) => value,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                }
                Err(_) => panic!("accept failed"),
            };
            socket.set_nonblocking(false).unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut buffer = [0; 4096];
                let count = socket.read(&mut buffer).unwrap_or(0);
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
            if !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                continue;
            }
            let request = String::from_utf8_lossy(&bytes);
            let body = if request.starts_with("GET /api/service/share/text?") {
                captured_shares.lock().unwrap().push(request.into_owned());
                r#"{"code":23006}"#
            } else if request.starts_with("GET /api/comments/v3/replies?") {
                if published {
                    r#"{"code":200,"data":{"comment_list":[{"id":101,"parentId":100,"nickname":"听友","content":"回复"},{"id":777,"uid":42,"content":"回复内容"}],"comment_total":2,"more":false}}"#
                } else {
                    r#"{"code":200,"data":{"comment_list":[{"id":101,"parentId":100,"nickname":"听友","content":"回复"}],"comment_total":1,"more":false}}"#
                }
            } else if request.starts_with("GET /api/comments/v3/hot?")
                || request.starts_with("GET /api/comments/v3/new?")
            {
                if request.contains("moduleId=2&") {
                    r#"{"code":200,"data":{"comment_list":[{"id":200,"content":"另一首"}],"comment_total":1,"more":false}}"#
                } else {
                    r#"{"code":200,"data":{"comment_list":[{"id":100,"nickname":"甲","content":"评论","likeCount":2}],"comment_total":1,"more":false}}"#
                }
            } else if request.starts_with("POST /api/comments/v3/like?")
                || request.starts_with("POST /api/comments/v3/publish?")
            {
                let operation = if request.starts_with("POST /api/comments/v3/like?") {
                    "LIKE"
                } else {
                    published = true;
                    "PUBLISH"
                };
                let payload =
                    serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
                captured.lock().unwrap().push((operation.into(), payload));
                r#"{"code":200,"data":{"commentId":777}}"#
            } else if request.starts_with("GET /api/service/playlist/fond?") {
                r#"{"code":200,"data":{}}"#
            } else if request.starts_with("GET /api/play/music/v2/checkRight?") {
                r#"{"code":200,"data":{"status":7}}"#
            } else if request.starts_with("GET /mobi.s?") {
                r#"{"code":200,"data":{"content":""}}"#
            } else {
                panic!("unexpected local request")
            };
            let _ = write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
        }
    });
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "bowen-comments-test-{}-{unique}",
        std::process::id()
    ));
    let paths = DataPaths::new(&root);
    QueueStore::new(paths.clone())
        .save(
            "42",
            &QueueSnapshot {
                tracks: vec![track(1), track(2)],
                current_index: Some(0),
                position_seconds: 0.0,
            },
        )
        .unwrap();
    let library = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../libmpv/libmpv-2.dll");
    let service = Service::start_with(ServiceOptions {
        paths: Some(paths),
        base_url: Some(format!("http://{address}/api/")),
        lyrics_endpoint: Some(format!("http://{address}/mobi.s")),
        library: Some(library),
        audio_output: Some("null".into()),
        credentials: Some(Arc::new(Memory)),
        ..Default::default()
    });
    wait(
        &service,
        |event| matches!(event, Event::Auth(view) if !view.busy && view.account.is_some()),
    );
    let Event::Player(player) = wait(
        &service,
        |event| matches!(event, Event::Player(view) if view.track_id == Some(1)),
    ) else {
        unreachable!()
    };
    service
        .commands
        .send(Command::ShareCurrent(
            player.queue_account_revision,
            player.queue_revision,
            1,
        ))
        .unwrap();
    let Event::CopyShareLink(link) =
        wait(&service, |event| matches!(event, Event::CopyShareLink(_)))
    else {
        unreachable!()
    };
    assert_eq!(
        link,
        "https://h5app.kuwo.cn/m/bodian/playMusic.html?uid=42&musicId=1&opusId="
    );
    wait(
        &service,
        |event| matches!(event, Event::Status(message) if message.contains("分享计数")),
    );
    service.commands.send(Command::OpenComments).unwrap();
    let Event::Comments(view) = wait(
        &service,
        |event| matches!(event, Event::Comments(view) if view.track_id == Some(1) && !view.loading && view.rows.len() == 1),
    ) else {
        unreachable!()
    };
    service
        .commands
        .send(Command::CommentSort(CommentSort::Latest, view.generation))
        .unwrap();
    let Event::Comments(view) = wait(
        &service,
        |event| matches!(event, Event::Comments(view) if view.sort == CommentSort::Latest && !view.loading && view.rows.len() == 1),
    ) else {
        unreachable!()
    };
    service
        .commands
        .send(Command::ToggleCommentLike(100, view.generation))
        .unwrap();
    let Event::Comments(view) = wait(
        &service,
        |event| matches!(event, Event::Comments(view) if view.rows.first().is_some_and(|row| row.id == 100 && row.is_liked && row.like_count == 3)),
    ) else {
        unreachable!()
    };
    service
        .commands
        .send(Command::OpenCommentThread(100, view.generation))
        .unwrap();
    let Event::Comments(view) = wait(
        &service,
        |event| matches!(event, Event::Comments(view) if view.root.as_ref().is_some_and(|root| root.id == 100) && !view.loading && view.rows.len() == 1),
    ) else {
        unreachable!()
    };
    service
        .commands
        .send(Command::ReplyToComment(101, view.generation))
        .unwrap();
    let Event::Comments(view) = wait(
        &service,
        |event| matches!(event, Event::Comments(view) if view.reply_target.as_ref().is_some_and(|target| target.0 == 101)),
    ) else {
        unreachable!()
    };
    service
        .commands
        .send(Command::PublishComment(
            view.generation,
            "回复内容".into(),
            true,
        ))
        .unwrap();
    wait(
        &service,
        |event| matches!(event, Event::CommentPostResult(generation, true) if *generation == view.generation),
    );
    wait(
        &service,
        |event| matches!(event, Event::Comments(view) if view.root.is_some() && !view.loading && view.reply_target.as_ref().is_some_and(|target| target.0 == 101) && view.rows.iter().any(|row| row.id == 777)),
    );
    service
        .commands
        .send(Command::EditQueue(
            player.queue_account_revision,
            player.queue_revision,
            QueueEdit::Select(1),
        ))
        .unwrap();
    let Event::Comments(next) = wait(
        &service,
        |event| matches!(event, Event::Comments(view) if view.track_id == Some(2) && !view.loading && view.rows.first().is_some_and(|row| row.id == 200)),
    ) else {
        unreachable!()
    };
    service
        .commands
        .send(Command::ShareCurrent(
            player.queue_account_revision,
            player.queue_revision,
            1,
        ))
        .unwrap();
    wait(
        &service,
        |event| matches!(event, Event::Status(message) if message.contains("变化")),
    );
    service
        .commands
        .send(Command::ToggleCommentLike(100, view.generation))
        .unwrap();
    wait(
        &service,
        |event| matches!(event, Event::Comments(view) if view.track_id == Some(2) && view.generation == next.generation),
    );
    drop(service);
    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();
    let shares = share_requests.lock().unwrap();
    assert_eq!(shares.len(), 1);
    assert!(shares[0].starts_with(
        "GET /api/service/share/text?shareTo=5&shareSource=0&sourceId=1&playlistType=4&"
    ));
    drop(shares);
    let writes = writes.lock().unwrap();
    assert_eq!(writes.len(), 2);
    assert_eq!(
        writes[0].1,
        serde_json::json!({"moduleId":1,"moduleType":2,"uid":42,"commentId":100,"op":1})
    );
    assert_eq!(
        writes[1].1,
        serde_json::json!({"moduleId":1,"moduleType":2,"uid":42,"content":"回复内容","parentId":100,"replyId":101,"anonymous":1})
    );
    drop(writes);
    std::fs::remove_dir_all(root).unwrap();
}
