use super::*;
use bo_music_api::{session::Session, transport::TransportOptions};
use std::{
    io::{Read, Write},
    net::TcpListener,
    time::Duration,
};

fn track(id: i64) -> Track {
    Track {
        id,
        title: id.to_string(),
        artist_text: String::new(),
        artists: vec![],
        album_name: None,
        album_id: 0,
        cover_url: None,
        duration_seconds: 30,
        available_qualities: vec![],
        audio_variants: vec![],
        requires_vip: false,
        requires_purchase: false,
        lyrics: None,
        has_mv: false,
        comment_count: None,
        favorite_count: None,
        share_count: None,
    }
}

async fn finish_next(
    browser: &mut Browser,
    rx: &mut mpsc::UnboundedReceiver<Command>,
    session: &Session,
) {
    let command = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    let Command::BrowseDone(generation, revision, result) = command else {
        panic!("unexpected event")
    };
    assert!(browser.finish(generation, revision, session.snapshot().revision, result));
}

#[path = "browse_tests_1.rs"]
mod cases_1;
#[path = "browse_tests_2.rs"]
mod cases_2;
