//! Charts, discovery modules, and AI playlist details.
use crate::{
    playlist::PlaylistDto,
    search::{TrackDto, http_url, number_or_string},
    transport::{ApiRequest, Transport, TransportError},
};
use bowen_domain::{
    discovery::{AiPlaylist, AiPreview, Chart, ChartSection, HomeContent, HomeFeed, HomeModule},
    paging::{PagedCursor, PagedResult},
    track::Track,
};
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("chart ID must be positive")]
    InvalidChartId,
    #[error("AI playlist index must not be negative")]
    InvalidIndex,
    #[error(transparent)]
    Transport(#[from] TransportError),
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ChartSectionDto {
    module_name: Option<String>,
    bang_list: Option<Vec<ChartDto>>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ChartDto {
    #[serde(deserialize_with = "number_or_string")]
    id: i64,
    name: Option<String>,
    pic: Option<String>,
    pub_str: Option<String>,
    musics: Option<Vec<TrackDto>>,
}

pub async fn chart_sections(transport: &Transport) -> Result<Vec<ChartSection>, DiscoveryError> {
    let payload = transport
        .send::<Vec<ChartSectionDto>>(&ApiRequest::get("service/home/bangNew"))
        .await?
        .data
        .unwrap_or_default();
    Ok(payload
        .into_iter()
        .filter_map(|section| {
            let charts: Vec<_> = section
                .bang_list
                .unwrap_or_default()
                .into_iter()
                .filter_map(|chart| {
                    if chart.id <= 0 {
                        return None;
                    }
                    Some(Chart {
                        id: chart.id,
                        name: chart.name.filter(|name| !name.trim().is_empty())?,
                        cover_url: chart.pic.as_deref().and_then(http_url),
                        update_text: chart.pub_str.unwrap_or_default(),
                        preview_tracks: chart
                            .musics
                            .unwrap_or_default()
                            .into_iter()
                            .map(TrackDto::into_track)
                            .collect(),
                    })
                })
                .collect();
            (!charts.is_empty()).then_some(ChartSection {
                title: section.module_name.unwrap_or_default(),
                charts,
            })
        })
        .collect())
}

#[derive(Deserialize)]
struct ChartTracksDto {
    musics: Option<Vec<TrackDto>>,
    total: Option<u64>,
}

pub async fn chart_tracks(
    transport: &Transport,
    chart_id: i64,
    cursor: &mut PagedCursor,
) -> Result<PagedResult<Track>, DiscoveryError> {
    if chart_id <= 0 {
        return Err(DiscoveryError::InvalidChartId);
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
    let mut request = ApiRequest::get(format!("service/bang/{chart_id}/musics"));
    request.query = vec![
        ("pn".into(), cursor.page_number().to_string()),
        ("rn".into(), page_size.to_string()),
    ];
    let payload = transport.send::<ChartTracksDto>(&request).await?.data;
    let (items, total): (Vec<_>, _) = match payload {
        Some(data) => (
            data.musics
                .unwrap_or_default()
                .into_iter()
                .map(TrackDto::into_track)
                .collect(),
            data.total,
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndexDto {
    module_list: Option<Vec<ModuleDto>>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ModuleDto {
    id: i32,
    #[serde(rename = "type")]
    kind: i32,
    name: Option<String>,
    delay_ms: i32,
}

pub async fn home_modules(transport: &Transport) -> Result<Vec<HomeModule>, DiscoveryError> {
    home_modules_inner(transport, None).await
}

pub async fn home_modules_at_revision(
    transport: &Transport,
    revision: u64,
) -> Result<Vec<HomeModule>, DiscoveryError> {
    home_modules_inner(transport, Some(revision)).await
}

async fn home_modules_inner(
    transport: &Transport,
    revision: Option<u64>,
) -> Result<Vec<HomeModule>, DiscoveryError> {
    let response = transport
        .send_bound::<IndexDto>(&ApiRequest::get("service/home/index"), revision)
        .await?;
    Ok(response
        .data
        .and_then(|data| data.module_list)
        .unwrap_or_default()
        .into_iter()
        .map(|item| HomeModule {
            id: item.id,
            kind: item.kind,
            name: item.name.unwrap_or_default(),
            delay_ms: item.delay_ms,
        })
        .collect())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TracksDto {
    module_name: Option<String>,
    music_list: Option<Vec<TrackDto>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GroupsDto {
    module_name: Option<String>,
    song_list: Option<Vec<GroupDto>>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct GroupDto {
    title: Option<String>,
    pass_rec_name: Option<String>,
    songs: Option<Vec<TrackDto>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlaylistsDto {
    module_name: Option<String>,
    song_list: Option<Vec<PlaylistDto>>,
}

pub async fn home_module(
    transport: &Transport,
    module: &HomeModule,
) -> Result<Option<HomeFeed>, DiscoveryError> {
    home_module_inner(transport, module, None).await
}

pub async fn home_module_at_revision(
    transport: &Transport,
    module: &HomeModule,
    revision: u64,
) -> Result<Option<HomeFeed>, DiscoveryError> {
    home_module_inner(transport, module, Some(revision)).await
}

async fn home_module_inner(
    transport: &Transport,
    module: &HomeModule,
    revision: Option<u64>,
) -> Result<Option<HomeFeed>, DiscoveryError> {
    if !module.is_supported() {
        return Ok(None);
    }
    let mut request = ApiRequest::get("service/home/module");
    request
        .query
        .push(("moduleId".into(), module.id.to_string()));
    match module.kind {
        3 | 10 => {
            let Some(data) = transport
                .send_bound::<TracksDto>(&request, revision)
                .await?
                .data
            else {
                return Ok(None);
            };
            let tracks: Vec<_> = data
                .music_list
                .unwrap_or_default()
                .into_iter()
                .map(TrackDto::into_track)
                .collect();
            Ok((!tracks.is_empty()).then_some(HomeFeed {
                title: data.module_name.unwrap_or_default(),
                content: HomeContent::Tracks(tracks),
            }))
        }
        4 | 11 => {
            let Some(data) = transport
                .send_bound::<GroupsDto>(&request, revision)
                .await?
                .data
            else {
                return Ok(None);
            };
            let groups: Vec<_> = data
                .song_list
                .unwrap_or_default()
                .into_iter()
                .enumerate()
                .filter_map(|(index, group)| {
                    let tracks: Vec<_> = group
                        .songs
                        .unwrap_or_default()
                        .into_iter()
                        .map(TrackDto::into_track)
                        .collect();
                    if tracks.is_empty() {
                        return None;
                    }
                    let pass_rec_name = group.pass_rec_name.unwrap_or_default();
                    Some(AiPreview {
                        title: group
                            .title
                            .filter(|title| !title.trim().is_empty())
                            .unwrap_or_else(|| pass_rec_name.clone()),
                        index,
                        pass_rec_name,
                        tracks,
                    })
                })
                .collect();
            Ok((!groups.is_empty()).then_some(HomeFeed {
                title: data.module_name.unwrap_or_default(),
                content: HomeContent::AiPreviews {
                    groups,
                    mosaic: module.kind == 4,
                },
            }))
        }
        5 => {
            let Some(data) = transport
                .send_bound::<PlaylistsDto>(&request, revision)
                .await?
                .data
            else {
                return Ok(None);
            };
            let playlists: Vec<_> = data
                .song_list
                .unwrap_or_default()
                .into_iter()
                .map(PlaylistDto::into_playlist)
                .collect();
            Ok((!playlists.is_empty()).then_some(HomeFeed {
                title: data.module_name.unwrap_or_default(),
                content: HomeContent::Playlists(playlists),
            }))
        }
        _ => unreachable!("checked above"),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AiDto {
    title: Option<String>,
    sub_title: Option<String>,
    big_title: Option<String>,
    music_list: Option<Vec<TrackDto>>,
}

pub async fn ai_playlist(
    transport: &Transport,
    index: i32,
    pass_rec_name: &str,
) -> Result<Option<AiPlaylist>, DiscoveryError> {
    ai_playlist_inner(transport, index, pass_rec_name, None).await
}

pub async fn ai_playlist_at_revision(
    transport: &Transport,
    index: i32,
    pass_rec_name: &str,
    revision: u64,
) -> Result<Option<AiPlaylist>, DiscoveryError> {
    ai_playlist_inner(transport, index, pass_rec_name, Some(revision)).await
}

async fn ai_playlist_inner(
    transport: &Transport,
    index: i32,
    pass_rec_name: &str,
    revision: Option<u64>,
) -> Result<Option<AiPlaylist>, DiscoveryError> {
    if index < 0 {
        return Err(DiscoveryError::InvalidIndex);
    }
    let mut request = ApiRequest::get("service/home/aiPlaylistDetail");
    request.query = vec![
        ("index".into(), index.to_string()),
        ("passRecName".into(), pass_rec_name.into()),
    ];
    Ok(transport
        .send_bound::<AiDto>(&request, revision)
        .await?
        .data
        .map(|data| AiPlaylist {
            title: data.title.unwrap_or_default(),
            subtitle: data.sub_title.unwrap_or_default(),
            big_title: data.big_title.unwrap_or_default(),
            tracks: data
                .music_list
                .unwrap_or_default()
                .into_iter()
                .map(TrackDto::into_track)
                .collect(),
        }))
}

#[cfg(test)]
#[path = "discovery_tests.rs"]
mod tests;
