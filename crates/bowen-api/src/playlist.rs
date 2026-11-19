//! Playlist operations. Account-scoped requests bind authorization to one session snapshot.
use bowen_domain::{
    catalog::{Playlist, PlaylistCategory},
    paging::{PagedCursor, PagedResult},
    track::Track,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    search::{TrackDto, http_url, number_or_string},
    transport::{ApiRequest, FormFile, Transport, TransportError},
};

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct PlaylistDto {
    #[serde(deserialize_with = "number_or_string")]
    id: i64,
    name: Option<String>,
    pic: Option<String>,
    music_count: i32,
    pub(crate) source_type: i32,
    is_private: i32,
    creator_id: i64,
    creator_name: Option<String>,
    creator_icon: Option<String>,
    description: Option<String>,
    play_num: i64,
    praise: i64,
    #[serde(rename = "collectedCnt")]
    collected_count: i64,
    collect_time: Option<String>,
    categories: Option<Vec<CategoryDto>>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct CategoryDto {
    id: i64,
    name: Option<String>,
}

impl PlaylistDto {
    pub(crate) fn into_playlist(self) -> Playlist {
        Playlist {
            id: self.id,
            name: self
                .name
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| "(未命名歌单)".into()),
            music_count: self.music_count,
            cover_url: self.pic.as_deref().and_then(http_url),
            cover_raw_url: self.pic.unwrap_or_default(),
            source_type: self.source_type,
            is_private: self.is_private != 0,
            creator_id: self.creator_id,
            creator_name: self.creator_name.unwrap_or_default(),
            creator_cover_url: self.creator_icon.as_deref().and_then(http_url),
            description: self.description.unwrap_or_default(),
            play_count: self.play_num,
            collected_count: if self.collected_count > 0 {
                self.collected_count
            } else {
                self.praise
            },
            collect_time: self.collect_time.unwrap_or_default(),
            categories: self
                .categories
                .unwrap_or_default()
                .into_iter()
                .map(|category| PlaylistCategory {
                    id: category.id,
                    name: category.name.unwrap_or_default(),
                })
                .collect(),
        }
    }
}

#[derive(Deserialize)]
struct PlaylistListPayload {
    #[serde(rename = "playLists")]
    playlists: Option<Vec<PlaylistDto>>,
}

pub async fn created(transport: &Transport) -> Result<Vec<Playlist>, PlaylistError> {
    created_inner(transport, None).await
}

pub async fn created_at_revision(
    transport: &Transport,
    revision: u64,
) -> Result<Vec<Playlist>, PlaylistError> {
    created_inner(transport, Some(revision)).await
}

async fn created_inner(
    transport: &Transport,
    expected: Option<u64>,
) -> Result<Vec<Playlist>, PlaylistError> {
    let session = transport.session_snapshot();
    if expected.is_some_and(|revision| revision != session.revision) {
        return Err(TransportError::SessionChanged.into());
    }
    if !session.is_authenticated() {
        return Err(PlaylistError::NotAuthenticated);
    }
    let mut request = ApiRequest::get("service/playlist/userCreate");
    request.query.push(("userId".into(), session.uid));
    request.signed = true;
    let payload = transport
        .send_bound::<PlaylistListPayload>(&request, Some(session.revision))
        .await?
        .data;
    Ok(payload
        .and_then(|payload| payload.playlists)
        .unwrap_or_default()
        .into_iter()
        .map(PlaylistDto::into_playlist)
        .collect())
}

pub async fn liked(transport: &Transport) -> Result<Option<Playlist>, PlaylistError> {
    liked_inner(transport, None).await
}

/// Bind every step of a multi-request operation to the initiating identity.
pub async fn liked_at_revision(
    transport: &Transport,
    revision: u64,
) -> Result<Option<Playlist>, PlaylistError> {
    liked_inner(transport, Some(revision)).await
}

async fn liked_inner(
    transport: &Transport,
    expected: Option<u64>,
) -> Result<Option<Playlist>, PlaylistError> {
    let session = transport.session_snapshot();
    if expected.is_some_and(|revision| revision != session.revision) {
        return Err(TransportError::SessionChanged.into());
    }
    if !session.is_authenticated() {
        return Err(PlaylistError::NotAuthenticated);
    }
    let mut request = ApiRequest::get("service/playlist/fond");
    request.query.push(("userId".into(), session.uid));
    request.signed = true;
    Ok(transport
        .send_bound::<PlaylistDto>(&request, Some(session.revision))
        .await?
        .data
        .filter(|playlist| playlist.id > 0)
        .map(PlaylistDto::into_playlist))
}

