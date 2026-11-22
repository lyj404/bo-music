use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;

fn transport(session: Session) -> Transport {
    Transport::new(TransportOptions::default(), session, "device".into()).unwrap()
}

#[test]
fn prepares_query_headers_and_exact_signed_body() {
    let session = Session::default();
    session.set("123".into(), "private".into());
    let transport = transport(session.clone());
    let mut request = ApiRequest::get("play/music/v2/checkRight");
    request.signed = true;
    request.method = Method::POST;
    request.json_body = Some("{\"musicId\":1}".into());
    request.query.push(("keyword".into(), "晴天 ~".into()));
    let prepared = transport
        .prepare(&request, &session.snapshot(), 1234)
        .unwrap();
    assert_eq!(prepared.headers()["uid"], "123");
    assert_eq!(prepared.headers()["token"], "private");
    assert_eq!(prepared.headers()["ver"], "1.1.7");
    let query = prepared.url().query().unwrap();
    assert!(query.contains("keyword=%E6%99%B4%E5%A4%A9+%7E"));
    assert!(query.contains("timestamp=1234&sign="));
    let seed = format!(
        "keyword=%E6%99%B4%E5%A4%A9+%7E&uid=123&token={}&timestamp=1234&sign=",
        session.snapshot().token
    );
    assert!(query.ends_with(&signer::sign_raw(
        &request.path,
        &seed,
        request.json_body.as_deref()
    )));
    assert_eq!(
        prepared.body().unwrap().as_bytes().unwrap(),
        request.json_body.unwrap().as_bytes()
    );
}

#[test]
fn rejects_paths_which_can_escape_the_base() {
    let transport = transport(Session::default());
    for path in ["../bad", "/bad", "https://bad", "foo?token=x", "foo/%2e%2e"] {
        assert!(matches!(
            transport.prepare(&ApiRequest::get(path), &Session::default().snapshot(), 0),
            Err(TransportError::InvalidRequest)
        ));
    }
}

#[test]
fn envelope_preserves_unknown_codes_and_optional_fields() {
    let result = parse_envelope::<Value>(
        br#"{"code":23456,"msg":"no","reqId":"r","data":{"x":1},"lrcx":2}"#,
        3,
    )
    .unwrap();
    assert_eq!(
        (result.code, result.session_revision, result.lrcx),
        (23456, 3, Some(2))
    );
    assert_eq!(result.data.unwrap()["x"], 1);
    assert!(parse_envelope::<Value>(b"not json", 0).is_err());
}

#[tokio::test]
async fn send_handles_success_and_enforces_response_limit() {
    fn server(body: &'static str) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let thread = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            assert!(socket.read(&mut request).unwrap() > 0);
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        });
        (format!("http://{address}/api/"), thread)
    }

    let (base_url, thread) = server(r#"{"code":200,"data":{"answer":42}}"#);
    let options = TransportOptions {
        base_url,
        ..Default::default()
    };
    let transport = Transport::new(options, Session::default(), "device".into()).unwrap();
    let response: Envelope<Value> = transport.send(&ApiRequest::get("test")).await.unwrap();
    assert_eq!(response.data.unwrap()["answer"], 42);
    thread.join().unwrap();

    let (base_url, thread) = server(r#"{"code":200,"data":{"answer":42}}"#);
    let options = TransportOptions {
        base_url,
        max_response_bytes: 8,
        ..Default::default()
    };
    let transport = Transport::new(options, Session::default(), "device".into()).unwrap();
    assert!(matches!(
        transport.send::<Value>(&ApiRequest::get("test")).await,
        Err(TransportError::ResponseTooLarge)
    ));
    thread.join().unwrap();
}

#[tokio::test]
async fn stale_unauthorized_response_does_not_clear_the_new_session() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let session = Session::default();
    session.set("old".into(), "old-token".into());
    let changed_session = session.clone();
    let thread = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut request = [0; 4096];
        assert!(socket.read(&mut request).unwrap() > 0);
        changed_session.set("new".into(), "new-token".into());
        let body = r#"{"code":11012}"#;
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });
    let options = TransportOptions {
        base_url: format!("http://{address}/api/"),
        ..Default::default()
    };
    let transport = Transport::new(options, session.clone(), "device".into()).unwrap();
    assert!(matches!(
        transport.send::<Value>(&ApiRequest::get("test")).await,
        Err(TransportError::SessionChanged)
    ));
    thread.join().unwrap();
    assert_eq!(session.snapshot().uid, "new");
}
