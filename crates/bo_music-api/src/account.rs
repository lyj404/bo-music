//! Account statistics and listening reports, bound to the active session.
use crate::{
    comments::optional_number,
    search::{TrackDto, http_url, number_or_string},
    session::SessionSnapshot,
    transport::{ApiRequest, Transport, TransportError},
};
use bo_music_domain::account::{
    AccountMetadata, AccountOverview, AccountPlayData, AccountVipInfo, MonthPeriod, MonthReport,
    MonthReportSummary, PlayRankingEntry, ReportTrack, VipBadge,
};
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AccountError {
    #[error("sign in to access account information")]
    NotAuthenticated,
    #[error("session user ID is invalid")]
    InvalidUserId,
    #[error(transparent)]
    Transport(#[from] TransportError),
}

fn signed_in(transport: &Transport) -> Result<SessionSnapshot, AccountError> {
    let session = transport.session_snapshot();
    if !session.is_authenticated() {
        return Err(AccountError::NotAuthenticated);
    }
    Ok(session)
}

fn numeric_uid(session: &SessionSnapshot) -> Result<i64, AccountError> {
    let uid = &session.uid;
    if uid.is_empty() || !uid.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(AccountError::InvalidUserId);
    }
    uid.parse::<i64>().map_err(|_| AccountError::InvalidUserId)
}