pub async fn info(
    transport: &Transport,
    id: i64,
    source: i32,
) -> Result<Option<Playlist>, PlaylistError> {
    info_inner(transport, id, source, None).await
}

pub async fn info_at_revision(
    transport: &Transport,
    id: i64,
    source: i32,
    revision: u64,
) -> Result<Option<Playlist>, PlaylistError> {
    info_inner(transport, id, source, Some(revision)).await
}

async fn info_inner(
    transport: &Transport,
    id: i64,
    source: i32,
    expected: Option<u64>,
) -> Result<Option<Playlist>, PlaylistError> {
    if id <= 0 {
        return Err(PlaylistError::InvalidId);
    }
    let mut request = ApiRequest::get(format!("service/playlist/info/{id}"));
    request.query.push(("source".into(), source.to_string()));
    request.signed = true;
    Ok(transport
        .send_bound::<PlaylistDto>(&request, expected)
        .await?
        .data
        .filter(|playlist| playlist.id > 0)
        .map(PlaylistDto::into_playlist))
}

#[derive(Debug, Error)]
pub enum PlaylistError {
    #[error("playlist ID must be positive")]
    InvalidId,
    #[error("sign in to access an account playlist")]
    NotAuthenticated,
    #[error("track IDs must contain 1 to 100 positive values")]
    InvalidTrackIds,
    #[error("playlist name cannot be blank")]
    BlankName,
    #[error("server did not return a new playlist ID")]
    MissingCreatedId,
    #[error("image bytes, filename, and content type must not be empty")]
    InvalidImage,
    #[error("server did not return a cover URL")]
    MissingCoverUrl,
    #[error(transparent)]
    Transport(#[from] TransportError),
}

pub async fn upload_cover(
    transport: &Transport,
    id: i64,
    bytes: Vec<u8>,
    filename: &str,
    content_type: &str,
) -> Result<String, PlaylistError> {
    upload_cover_inner(transport, id, bytes, filename, content_type, None).await
}

pub async fn upload_cover_at_revision(
    transport: &Transport,
    id: i64,
    bytes: Vec<u8>,
    filename: &str,
    content_type: &str,
    revision: u64,
) -> Result<String, PlaylistError> {
    upload_cover_inner(transport, id, bytes, filename, content_type, Some(revision)).await
}

async fn upload_cover_inner(
    transport: &Transport,
    id: i64,
    bytes: Vec<u8>,
    filename: &str,
    content_type: &str,
    expected: Option<u64>,
) -> Result<String, PlaylistError> {
    if id <= 0 {
        return Err(PlaylistError::InvalidId);
    }
    if bytes.is_empty() || filename.trim().is_empty() || content_type.trim().is_empty() {
        return Err(PlaylistError::InvalidImage);
    }
    let session = transport.session_snapshot();
    if expected.is_some_and(|revision| revision != session.revision) {
        return Err(TransportError::SessionChanged.into());
    }
    if !session.is_authenticated() {
        return Err(PlaylistError::NotAuthenticated);
    }
    let mut request = ApiRequest::get(format!("service/playlist/uploadPic/{id}"));
    request.method = reqwest::Method::POST;
    request.signed = true;
    request.file = Some(FormFile {
        field_name: "file".into(),
        file_name: filename.into(),
        content_type: content_type.into(),
        content: bytes,
    });
    let payload = transport
        .send_bound::<serde_json::Value>(&request, Some(session.revision))
        .await?
        .data;
    let value = payload
        .as_ref()
        .and_then(|value| {
            value.as_str().or_else(|| {
                ["imgUrl", "imgurl", "pic", "url", "cover", "coverUrl"]
                    .iter()
                    .find_map(|key| {
                        value
                            .get(*key)
                            .and_then(serde_json::Value::as_str)
                            .filter(|text| !text.trim().is_empty())
                    })
            })
        })
        .filter(|text| !text.trim().is_empty());
    value
        .map(str::to_owned)
        .ok_or(PlaylistError::MissingCoverUrl)
}

#[derive(Serialize)]
struct CreateBody<'a> {
    name: &'a str,
    private: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DeleteBody {
    playlist_ids: [i64; 1],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateBody<'a> {
    id: i64,
    name: &'a str,
    description: &'a str,
    pic: &'a str,
    category_list: &'a [i32],
}

pub struct PlaylistEdit<'a> {
    pub id: i64,
    pub name: &'a str,
    pub description: &'a str,
    /// Original URL as received from the server, not a normalized image URL.
    pub cover_raw_url: &'a str,
    pub category_ids: &'a [i32],
}

