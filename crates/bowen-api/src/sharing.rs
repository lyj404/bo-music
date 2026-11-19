//! Local share URLs and the song share-count reporting endpoint.
use crate::transport::{ApiRequest, Transport, TransportError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ShareError {
    #[error("shared object ID must be positive")]
    InvalidId,
    #[error(transparent)]
    Transport(#[from] TransportError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShareTarget {
    Track,
    Artist,
    Album,
    Playlist { source: i32 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShareOutcome {
    Succeeded,
    Unsupported,
}

fn encode_uid(uid: &str) -> String {
    let mut encoded = String::new();
    for byte in uid.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write;
            write!(&mut encoded, "%{byte:02X}").expect("write to String");
        }
    }
    encoded
}

pub fn link(target: ShareTarget, id: i64, uid: &str) -> Result<String, ShareError> {
    if id <= 0 {
        return Err(ShareError::InvalidId);
    }
    let uid = encode_uid(uid);
    let path = match target {
        ShareTarget::Track => format!("playMusic.html?uid={uid}&musicId={id}&opusId="),
        ShareTarget::Artist => format!("singer.html?uid={uid}&singerId={id}"),
        ShareTarget::Album => format!("album.html?uid={uid}&albumid={id}"),
        ShareTarget::Playlist { source } => {
            format!("collection.html?uid={uid}&playlistId={id}&source={source}")
        }
    };
    Ok(format!("https://h5app.kuwo.cn/m/bodian/{path}"))
}

/// This increments the service's share count and is not automatically retried.
pub async fn report_track(
    transport: &Transport,
    music_id: i64,
) -> Result<ShareOutcome, ShareError> {
    report_track_inner(transport, music_id, None).await
}

/// Bind the report to the identity that initiated sharing.
pub async fn report_track_at_revision(
    transport: &Transport,
    music_id: i64,
    revision: u64,
) -> Result<ShareOutcome, ShareError> {
    report_track_inner(transport, music_id, Some(revision)).await
}

async fn report_track_inner(
    transport: &Transport,
    music_id: i64,
    revision: Option<u64>,
) -> Result<ShareOutcome, ShareError> {
    if music_id <= 0 {
        return Err(ShareError::InvalidId);
    }
    let mut request = ApiRequest::get("service/share/text");
    request.signed = true;
    request.query = vec![
        ("shareTo".into(), "5".into()),
        ("shareSource".into(), "0".into()),
        ("sourceId".into(), music_id.to_string()),
        ("playlistType".into(), "4".into()),
    ];
    request.accepted_codes.push(23006);
    let response = transport
        .send_bound::<serde_json::Value>(&request, revision)
        .await?;
    Ok(if response.code == 200 {
        ShareOutcome::Succeeded
    } else {
        ShareOutcome::Unsupported
    })
}

#[cfg(test)]
#[path = "sharing_tests.rs"]
mod tests;
