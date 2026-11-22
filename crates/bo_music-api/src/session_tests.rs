use super::*;

#[test]
fn stale_account_switch_does_not_write_or_replace_session() {
    let session = Session::default();
    let old = session.snapshot().revision;
    session.set("42".into(), "new".into());
    let called = std::cell::Cell::new(false);
    let result = session.switch_after(old, || {
        called.set(true);
        Ok::<_, ()>(StoredIdentity::new("43".into(), "old".into()).unwrap())
    });
    assert!(result.is_none());
    assert!(!called.get());
    assert_eq!(session.snapshot().uid, "42");
}

#[test]
fn late_unauthorized_cannot_clear_new_account() {
    let session = Session::default();
    session.set("first".into(), "secret".into());
    let old = session.snapshot();
    session.set("second".into(), "other secret".into());
    assert!(!session.clear_if_revision(Some(old.revision)));
    assert_eq!(session.snapshot().uid, "second");
    let current = session.snapshot();
    assert!(session.clear_if_revision(Some(current.revision)));
    assert!(!session.snapshot().is_authenticated());
    let revision = session.snapshot().revision;
    session.clear();
    assert_eq!(session.snapshot().revision, revision);
    assert!(!format!("{old:?}").contains("secret"));
}

#[test]
fn late_login_cannot_replace_a_new_account() {
    let session = Session::default();
    let revision = session.snapshot().revision;
    session.set("second".into(), "secret".into());
    assert!(!session.set_if_revision(revision, "first".into(), "old".into()));
    assert_eq!(session.snapshot().uid, "second");
    let revision = session.snapshot().revision;
    assert!(session.set_if_revision(revision, "third".into(), "new".into()));
    assert_eq!(session.snapshot().uid, "third");
}
