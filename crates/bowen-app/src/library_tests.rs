use super::*;
use bowen_api::{session::Session, transport::TransportOptions};
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};

async fn page(
    library: &mut MusicLibrary,
    rx: &mut mpsc::UnboundedReceiver<Command>,
    revision: u64,
) -> (u64, PagedCursor) {
    let Command::LibraryAlbumsDone(epoch, request_revision, cursor, result) =
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("expected album page")
    };
    assert_eq!(request_revision, revision);
    assert!(library.finish_albums(epoch, request_revision, cursor.clone(), result, revision));
    (epoch, cursor)
}

#[tokio::test]
async fn navigation_children_sort_pagination_retry_and_sample_are_account_bound() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let responses = [
            r#"{"code":200,"data":[{"id":"2005","ptypeName":"流行","childList":[{"id":"8101","name":"华语","albumVo":{"id":99,"name":"推荐"}},{"id":"8102","name":"欧美"}]}]}"#,
            r#"{"code":200,"data":{"list":[{"id":11,"name":"首张"}]}}"#,
            r#"{"code":200,"data":{"list":[]}}"#,
            r#"{"code":500}"#,
            r#"{"code":200,"data":{"list":[{"id":12,"name":"最新"}]}}"#,
            r#"{"code":200,"data":{"list":[{"id":13,"name":"欧美专辑"}]}}"#,
        ];
        let mut requests = Vec::new();
        for body in responses {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut buffer = [0; 4096];
            let count = socket.read(&mut buffer).unwrap();
            requests.push(String::from_utf8_lossy(&buffer[..count]).into_owned());
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
    let mut library = MusicLibrary::default();
    library.reload(transport.clone(), tx.clone());
    let Command::LibraryNavigationDone(epoch, revision, result) =
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("expected navigation")
    };
    assert!(library.finish_navigation(epoch, revision, result, revision));
    let view = library.view();
    assert_eq!(view.groups.len(), 1);
    library.select_group(0, view.generation, transport.clone(), tx.clone());
    let (old_epoch, old_cursor) = page(&mut library, &mut rx, revision).await;
    let view = library.view();
    assert_eq!(view.selected_child, Some(0));
    assert_eq!(
        library
            .album(None, 99, view.generation, revision)
            .unwrap()
            .id,
        99
    );
    assert_eq!(
        library
            .album(Some(0), 11, view.generation, revision)
            .unwrap()
            .id,
        11
    );
    assert!(view.has_more);
    library.next(transport.clone(), tx.clone());
    assert!(!library.finish_albums(old_epoch, revision, old_cursor, Ok(vec![]), revision));
    page(&mut library, &mut rx, revision).await;
    assert!(!library.view().has_more);
    library.sort(
        LibrarySort::Newest,
        library.generation(),
        transport.clone(),
        tx.clone(),
    );
    page(&mut library, &mut rx, revision).await;
    assert!(library.view().error.is_some() && library.view().has_more);
    library.next(transport.clone(), tx.clone());
    page(&mut library, &mut rx, revision).await;
    assert_eq!(library.view().albums[0].id, 12);
    library.select_child(1, library.generation(), transport.clone(), tx.clone());
    page(&mut library, &mut rx, revision).await;
    let view = library.view();
    assert_eq!(view.albums[0].id, 13);
    assert!(
        library
            .album(Some(0), 13, view.generation, revision + 1)
            .is_none()
    );
    session.set("42".into(), "synthetic".into());
    library.invalidate();
    assert!(!library.finish_albums(
        old_epoch,
        revision,
        service_cursor(30),
        Ok(vec![]),
        session.snapshot().revision
    ));
    let requests = server.join().unwrap();
    assert!(requests[1].contains("pn=1&rn=30&pTypeId=2005&cTypeId=8101&sort=1"));
    assert!(requests[2].contains("pn=2&rn=30"));
    assert!(requests[3].contains("pn=1&rn=30&pTypeId=2005&cTypeId=8101&sort=2"));
    assert!(requests[4].contains("pn=1&rn=30&pTypeId=2005&cTypeId=8101&sort=2"));
    assert!(requests[5].contains("cTypeId=8102&sort=2"));
}

#[tokio::test]
async fn missing_category_id_does_not_start_a_request() {
    use bowen_domain::catalog::LibraryChild;
    let session = Session::default();
    let transport = Arc::new(
        Transport::new(
            TransportOptions {
                base_url: "http://127.0.0.1:9/api/".into(),
                ..Default::default()
            },
            session,
            "device".into(),
        )
        .unwrap(),
    );
    let mut library = MusicLibrary {
        revision: 0,
        groups: vec![LibraryGroup {
            id: String::new(),
            name: "分类".into(),
            title: String::new(),
            description: String::new(),
            cover_url: None,
            album_count: 0,
            children: vec![LibraryChild {
                id: "8101".into(),
                name: "子类".into(),
                title: String::new(),
                description: String::new(),
                sample_album: None,
            }],
        }],
        ..Default::default()
    };
    let (tx, mut rx) = mpsc::unbounded_channel();
    library.select_group(0, 0, transport, tx);
    assert!(library.view().error.is_some());
    assert!(!library.view().has_more);
    assert!(!library.view().loading);
    assert!(rx.try_recv().is_err());
}
