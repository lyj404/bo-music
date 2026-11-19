//! Collection operations use explicit target states and never toggle implicitly.
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    playlist::PlaylistDto,
    search::{AlbumDto, TrackDto, http_url, number_or_string},
    transport::{ApiRequest, Transport, TransportError},
};
use bowen_domain::{
    catalog::{Album, Artist, Playlist},
    paging::{PagedCursor, PagedResult},
    track::Track,
};

#[derive(Debug, Error)]
pub enum CollectionError {
    #[error("target ID must be positive")]
    InvalidId,
    #[error("sign in to access collections")]
    NotAuthenticated,
    #[error("session user ID is invalid")]
    InvalidUserId,
    #[error(transparent)]
    Transport(#[from] TransportError),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CollectBody<'a> {
    source: i32,
    source_id: [i64; 1],
    op: i32,
    uid: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    token: Option<&'a str>,
}

pub async fn set_playlist(
    transport: &Transport,
    id: i64,
    source: i32,
    collected: bool,
) -> Result<(), CollectionError> {
    write(transport, id, source, collected, false, None).await
}

pub async fn set_album(
    transport: &Transport,
    id: i64,
    collected: bool,
) -> Result<(), CollectionError> {
    write(transport, id, 6, collected, false, None).await
}

pub async fn set_artist(
    transport: &Transport,
    id: i64,
    followed: bool,
) -> Result<(), CollectionError> {
    write(transport, id, 7, followed, true, None).await
}

/// Apply an explicit state only for the identity that initiated the operation.
pub async fn set_at_revision(
    transport: &Transport,
    id: i64,
    source: i32,
    enabled: bool,
    expected_revision: u64,
) -> Result<(), CollectionError> {
    write(
        transport,
        id,
        source,
        enabled,
        source == 7,
        Some(expected_revision),
    )
    .await
}

#[derive(Deserialize)]
struct StatesPayload {
    result: Option<Vec<StateDto>>,
}

#[derive(Deserialize)]
struct StateDto {
    id: i64,
    collect: bool,
}

#[derive(Deserialize)]
struct CollectedPayload<T> {
    #[serde(rename = "playLists")]
    playlists: Option<Vec<T>>,
    total: Option<u64>,
}

/// The endpoint mixes playlists and albums. Advance based on the unfiltered
/// response, so a page containing only the other kind does not end pagination.
async fn collected_page<D: serde::de::DeserializeOwned, T>(
    transport: &Transport,
    cursor: &mut PagedCursor,
    select: impl Fn(D) -> Option<T>,
) -> Result<PagedResult<T>, CollectionError> {
    let session = transport.session_snapshot();
    if !session.is_authenticated() {
        return Err(CollectionError::NotAuthenticated);
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
    let mut request = ApiRequest::get("service/collect/4/list");
    request.query = vec![
        ("pn".into(), cursor.page_number().to_string()),
        ("rn".into(), page_size.to_string()),
        ("userId".into(), session.uid.clone()),
        ("fromUid".into(), session.uid),
    ];
    request.signed = true;
    let payload = transport
        .send_bound::<CollectedPayload<D>>(&request, Some(session.revision))
        .await?
        .data;
    let (raw, total) = match payload {
        Some(payload) => (payload.playlists.unwrap_or_default(), payload.total),
        None => (Vec::new(), None),
    };
    cursor.advance(raw.len());
    Ok(PagedResult {
        items: raw.into_iter().filter_map(select).collect(),
        offset,
        page_size,
        total,
    })
}

pub async fn albums(
    transport: &Transport,
    cursor: &mut PagedCursor,
) -> Result<PagedResult<Album>, CollectionError> {
    collected_page(transport, cursor, |item: AlbumDto| {
        (item.source_type != 4).then(|| item.into_album())
    })
    .await
}

pub async fn playlists(
    transport: &Transport,
    cursor: &mut PagedCursor,
) -> Result<PagedResult<Playlist>, CollectionError> {
    collected_page(transport, cursor, |item: PlaylistDto| {
        (item.source_type == 4).then(|| item.into_playlist())
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PurchasedPayload {
    music_list: Option<Vec<TrackDto>>,
    album_list: Option<Vec<AlbumDto>>,
    size: Option<u64>,
}

async fn purchased_page<T>(
    transport: &Transport,
    cursor: &mut PagedCursor,
    path: &str,
    select: impl FnOnce(PurchasedPayload) -> Vec<T>,
) -> Result<PagedResult<T>, CollectionError> {
    let session = transport.session_snapshot();
    if !session.is_authenticated() {
        return Err(CollectionError::NotAuthenticated);
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
    let mut request = ApiRequest::get(path);
    request.query = vec![
        ("pn".into(), cursor.page_number().to_string()),
        ("rn".into(), page_size.to_string()),
    ];
    request.signed = true;
    let payload = transport
        .send_bound::<PurchasedPayload>(&request, Some(session.revision))
        .await?
        .data;
    let (items, total) = match payload {
        Some(payload) => {
            let total = payload.size;
            (select(payload), total)
        }
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

pub async fn purchased_singles(
    transport: &Transport,
    cursor: &mut PagedCursor,
) -> Result<PagedResult<Track>, CollectionError> {
    purchased_page(
        transport,
        cursor,
        "ucenter/pay/album/music/purchasedList",
        |payload| {
            payload
                .music_list
                .unwrap_or_default()
                .into_iter()
                .map(TrackDto::into_track)
                .collect()
        },
    )
    .await
}

pub async fn purchased_albums(
    transport: &Transport,
    cursor: &mut PagedCursor,
) -> Result<PagedResult<Album>, CollectionError> {
    purchased_page(
        transport,
        cursor,
        "ucenter/pay/album/purchasedList",
        |payload| {
            payload
                .album_list
                .unwrap_or_default()
                .into_iter()
                .map(AlbumDto::into_album)
                .collect()
        },
    )
    .await
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct FollowedArtistDto {
    #[serde(deserialize_with = "number_or_string")]
    id: i64,
    name: Option<String>,
    alias_name: Option<String>,
    pic: Option<String>,
    fans_cnt: i64,
    music_cnt: i32,
    album_cnt: i32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FollowedPayload {
    artist_list: Option<Vec<FollowedArtistDto>>,
}

pub async fn followed_artists(transport: &Transport) -> Result<Vec<Artist>, CollectionError> {
    let session = transport.session_snapshot();
    if !session.is_authenticated() {
        return Err(CollectionError::NotAuthenticated);
    }
    let mut request = ApiRequest::get("service/collect/7/list");
    request.query = vec![
        ("userId".into(), session.uid.clone()),
        ("fromUid".into(), session.uid),
        ("pn".into(), "1".into()),
        ("rn".into(), "400".into()),
    ];
    request.signed = true;
    Ok(transport
        .send_bound::<FollowedPayload>(&request, Some(session.revision))
        .await?
        .data
        .and_then(|payload| payload.artist_list)
        .unwrap_or_default()
        .into_iter()
        .map(|artist| Artist {
            id: artist.id,
            name: artist
                .name
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| "(未命名歌手)".into()),
            cover_url: artist.pic.as_deref().and_then(http_url),
            song_count: artist.music_cnt,
            album_count: artist.album_cnt,
            alias: artist.alias_name.unwrap_or_default(),
            fans_count: artist.fans_cnt,
            description: String::new(),
        })
        .collect())
}

/// None means that the current state is unknown, including for anonymous users.
pub async fn album_state(transport: &Transport, id: i64) -> Result<Option<bool>, CollectionError> {
    if id <= 0 {
        return Err(CollectionError::InvalidId);
    }
    let session = transport.session_snapshot();
    if !session.is_authenticated() {
        return Ok(None);
    }
    let mut request = ApiRequest::get("service/collect/multipleState");
    request.query = vec![
        ("source".into(), "6".into()),
        ("sourceIds".into(), id.to_string()),
    ];
    request.signed = true;
    let response = match transport
        .send_bound::<StatesPayload>(&request, Some(session.revision))
        .await
    {
        Ok(response) => response,
        Err(TransportError::SessionChanged) => {
            return Err(CollectionError::Transport(TransportError::SessionChanged));
        }
        Err(_) => return Ok(None),
    };
    Ok(response
        .data
        .and_then(|payload| payload.result)
        .and_then(|items| items.into_iter().find(|item| item.id == id))
        .map(|item| item.collect))
}

async fn write(
    transport: &Transport,
    id: i64,
    source: i32,
    enabled: bool,
    include_token: bool,
    expected_revision: Option<u64>,
) -> Result<(), CollectionError> {
    if id <= 0 {
        return Err(CollectionError::InvalidId);
    }
    let session = transport.session_snapshot();
    if expected_revision.is_some_and(|revision| revision != session.revision) {
        return Err(CollectionError::Transport(TransportError::SessionChanged));
    }
    if !session.is_authenticated() {
        return Err(CollectionError::NotAuthenticated);
    }
    let uid = session
        .uid
        .parse::<i64>()
        .map_err(|_| CollectionError::InvalidUserId)?;
    let body = serde_json::to_string(&CollectBody {
        source,
        source_id: [id],
        op: if enabled { 1 } else { 2 },
        uid,
        token: include_token.then_some(session.token.as_str()),
    })
    .expect("serializing primitive fields is infallible");
    let mut request = ApiRequest::get("service/collect");
    request.method = reqwest::Method::POST;
    request.signed = true;
    request.json_body = Some(body);
    transport
        .send_bound::<serde_json::Value>(&request, Some(session.revision))
        .await?;
    Ok(())
}

#[cfg(test)]
#[path = "collection_tests.rs"]
mod tests;
