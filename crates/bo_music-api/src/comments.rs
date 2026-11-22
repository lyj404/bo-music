//! Song comments and replies use the service's Android v3 protocol.
use crate::{
    search::{http_url, number_or_string},
    transport::{ApiRequest, Transport, TransportError},
};
use bo_music_domain::comments::{CommentPage, CommentSort, SongComment};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CommentError {
    #[error("music ID must fit the comment service's positive 32-bit range")]
    InvalidMusicId,
    #[error("invalid comment, parent, reply, or page ID")]
    InvalidParameter,
    #[error("comment text cannot be blank")]
    BlankContent,
    #[error("sign in to publish or like comments")]
    NotAuthenticated,
    #[error("session user ID is invalid")]
    InvalidUserId,
    #[error("comment response is missing data")]
    MissingData,
    #[error(transparent)]
    Transport(#[from] TransportError),
}

fn validate_music_id(id: i64) -> Result<(), CommentError> {
    if id <= 0 || id > i64::from(i32::MAX) {
        Err(CommentError::InvalidMusicId)
    } else {
        Ok(())
    }
}

pub(crate) fn optional_number<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<i64>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(d)?;
    match value {
        None => Ok(None),
        Some(serde_json::Value::Number(n)) => n
            .as_i64()
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom("invalid integer")),
        Some(serde_json::Value::String(s)) => s.parse().map(Some).map_err(serde::de::Error::custom),
        _ => Err(serde::de::Error::custom("invalid integer")),
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct Payload {
    #[serde(rename = "comment_list")]
    comments: Option<Vec<CommentDto>>,
    #[serde(rename = "comment_total", deserialize_with = "number_or_string")]
    total: i64,
    more: bool,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct CommentDto {
    #[serde(deserialize_with = "number_or_string")]
    id: i64,
    #[serde(rename = "uid", deserialize_with = "number_or_string")]
    user_id: i64,
    #[serde(deserialize_with = "optional_number")]
    parent_id: Option<i64>,
    #[serde(deserialize_with = "optional_number")]
    reply_id: Option<i64>,
    user_info: Option<UserDto>,
    reply_user_info: Option<UserDto>,
    nickname: Option<String>,
    head_img: Option<String>,
    content: Option<String>,
    img_url: Option<String>,
    create_time: Option<String>,
    publish_time: Option<String>,
    ip_city: Option<String>,
    ip_province: Option<String>,
    #[serde(deserialize_with = "number_or_string")]
    like_count: i64,
    #[serde(deserialize_with = "number_or_string")]
    reply_count: i64,
    #[serde(deserialize_with = "number_or_string")]
    like: i64,
    #[serde(deserialize_with = "number_or_string")]
    anonymous: i64,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct UserDto {
    nickname: Option<String>,
    head_img: Option<String>,
    ip_city: Option<String>,
}

fn first(values: &[Option<&str>]) -> String {
    values
        .iter()
        .flatten()
        .find(|value| !value.trim().is_empty())
        .copied()
        .unwrap_or_default()
        .into()
}

fn map_page(payload: Payload) -> CommentPage {
    let items = payload
        .comments
        .unwrap_or_default()
        .into_iter()
        .filter(|item| item.id > 0)
        .map(|item| {
            let info = item.user_info.as_ref();
            let anonymous = item.anonymous != 0;
            let avatar = first(&[
                item.head_img.as_deref(),
                info.and_then(|user| user.head_img.as_deref()),
            ]);
            SongComment {
                id: item.id,
                user_id: item.user_id,
                parent_id: item.parent_id.unwrap_or(0),
                reply_id: item.reply_id.unwrap_or(0),
                reply_nickname: item
                    .reply_user_info
                    .and_then(|user| user.nickname)
                    .unwrap_or_default(),
                nickname: if anonymous {
                    "匿名听友".into()
                } else {
                    first(&[
                        item.nickname.as_deref(),
                        info.and_then(|user| user.nickname.as_deref()),
                        Some("听友"),
                    ])
                },
                avatar_url: if anonymous { None } else { http_url(&avatar) },
                content: item.content.unwrap_or_default(),
                image_url: item.img_url.as_deref().and_then(http_url),
                publish_time: first(&[item.publish_time.as_deref(), item.create_time.as_deref()]),
                location: first(&[
                    item.ip_city.as_deref(),
                    item.ip_province.as_deref(),
                    info.and_then(|user| user.ip_city.as_deref()),
                ]),
                like_count: item.like_count.max(0),
                reply_count: item.reply_count.max(0),
                is_liked: item.like != 0,
            }
        })
        .collect();
    CommentPage {
        items,
        total_count: payload.total.max(0),
        has_more: payload.more,
    }
}

async fn read(
    transport: &Transport,
    music_id: i64,
    parent_id: Option<i64>,
    page: i32,
    path: &str,
    expected_revision: Option<u64>,
) -> Result<CommentPage, CommentError> {
    validate_music_id(music_id)?;
    if page < 1 || parent_id.is_some_and(|id| id <= 0) {
        return Err(CommentError::InvalidParameter);
    }
    let mut request = ApiRequest::get(path);
    request.platform = Some("android".into());
    request.query = vec![
        ("moduleType".into(), "2".into()),
        ("moduleId".into(), music_id.to_string()),
    ];
    if let Some(parent_id) = parent_id {
        request
            .query
            .push(("parentId".into(), parent_id.to_string()));
    }
    request
        .query
        .extend([("pn".into(), page.to_string()), ("rn".into(), "30".into())]);
    let data = transport
        .send_bound::<Payload>(&request, expected_revision)
        .await?
        .data
        .ok_or(CommentError::MissingData)?;
    Ok(map_page(data))
}

pub async fn list(
    transport: &Transport,
    music_id: i64,
    sort: CommentSort,
    page: i32,
) -> Result<CommentPage, CommentError> {
    read(
        transport,
        music_id,
        None,
        page,
        match sort {
            CommentSort::Recommended => "comments/v3/hot",
            CommentSort::Latest => "comments/v3/new",
        },
        None,
    )
    .await
}

pub async fn replies(
    transport: &Transport,
    music_id: i64,
    parent_id: i64,
    page: i32,
) -> Result<CommentPage, CommentError> {
    read(
        transport,
        music_id,
        Some(parent_id),
        page,
        "comments/v3/replies",
        None,
    )
    .await
}

pub async fn list_at_revision(
    transport: &Transport,
    music_id: i64,
    sort: CommentSort,
    page: i32,
    revision: u64,
) -> Result<CommentPage, CommentError> {
    let path = match sort {
        CommentSort::Recommended => "comments/v3/hot",
        CommentSort::Latest => "comments/v3/new",
    };
    read(transport, music_id, None, page, path, Some(revision)).await
}

pub async fn replies_at_revision(
    transport: &Transport,
    music_id: i64,
    parent_id: i64,
    page: i32,
    revision: u64,
) -> Result<CommentPage, CommentError> {
    read(
        transport,
        music_id,
        Some(parent_id),
        page,
        "comments/v3/replies",
        Some(revision),
    )
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PublishBody<'a> {
    module_id: i64,
    module_type: u8,
    uid: i64,
    content: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_id: Option<i64>,
    anonymous: u8,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LikeBody {
    module_id: i64,
    module_type: u8,
    uid: i64,
    comment_id: i64,
    op: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_id: Option<i64>,
}

fn published_id(value: &serde_json::Value) -> Option<i64> {
    match value {
        serde_json::Value::Number(n) => n.as_i64(),
        serde_json::Value::String(s) => s.parse().ok(),
        serde_json::Value::Object(o) => o
            .get("id")
            .and_then(published_id)
            .or_else(|| o.get("commentId").and_then(published_id)),
        _ => None,
    }
    .filter(|id| *id > 0)
}

pub async fn publish(
    transport: &Transport,
    music_id: i64,
    content: &str,
    parent_id: i64,
    reply_id: i64,
    anonymous: bool,
) -> Result<Option<i64>, CommentError> {
    publish_inner(
        transport, music_id, content, parent_id, reply_id, anonymous, None,
    )
    .await
}

pub struct PublishComment<'a> {
    pub music_id: i64,
    pub content: &'a str,
    pub parent_id: i64,
    pub reply_id: i64,
    pub anonymous: bool,
}

pub async fn publish_at_revision(
    transport: &Transport,
    comment: PublishComment<'_>,
    revision: u64,
) -> Result<Option<i64>, CommentError> {
    publish_inner(
        transport,
        comment.music_id,
        comment.content,
        comment.parent_id,
        comment.reply_id,
        comment.anonymous,
        Some(revision),
    )
    .await
}

async fn publish_inner(
    transport: &Transport,
    music_id: i64,
    content: &str,
    parent_id: i64,
    reply_id: i64,
    anonymous: bool,
    expected: Option<u64>,
) -> Result<Option<i64>, CommentError> {
    validate_music_id(music_id)?;
    if content.trim().is_empty() {
        return Err(CommentError::BlankContent);
    }
    if parent_id < 0 || reply_id < 0 || (reply_id > 0 && parent_id == 0) {
        return Err(CommentError::InvalidParameter);
    }
    let session = transport.session_snapshot();
    if expected.is_some_and(|revision| revision != session.revision) {
        return Err(TransportError::SessionChanged.into());
    }
    if !session.is_authenticated() {
        return Err(CommentError::NotAuthenticated);
    }
    let uid = session
        .uid
        .parse()
        .map_err(|_| CommentError::InvalidUserId)?;
    let body = PublishBody {
        module_id: music_id,
        module_type: 2,
        uid,
        content: content.trim(),
        parent_id: (parent_id > 0).then_some(parent_id),
        reply_id: (reply_id > 0).then_some(reply_id),
        anonymous: u8::from(anonymous),
    };
    let mut request = ApiRequest::get("comments/v3/publish");
    request.method = reqwest::Method::POST;
    request.platform = Some("android".into());
    request.json_body = Some(serde_json::to_string(&body).expect("serializing primitive fields"));
    let response = transport
        .send_bound::<serde_json::Value>(&request, Some(session.revision))
        .await?;
    Ok(response.data.as_ref().and_then(published_id))
}

pub async fn set_liked(
    transport: &Transport,
    music_id: i64,
    comment_id: i64,
    liked: bool,
    parent_id: i64,
) -> Result<(), CommentError> {
    set_liked_inner(transport, music_id, comment_id, liked, parent_id, None).await
}

pub async fn set_liked_at_revision(
    transport: &Transport,
    music_id: i64,
    comment_id: i64,
    liked: bool,
    parent_id: i64,
    revision: u64,
) -> Result<(), CommentError> {
    set_liked_inner(
        transport,
        music_id,
        comment_id,
        liked,
        parent_id,
        Some(revision),
    )
    .await
}

async fn set_liked_inner(
    transport: &Transport,
    music_id: i64,
    comment_id: i64,
    liked: bool,
    parent_id: i64,
    expected: Option<u64>,
) -> Result<(), CommentError> {
    validate_music_id(music_id)?;
    if comment_id <= 0 || parent_id < 0 {
        return Err(CommentError::InvalidParameter);
    }
    let session = transport.session_snapshot();
    if expected.is_some_and(|revision| revision != session.revision) {
        return Err(TransportError::SessionChanged.into());
    }
    if !session.is_authenticated() {
        return Err(CommentError::NotAuthenticated);
    }
    let uid = session
        .uid
        .parse()
        .map_err(|_| CommentError::InvalidUserId)?;
    let body = LikeBody {
        module_id: music_id,
        module_type: 2,
        uid,
        comment_id,
        op: if liked { 1 } else { 2 },
        parent_id: (parent_id > 0).then_some(parent_id),
    };
    let mut request = ApiRequest::get("comments/v3/like");
    request.method = reqwest::Method::POST;
    request.platform = Some("android".into());
    request.json_body = Some(serde_json::to_string(&body).expect("serializing primitive fields"));
    transport
        .send_bound::<serde_json::Value>(&request, Some(session.revision))
        .await?;
    Ok(())
}

#[cfg(test)]
#[path = "comments_tests.rs"]
mod tests;
