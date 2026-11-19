//! Music-library navigation and album lists.
use crate::{
    playlist::PlaylistDto,
    search::{AlbumDto, http_url, number_or_string},
    transport::{ApiRequest, Transport, TransportError},
};
use bowen_domain::{
    catalog::{Album, CategoryGroup, LibraryChild, LibraryGroup, Playlist, PlaylistCategory},
    paging::{PagedCursor, PagedResult},
};
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LibraryError {
    #[error("category IDs cannot be blank")]
    BlankCategory,
    #[error("category ID must be positive")]
    InvalidCategory,
    #[error(transparent)]
    Transport(#[from] TransportError),
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct GroupDto {
    id: Option<String>,
    #[serde(rename = "ptypeName")]
    ptype_name: Option<String>,
    internal_title: Option<String>,
    external_title: Option<String>,
    long_desc: Option<String>,
    cover_pic: Option<String>,
    album_total: i32,
    child_list: Option<Vec<ChildDto>>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ChildDto {
    id: Option<String>,
    name: Option<String>,
    long_desc_title: Option<String>,
    long_desc: Option<String>,
    album_vo: Option<AlbumDto>,
}

pub async fn navigation(transport: &Transport) -> Result<Vec<LibraryGroup>, LibraryError> {
    navigation_inner(transport, None).await
}

pub async fn navigation_at_revision(
    transport: &Transport,
    revision: u64,
) -> Result<Vec<LibraryGroup>, LibraryError> {
    navigation_inner(transport, Some(revision)).await
}

async fn navigation_inner(
    transport: &Transport,
    revision: Option<u64>,
) -> Result<Vec<LibraryGroup>, LibraryError> {
    let data = transport
        .send_bound::<Vec<GroupDto>>(&ApiRequest::get("play/music/library/navigation"), revision)
        .await?
        .data
        .unwrap_or_default();
    Ok(data
        .into_iter()
        .filter_map(|group| {
            let name = group.ptype_name.filter(|name| !name.trim().is_empty())?;
            let title = group
                .external_title
                .or(group.internal_title)
                .unwrap_or_default()
                .replace("\r\n", " ")
                .replace(['\r', '\n'], " ")
                .trim()
                .to_owned();
            Some(LibraryGroup {
                id: group.id.unwrap_or_default(),
                name,
                title,
                description: group.long_desc.unwrap_or_default(),
                cover_url: group.cover_pic.as_deref().and_then(http_url),
                album_count: group.album_total,
                children: group
                    .child_list
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|child| {
                        Some(LibraryChild {
                            id: child.id.unwrap_or_default(),
                            name: child.name.filter(|name| !name.trim().is_empty())?,
                            title: child.long_desc_title.unwrap_or_default(),
                            description: child.long_desc.unwrap_or_default(),
                            sample_album: child.album_vo.map(AlbumDto::into_album),
                        })
                    })
                    .collect(),
            })
        })
        .collect())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LibrarySort {
    Curated,
    Newest,
}

#[derive(Deserialize)]
struct AlbumsPayload {
    list: Option<Vec<AlbumDto>>,
    total: Option<u64>,
}

#[derive(Deserialize)]
struct CategoriesPayload {
    categories: Option<Vec<CategoryGroupDto>>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct CategoryGroupDto {
    name: Option<String>,
    sub_categories: Option<Vec<CategoryDto>>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct CategoryDto {
    #[serde(deserialize_with = "number_or_string")]
    id: i64,
    name: Option<String>,
}

pub async fn categories(transport: &Transport) -> Result<Vec<CategoryGroup>, LibraryError> {
    categories_at_revision(transport, transport.session().snapshot().revision).await
}

pub async fn categories_at_revision(
    transport: &Transport,
    revision: u64,
) -> Result<Vec<CategoryGroup>, LibraryError> {
    let response = transport
        .send_bound::<CategoriesPayload>(&ApiRequest::get("service/category/list"), Some(revision))
        .await?;
    Ok(response
        .data
        .and_then(|data| data.categories)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|group| {
            let name = group.name.filter(|name| !name.trim().is_empty())?;
            let categories: Vec<_> = group
                .sub_categories
                .unwrap_or_default()
                .into_iter()
                .filter_map(|item| {
                    if item.id <= 0 {
                        return None;
                    }
                    Some(PlaylistCategory {
                        id: item.id,
                        name: item.name.filter(|name| !name.trim().is_empty())?,
                    })
                })
                .collect();
            (!categories.is_empty()).then_some(CategoryGroup { name, categories })
        })
        .collect())
}

#[derive(Deserialize)]
struct CategoryPlaylistsPayload {
    #[serde(rename = "playLists")]
    playlists: Option<Vec<PlaylistDto>>,
    total: Option<u64>,
}

pub async fn category_playlists(
    transport: &Transport,
    category_id: i64,
    cursor: &mut PagedCursor,
) -> Result<PagedResult<Playlist>, LibraryError> {
    category_playlists_at_revision(
        transport,
        category_id,
        cursor,
        transport.session().snapshot().revision,
    )
    .await
}

pub async fn category_playlists_at_revision(
    transport: &Transport,
    category_id: i64,
    cursor: &mut PagedCursor,
    revision: u64,
) -> Result<PagedResult<Playlist>, LibraryError> {
    if category_id <= 0 {
        return Err(LibraryError::InvalidCategory);
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
    let mut request = ApiRequest::get(format!("service/category/{category_id}/playlist"));
    request.query = vec![
        ("pn".into(), cursor.page_number().to_string()),
        ("rn".into(), page_size.to_string()),
    ];
    let payload = transport
        .send_bound::<CategoryPlaylistsPayload>(&request, Some(revision))
        .await?
        .data;
    let (items, total): (Vec<_>, _) = match payload {
        Some(payload) => (
            payload
                .playlists
                .unwrap_or_default()
                .into_iter()
                .map(PlaylistDto::into_playlist)
                .collect(),
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

pub async fn albums(
    transport: &Transport,
    parent_id: &str,
    child_id: &str,
    sort: LibrarySort,
    cursor: &mut PagedCursor,
) -> Result<PagedResult<Album>, LibraryError> {
    albums_inner(transport, parent_id, child_id, sort, cursor, None).await
}

pub async fn albums_at_revision(
    transport: &Transport,
    parent_id: &str,
    child_id: &str,
    sort: LibrarySort,
    cursor: &mut PagedCursor,
    revision: u64,
) -> Result<PagedResult<Album>, LibraryError> {
    albums_inner(transport, parent_id, child_id, sort, cursor, Some(revision)).await
}

async fn albums_inner(
    transport: &Transport,
    parent_id: &str,
    child_id: &str,
    sort: LibrarySort,
    cursor: &mut PagedCursor,
    revision: Option<u64>,
) -> Result<PagedResult<Album>, LibraryError> {
    if parent_id.trim().is_empty() || child_id.trim().is_empty() {
        return Err(LibraryError::BlankCategory);
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
    let mut request = ApiRequest::get("play/music/library/albums");
    request.query = vec![
        ("pn".into(), cursor.page_number().to_string()),
        ("rn".into(), page_size.to_string()),
        ("pTypeId".into(), parent_id.into()),
        ("cTypeId".into(), child_id.into()),
        (
            "sort".into(),
            match sort {
                LibrarySort::Curated => "1",
                LibrarySort::Newest => "2",
            }
            .into(),
        ),
    ];
    let payload = transport
        .send_bound::<AlbumsPayload>(&request, revision)
        .await?
        .data;
    let (items, total): (Vec<_>, _) = match payload {
        Some(payload) => (
            payload
                .list
                .unwrap_or_default()
                .into_iter()
                .map(AlbumDto::into_album)
                .collect(),
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
#[path = "library_tests.rs"]
mod tests;
