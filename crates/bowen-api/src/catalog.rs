//! Track details and MV metadata; playback authorization remains a separate API.
use bowen_domain::{
    catalog::{Album, Artist},
    paging::{PageOrigin, PagedCursor, PagedResult},
    track::Track,
};
use serde::Deserialize;
use thiserror::Error;

use crate::{
    search::{AlbumDto, CategoryPayload, TrackDto, http_url},
    transport::{ApiRequest, Transport, TransportError},
};

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("music ID must be positive")]
    InvalidMusicId,
    #[error("catalog ID must be positive")]
    InvalidId,
    #[error(transparent)]
    Transport(#[from] TransportError),
}

pub fn service_cursor(page_size: i32) -> PagedCursor {
    PagedCursor::new(PageOrigin::One, page_size)
}

async fn page<D: serde::de::DeserializeOwned, T>(
    transport: &Transport,
    path: String,
    cursor: &mut PagedCursor,
    map: impl Fn(D) -> T,
) -> Result<PagedResult<T>, CatalogError> {
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
    let payload = transport.send::<CategoryPayload<D>>(&request).await?.data;
    let (items, total) = match payload {
        Some(payload) => (
            payload
                .result_list
                .unwrap_or_default()
                .into_iter()
                .map(map)
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

pub async fn artist_tracks(
    transport: &Transport,
    id: i64,
    cursor: &mut PagedCursor,
) -> Result<PagedResult<Track>, CatalogError> {
    if id <= 0 {
        return Err(CatalogError::InvalidId);
    }
    page(
        transport,
        format!("service/artist/music/{id}"),
        cursor,
        TrackDto::into_track,
    )
    .await
}

pub async fn artist_albums(
    transport: &Transport,
    id: i64,
    cursor: &mut PagedCursor,
) -> Result<PagedResult<Album>, CatalogError> {
    if id <= 0 {
        return Err(CatalogError::InvalidId);
    }
    page(
        transport,
        format!("service/artist/album/{id}"),
        cursor,
        AlbumDto::into_album,
    )
    .await
}

pub async fn album_tracks(
    transport: &Transport,
    id: i64,
    cursor: &mut PagedCursor,
) -> Result<PagedResult<Track>, CatalogError> {
    if id <= 0 {
        return Err(CatalogError::InvalidId);
    }
    page(
        transport,
        format!("service/album/music/{id}"),
        cursor,
        TrackDto::into_track,
    )
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AlbumPayload {
    album_info: Option<AlbumDto>,
}

pub async fn album(transport: &Transport, id: i64) -> Result<Option<Album>, CatalogError> {
    if id <= 0 {
        return Err(CatalogError::InvalidId);
    }
    let request = ApiRequest::get(format!("service/album/{id}"));
    Ok(transport
        .send::<AlbumPayload>(&request)
        .await?
        .data
        .and_then(|data| data.album_info)
        .map(AlbumDto::into_album))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArtistPayload {
    artist_info: Option<ArtistInfoDto>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ArtistInfoDto {
    name: Option<String>,
    alias_name: Option<String>,
    pic: Option<String>,
    fans_cnt: i64,
    music_cnt: i32,
    album_cnt: i32,
    desc: Option<String>,
}

pub async fn artist(transport: &Transport, id: i64) -> Result<Option<Artist>, CatalogError> {
    if id <= 0 {
        return Err(CatalogError::InvalidId);
    }
    let request = ApiRequest::get(format!("service/artist/{id}"));
    Ok(transport
        .send::<ArtistPayload>(&request)
        .await?
        .data
        .and_then(|data| data.artist_info)
        .map(|data| Artist {
            id,
            name: data
                .name
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| "(未命名歌手)".into()),
            cover_url: data.pic.as_deref().and_then(http_url),
            song_count: data.music_cnt,
            album_count: data.album_cnt,
            alias: data.alias_name.unwrap_or_default(),
            fans_count: data.fans_cnt,
            description: data.desc.unwrap_or_default(),
        }))
}

pub async fn track(transport: &Transport, id: i64) -> Result<Option<Track>, CatalogError> {
    track_at_revision(transport, id, transport.session_snapshot().revision).await
}

pub async fn track_at_revision(
    transport: &Transport,
    id: i64,
    revision: u64,
) -> Result<Option<Track>, CatalogError> {
    if id <= 0 {
        return Err(CatalogError::InvalidMusicId);
    }
    let mut request = ApiRequest::get("service/music/info");
    request.signed = true;
    request.query.push(("musicId".into(), id.to_string()));
    let response = transport
        .send_bound::<TrackDto>(&request, Some(revision))
        .await?;
    Ok(response.data.map(TrackDto::into_track))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MvQuality {
    Low,
    Standard,
    High,
}

impl MvQuality {
    fn parameter(self) -> &'static str {
        match self {
            Self::Low => "0",
            Self::Standard => "2",
            Self::High => "3",
        }
    }
}

/// URL is signed by the service. Deliberately does not implement Debug.
pub struct MvInfo {
    pub video_url: String,
    pub preview_limit_seconds: Option<u32>,
}

#[derive(Deserialize)]
struct MvPayload {
    mv: Option<MvDto>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MvDto {
    high_url: Option<String>,
    #[serde(default)]
    play_limit_time: i32,
}

pub async fn mv_info(
    transport: &Transport,
    id: i64,
    quality: MvQuality,
) -> Result<Option<MvInfo>, CatalogError> {
    mv_info_at_revision(
        transport,
        id,
        quality,
        transport.session_snapshot().revision,
    )
    .await
}

pub async fn mv_info_at_revision(
    transport: &Transport,
    id: i64,
    quality: MvQuality,
    revision: u64,
) -> Result<Option<MvInfo>, CatalogError> {
    if id <= 0 {
        return Err(CatalogError::InvalidMusicId);
    }
    let mut request = ApiRequest::get("service/mv/info");
    request.query = vec![
        ("musicId".into(), id.to_string()),
        ("wifi".into(), quality.parameter().into()),
    ];
    request.accepted_codes.push(20048);
    let response = transport
        .send_bound::<MvPayload>(&request, Some(revision))
        .await?;
    Ok(response.data.and_then(|payload| payload.mv).and_then(|mv| {
        let video_url = http_url(mv.high_url.as_deref()?)?;
        Some(MvInfo {
            video_url,
            preview_limit_seconds: u32::try_from(mv.play_limit_time)
                .ok()
                .filter(|value| *value > 0),
        })
    }))
}