pub async fn create(
    transport: &Transport,
    name: &str,
    private: bool,
) -> Result<i64, PlaylistError> {
    create_inner(transport, name, private, None).await
}

pub async fn create_at_revision(
    transport: &Transport,
    name: &str,
    private: bool,
    revision: u64,
) -> Result<i64, PlaylistError> {
    create_inner(transport, name, private, Some(revision)).await
}

async fn create_inner(
    transport: &Transport,
    name: &str,
    private: bool,
    expected: Option<u64>,
) -> Result<i64, PlaylistError> {
    let name = name.trim();
    if name.is_empty() {
        return Err(PlaylistError::BlankName);
    }
    let session = transport.session_snapshot();
    if expected.is_some_and(|revision| revision != session.revision) {
        return Err(TransportError::SessionChanged.into());
    }
    if !session.is_authenticated() {
        return Err(PlaylistError::NotAuthenticated);
    }
    let body = serde_json::to_string(&CreateBody { name, private })
        .expect("serializing primitive fields is infallible");
    let mut request = ApiRequest::get("service/playlist");
    request.method = reqwest::Method::POST;
    request.signed = true;
    request.json_body = Some(body);
    let response = transport
        .send_bound::<PlaylistDto>(&request, Some(session.revision))
        .await?;
    response
        .data
        .map(|data| data.id)
        .filter(|id| *id > 0)
        .ok_or(PlaylistError::MissingCreatedId)
}

pub async fn delete(transport: &Transport, id: i64) -> Result<(), PlaylistError> {
    delete_inner(transport, id, None).await
}

pub async fn delete_at_revision(
    transport: &Transport,
    id: i64,
    revision: u64,
) -> Result<(), PlaylistError> {
    delete_inner(transport, id, Some(revision)).await
}

async fn delete_inner(
    transport: &Transport,
    id: i64,
    expected: Option<u64>,
) -> Result<(), PlaylistError> {
    if id <= 0 {
        return Err(PlaylistError::InvalidId);
    }
    let session = transport.session_snapshot();
    if expected.is_some_and(|revision| revision != session.revision) {
        return Err(TransportError::SessionChanged.into());
    }
    if !session.is_authenticated() {
        return Err(PlaylistError::NotAuthenticated);
    }
    let body = serde_json::to_string(&DeleteBody { playlist_ids: [id] })
        .expect("serializing primitive fields is infallible");
    let mut request = ApiRequest::get("service/playlist");
    request.method = reqwest::Method::DELETE;
    request.signed = true;
    request.json_body = Some(body);
    transport
        .send_bound::<serde_json::Value>(&request, Some(session.revision))
        .await?;
    Ok(())
}

pub async fn update(transport: &Transport, edit: PlaylistEdit<'_>) -> Result<(), PlaylistError> {
    update_inner(transport, edit, None).await
}

pub async fn update_at_revision(
    transport: &Transport,
    edit: PlaylistEdit<'_>,
    revision: u64,
) -> Result<(), PlaylistError> {
    update_inner(transport, edit, Some(revision)).await
}

async fn update_inner(
    transport: &Transport,
    edit: PlaylistEdit<'_>,
    expected: Option<u64>,
) -> Result<(), PlaylistError> {
    if edit.id <= 0 {
        return Err(PlaylistError::InvalidId);
    }
    let name = edit.name.trim();
    if name.is_empty() {
        return Err(PlaylistError::BlankName);
    }
    let session = transport.session_snapshot();
    if expected.is_some_and(|revision| revision != session.revision) {
        return Err(TransportError::SessionChanged.into());
    }
    if !session.is_authenticated() {
        return Err(PlaylistError::NotAuthenticated);
    }
    let body = serde_json::to_string(&UpdateBody {
        id: edit.id,
        name,
        description: edit.description,
        pic: edit.cover_raw_url,
        category_list: edit.category_ids,
    })
    .expect("serializing primitive fields is infallible");
    let mut request = ApiRequest::get("service/playlist");
    request.method = reqwest::Method::PUT;
    request.signed = true;
    request.json_body = Some(body);
    transport
        .send_bound::<serde_json::Value>(&request, Some(session.revision))
        .await?;
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlaylistMusicBody<'a> {
    play_list_id: i64,
    music_id_list: &'a [i64],
}

