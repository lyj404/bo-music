use super::*;
use crate::{session::Session, transport::TransportOptions};

#[tokio::test]
async fn navigation_and_albums_reject_an_old_account_before_network() {
    let session = Session::default();
    let old = session.snapshot().revision;
    session.set("42".into(), "synthetic".into());
    let transport = Transport::new(
        TransportOptions {
            base_url: "http://127.0.0.1:9/api/".into(),
            ..Default::default()
        },
        session,
        "device".into(),
    )
    .unwrap();
    let mut cursor = crate::catalog::service_cursor(30);
    assert!(matches!(
        navigation_at_revision(&transport, old).await,
        Err(LibraryError::Transport(TransportError::SessionChanged))
    ));
    assert!(matches!(
        albums_at_revision(
            &transport,
            "2005",
            "8101",
            LibrarySort::Curated,
            &mut cursor,
            old
        )
        .await,
        Err(LibraryError::Transport(TransportError::SessionChanged))
    ));
    assert_eq!(cursor.offset(), 0);
}
