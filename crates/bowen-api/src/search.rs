//! Song search and the API-to-domain mapping shared by future track endpoints.
use std::collections::HashMap;

use bowen_domain::{
    catalog::{Album, Artist, Playlist},
    paging::{PageOrigin, PagedCursor, PagedResult},
    track::{AudioQuality, AudioVariant, LyricAvailability, Track, TrackArtist},
};
use serde::Deserialize;
use thiserror::Error;

use crate::transport::{ApiRequest, Transport, TransportError};

#[derive(Debug, Error)]
pub enum SearchError {
    #[error("search keyword cannot be blank")]
    BlankKeyword,
    #[error(transparent)]
    Transport(#[from] TransportError),
}

pub fn search_cursor(page_size: i32) -> PagedCursor {
    PagedCursor::new(PageOrigin::Zero, page_size)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchHotWord {
    pub keyword: String,
    pub rank: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SearchSection {
    Tracks(Vec<Track>),
    Playlists(Vec<Playlist>),
    Albums(Vec<Album>),
    Artists(Vec<Artist>),
}

pub async fn comprehensive(
    transport: &Transport,
    keyword: &str,
) -> Result<Vec<SearchSection>, SearchError> {
    comprehensive_at_revision(transport, keyword, transport.session().snapshot().revision).await
}

pub async fn comprehensive_at_revision(
    transport: &Transport,
    keyword: &str,
    revision: u64,
) -> Result<Vec<SearchSection>, SearchError> {
    if keyword.trim().is_empty() {
        return Err(SearchError::BlankKeyword);
    }
    let mut request = ApiRequest::get("search/comprehensive/v2/list");
    request.query.push(("keyword".into(), keyword.into()));
    let payload = transport
        .send_bound::<ComprehensivePayload>(&request, Some(revision))
        .await?
        .data
        .unwrap_or_default();
    let mut sections = Vec::new();
    for content in payload.content.unwrap_or_default() {
        if let Some(items) = content.tracks.filter(|items| !items.is_empty()) {
            sections.push(SearchSection::Tracks(
                items.into_iter().map(TrackDto::into_track).collect(),
            ));
        }
        if let Some(items) = content.playlists.filter(|items| !items.is_empty()) {
            sections.push(SearchSection::Playlists(
                items
                    .into_iter()
                    .map(SearchPlaylistDto::into_playlist)
                    .collect(),
            ));
        }
        if let Some(items) = content.albums.filter(|items| !items.is_empty()) {
            sections.push(SearchSection::Albums(
                items.into_iter().map(AlbumDto::into_album).collect(),
            ));
        }
        if let Some(items) = content.artists.filter(|items| !items.is_empty()) {
            sections.push(SearchSection::Artists(
                items
                    .into_iter()
                    .map(SearchArtistDto::into_artist)
                    .collect(),
            ));
        }
    }
    Ok(sections)
}

#[derive(Default, Deserialize)]
struct ComprehensivePayload {
    content: Option<Vec<ComprehensiveContent>>,
}

#[derive(Deserialize)]
struct ComprehensiveContent {
    #[serde(rename = "musicpage")]
    tracks: Option<Vec<TrackDto>>,
    #[serde(rename = "songlistpage")]
    playlists: Option<Vec<SearchPlaylistDto>>,
    #[serde(rename = "albumpage")]
    albums: Option<Vec<AlbumDto>>,
    #[serde(rename = "artistpage")]
    artists: Option<Vec<SearchArtistDto>>,
}

pub async fn suggestions(transport: &Transport, keyword: &str) -> Result<Vec<String>, SearchError> {
    suggestions_at_revision(transport, keyword, transport.session().snapshot().revision).await
}

pub async fn suggestions_at_revision(
    transport: &Transport,
    keyword: &str,
    revision: u64,
) -> Result<Vec<String>, SearchError> {
    if keyword.trim().is_empty() {
        return Err(SearchError::BlankKeyword);
    }
    let mut request = ApiRequest::get("search/tip/v2/list");
    request.query.push(("keyword".into(), keyword.into()));
    let payload = transport
        .send_bound::<TipsPayload>(&request, Some(revision))
        .await?
        .data
        .unwrap_or_default();
    let mut words: Vec<String> = Vec::new();
    for word in payload
        .result_list
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| item.word)
    {
        if !word.trim().is_empty()
            && !words
                .iter()
                .any(|existing| existing.to_lowercase() == word.to_lowercase())
        {
            words.push(word);
        }
    }
    Ok(words)
}

pub async fn hot_words(transport: &Transport) -> Result<Vec<SearchHotWord>, SearchError> {
    let payload = transport
        .send::<TopicsPayload>(&ApiRequest::get("search/topic/word/list"))
        .await?
        .data
        .unwrap_or_default();
    let mut words: Vec<_> = payload
        .hot_words
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            let keyword = item.keyword?;
            if keyword.trim().is_empty() {
                return None;
            }
            Some(SearchHotWord {
                keyword,
                rank: item.rank,
            })
        })
        .collect();
    words.sort_by_key(|word| word.rank);
    Ok(words)
}

