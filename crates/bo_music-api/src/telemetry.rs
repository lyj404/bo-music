//! Explicit playback-history and statistics writes. Callers decide when to report.
use crate::transport::{ApiRequest, Transport, TransportError};
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum TelemetryError {
    #[error("invalid track, collection, or duration")]
    InvalidParameter,
    #[error("sign in before reporting playback")]
    NotAuthenticated,
    #[error(transparent)]
    Transport(#[from] TransportError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollectionKind {
    Playlist,
    Album,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlayOperation {
    Complete,
    Switch,
    InProgress,
}

impl PlayOperation {
    fn value(self) -> u8 {
        match self {
            Self::Complete => 0,
            Self::Switch => 1,
            Self::InProgress => 5,
        }
    }
    pub fn for_position(duration_seconds: u32, played_seconds: u32) -> Option<Self> {
        if duration_seconds == 0 {
            return None;
        }
        let played = u64::from(played_seconds.min(duration_seconds));
        let duration = u64::from(duration_seconds);
        Some(if played * 10 < duration * 3 {
            Self::Switch
        } else if played * 10 <= duration * 9 {
            Self::InProgress
        } else {
            Self::Complete
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryBody {
    source_type: u8,
    items: [HistoryItem; 1],
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryItem {
    source_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    playlist_type: Option<i32>,
}

async fn post<T: Serialize>(
    transport: &Transport,
    path: &str,
    body: &T,
    expected_revision: Option<u64>,
) -> Result<(), TelemetryError> {
    let session = transport.session_snapshot();
    if expected_revision.is_some_and(|revision| revision != session.revision) {
        return Err(TransportError::SessionChanged.into());
    }
    if !session.is_authenticated() {
        return Err(TelemetryError::NotAuthenticated);
    }
    let mut request = ApiRequest::get(path);
    request.method = reqwest::Method::POST;
    request.signed = true;
    request.json_body = Some(serde_json::to_string(body).expect("serializing primitive fields"));
    transport
        .send_bound::<serde_json::Value>(&request, Some(session.revision))
        .await?;
    Ok(())
}

pub async fn song_history(transport: &Transport, music_id: i64) -> Result<(), TelemetryError> {
    song_history_for_revision(transport, music_id, None).await
}

pub async fn song_history_for_revision(
    transport: &Transport,
    music_id: i64,
    expected_revision: Option<u64>,
) -> Result<(), TelemetryError> {
    if music_id <= 0 {
        return Err(TelemetryError::InvalidParameter);
    }
    post(
        transport,
        "playlist/history/add",
        &HistoryBody {
            source_type: 1,
            items: [HistoryItem {
                source_id: music_id,
                playlist_type: None,
            }],
        },
        expected_revision,
    )
    .await
}

pub async fn collection_history(
    transport: &Transport,
    kind: CollectionKind,
    source_id: i64,
    playlist_type: i32,
) -> Result<(), TelemetryError> {
    collection_history_for_revision(transport, kind, source_id, playlist_type, None).await
}

pub async fn collection_history_for_revision(
    transport: &Transport,
    kind: CollectionKind,
    source_id: i64,
    playlist_type: i32,
    expected_revision: Option<u64>,
) -> Result<(), TelemetryError> {
    if source_id <= 0 || playlist_type <= 0 {
        return Err(TelemetryError::InvalidParameter);
    }
    post(
        transport,
        "playlist/history/add",
        &HistoryBody {
            source_type: match kind {
                CollectionKind::Playlist => 2,
                CollectionKind::Album => 3,
            },
            items: [HistoryItem {
                source_id,
                playlist_type: Some(playlist_type),
            }],
        },
        expected_revision,
    )
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordBody {
    music_list: [RecordItem; 1],
    operate: u8,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordItem {
    id: i64,
    src: bool,
    current_dur: i32,
    current_spt: i32,
    p_src: &'static str,
}

pub async fn play_record(
    transport: &Transport,
    music_id: i64,
    duration_seconds: i32,
    played_seconds: i32,
    operation: PlayOperation,
) -> Result<(), TelemetryError> {
    play_record_for_revision(
        transport,
        music_id,
        duration_seconds,
        played_seconds,
        operation,
        None,
    )
    .await
}

pub async fn play_record_for_revision(
    transport: &Transport,
    music_id: i64,
    duration_seconds: i32,
    played_seconds: i32,
    operation: PlayOperation,
    expected_revision: Option<u64>,
) -> Result<(), TelemetryError> {
    if music_id <= 0 || duration_seconds <= 0 {
        return Err(TelemetryError::InvalidParameter);
    }
    let body = RecordBody {
        music_list: [RecordItem {
            id: music_id,
            src: false,
            current_dur: duration_seconds,
            current_spt: played_seconds.clamp(0, duration_seconds),
            p_src: "",
        }],
        operate: operation.value(),
    };
    post(transport, "service/music/record", &body, expected_revision).await
}

#[cfg(test)]
#[path = "telemetry_tests.rs"]
mod tests;
