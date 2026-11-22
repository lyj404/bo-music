use super::*;
use crate::{session::Session, transport::TransportOptions};
use std::{
    io::{Read, Write},
    net::TcpListener,
};

fn server(bodies: Vec<&'static str>) -> (Transport, Session, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let thread = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for body in bodies {
            let (mut socket, _) = listener.accept().unwrap();
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
    let options = TransportOptions {
        base_url: format!("http://{address}/api/"),
        ..Default::default()
    };
    (
        Transport::new(options, session.clone(), "device".into()).unwrap(),
        session,
        thread,
    )
}

#[tokio::test]
async fn listening_reports_map_data_and_bound_write() {
    let (transport, session, server) = server(vec![
        r#"{"code":200,"data":{"musicList":[{"id":118991,"name":"A","playcnt":3},{"id":0}]}}"#,
        r#"{"code":200,"data":{"monthReport":{"monthDesc":"10月听歌","listenTotal":"175","pic":"https://img.example/report.jpg"}}}"#,
        r#"{"code":200,"data":{"isUpdate":1,"info":{"monthDate":"2026-10-01","playTotal":106,"listenTotal":"175","songNum":81,"timeRank":43,"musicTop10":[{"id":5,"name":"A","playCount":6}],"albumTop10":["https://img.example/a.jpg","bad"]}}}"#,
        r#"{"code":200,"data":{"isUpdate":1}}"#,
        r#"{"code":200,"data":{}}"#,
    ]);
    session.set("42".into(), "example".into());
    let ranking = weekly_ranking(&transport).await.unwrap().unwrap();
    assert_eq!(
        (ranking.len(), ranking[0].track.id, ranking[0].play_count),
        (1, 118991, 3)
    );
    let summary = month_summary(&transport).await.unwrap().unwrap();
    assert_eq!(summary.month_desc.as_deref(), Some("10月听歌"));
    let period = MonthPeriod::new(2026, 10).unwrap();
    let report = month_report(&transport, period).await.unwrap().unwrap();
    assert_eq!(
        (report.period, report.play_count, report.song_count),
        (period, Some(106), Some(81))
    );
    assert_eq!(
        (report.top_tracks[0].id, report.top_tracks[0].play_count),
        (5, Some(6))
    );
    assert_eq!(report.album_covers.len(), 1);
    assert!(month_report(&transport, period).await.unwrap().is_none());
    assert!(create_report_playlist(&transport, period).await.unwrap());
    let requests = server.join().unwrap();
    assert!(requests[0].starts_with("GET /api/ucenter/playdata/play_ranking?userId=42&type=1&"));
    assert!(requests[1].starts_with("GET /api/play/month/report/summary?userId=42&"));
    assert!(requests[2].starts_with("GET /api/play/month/report/info?date=202610&"));
    assert!(requests[4].starts_with("GET /api/play/month/report/playlist?date=202610&"));
    assert!(requests.iter().all(|request| request.contains("&sign=")));
}

#[tokio::test]
async fn month_report_distinguishes_empty_data_from_failure_and_rejects_old_writes() {
    let (transport, session, server) = server(vec![
        r#"{"code":200,"data":{}}"#,
        r#"{"code":500}"#,
        r#"{"code":200,"data":{}}"#,
    ]);
    session.set("42".into(), "example".into());
    let revision = session.snapshot().revision;
    let period = MonthPeriod::new(2026, 1).unwrap();
    assert!(
        month_report_at_revision(&transport, period, revision)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        month_report_at_revision(&transport, period, revision)
            .await
            .is_err()
    );
    assert!(matches!(
        create_report_playlist_at_revision(&transport, period, revision.wrapping_sub(1)).await,
        Err(AccountError::Transport(TransportError::SessionChanged))
    ));
    assert!(
        create_report_playlist_at_revision(&transport, period, revision)
            .await
            .unwrap()
    );
    let requests = server.join().unwrap();
    assert!(requests[2].starts_with("GET /api/play/month/report/playlist?date=202601&"));
}

#[test]
fn vip_signal_badges_and_expiry_are_display_only() {
    let pay: PayInfoDto = serde_json::from_str(r#"{"isVip":1,"vipType":1,"payVipType":2,"expireDate":4102444800001,"actExpireDate":1853769599999}"#).unwrap();
    assert_eq!(pay.badge(), VipBadge::Standard);
    assert_eq!(pay.expiry(), Some(1853769599999));
    assert_eq!(PayInfoDto::default().badge(), VipBadge::None);
    assert_eq!(MonthPeriod::new(2026, 0), None);
}

#[tokio::test]
async fn late_account_result_cannot_be_returned_after_switch() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let session = Session::default();
    session.set("42".into(), "old".into());
    let changed = session.clone();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut buffer = [0; 4096];
        assert!(socket.read(&mut buffer).unwrap() > 0);
        changed.set("43".into(), "new".into());
        let body = r#"{"code":200,"data":{"followCount":10}}"#;
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });
    let transport = Transport::new(
        TransportOptions {
            base_url: format!("http://{address}/api/"),
            ..Default::default()
        },
        session.clone(),
        "device".into(),
    )
    .unwrap();
    assert!(matches!(
        metadata(&transport).await,
        Err(AccountError::Transport(TransportError::SessionChanged))
    ));
    server.join().unwrap();
    assert_eq!(session.snapshot().uid, "43");
}
