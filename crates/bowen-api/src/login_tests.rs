use super::*;
use crate::{session::Session, transport::TransportOptions};
use std::{
    io::{Read, Write},
    net::TcpListener,
};

fn validate_login(data: LoginDto, revision: u64) -> ExchangeOutcome {
    super::validate_login(data, revision, Session::default())
}

#[test]
fn adoption_is_bound_to_the_original_session_and_revision() {
    let session = Session::default();
    let response =
        || serde_json::from_str::<LoginDto>(r#"{"id":42,"bid":42,"token":"example"}"#).unwrap();
    let ExchangeOutcome::Credential(stale) = super::validate_login(response(), 0, session.clone())
    else {
        panic!("expected credential")
    };
    session.set("7".into(), "new".into());
    assert!(matches!(stale.adopt(), Err(LoginError::SessionChanged)));
    assert_eq!(session.snapshot().uid, "7");
    let ExchangeOutcome::Credential(current) =
        super::validate_login(response(), session.snapshot().revision, session.clone())
    else {
        panic!("expected credential")
    };
    current.adopt().unwrap();
    assert_eq!(session.snapshot().uid, "42");
}

#[test]
fn persisted_adoption_is_atomic_with_session_revision() {
    use std::cell::Cell;
    let session = Session::default();
    let response =
        || serde_json::from_str::<LoginDto>(r#"{"id":42,"bid":42,"token":"example"}"#).unwrap();
    let make = |revision| {
        let ExchangeOutcome::Credential(value) =
            super::validate_login(response(), revision, session.clone())
        else {
            panic!("expected credential")
        };
        value
    };
    let called = Cell::new(false);
    let stale = make(0);
    session.set("7".into(), "old".into());
    assert!(matches!(
        stale.adopt_with(|_, _, _| {
            called.set(true);
            Ok::<_, ()>(())
        }),
        Err(LoginError::SessionChanged)
    ));
    assert!(!called.get());

    let credential = make(session.snapshot().revision);
    assert_eq!(
        credential.adopt_with(|_, _, _| Err("locked")).unwrap(),
        Err("locked")
    );
    assert_eq!(session.snapshot().uid, "7");
    let credential = make(session.snapshot().revision);
    credential
        .adopt_with(|uid, token, _| {
            assert_eq!(uid, "42");
            assert_eq!(token, "example");
            Ok::<_, ()>(())
        })
        .unwrap()
        .unwrap();
    assert_eq!(session.snapshot().uid, "42");
}

#[tokio::test]
async fn old_qr_challenge_cannot_login_after_account_switch() {
    let session = Session::default();
    let transport = Transport::new(
        TransportOptions::default(),
        session.clone(),
        "device".into(),
    )
    .unwrap();
    let challenge = QrChallenge {
        key: "test".into(),
        landing_url: String::new(),
        session: session.clone(),
        session_revision: 0,
    };
    session.set("7".into(), "new".into());
    assert!(matches!(
        exchange_qr(&transport, &challenge).await,
        Err(LoginError::Transport(TransportError::SessionChanged))
    ));
    assert!(matches!(
        poll(&transport, &challenge).await,
        Err(LoginError::Transport(TransportError::SessionChanged))
    ));
    let another = Transport::new(
        TransportOptions::default(),
        Session::default(),
        "device".into(),
    )
    .unwrap();
    assert!(matches!(
        exchange_qr(&another, &challenge).await,
        Err(LoginError::SessionChanged)
    ));
}

#[test]
fn identity_requires_two_matching_values_and_a_token() {
    fn response(value: &str) -> LoginDto {
        serde_json::from_str(value).unwrap()
    }
    assert!(
        matches!(validate_login(response(r#"{"id":42,"bid":42,"token":"example","userInfo":{"id":42,"nickname":"test"}}"#), 7), ExchangeOutcome::Credential(c) if c.uid == 42 && c.session_revision == 7)
    );
    assert!(matches!(
        validate_login(response(r#"{"id":42,"bid":43,"token":"example"}"#), 0),
        ExchangeOutcome::IdentityMismatch
    ));
    assert!(matches!(
        validate_login(response(r#"{"id":42,"token":"example"}"#), 0),
        ExchangeOutcome::IdentityMismatch
    ));
    assert!(matches!(
        validate_login(response(r#"{"id":42,"bid":42}"#), 0),
        ExchangeOutcome::InvalidResponse
    ));
    assert!(matches!(
        validate_login(
            response(r#"{"id":"<redacted>","bid":"<redacted>","token":"example"}"#),
            0
        ),
        ExchangeOutcome::IdentityMismatch
    ));
}

#[test]
fn auth_types_and_json_fields_are_fixed() {
    assert_eq!(
        serde_json::to_string(&QrExchangeBody {
            auth_type: 10,
            qr_code: "test"
        })
        .unwrap(),
        r#"{"authType":10,"qrCode":"test"}"#
    );
    assert_eq!(
        serde_json::to_string(&PhoneExchangeBody {
            auth_type: 1,
            mobile: "test",
            verify_code: "1234"
        })
        .unwrap(),
        r#"{"authType":1,"mobile":"test","verifyCode":"1234"}"#
    );
}

#[tokio::test]
async fn challenge_and_poll_follow_desktop_contract() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let bodies = [
            r#"{"code":200,"data":{"qrCode":"test-key"}}"#,
            r#"{"code":200,"data":{"status":3}}"#,
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
    let challenge = create_challenge(&transport).await.unwrap();
    assert_eq!(
        challenge.landing_url,
        "https://bodian-oia.kuwo.cn/bodian/download.html?pageName=login_pc&pt=3&id=test-key"
    );
    assert_eq!(
        poll(&transport, &challenge).await.unwrap(),
        QrScanStatus::Confirmed
    );
    let requests = server.join().unwrap();
    assert!(requests[0].starts_with("GET /api/ucenter/login/qrCode?"));
    assert!(requests[1].starts_with("GET /api/ucenter/login/qrCodeStatus?qrCode=test-key&"));
    assert!(requests.iter().all(|request| request.contains("&sign=")));
}

#[tokio::test]
async fn qr_exchange_retries_pending_then_accepts_valid_credential() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for attempt in 0..3 {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            let count = socket.read(&mut request).unwrap();
            assert!(request[..count].starts_with(b"POST /api/ucenter/users/login?"));
            let body = if attempt < 2 {
                r#"{"code":11027}"#
            } else {
                r#"{"code":200,"data":{"id":42,"bid":42,"token":"sample"}}"#
            };
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .unwrap();
        }
    });
    let session = Session::default();
    let transport = Transport::new(
        TransportOptions {
            base_url: format!("http://{addr}/api/"),
            ..Default::default()
        },
        session.clone(),
        "device".into(),
    )
    .unwrap();
    let challenge = QrChallenge {
        key: "test-key".into(),
        landing_url: String::new(),
        session: session.clone(),
        session_revision: session.snapshot().revision,
    };
    let ExchangeOutcome::Credential(credential) =
        complete_qr_with_policy(&transport, &challenge, 5, std::time::Duration::ZERO)
            .await
            .unwrap()
    else {
        panic!("expected credential")
    };
    credential.adopt().unwrap();
    assert_eq!(session.snapshot().uid, "42");
    server.join().unwrap();
}

#[tokio::test]
async fn sms_failure_is_a_business_result() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut bytes = [0; 4096];
        let length = socket.read(&mut bytes).unwrap();
        let body = r#"{"code":11003}"#;
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
        String::from_utf8_lossy(&bytes[..length]).into_owned()
    });
    let options = TransportOptions {
        base_url: format!("http://{addr}/api/"),
        ..Default::default()
    };
    let transport = Transport::new(options, Session::default(), "device".into()).unwrap();
    assert!(matches!(
        send_sms(&transport, " ").await,
        Err(LoginError::BlankInput)
    ));
    assert_eq!(
        send_sms(&transport, "13800000000").await.unwrap(),
        SmsOutcome::Failed { code: 11003 }
    );
    let request = server.join().unwrap();
    assert!(request.starts_with("GET /api/ucenter/code/sendsms?type=2&mobile="));
    assert!(request.contains("&sign="));
}

#[tokio::test]
async fn exchange_distinguishes_pending_wrong_code_and_valid_identity() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let bodies = [
            r#"{"code":11027}"#,
            r#"{"code":11004}"#,
            r#"{"code":200,"data":{"id":42,"bid":42,"token":"example"}}"#,
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
    let session = Session::default();
    let transport = Transport::new(options, session.clone(), "device".into()).unwrap();
    let challenge = QrChallenge {
        key: "test-key".into(),
        landing_url: String::new(),
        session,
        session_revision: 0,
    };
    assert!(matches!(
        exchange_qr(&transport, &challenge).await.unwrap(),
        ExchangeOutcome::Pending
    ));
    assert!(matches!(
        exchange_phone(&transport, "test-mobile", "1234")
            .await
            .unwrap(),
        ExchangeOutcome::WrongCode
    ));
    assert!(
        matches!(exchange_phone(&transport, "test-mobile", "1234").await.unwrap(), ExchangeOutcome::Credential(c) if c.uid == 42)
    );
    let requests = server.join().unwrap();
    assert!(requests.iter().all(
        |request| request.starts_with("POST /api/ucenter/users/login?")
            && request.contains("&sign=")
    ));
}