pub async fn add_tracks(
    transport: &Transport,
    id: i64,
    track_ids: &[i64],
) -> Result<(), PlaylistError> {
    write_tracks(transport, id, track_ids, "service/playlist/music", None).await
}

pub async fn remove_tracks(
    transport: &Transport,
    id: i64,
    track_ids: &[i64],
) -> Result<(), PlaylistError> {
    write_tracks(
        transport,
        id,
        track_ids,
        "service/playlist/music/delete",
        None,
    )
    .await
}

/// Write one or more tracks only for the specified session revision.
pub async fn set_tracks_at_revision(
    transport: &Transport,
    id: i64,
    track_ids: &[i64],
    add: bool,
    revision: u64,
) -> Result<(), PlaylistError> {
    let path = if add {
        "service/playlist/music"
    } else {
        "service/playlist/music/delete"
    };
    write_tracks(transport, id, track_ids, path, Some(revision)).await
}

async fn write_tracks(
    transport: &Transport,
    id: i64,
    track_ids: &[i64],
    path: &str,
    expected: Option<u64>,
) -> Result<(), PlaylistError> {
    if id <= 0 {
        return Err(PlaylistError::InvalidId);
    }
    if track_ids.is_empty() || track_ids.len() > 100 || track_ids.iter().any(|id| *id <= 0) {
        return Err(PlaylistError::InvalidTrackIds);
    }
    let session = transport.session_snapshot();
    if expected.is_some_and(|revision| revision != session.revision) {
        return Err(TransportError::SessionChanged.into());
    }
    if !session.is_authenticated() {
        return Err(PlaylistError::NotAuthenticated);
    }
    let body = serde_json::to_string(&PlaylistMusicBody {
        play_list_id: id,
        music_id_list: track_ids,
    })
    .expect("serializing primitive fields is infallible");
    let mut request = ApiRequest::get(path);
    request.method = reqwest::Method::POST;
    request.json_body = Some(body);
    request.signed = true;
    transport
        .send_bound::<serde_json::Value>(&request, Some(session.revision))
        .await?;
    Ok(())
}

#[derive(Deserialize)]
struct TracksPayload {
    list: Option<Vec<TrackDto>>,
    total: Option<u64>,
}

pub async fn tracks(
    transport: &Transport,
    id: i64,
    source: i32,
    cursor: &mut PagedCursor,
) -> Result<PagedResult<Track>, PlaylistError> {
    tracks_inner(transport, id, source, cursor, None).await
}

pub async fn tracks_at_revision(
    transport: &Transport,
    id: i64,
    source: i32,
    cursor: &mut PagedCursor,
    revision: u64,
) -> Result<PagedResult<Track>, PlaylistError> {
    tracks_inner(transport, id, source, cursor, Some(revision)).await
}

async fn tracks_inner(
    transport: &Transport,
    id: i64,
    source: i32,
    cursor: &mut PagedCursor,
    expected: Option<u64>,
) -> Result<PagedResult<Track>, PlaylistError> {
    if id <= 0 {
        return Err(PlaylistError::InvalidId);
    }
    let session = transport.session_snapshot();
    if expected.is_some_and(|revision| revision != session.revision) {
        return Err(TransportError::SessionChanged.into());
    }
    if source == 5 && !session.is_authenticated() {
        return Err(PlaylistError::NotAuthenticated);
    }
    let offset = cursor.offset();
    let page_size = cursor.page_size();
    if cursor.exhausted() {
        return Ok(PagedResult {
            items: Vec::new(),
            offset,
            page_size,
            total: None,
        });
    }
    let mut request = ApiRequest::get(format!("service/playlist/{id}/musicList"));
    request.query = vec![
        ("pn".into(), cursor.page_number().to_string()),
        ("rn".into(), page_size.to_string()),
        ("source".into(), source.to_string()),
    ];
    request.signed = true;
    let payload = transport
        .send_bound::<TracksPayload>(&request, Some(session.revision))
        .await?
        .data;
    let (items, total) = match payload {
        Some(payload) => (
            payload
                .list
                .unwrap_or_default()
                .into_iter()
                .map(TrackDto::into_track)
                .collect::<Vec<_>>(),
            payload.total,
        ),
        None => (Vec::new(), None),
    };
    cursor.advance(items.len());
    Ok(PagedResult {
        items,
        offset,
        page_size,
        total,
    })
}

#[cfg(test)]
#[path = "playlist_tests.rs"]
mod tests;
