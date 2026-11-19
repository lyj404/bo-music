//! Account-scoped, versioned queue snapshots compatible with legacy queue JSON.
use crate::{
    atomic,
    paths::{DataPaths, PathError},
};
use bowen_domain::{
    queue::PlayQueue,
    track::{AudioQuality, AudioVariant, Track},
};
use serde_json::{Value, json};
use std::{
    io,
    path::{Path, PathBuf},
    sync::Mutex,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum QueueError {
    #[error(transparent)]
    Path(#[from] PathError),
    #[error("queue I/O failed")]
    Io(#[from] io::Error),
    #[error("invalid queue snapshot")]
    Invalid,
    #[error("queue snapshot has an unsupported schema {0}")]
    FutureVersion(u64),
    #[error("queue writer lock poisoned")]
    Poisoned,
}

#[derive(Clone, Debug, Default)]
pub struct QueueSnapshot {
    pub tracks: Vec<Track>,
    pub current_index: Option<usize>,
    pub position_seconds: f64,
}

impl QueueSnapshot {
    pub fn capture(queue: &PlayQueue, position_seconds: f64) -> Self {
        Self {
            tracks: queue.items().to_vec(),
            current_index: queue.current_index(),
            position_seconds: if position_seconds.is_finite() && position_seconds > 0.0 {
                position_seconds
            } else {
                0.0
            },
        }
    }
}

pub(crate) fn field<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    value
        .as_object()?
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value)
}
fn quality(value: &Value) -> Option<AudioQuality> {
    match value {
        Value::String(s) if s.eq_ignore_ascii_case("Standard") => Some(AudioQuality::Standard),
        Value::String(s) if s.eq_ignore_ascii_case("High") => Some(AudioQuality::High),
        Value::String(s) if s.eq_ignore_ascii_case("Lossless") => Some(AudioQuality::Lossless),
        Value::Number(n) if n.as_u64() == Some(0) => Some(AudioQuality::Standard),
        Value::Number(n) if n.as_u64() == Some(1) => Some(AudioQuality::High),
        Value::Number(n) if n.as_u64() == Some(2) => Some(AudioQuality::Lossless),
        _ => None,
    }
}
fn quality_name(value: AudioQuality) -> &'static str {
    match value {
        AudioQuality::Standard => "Standard",
        AudioQuality::High => "High",
        AudioQuality::Lossless => "Lossless",
    }
}
fn parse_variant(value: &Value) -> Option<AudioVariant> {
    let quality = quality(field(value, "quality")?)?;
    let level = field(value, "level")?.as_str()?.to_owned();
    let format = field(value, "format")?.as_str()?.to_ascii_lowercase();
    let bitrate_kbps = u32::try_from(field(value, "bitrateKbps")?.as_u64()?).ok()?;
    let level_quality = match level.as_str() {
        "s" | "h" => AudioQuality::Standard,
        "p" => AudioQuality::High,
        "ff" => AudioQuality::Lossless,
        _ => return None,
    };
    if quality != level_quality
        || bitrate_kbps == 0
        || !matches!(format.as_str(), "mp3" | "aac" | "ogg" | "flac")
    {
        return None;
    }
    Some(AudioVariant {
        quality,
        level,
        format,
        bitrate_kbps,
        size_bytes: field(value, "sizeBytes")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    })
}
pub(crate) fn parse_track(value: &Value) -> Option<Track> {
    let id = field(value, "id")
        .or_else(|| field(value, "musicId"))?
        .as_i64()
        .filter(|n| *n > 0)?;
    let title = field(value, "title")?.as_str()?.to_owned();
    if title.is_empty() {
        return None;
    }
    let mut qualities: Vec<_> = field(value, "availableQualities")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(quality)
        .collect();
    qualities.sort_unstable_by(|a, b| b.cmp(a));
    qualities.dedup();
    let variants = field(value, "audioVariants")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(parse_variant)
        .collect();
    let text = |key| {
        field(value, key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let number = |key| field(value, key).and_then(Value::as_i64).unwrap_or(0);
    let flag = |key| field(value, key).and_then(Value::as_bool).unwrap_or(false);
    let cover = field(value, "coverImage")
        .and_then(Value::as_str)
        .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
        .map(str::to_owned);
    Some(Track {
        id,
        title,
        artist_text: text("artistText"),
        artists: vec![],
        album_name: field(value, "albumName")
            .and_then(Value::as_str)
            .map(str::to_owned),
        album_id: number("albumId"),
        cover_url: cover,
        duration_seconds: i32::try_from(number("durationSeconds")).unwrap_or(0).max(0),
        available_qualities: qualities,
        audio_variants: variants,
        requires_vip: flag("requiresVip"),
        requires_purchase: flag("requiresPurchase"),
        lyrics: None,
        has_mv: flag("hasMv"),
        comment_count: None,
        favorite_count: None,
        share_count: None,
    })
}
fn parse(bytes: &[u8]) -> Result<QueueSnapshot, QueueError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| QueueError::Invalid)?;
    let version = field(&value, "schemaVersion")
        .map(|v| v.as_u64().ok_or(QueueError::Invalid))
        .transpose()?
        .unwrap_or(1);
    if version > 1 {
        return Err(QueueError::FutureVersion(version));
    }
    if version == 0 {
        return Err(QueueError::Invalid);
    }
    let items = field(&value, "items")
        .and_then(Value::as_array)
        .ok_or(QueueError::Invalid)?;
    let tracks: Vec<_> = items.iter().filter_map(parse_track).collect();
    let id = field(&value, "currentTrackId")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let index = field(&value, "currentIndex")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let current_index = if tracks.is_empty() {
        None
    } else {
        Some(
            tracks
                .iter()
                .position(|t| t.id == id && id > 0)
                .unwrap_or_else(|| usize::try_from(index).unwrap_or(0).min(tracks.len() - 1)),
        )
    };
    let position_seconds = field(&value, "positionSeconds")
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite() && *n > 0.0)
        .unwrap_or(0.0);
    Ok(QueueSnapshot {
        tracks,
        current_index,
        position_seconds,
    })
}
pub(crate) fn track_value(track: &Track) -> Value {
    let variants: Vec<_> = track
        .audio_variants
        .iter()
        .map(|v| {
            json!({
                "Quality": quality_name(v.quality), "Level": v.level, "Format": v.format,
                "BitrateKbps": v.bitrate_kbps, "SizeBytes": v.size_bytes
            })
        })
        .collect();
    json!({
        "Id": track.id, "Title": track.title, "ArtistText": track.artist_text,
        "AlbumName": track.album_name, "AlbumId": track.album_id,
        "CoverImage": track.cover_url, "DurationSeconds": track.duration_seconds,
        "AvailableQualities": track.available_qualities.iter().map(|q| quality_name(*q)).collect::<Vec<_>>(),
        "AudioVariants": variants, "RequiresVip": track.requires_vip,
        "RequiresPurchase": track.requires_purchase, "HasMv": track.has_mv
    })
}
fn encode(snapshot: &QueueSnapshot) -> Result<Vec<u8>, QueueError> {
    let tracks: Vec<_> = snapshot.tracks.iter().map(track_value).collect();
    let index = snapshot.current_index.filter(|i| *i < tracks.len());
    let position = if snapshot.position_seconds.is_finite() && snapshot.position_seconds > 0.0 {
        snapshot.position_seconds
    } else {
        0.0
    };
    serde_json::to_vec_pretty(&json!({
        "SchemaVersion": 1, "Items": tracks,
        "CurrentTrackId": index.map(|i| snapshot.tracks[i].id).unwrap_or(0),
        "CurrentIndex": index.map(|i| i as i64).unwrap_or(-1), "PositionSeconds": position
    }))
    .map_err(|_| QueueError::Invalid)
}
fn backup(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".bak");
    PathBuf::from(name)
}

