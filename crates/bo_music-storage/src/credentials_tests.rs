use super::*;
use std::cell::{Cell, RefCell};

#[derive(Default)]
struct Fake {
    value: RefCell<Option<String>>,
    named: RefCell<std::collections::HashMap<String, String>>,
    fail: Cell<bool>,
    fail_primary_write: Cell<bool>,
    fail_named_delete: Cell<bool>,
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
        if self.fail.get() || self.fail_primary_write.get() {
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
            Err(CredentialError::Unavailable)
        } else {
            Ok(self.named.borrow().get(name).cloned())
        }
    }
    fn write_named(&self, name: &str, value: &str) -> Result<(), CredentialError> {
        if self.fail.get() {
            return Err(CredentialError::Unavailable);
        }
        self.named.borrow_mut().insert(name.into(), value.into());
        Ok(())
    }
    fn delete_named(&self, name: &str) -> Result<(), CredentialError> {
        if self.fail.get() || self.fail_named_delete.get() {
            return Err(CredentialError::Unavailable);
        }
        self.named.borrow_mut().remove(name);
        Ok(())
    }
}
fn credential() -> Credential {
    Credential {
        uid: "42".into(),
        token: "sample-secret".into(),
        nickname: Some("测试".into()),
    }
}
#[test]
fn saves_loads_and_deletes_only_through_backend() {
    let store = CredentialStore::new(Fake::default());
    assert!(store.load().unwrap().is_none());
    store.save(credential()).unwrap();
    let loaded = store.load().unwrap().unwrap();
    assert_eq!(loaded.uid, "42");
    assert_eq!(loaded.token, "sample-secret");
    store.clear().unwrap();
    store.clear().unwrap();
    assert!(store.load().unwrap().is_none());
}
#[test]
fn rejects_invalid_and_newer_records_without_overwriting() {
    let store = CredentialStore::new(Fake::default());
    store.save(credential()).unwrap();
    let old = store.backend.read().unwrap();
    let mut bad = credential();
    bad.uid = "-1".into();
    assert_eq!(store.save(bad), Err(CredentialError::Invalid));
    let mut bad = credential();
    bad.token = "x".repeat(MAX_ENCODED_BYTES);
    assert_eq!(store.save(bad), Err(CredentialError::Invalid));
    assert_eq!(store.backend.read().unwrap(), old);
    store
        .backend
        .write(r#"{"version":2,"credential":{"uid":"42","token":"secret","nickname":null}}"#)
        .unwrap();
    assert!(matches!(store.load(), Err(CredentialError::Invalid)));
}
#[test]
fn unavailable_store_never_falls_back_to_plaintext() {
    let store = CredentialStore::new(Fake::default());
    store.backend.fail.set(true);
    assert_eq!(store.save(credential()), Err(CredentialError::Unavailable));
    assert!(matches!(store.load(), Err(CredentialError::Unavailable)));
    assert_eq!(store.clear(), Err(CredentialError::Unavailable));
    assert!(store.backend.value.borrow().is_none());
}

#[test]
fn legacy_login_migrates_and_keeps_multiple_large_credentials_in_separate_entries() {
    let store = CredentialStore::new(Fake::default());
    store
        .backend
        .write(&encode_credential(&credential()).unwrap())
        .unwrap();
    let second = Credential {
        uid: "43".into(),
        token: "b".repeat(1300),
        nickname: Some("另一个".into()),
    };
    store.save(second).unwrap();
    assert_eq!(
        store
            .remembered()
            .unwrap()
            .iter()
            .map(|item| item.uid.as_str())
            .collect::<Vec<_>>(),
        vec!["43", "42"]
    );
    assert_eq!(store.load().unwrap().unwrap().uid, "43");
    assert_eq!(store.switch("42").unwrap().token, "sample-secret");
    let primary = store.backend.read().unwrap().unwrap();
    assert!(!primary.contains("sample-secret"));
    assert!(!primary.contains(&"b".repeat(100)));
    store.clear().unwrap();
    assert!(store.load().unwrap().is_none());
    assert_eq!(store.switch("43").unwrap().token.len(), 1300);
}

#[test]
fn five_account_limit_forgetting_active_and_delayed_cleanup() {
    let store = CredentialStore::new(Fake::default());
    for id in 1..=6 {
        store
            .save(Credential {
                uid: id.to_string(),
                token: format!("token-{id}"),
                nickname: None,
            })
            .unwrap();
    }
    assert_eq!(
        store
            .remembered()
            .unwrap()
            .iter()
            .map(|item| item.uid.as_str())
            .collect::<Vec<_>>(),
        vec!["6", "5", "4", "3", "2"]
    );
    assert_eq!(store.backend.named.borrow().len(), 5);
    store.forget("6").unwrap();
    assert_eq!(store.load().unwrap().unwrap().uid, "6");
    assert_eq!(store.remembered().unwrap().len(), 4);
    store.clear().unwrap();
    assert_eq!(store.backend.named.borrow().len(), 4);
    store.backend.fail_named_delete.set(true);
    assert_eq!(store.forget("5"), Err(CredentialError::Unavailable));
    assert!(store.has_pending_cleanup().unwrap());
    let before_listing = store.backend.read().unwrap();
    assert!(
        !store
            .remembered()
            .unwrap()
            .iter()
            .any(|entry| entry.uid == "5")
    );
    assert_eq!(store.backend.read().unwrap(), before_listing);
    store.backend.fail_named_delete.set(false);
    store.retry_cleanup().unwrap();
    assert!(!store.has_pending_cleanup().unwrap());
    assert_eq!(store.backend.named.borrow().len(), 3);
}

#[test]
fn failed_index_commit_preserves_previous_active_credential() {
    let store = CredentialStore::new(Fake::default());
    store.save(credential()).unwrap();
    let previous = store.backend.read().unwrap();
    let count = store.backend.named.borrow().len();
    store.backend.fail_primary_write.set(true);
    let next = Credential {
        uid: "43".into(),
        token: "different".into(),
        nickname: None,
    };
    assert_eq!(store.save(next), Err(CredentialError::Unavailable));
    store.backend.fail_primary_write.set(false);
    assert_eq!(store.backend.read().unwrap(), previous);
    assert_eq!(store.backend.named.borrow().len(), count);
    assert_eq!(store.load().unwrap().unwrap().uid, "42");
}

#[test]
fn failed_legacy_migration_keeps_original_and_discards_staged_entries() {
    let store = CredentialStore::new(Fake::default());
    let legacy = encode_credential(&credential()).unwrap();
    store.backend.write(&legacy).unwrap();
    store.backend.fail_primary_write.set(true);
    assert_eq!(
        store.save(Credential {
            uid: "43".into(),
            token: "new".into(),
            nickname: None
        }),
        Err(CredentialError::Unavailable)
    );
    store.backend.fail_primary_write.set(false);
    assert_eq!(
        store.backend.read().unwrap().as_deref(),
        Some(legacy.as_str())
    );
    assert!(store.backend.named.borrow().is_empty());
    assert_eq!(store.load().unwrap().unwrap().uid, "42");
}

#[test]
fn expired_account_cannot_be_restored_until_reauthentication() {
    let store = CredentialStore::new(Fake::default());
    store.save(credential()).unwrap();
    store.expire_active().unwrap();
    assert!(store.load().unwrap().is_none());
    assert!(store.remembered().unwrap()[0].stale);
    assert!(matches!(store.switch("42"), Err(CredentialError::Invalid)));
    let mut fresh = credential();
    fresh.token = "fresh-token".into();
    store.save(fresh).unwrap();
    assert!(!store.remembered().unwrap()[0].stale);
    assert_eq!(store.switch("42").unwrap().token, "fresh-token");
}

#[test]
fn cleanup_reports_index_write_failure_and_can_be_retried() {
    let store = CredentialStore::new(Fake::default());
    store.save(credential()).unwrap();
    store
        .save(Credential {
            uid: "43".into(),
            token: "second".into(),
            nickname: None,
        })
        .unwrap();
    store.backend.fail_named_delete.set(true);
    assert_eq!(store.forget("42"), Err(CredentialError::Unavailable));
    store.backend.fail_named_delete.set(false);
    store.backend.fail_primary_write.set(true);
    assert_eq!(store.retry_cleanup(), Err(CredentialError::Unavailable));
    store.backend.fail_primary_write.set(false);
    assert!(store.has_pending_cleanup().unwrap());
    store.retry_cleanup().unwrap();
    assert!(!store.has_pending_cleanup().unwrap());
    assert_eq!(store.load().unwrap().unwrap().uid, "43");
}