/// Account detail is optional on service/network failures; stale results are errors.
async fn fetch<T: serde::de::DeserializeOwned>(
    transport: &Transport,
    session: &SessionSnapshot,
    path: impl Into<String>,
    query: Vec<(String, String)>,
) -> Result<Option<T>, AccountError> {
    let mut request = ApiRequest::get(path);
    request.query = query;
    request.signed = true;
    match transport
        .send_bound::<T>(&request, Some(session.revision))
        .await
    {
        Ok(response) => Ok(response.data),
        Err(TransportError::SessionChanged) => {
            Err(AccountError::Transport(TransportError::SessionChanged))
        }
        Err(_) => Ok(None),
    }
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct MetadataDto {
    #[serde(deserialize_with = "optional_number")]
    follow_count: Option<i64>,
    #[serde(deserialize_with = "optional_number")]
    fans_count: Option<i64>,
    #[serde(deserialize_with = "optional_number")]
    follow_artist_count: Option<i64>,
    #[serde(deserialize_with = "optional_number")]
    praised: Option<i64>,
}

pub async fn metadata(transport: &Transport) -> Result<Option<AccountMetadata>, AccountError> {
    let session = signed_in(transport)?;
    metadata_for(transport, &session).await
}

async fn metadata_for(
    transport: &Transport,
    session: &SessionSnapshot,
) -> Result<Option<AccountMetadata>, AccountError> {
    let uid = numeric_uid(session)?;
    Ok(fetch::<MetadataDto>(
        transport,
        session,
        format!("service/users/{uid}/metadata"),
        vec![],
    )
    .await?
    .map(|data| AccountMetadata {
        follow_count: data.follow_count,
        fans_count: data.fans_count,
        follow_artist_count: data.follow_artist_count,
        praised: data.praised,
    }))
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct PlayDataDto {
    #[serde(rename = "playcnt", deserialize_with = "optional_number")]
    play_count: Option<i64>,
    #[serde(rename = "playTime", deserialize_with = "optional_number")]
    play_seconds: Option<i64>,
}

pub async fn play_data(transport: &Transport) -> Result<Option<AccountPlayData>, AccountError> {
    let session = signed_in(transport)?;
    play_data_for(transport, &session).await
}

async fn play_data_for(
    transport: &Transport,
    session: &SessionSnapshot,
) -> Result<Option<AccountPlayData>, AccountError> {
    Ok(fetch::<PlayDataDto>(
        transport,
        session,
        "ucenter/playdata/user_data",
        vec![("userId".into(), session.uid.clone())],
    )
    .await?
    .map(|data| AccountPlayData {
        play_count: data.play_count,
        play_seconds: data.play_seconds,
    }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RankingDto {
    music_list: Option<Vec<RankingItemDto>>,
}

#[derive(Deserialize)]
struct RankingItemDto {
    #[serde(flatten)]
    track: TrackDto,
    #[serde(default, rename = "playcnt", deserialize_with = "optional_number")]
    play_count: Option<i64>,
}

pub async fn weekly_ranking(
    transport: &Transport,
) -> Result<Option<Vec<PlayRankingEntry>>, AccountError> {
    let session = signed_in(transport)?;
    weekly_ranking_for(transport, &session).await
}

async fn weekly_ranking_for(
    transport: &Transport,
    session: &SessionSnapshot,
) -> Result<Option<Vec<PlayRankingEntry>>, AccountError> {
    Ok(fetch::<RankingDto>(
        transport,
        session,
        "ucenter/playdata/play_ranking",
        vec![
            ("userId".into(), session.uid.clone()),
            ("type".into(), "1".into()),
        ],
    )
    .await?
    .map(|data| {
        data.music_list
            .unwrap_or_default()
            .into_iter()
            .filter_map(|item| {
                let track = item.track.into_track();
                (track.id > 0).then_some(PlayRankingEntry {
                    track,
                    play_count: item.play_count.unwrap_or(0),
                })
            })
            .collect()
    }))
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct PayInfoDto {
    #[serde(deserialize_with = "number_or_string")]
    is_vip: i64,
    is_vip_boolean: bool,
    is_pay_vip_boolean: bool,
    is_big_vip_boolean: bool,
    is_act_vip_boolean: bool,
    is_big_pay_vip_boolean: bool,
    is_ct_vip_boolean: bool,
    #[serde(deserialize_with = "number_or_string")]
    vip_type: i64,
    #[serde(deserialize_with = "number_or_string")]
    pay_vip_type: i64,
    #[serde(deserialize_with = "number_or_string")]
    act_vip_type: i64,
    #[serde(deserialize_with = "number_or_string")]
    expire_date: i64,
    #[serde(deserialize_with = "number_or_string")]
    pay_expire_date: i64,
    #[serde(deserialize_with = "number_or_string")]
    big_expire_date: i64,
    #[serde(deserialize_with = "number_or_string")]
    big_pay_expire_date: i64,
    #[serde(deserialize_with = "number_or_string")]
    ct_expire_date: i64,
    #[serde(deserialize_with = "number_or_string")]
    ct_pay_expire_date: i64,
    #[serde(deserialize_with = "number_or_string")]
    act_expire_date: i64,
}

impl PayInfoDto {
    fn badge(&self) -> VipBadge {
        let is_vip = self.is_vip_boolean
            || self.is_vip > 0
            || self.is_pay_vip_boolean
            || self.is_big_vip_boolean
            || self.is_act_vip_boolean
            || self.is_big_pay_vip_boolean
            || self.is_ct_vip_boolean
            || self.vip_type > 0
            || self.pay_vip_type > 0
            || self.act_vip_type > 0;
        if !is_vip {
            VipBadge::None
        } else if self.vip_type == 1 && self.pay_vip_type == 2 {
            VipBadge::Standard
        } else if self.vip_type == 2 {
            VipBadge::Welfare
        } else {
            VipBadge::Big
        }
    }
    fn expiry(&self) -> Option<i64> {
        [
            self.expire_date,
            self.pay_expire_date,
            self.big_expire_date,
            self.big_pay_expire_date,
            self.ct_expire_date,
            self.ct_pay_expire_date,
            self.act_expire_date,
        ]
        .into_iter()
        .filter(|date| (1..=4_102_444_800_000).contains(date))
        .max()
    }
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct UserPubDto {
    pay_info: Option<PayInfoDto>,
    user_info: Option<UserInfoDto>,
}
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct UserInfoDto {
    create_time: Option<String>,
}

pub async fn vip_info(transport: &Transport) -> Result<Option<AccountVipInfo>, AccountError> {
    let session = signed_in(transport)?;
    vip_info_for(transport, &session).await
}

async fn vip_info_for(
    transport: &Transport,
    session: &SessionSnapshot,
) -> Result<Option<AccountVipInfo>, AccountError> {
    let uid = numeric_uid(session)?;
    Ok(fetch::<UserPubDto>(
        transport,
        session,
        format!("ucenter/users/pub/{uid}"),
        vec![],
    )
    .await?
    .map(|data| {
        let pay = data.pay_info.unwrap_or_default();
        AccountVipInfo {
            badge: pay.badge(),
            expires_unix_ms: pay.expiry(),
            created_at: data
                .user_info
                .and_then(|user| user.create_time)
                .filter(|value| !value.trim().is_empty()),
        }
    }))
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct SummaryDto {
    month_report: Option<PreviewDto>,
}
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct PreviewDto {
    month_desc: Option<String>,
    listen_total: Option<String>,
    listen_chain: Option<String>,
    pic: Option<String>,
}

pub async fn month_summary(
    transport: &Transport,
) -> Result<Option<MonthReportSummary>, AccountError> {
    let session = signed_in(transport)?;
    month_summary_for(transport, &session).await
}

async fn month_summary_for(
    transport: &Transport,
    session: &SessionSnapshot,
) -> Result<Option<MonthReportSummary>, AccountError> {
    Ok(fetch::<SummaryDto>(
        transport,
        session,
        "play/month/report/summary",
        vec![("userId".into(), session.uid.clone())],
    )
    .await?
    .and_then(|data| data.month_report)
    .map(|data| MonthReportSummary {
        month_desc: data.month_desc,
        listen_total_minutes: data.listen_total,
        listen_chain: data.listen_chain,
        cover_url: data.pic.as_deref().and_then(http_url),
    }))
}

pub async fn overview_at_revision(
    transport: &Transport,
    revision: u64,
) -> Result<AccountOverview, AccountError> {
    let session = transport.session_snapshot();
    if session.revision != revision {
        return Err(TransportError::SessionChanged.into());
    }
    if !session.is_authenticated() {
        return Err(AccountError::NotAuthenticated);
    }
    let (metadata, playback, vip, weekly, month_summary) = tokio::join!(
        metadata_for(transport, &session),
        play_data_for(transport, &session),
        vip_info_for(transport, &session),
        weekly_ranking_for(transport, &session),
        month_summary_for(transport, &session),
    );
    if transport.session_snapshot().revision != revision {
        return Err(TransportError::SessionChanged.into());
    }
    Ok(AccountOverview {
        metadata: metadata.ok().flatten(),
        playback: playback.ok().flatten(),
        vip: vip.ok().flatten(),
        weekly: weekly.ok().flatten(),
        month_summary: month_summary.ok().flatten(),
    })
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct ReportDto {
    info: Option<ReportInfoDto>,
}
#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ReportInfoDto {
    month_date: Option<String>,
    listen_total: Option<String>,
    #[serde(deserialize_with = "optional_number")]
    play_total: Option<i64>,
    #[serde(deserialize_with = "optional_number")]
    song_num: Option<i64>,
    #[serde(deserialize_with = "optional_number")]
    time_rank: Option<i64>,
    listen_chain: Option<String>,
    play_chain: Option<String>,
    listen_content: Option<String>,
    listen_top1_music_name: Option<String>,
    play_content: Option<String>,
    artist_name: Option<String>,
    artist_pic: Option<String>,
    artist_music_name: Option<String>,
    artist_music_pic: Option<String>,
    artist_content: Option<String>,
    music_top10: Option<Vec<ReportTrackDto>>,
    album_top10: Option<Vec<String>>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct ReportTrackDto {
    #[serde(deserialize_with = "number_or_string")]
    id: i64,
    name: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    album_pic: Option<String>,
    #[serde(deserialize_with = "optional_number")]
    play_count: Option<i64>,
}

pub async fn month_report(
    transport: &Transport,
    period: MonthPeriod,
) -> Result<Option<MonthReport>, AccountError> {
    let session = signed_in(transport)?;
    match month_report_at_revision(transport, period, session.revision).await {
        Err(AccountError::Transport(TransportError::SessionChanged)) => {
            Err(TransportError::SessionChanged.into())
        }
        Err(_) => Ok(None),
        result => result,
    }
}

pub async fn month_report_at_revision(
    transport: &Transport,
    period: MonthPeriod,
    revision: u64,
) -> Result<Option<MonthReport>, AccountError> {
    check_account_revision(transport, revision)?;
    let mut request = ApiRequest::get("play/month/report/info");
    request.signed = true;
    request.query.push(("date".into(), period.request_value()));
    Ok(transport
        .send_bound::<ReportDto>(&request, Some(revision))
        .await?
        .data
        .and_then(|data| data.info)
        .map(|info| MonthReport {
            period: info
                .month_date
                .as_deref()
                .and_then(MonthPeriod::from_date)
                .unwrap_or(period),
            listen_total_minutes: info.listen_total,
            play_count: info.play_total,
            song_count: info.song_num,
            time_rank: info.time_rank,
            listen_chain: info.listen_chain,
            play_chain: info.play_chain,
            listen_content: info.listen_content,
            listen_top_track_title: info.listen_top1_music_name,
            play_content: info.play_content,
            artist_name: info.artist_name,
            artist_cover_url: info.artist_pic.as_deref().and_then(http_url),
            artist_music_name: info.artist_music_name,
            artist_music_cover_url: info.artist_music_pic.as_deref().and_then(http_url),
            artist_content: info.artist_content,
            top_tracks: info
                .music_top10
                .unwrap_or_default()
                .into_iter()
                .filter(|track| track.id > 0)
                .map(|track| ReportTrack {
                    id: track.id,
                    title: track.name.unwrap_or_else(|| "(未知曲目)".into()),
                    artist: track.artist.unwrap_or_default(),
                    album_name: track.album,
                    cover_url: track.album_pic.as_deref().and_then(http_url),
                    play_count: track.play_count,
                })
                .collect(),
            album_covers: info
                .album_top10
                .unwrap_or_default()
                .into_iter()
                .filter_map(|url| http_url(&url))
                .collect(),
        }))
}

/// This GET creates a playlist on the service. Never automatically retry it.
pub async fn create_report_playlist(
    transport: &Transport,
    period: MonthPeriod,
) -> Result<bool, AccountError> {
    let session = signed_in(transport)?;
    create_report_playlist_at_revision(transport, period, session.revision).await
}

pub async fn create_report_playlist_at_revision(
    transport: &Transport,
    period: MonthPeriod,
    revision: u64,
) -> Result<bool, AccountError> {
    check_account_revision(transport, revision)?;
    let mut request = ApiRequest::get("play/month/report/playlist");
    request.signed = true;
    request.query.push(("date".into(), period.request_value()));
    match transport
        .send_bound::<serde_json::Value>(&request, Some(revision))
        .await
    {
        Ok(_) => Ok(true),
        Err(TransportError::SessionChanged) => {
            Err(AccountError::Transport(TransportError::SessionChanged))
        }
        Err(_) => Ok(false),
    }
}

fn check_account_revision(transport: &Transport, revision: u64) -> Result<(), AccountError> {
    let session = transport.session_snapshot();
    if session.revision != revision {
        return Err(TransportError::SessionChanged.into());
    }
    if !session.is_authenticated() {
        return Err(AccountError::NotAuthenticated);
    }
    Ok(())
}

#[cfg(test)]
#[path = "account_tests.rs"]
mod tests;
