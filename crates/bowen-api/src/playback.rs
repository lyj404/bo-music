//! Resolves only service-authorized plaintext audio; this does not play or decrypt media.
use bowen_domain::track::{AudioQuality, AudioVariant, Track};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    search::{http_url, parse_size},
    transport::{ApiRequest, Transport, TransportError},
};

#[derive(Debug, Error)]
pub enum ResolveError {
    #[error("music ID must be positive")]
    InvalidId,
    #[error(transparent)]
    Transport(#[from] TransportError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Denial {
    TrackUnavailable,
    NotAuthenticated,
    NoPermission,
    AuditionUnavailable,
    NoUsableQuality,
    NoStreamUrl,
}

/// URLs are signed and must not be logged. Intentionally not Debug.
#[derive(Clone)]
pub struct AudioSource {
    pub url: String,
    pub requested_quality: Option<AudioQuality>,
    pub requested_variant: Option<AudioVariant>,
    pub format: String,
    pub bitrate_kbps: i32,
    pub size_bytes: u64,
    pub downgraded: bool,
}

#[derive(Clone)]
pub enum PlaybackResolution {
    Playable(AudioSource),
    Audition {
        source: AudioSource,
        start_seconds: i32,
        end_seconds: i32,
    },
    Denied(Denial),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CheckBody {
    music_id: i64,
    free_sign: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AudioBody<'a> {
    dev_id: &'a str,
    music_id: i64,
    br: &'a str,
    free_sign: &'static str,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct RightDto {
    status: i32,
    audition: Option<AuditionDto>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct AuditionDto {
    start: i32,
    end: i32,
    br: i32,
    format: Option<String>,
    url: Option<String>,
    https: Option<String>,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct AudioDto {
    audio_url: Option<String>,
    audio_https_url: Option<String>,
    format: Option<String>,
    bitrate: i32,
    size: Option<String>,
}

fn playable_format(format: &str) -> bool {
    matches!(
        format.to_ascii_lowercase().as_str(),
        "aac" | "mp3" | "ogg" | "flac"
    )
}

fn supported_variant(variant: &AudioVariant) -> bool {
    let quality = match variant.level.as_str() {
        "s" | "h" => AudioQuality::Standard,
        "p" => AudioQuality::High,
        "ff" => AudioQuality::Lossless,
        _ => return false,
    };
    variant.quality == quality && playable_format(&variant.format) && variant.bitrate_kbps > 0
}

pub async fn resolve(
    transport: &Transport,
    track: &Track,
    preferred: Option<AudioQuality>,
) -> Result<PlaybackResolution, ResolveError> {
    let revision = transport.session_snapshot().revision;
    resolve_for_revision(transport, track, preferred, revision).await
}

/// Resolve only for the session revision captured when the request was scheduled.
pub async fn resolve_for_revision(
    transport: &Transport,
    track: &Track,
    preferred: Option<AudioQuality>,
    revision: u64,
) -> Result<PlaybackResolution, ResolveError> {
    if track.id <= 0 {
        return Err(ResolveError::InvalidId);
    }
    let session = transport.session_snapshot();
    if session.revision != revision {
        return Err(TransportError::SessionChanged.into());
    }
    let body = serde_json::to_string(&CheckBody {
        music_id: track.id,
        free_sign: "",
    })
    .expect("serializing primitive fields is infallible");
    let mut request = ApiRequest::get("play/music/v2/checkRight");
    request.query = vec![
        ("musicId".into(), track.id.to_string()),
        ("freeSign".into(), String::new()),
    ];
    request.json_body = Some(body);
    request.signed = true;
    let right = transport
        .send_bound::<RightDto>(&request, Some(session.revision))
        .await?
        .data;
    let Some(right) = right else {
        return Ok(PlaybackResolution::Denied(Denial::TrackUnavailable));
    };
    if right.status == 7 {
        return Ok(PlaybackResolution::Denied(if session.is_authenticated() {
            Denial::NoPermission
        } else {
            Denial::NotAuthenticated
        }));
    }
    if right.status == 3 {
        let Some(audition) = right.audition else {
            return Ok(PlaybackResolution::Denied(Denial::AuditionUnavailable));
        };
        let Some(url) = audition
            .https
            .as_deref()
            .and_then(http_url)
            .or_else(|| audition.url.as_deref().and_then(http_url))
        else {
            return Ok(PlaybackResolution::Denied(Denial::AuditionUnavailable));
        };
        let Some(format) = audition.format.filter(|format| playable_format(format)) else {
            return Ok(PlaybackResolution::Denied(Denial::AuditionUnavailable));
        };
        return Ok(PlaybackResolution::Audition {
            source: AudioSource {
                url,
                requested_quality: None,
                requested_variant: None,
                format,
                bitrate_kbps: audition.br,
                size_bytes: 0,
                downgraded: false,
            },
            start_seconds: audition.start,
            end_seconds: audition.end,
        });
    }
    let selected = track
        .audio_variants
        .iter()
        .filter(|variant| {
            preferred.is_none_or(|quality| variant.quality <= quality) && supported_variant(variant)
        })
        .max_by_key(|variant| (variant.quality, variant.bitrate_kbps))
        .cloned();
    let quality = selected
        .as_ref()
        .map(|variant| variant.quality)
        .or_else(|| {
            track
                .available_qualities
                .iter()
                .copied()
                .filter(|quality| preferred.is_none_or(|preferred| *quality <= preferred))
                .max()
        });
    let Some(quality) = quality else {
        return Ok(PlaybackResolution::Denied(Denial::NoUsableQuality));
    };
    let (expected_format, expected_bitrate) = selected
        .as_ref()
        .map(|variant| (variant.format.as_str(), variant.bitrate_kbps))
        .unwrap_or(match quality {
            AudioQuality::Standard => ("mp3", 128),
            AudioQuality::High => ("mp3", 320),
            AudioQuality::Lossless => ("flac", 2000),
        });
    let bitrate = format!("{expected_bitrate}k{expected_format}");
    let body = serde_json::to_string(&AudioBody {
        dev_id: transport.device_id(),
        music_id: track.id,
        br: &bitrate,
        free_sign: "",
    })
    .expect("serializing primitive fields is infallible");
    let mut request = ApiRequest::get("play/music/v2/audioUrl");
    request.query = vec![
        ("devId".into(), transport.device_id().into()),
        ("musicId".into(), track.id.to_string()),
        ("br".into(), bitrate),
        ("freeSign".into(), String::new()),
    ];
    request.json_body = Some(body);
    request.signed = true;
    request.accepted_codes = vec![20018, 20012];
    let response = transport
        .send_bound::<AudioDto>(&request, Some(session.revision))
        .await?;
    if response.code == 20012 {
        return Ok(PlaybackResolution::Denied(Denial::TrackUnavailable));
    }
    if response.code != 200 {
        return Ok(PlaybackResolution::Denied(Denial::NoPermission));
    }
    let Some(data) = response.data else {
        return Ok(PlaybackResolution::Denied(Denial::NoStreamUrl));
    };
    let Some(url) = data
        .audio_https_url
        .as_deref()
        .and_then(http_url)
        .or_else(|| data.audio_url.as_deref().and_then(http_url))
    else {
        return Ok(PlaybackResolution::Denied(Denial::NoStreamUrl));
    };
    let Some(format) = data.format.filter(|format| playable_format(format)) else {
        return Ok(PlaybackResolution::Denied(Denial::NoUsableQuality));
    };
    let matches = |expected: &AudioVariant| {
        expected.format.eq_ignore_ascii_case(&format)
            && (data.bitrate <= 0 || data.bitrate as u32 >= expected.bitrate_kbps)
    };
    let size_bytes = data
        .size
        .as_deref()
        .and_then(parse_size)
        .filter(|size| *size > 0)
        .or_else(|| {
            track
                .audio_variants
                .iter()
                .find(|candidate| supported_variant(candidate) && matches(candidate))
                .map(|candidate| candidate.size_bytes)
        })
        .unwrap_or(0);
    let downgraded = !expected_format.eq_ignore_ascii_case(&format)
        || (data.bitrate > 0 && (data.bitrate as u32) < expected_bitrate);
    Ok(PlaybackResolution::Playable(AudioSource {
        url,
        requested_quality: Some(quality),
        requested_variant: selected,
        format,
        bitrate_kbps: data.bitrate,
        size_bytes,
        downgraded,
    }))
}

#[cfg(test)]
#[path = "playback_tests.rs"]
mod tests;