#[derive(Default, Deserialize)]
struct TipsPayload {
    #[serde(rename = "resultList")]
    result_list: Option<Vec<TipDto>>,
}

#[derive(Deserialize)]
struct TipDto {
    #[serde(rename = "relword")]
    word: Option<String>,
}

#[derive(Default, Deserialize)]
struct TopicsPayload {
    #[serde(rename = "hotWord")]
    hot_words: Option<Vec<HotWordDto>>,
}

#[derive(Deserialize)]
struct HotWordDto {
    #[serde(rename = "key")]
    keyword: Option<String>,
    #[serde(rename = "sort", default)]
    rank: i32,
}

pub async fn search_tracks(
    transport: &Transport,
    keyword: &str,
    cursor: &mut PagedCursor,
) -> Result<PagedResult<Track>, SearchError> {
    if keyword.trim().is_empty() {
        return Err(SearchError::BlankKeyword);
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
    let mut request = ApiRequest::get("search/music/list");
    request.query = vec![
        ("pn".into(), cursor.page_number().to_string()),
        ("rn".into(), page_size.to_string()),
        ("keyword".into(), keyword.into()),
        ("correct".into(), "1".into()),
    ];
    let envelope = transport.send::<SearchPayload>(&request).await?;
    let payload = envelope.data.unwrap_or_default();
    let items: Vec<Track> = payload
        .result_list
        .unwrap_or_default()
        .into_iter()
        .map(TrackDto::into_track)
        .collect();
    cursor.advance(items.len());
    Ok(PagedResult {
        items,
        offset,
        page_size,
        total: payload.total,
    })
}

#[derive(Default, Deserialize)]
struct SearchPayload {
    #[serde(rename = "resultList")]
    result_list: Option<Vec<TrackDto>>,
    total: Option<u64>,
}

#[derive(Deserialize)]
pub(crate) struct CategoryPayload<T> {
    #[serde(rename = "resultList")]
    pub(crate) result_list: Option<Vec<T>>,
    pub(crate) total: Option<u64>,
}

async fn search_category<D: serde::de::DeserializeOwned, T>(
    transport: &Transport,
    keyword: &str,
    cursor: &mut PagedCursor,
    path: &str,
    map: impl Fn(D) -> T,
) -> Result<PagedResult<T>, SearchError> {
    if keyword.trim().is_empty() {
        return Err(SearchError::BlankKeyword);
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
        ("keyword".into(), keyword.into()),
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

pub async fn search_albums(
    transport: &Transport,
    keyword: &str,
    cursor: &mut PagedCursor,
) -> Result<PagedResult<Album>, SearchError> {
    search_category(
        transport,
        keyword,
        cursor,
        "search/album/list",
        AlbumDto::into_album,
    )
    .await
}

pub async fn search_artists(
    transport: &Transport,
    keyword: &str,
    cursor: &mut PagedCursor,
) -> Result<PagedResult<Artist>, SearchError> {
    search_category(
        transport,
        keyword,
        cursor,
        "search/artist/list",
        SearchArtistDto::into_artist,
    )
    .await
}

pub async fn search_playlists(
    transport: &Transport,
    keyword: &str,
    cursor: &mut PagedCursor,
) -> Result<PagedResult<Playlist>, SearchError> {
    search_category(
        transport,
        keyword,
        cursor,
        "search/playlist/list",
        SearchPlaylistDto::into_playlist,
    )
    .await
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct AlbumDto {
    #[serde(deserialize_with = "number_or_string")]
    id: i64,
    #[serde(deserialize_with = "number_or_string")]
    album_id: i64,
    pub(crate) source_type: i32,
    name: Option<String>,
    artist: Option<String>,
    creator_name: Option<String>,
    artist_id: i64,
    artists: Option<Vec<ArtistDto>>,
    pic: Option<String>,
    music_count: i32,
    artist_pic: Option<String>,
    #[serde(rename = "showtime")]
    show_time: Option<String>,
    info: Option<String>,
}

impl AlbumDto {
    pub(crate) fn into_album(self) -> Album {
        Album {
            id: if self.album_id > 0 {
                self.album_id
            } else {
                self.id
            },
            source_type: self.source_type,
            name: self
                .name
                .filter(|name| !name.trim().is_empty())
                .unwrap_or_else(|| "(未命名专辑)".into()),
            artist_text: self
                .artist
                .filter(|name| !name.trim().is_empty())
                .or(self.creator_name)
                .unwrap_or_default(),
            artist_id: self.artist_id,
            artists: self
                .artists
                .unwrap_or_default()
                .into_iter()
                .map(ArtistDto::into_artist)
                .collect(),
            cover_url: self.pic.as_deref().and_then(http_url),
            music_count: self.music_count,
            artist_cover_url: self.artist_pic.as_deref().and_then(http_url),
            release_date: self.show_time.unwrap_or_default(),
            description: self.info.unwrap_or_default(),
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct SearchArtistDto {
    #[serde(deserialize_with = "number_or_string")]
    artist_id: i64,
    name: Option<String>,
    pic: Option<String>,
    song_num: i32,
    album_num: i32,
}

impl SearchArtistDto {
    fn into_artist(self) -> Artist {
        Artist {
            id: self.artist_id,
            name: self.name.unwrap_or_else(|| "(未命名歌手)".into()),
            cover_url: self.pic.as_deref().and_then(http_url),
            song_count: self.song_num,
            album_count: self.album_num,
            alias: String::new(),
            fans_count: 0,
            description: String::new(),
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct SearchPlaylistDto {
    #[serde(deserialize_with = "number_or_string")]
    id: i64,
    name: Option<String>,
    pic: Option<String>,
    musicnum: i32,
    source: i32,
}

impl SearchPlaylistDto {
    fn into_playlist(self) -> Playlist {
        Playlist {
            id: self.id,
            name: self.name.unwrap_or_else(|| "(未命名歌单)".into()),
            music_count: self.musicnum,
            cover_url: self.pic.as_deref().and_then(http_url),
            source_type: self.source,
            ..Default::default()
        }
    }
}

pub(crate) fn number_or_string<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<i64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Number {
        Integer(i64),
        String(String),
    }
    match Number::deserialize(deserializer)? {
        Number::Integer(value) => Ok(value),
        Number::String(value) => value.parse().map_err(serde::de::Error::custom),
    }
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub(crate) struct TrackDto {
    #[serde(deserialize_with = "number_or_string")]
    id: i64,
    name: Option<String>,
    song_name: Option<String>,
    artist: Option<String>,
    artists: Option<Vec<ArtistDto>>,
    album: Option<String>,
    #[serde(deserialize_with = "number_or_string")]
    album_id: i64,
    album_pic: Option<String>,
    album_pic120: Option<String>,
    duration: i32,
    audios: Option<Vec<AudioDto>>,
    pay_info: Option<PayDto>,
    #[serde(rename = "lrc_info")]
    lyrics: Option<LyricsDto>,
    is_mv: i32,
    #[serde(deserialize_with = "number_or_string")]
    vid: i64,
    comment: Option<i64>,
    favorite: Option<i64>,
    share: Option<i64>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ArtistDto {
    #[serde(deserialize_with = "number_or_string")]
    id: i64,
    name: Option<String>,
    pic: Option<String>,
}

impl ArtistDto {
    fn into_artist(self) -> TrackArtist {
        TrackArtist {
            id: self.id,
            name: self.name.unwrap_or_default(),
            image_url: self.pic.as_deref().and_then(artist_image),
        }
    }
}
#[derive(Default, Deserialize)]
#[serde(default)]
struct AudioDto {
    level: Option<String>,
    format: Option<String>,
    bitrate: Option<String>,
    size: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct PayDto {
    fee_type: Option<HashMap<String, String>>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct LyricsDto {
    lrc: i32,
    lrcx: i32,
}

impl TrackDto {
    pub(crate) fn into_track(self) -> Track {
        let artists: Vec<TrackArtist> = self
            .artists
            .unwrap_or_default()
            .into_iter()
            .map(ArtistDto::into_artist)
            .collect();
        let artist_text = self.artist.unwrap_or_else(|| {
            artists
                .iter()
                .map(|a| a.name.as_str())
                .filter(|name| !name.trim().is_empty())
                .collect::<Vec<_>>()
                .join("、")
        });
        let fee = self.pay_info.and_then(|p| p.fee_type).unwrap_or_default();
        let has_fee = |key: &str| fee.get(key).is_some_and(|v| v == "1");
        let mut variants = Vec::new();
        for audio in self.audios.unwrap_or_default() {
            if let Some(variant) = audio.into_variant()
                && !variants.contains(&variant)
            {
                variants.push(variant);
            }
        }
        let mut available_qualities: Vec<_> = variants.iter().map(|v| v.quality).collect();
        available_qualities.sort_unstable_by(|a, b| b.cmp(a));
        available_qualities.dedup();
        Track {
            id: self.id,
            title: self
                .name
                .or(self.song_name)
                .unwrap_or_else(|| "(未知曲目)".into()),
            artist_text,
            artists,
            album_name: self.album,
            album_id: self.album_id,
            cover_url: self
                .album_pic120
                .as_deref()
                .and_then(http_url)
                .or_else(|| self.album_pic.as_deref().and_then(http_url)),
            duration_seconds: self.duration,
            available_qualities,
            audio_variants: variants,
            requires_vip: has_fee("vip"),
            requires_purchase: ["song", "album", "bodianAlbum", "bookvip"]
                .iter()
                .any(|key| has_fee(key)),
            lyrics: self.lyrics.map(|l| LyricAvailability {
                line: l.lrc == 1,
                word: l.lrcx == 1,
            }),
            has_mv: self.is_mv == 1 || self.vid > 0,
            comment_count: self.comment.map(|n| n.max(0)),
            favorite_count: self.favorite.map(|n| n.max(0)),
            share_count: self.share.map(|n| n.max(0)),
        }
    }
}

impl AudioDto {
    fn into_variant(self) -> Option<AudioVariant> {
        let level = self.level?;
        let quality = match level.as_str() {
            "s" | "h" => AudioQuality::Standard,
            "p" => AudioQuality::High,
            "ff" => AudioQuality::Lossless,
            _ => return None,
        };
        let format = self.format?.to_ascii_lowercase();
        if format == level.to_ascii_lowercase()
            || !matches!(format.as_str(), "aac" | "mp3" | "ogg" | "flac")
        {
            return None;
        }
        let bitrate_text = self.bitrate?;
        if bitrate_text.is_empty() || !bitrate_text.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let bitrate_kbps = bitrate_text.parse::<u32>().ok()?;
        if bitrate_kbps == 0 || bitrate_kbps > i32::MAX as u32 {
            return None;
        }
        let size_bytes = parse_size(self.size.as_deref()?)?;
        Some(AudioVariant {
            quality,
            level,
            format,
            bitrate_kbps,
            size_bytes,
        })
    }
}

pub(crate) fn parse_size(value: &str) -> Option<u64> {
    let split = value.len().checked_sub(2)?;
    let number = value.get(..split)?;
    let unit = value.get(split..)?;
    let mut parts = number.split('.');
    let integer = parts.next()?;
    let fraction = parts.next();
    if parts.next().is_some()
        || integer.is_empty()
        || !integer.bytes().all(|c| c.is_ascii_digit())
        || fraction.is_some_and(|f| f.is_empty() || !f.bytes().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    let scale = match unit.to_ascii_lowercase().as_str() {
        "kb" => 1024.0,
        "mb" => 1_048_576.0,
        "gb" => 1_073_741_824.0,
        _ => return None,
    };
    let bytes = number.parse::<f64>().ok()? * scale;
    if !bytes.is_finite() || bytes <= 0.0 || bytes >= i64::MAX as f64 {
        return Some(0);
    }
    Some(bytes.round_ties_even() as u64)
}

pub(crate) fn http_url(value: &str) -> Option<String> {
    let mut url = reqwest::Url::parse(value).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    if url
        .host_str()
        .is_some_and(|host| host.eq_ignore_ascii_case("y.gtimg.cn"))
        && value.contains("_o.jpg")
    {
        url = reqwest::Url::parse(&value.replace("_o.jpg", "_0.jpg")).ok()?;
    }
    Some(url.into())
}

fn artist_image(value: &str) -> Option<String> {
    if value
        .to_ascii_lowercase()
        .ends_with("/default_artist_pic.png")
    {
        None
    } else {
        http_url(value)
    }
}

#[cfg(test)]
#[path = "search_tests.rs"]
mod tests;
