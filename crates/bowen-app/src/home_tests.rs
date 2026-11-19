use super::*;
use bowen_api::{session::Session, transport::TransportOptions};
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};

#[tokio::test]
async fn loads_batches_recovers_a_module_and_opens_ai_and_playlists() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let responses = [
            r#"{"code":200,"data":{"moduleList":[{"id":1,"type":3,"name":"推荐"},{"id":2,"type":4,"name":"AI"},{"id":3,"type":5,"name":"歌单"},{"id":4,"type":10,"name":"新歌"},{"id":5,"type":3,"name":"更多"},{"id":6,"type":99}]}}"#,
            r#"{"code":200,"data":{"moduleName":"推荐","musicList":[{"id":11,"name":"歌曲"}]}}"#,
            r#"{"code":200,"data":{"moduleName":"AI","songList":[{"title":"空","songs":[]},{"title":"推荐歌单","passRecName":"来自 推荐","songs":[{"id":21,"name":"预览"}]}]}}"#,
            r#"{"code":200,"data":{"moduleName":"歌单","songList":[{"id":99,"name":"精选","sourceType":7}]}}"#,
            r#"{"code":500}"#,
            r#"{"code":200,"data":{"moduleName":"新歌","musicList":[{"id":44,"name":"重试成功"}]}}"#,
            r#"{"code":200,"data":{"moduleName":"更多","musicList":[{"id":55,"name":"更多歌曲"}]}}"#,
            r#"{"code":200,"data":{"title":"完整推荐","musicList":[{"id":22,"name":"完整歌曲"}]}}"#,
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
    let mut home = Home::default();
    home.reload(transport.clone(), tx.clone());
    let Command::HomeLayoutDone(epoch, revision, result) =
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("expected layout")
    };
    assert!(home.finish_layout(epoch, revision, result, transport.clone(), tx.clone()));
    let Command::HomeBatchDone(epoch, revision, start, rows) =
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("expected batch")
    };
    assert!(home.finish_batch(epoch, revision, start, rows, session.snapshot().revision));
    let view = home.view();
    assert_eq!(view.items.len(), 4);
    assert!(view.has_more && view.items[3].error);
    assert_eq!(
        home.track(Some(0), 0, 11, view.generation, revision)
            .unwrap()
            .id,
        11
    );
    assert_eq!(
        home.module_tracks(0, view.generation, revision).unwrap()[0].id,
        11
    );
    assert!(
        home.module_tracks(0, view.generation.wrapping_add(1), revision)
            .is_none()
    );
    assert!(home.module_tracks(3, view.generation, revision).is_none());
    assert!(
        home.track(Some(0), 0, 11, view.generation.wrapping_add(1), revision)
            .is_none()
    );
    assert_eq!(
        home.playlist(2, 0, 99, view.generation, revision)
            .unwrap()
            .source_type,
        7
    );
    home.refresh(3, view.generation, transport.clone(), tx.clone());
    let Command::HomeRefreshDone(epoch, revision, index, result) =
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("expected refresh")
    };
    assert!(home.finish_refresh(epoch, revision, index, result, session.snapshot().revision));
    assert!(!home.view().items[3].error);
    home.load_more(3, transport.clone(), tx.clone());
    let Command::HomeBatchDone(epoch, revision, start, rows) =
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("expected second batch")
    };
    assert!(home.finish_batch(epoch, revision, start, rows, session.snapshot().revision));
    let view = home.view();
    assert_eq!(view.items.len(), 5);
    assert!(!view.has_more);
    home.open_ai(1, 0, view.generation, transport.clone(), tx.clone());
    let Command::HomeAiDone(epoch, revision, generation, result) =
        tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap()
    else {
        panic!("expected AI detail")
    };
    assert!(home.finish_ai(
        epoch,
        revision,
        generation,
        result,
        session.snapshot().revision
    ));
    let view = home.view();
    assert_eq!(
        home.track(None, 0, 22, view.generation, revision)
            .unwrap()
            .id,
        22
    );
    assert_eq!(home.ai_tracks(view.generation, revision).unwrap().len(), 1);
    home.close_ai(view.generation, revision);
    assert!(home.track(None, 0, 22, view.generation, revision).is_none());
    let requests = server.join().unwrap();
    assert!(requests[7].starts_with("GET /api/service/home/aiPlaylistDetail?index=1&passRecName="));
}

#[test]
fn old_account_and_old_reload_results_are_rejected() {
    let mut home = Home {
        revision: 4,
        epoch: 2,
        version: 3,
        loading: true,
        ..Default::default()
    };
    assert!(!home.finish_batch(2, 4, 0, vec![], 5));
    home.invalidate();
    assert!(!home.finish_batch(2, 4, 0, vec![], 4));
    assert!(home.view().items.is_empty());
}