pub struct QueueStore {
    paths: DataPaths,
    gate: Mutex<()>,
}
impl QueueStore {
    pub fn new(paths: DataPaths) -> Self {
        Self {
            paths,
            gate: Mutex::new(()),
        }
    }
    fn path(&self, scope: &str) -> Result<PathBuf, QueueError> {
        Ok(self.paths.account_dir(scope)?.join("queue.json"))
    }
    pub fn load(&self, scope: &str) -> Result<QueueSnapshot, QueueError> {
        let _guard = self.gate.lock().map_err(|_| QueueError::Poisoned)?;
        let path = self.path(scope)?;
        match atomic::read_limited(&path) {
            Ok(bytes) => match parse(&bytes) {
                Ok(snapshot) => Ok(snapshot),
                Err(QueueError::Invalid) => self.load_backup(&path, false),
                Err(error) => Err(error),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => self.load_backup(&path, true),
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                self.load_backup(&path, false)
            }
            Err(error) => Err(error.into()),
        }
    }
    fn load_backup(&self, path: &Path, missing: bool) -> Result<QueueSnapshot, QueueError> {
        match atomic::read_limited(&backup(path)) {
            Ok(bytes) => parse(&bytes),
            Err(error) if error.kind() == io::ErrorKind::NotFound && missing => {
                Ok(QueueSnapshot::default())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Err(QueueError::Invalid),
            Err(error) => Err(error.into()),
        }
    }
    pub fn save(&self, scope: &str, snapshot: &QueueSnapshot) -> Result<(), QueueError> {
        let _guard = self.gate.lock().map_err(|_| QueueError::Poisoned)?;
        let path = self.path(scope)?;
        let bytes = encode(snapshot)?;
        match atomic::read_limited(&path) {
            Ok(previous) => match parse(&previous) {
                Ok(_) => atomic::replace(&backup(&path), &previous)?,
                Err(QueueError::Invalid) => {}
                Err(error) => return Err(error),
            },
            Err(error)
                if error.kind() == io::ErrorKind::NotFound
                    || error.kind() == io::ErrorKind::InvalidData => {}
            Err(error) => return Err(error.into()),
        }
        atomic::replace(&path, &bytes)?;
        Ok(())
    }
}
