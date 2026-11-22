use super::*;
use crate::{session::Session, transport::TransportOptions};

#[tokio::test]
async fn discovery_requests_reject_a_stale_account_before_network() {
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
    let module = HomeModule {
        id: 1,
        kind: 3,
        name: String::new(),
        delay_ms: 0,
    };
    assert!(matches!(
        home_modules_at_revision(&transport, old).await,
        Err(DiscoveryError::Transport(TransportError::SessionChanged))
    ));
    assert!(matches!(
        home_module_at_revision(&transport, &module, old).await,
        Err(DiscoveryError::Transport(TransportError::SessionChanged))
    ));
    assert!(matches!(
        ai_playlist_at_revision(&transport, 0, "", old).await,
        Err(DiscoveryError::Transport(TransportError::SessionChanged))
    ));
}
