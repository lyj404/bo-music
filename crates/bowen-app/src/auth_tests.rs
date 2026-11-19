use super::*;
use std::cell::{Cell, RefCell};

#[derive(Default)]
struct Fake {
    value: RefCell<Option<String>>,
    named: RefCell<std::collections::HashMap<String, String>>,
    fail: Cell<bool>,
}
impl SecretBackend for Fake {
    fn read(&self) -> Result<Option<String>, CredentialError> {
        if self.fail.get() {
            Err(CredentialError::Unavailable)
        } else {
            Ok(self.value.borrow().clone())
        }
    }
    fn write(&self, value: &str) -> Result<(), CredentialError> {
        if self.fail.get() {
            return Err(CredentialError::Unavailable);
        }
        *self.value.borrow_mut() = Some(value.into());
        Ok(())
    }
    fn delete(&self) -> Result<(), CredentialError> {
        if self.fail.get() {
            return Err(CredentialError::Unavailable);
        }
        *self.value.borrow_mut() = None;
        Ok(())
    }
    fn read_named(&self, name: &str) -> Result<Option<String>, CredentialError> {
        if self.fail.get() {
            return Err(CredentialError::Unavailable);
        }
        Ok(self.named.borrow().get(name).cloned())
    }
    fn write_named(&self, name: &str, value: &str) -> Result<(), CredentialError> {
        if self.fail.get() {
            return Err(CredentialError::Unavailable);
        }
        self.named.borrow_mut().insert(name.into(), value.into());
        Ok(())
    }
    fn delete_named(&self, name: &str) -> Result<(), CredentialError> {
        if self.fail.get() {
            return Err(CredentialError::Unavailable);
        }
        self.named.borrow_mut().remove(name);
        Ok(())
    }
}

#[test]
fn restore_and_sign_out_follow_store_outcomes() {
    let backend = Fake::default();
    let store = CredentialStore::new(backend);
    store
        .save(Credential {
            uid: "42".into(),
            token: "synthetic".into(),
            nickname: Some("听众".into()),
        })
        .unwrap();
    let session = Session::default();
    assert_eq!(restore(&session, &store).unwrap().unwrap().uid, "42");
    assert_eq!(session.snapshot().uid, "42");
    sign_out(&session, &store).unwrap();
    assert!(!session.snapshot().is_authenticated());
    assert!(restore(&session, &store).unwrap().is_none());
}

#[test]
fn switches_remembered_accounts_and_keeps_them_after_sign_out() {
    let store = CredentialStore::new(Fake::default());
    store
        .save(Credential {
            uid: "42".into(),
            token: "one".into(),
            nickname: Some("一".into()),
        })
        .unwrap();
    store
        .save(Credential {
            uid: "43".into(),
            token: "two".into(),
            nickname: Some("二".into()),
        })
        .unwrap();
    let session = Session::default();
    assert_eq!(restore(&session, &store).unwrap().unwrap().uid, "43");
    let before = session.snapshot().revision;
    assert_eq!(switch(&session, &store, "42").unwrap().uid, "42");
    assert!(session.snapshot().revision > before);
    assert_eq!(session.snapshot().token, "one");
    sign_out(&session, &store).unwrap();
    assert!(restore(&Session::default(), &store).unwrap().is_none());
    assert_eq!(store.remembered().unwrap().len(), 2);
    assert_eq!(switch(&session, &store, "43").unwrap().uid, "43");
}

#[test]
fn failed_sign_out_does_not_change_active_session() {
    let store = CredentialStore::new(Fake {
        fail: Cell::new(true),
        ..Default::default()
    });
    let session = Session::default();
    session.set("42".into(), "synthetic".into());
    assert!(matches!(
        sign_out(&session, &store),
        Err(AuthError::Storage(CredentialError::Unavailable))
    ));
    assert_eq!(session.snapshot().uid, "42");
}

#[test]
fn delayed_restore_cannot_undo_anonymous_sign_out() {
    struct Delayed {
        session: Session,
    }
    impl SecretBackend for Delayed {
        fn read(&self) -> Result<Option<String>, CredentialError> {
            let revision = self.session.snapshot().revision;
            self.session
                .clear_after(revision, || Ok::<_, CredentialError>(()))
                .unwrap()?;
            Ok(Some(
                r#"{"version":1,"credential":{"uid":"42","token":"synthetic","nickname":null}}"#
                    .into(),
            ))
        }
        fn write(&self, _: &str) -> Result<(), CredentialError> {
            unreachable!()
        }
        fn delete(&self) -> Result<(), CredentialError> {
            unreachable!()
        }
    }
    let session = Session::default();
    let store = CredentialStore::new(Delayed {
        session: session.clone(),
    });
    assert!(matches!(
        restore(&session, &store),
        Err(AuthError::SessionChanged)
    ));
    assert!(!session.snapshot().is_authenticated());
}

#[tokio::test]
async fn validated_login_can_be_persisted_and_restored() {
    use bowen_api::{
        login::{self, ExchangeOutcome},
        transport::{Transport, TransportOptions},
    };
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut bytes = [0; 4096];
        let length = socket.read(&mut bytes).unwrap();
        assert!(bytes[..length].starts_with(b"POST /api/ucenter/users/login?"));
        let body = r#"{"code":200,"data":{"id":42,"bid":42,"token":"synthetic","userInfo":{"id":42,"nickname":"听众"}}}"#;
        write!(
            socket,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
    });
    let session = Session::default();
    let transport = Transport::new(
        TransportOptions {
            base_url: format!("http://{address}/api/"),
            ..Default::default()
        },
        session.clone(),
        "device".into(),
    )
    .unwrap();
    let ExchangeOutcome::Credential(credential) =
        login::exchange_phone(&transport, "13800000000", "123456")
            .await
            .unwrap()
    else {
        panic!("expected credential")
    };
    let store = CredentialStore::new(Fake::default());
    assert_eq!(install(credential, &store).unwrap().uid, "42");
    assert_eq!(session.snapshot().uid, "42");
    let restarted = Session::default();
    assert_eq!(restore(&restarted, &store).unwrap().unwrap().uid, "42");
    assert_eq!(restarted.snapshot().token, "synthetic");
    server.join().unwrap();
}
