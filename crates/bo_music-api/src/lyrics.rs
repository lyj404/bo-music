//! Fetch and decode lyrics from Kuwo's dedicated lyric endpoint.
use base64::{Engine, engine::general_purpose::STANDARD};
use bo_music_domain::{
    lyrics::{self, LyricDocument},
    track::Track,
};
use serde::Deserialize;
use thiserror::Error;

use crate::transport::{Transport, TransportError};

const LYRIC_ENDPOINT: &str = "https://mlyric.kuwo.cn/mobi.s";

#[derive(Debug, Error)]
pub enum LyricError {
    #[error("music ID must be positive")]
    InvalidId,
    #[error("invalid base64 lyric content")]
    InvalidContent,
    #[error("invalid lyric endpoint")]
    InvalidEndpoint,
    #[error(transparent)]
    Transport(#[from] TransportError),
}

#[derive(Deserialize)]
struct ContentDto {
    content: Option<String>,
}

fn request_payload(music_id: i64, lrcx: i32) -> String {
    format!(
        "type=lyric&req=2&lrcx={lrcx}&rid={music_id}&songname=&artist=&corp=kuwo&fromchannel=bodian"
    )
}

fn decode(content: Option<String>) -> Result<String, LyricError> {
    let cleaned: String = content
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let bytes = STANDARD
        .decode(cleaned)
        .map_err(|_| LyricError::InvalidContent)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

async fn raw_from(
    transport: &Transport,
    endpoint: &reqwest::Url,
    music_id: i64,
    lrcx: i32,
    revision: u64,
) -> Result<String, LyricError> {
    if music_id <= 0 {
        return Err(LyricError::InvalidId);
    }
    let payload = STANDARD.encode(request_payload(music_id, lrcx));
    let response = transport
        .send_lyric::<ContentDto>(endpoint.clone(), &payload, revision)
        .await?;
    decode(response.data.and_then(|data| data.content))
}

/// Retrieve the requested format as decoded text. Empty lyrics are a valid result.
pub async fn raw(transport: &Transport, music_id: i64, lrcx: i32) -> Result<String, LyricError> {
    let endpoint = reqwest::Url::parse(LYRIC_ENDPOINT).expect("constant URL");
    raw_from(
        transport,
        &endpoint,
        music_id,
        lrcx,
        transport.session_snapshot().revision,
    )
    .await
}

async fn get_from(
    transport: &Transport,
    endpoint: &reqwest::Url,
    track: &Track,
    revision: u64,
) -> Result<LyricDocument, LyricError> {
    if transport.session_snapshot().revision != revision {
        return Err(TransportError::SessionChanged.into());
    }
    let word = track
        .lyrics
        .as_ref()
        .is_none_or(|availability| availability.word);
    let mut text = raw_from(transport, endpoint, track.id, i32::from(word), revision).await?;
    if word && text.trim().is_empty() {
        text = raw_from(transport, endpoint, track.id, 0, revision).await?;
    }
    let duration = u64::try_from(track.duration_seconds)
        .ok()
        .filter(|seconds| *seconds > 0)
        .map(|seconds| seconds * 1000);
    Ok(lyrics::parse(&text, duration))
}

/// Try word timing first when availability is unknown, and fall back to line lyrics if empty.
pub async fn get(transport: &Transport, track: &Track) -> Result<LyricDocument, LyricError> {
    get_for_revision(transport, track, transport.session_snapshot().revision).await
}

/// Reject a request scheduled for an earlier account before contacting the lyric host.
pub async fn get_for_revision(
    transport: &Transport,
    track: &Track,
    revision: u64,
) -> Result<LyricDocument, LyricError> {
    get_at(transport, track, revision, LYRIC_ENDPOINT).await
}

/// The lyric endpoint is separate from the API origin and never receives session credentials.
/// Transport restricts this to the official host; the test-support feature also permits loopback.
pub async fn get_at(
    transport: &Transport,
    track: &Track,
    revision: u64,
    endpoint: &str,
) -> Result<LyricDocument, LyricError> {
    let endpoint = reqwest::Url::parse(endpoint).map_err(|_| LyricError::InvalidEndpoint)?;
    if !matches!(endpoint.scheme(), "http" | "https")
        || endpoint.host_str().is_none()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(LyricError::InvalidEndpoint);
    }
    get_from(transport, &endpoint, track, revision).await
}

#[cfg(test)]
#[path = "lyrics_tests.rs"]
mod tests;
