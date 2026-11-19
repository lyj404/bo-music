//! Account-scoped recent plays, including imported WinUI history snapshots.
use crate::{
    atomic,
    paths::{DataPaths, PathError},
    queue::{field, parse_track, track_value},
};
use bowen_domain::track::Track;
use serde_json::{Value, json};
use std::{
    collections::HashSet,
    io,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

const CAPACITY: usize = 500;

#[derive(Debug, Error)]
pub enum HistoryError {
    #[error(transparent)]
    Path(#[from] PathError),
    #[error("history I/O failed")]
    Io(#[from] io::Error),
    #[error("history file is invalid")]
    Invalid,
    #[error("history lock poisoned")]
    Poisoned,
}

struct Entry {
    track: Track,
    played_at: Option<Value>,
    played_at_ms: Option<u64>,
}

// WinUI persists DateTimeOffset strings; normalize offsets before sorting them.
fn legacy_time_ms(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || !matches!(bytes.get(10), Some(b'T' | b't'))
        || bytes.get(13) != Some(&b':')
        || bytes.get(16) != Some(&b':')
    {
        return None;
    }
    let number = |start, end| {
        let digits = value.get(start..end)?;
        if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        digits.parse::<i64>().ok()
    };
    let (mut year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if day < 1 || day > max_day || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let mut offset_index = 19;
    let mut millis = 0_i64;
    if bytes.get(offset_index) == Some(&b'.') {
        offset_index += 1;
        let start = offset_index;
        while bytes.get(offset_index).is_some_and(u8::is_ascii_digit) {
            offset_index += 1;
        }
        let digits = value.get(start..offset_index)?;
        if digits.is_empty() {
            return None;
        }
        for (index, byte) in digits.bytes().take(3).enumerate() {
            millis += i64::from(byte - b'0') * [100, 10, 1][index];
        }
    }
    let offset = match bytes.get(offset_index)? {
        b'Z' | b'z' if offset_index + 1 == bytes.len() => 0,
        sign @ (b'+' | b'-')
            if offset_index + 6 == bytes.len() && bytes.get(offset_index + 3) == Some(&b':') =>
        {
            let hours = number(offset_index + 1, offset_index + 3)?;
            let minutes = number(offset_index + 4, offset_index + 6)?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            (hours * 60 + minutes) * 60 * if *sign == b'+' { 1 } else { -1 }
        }
        _ => return None,
    };
    year -= i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let shifted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let days = era * 146097 + year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year
        - 719468;
    Some(((days * 24 + hour) * 60 * 60 + minute * 60 + second - offset) * 1000 + millis)
}

fn parse(bytes: &[u8]) -> Result<Vec<Entry>, HistoryError> {
    let value: Value = serde_json::from_slice(bytes).map_err(|_| HistoryError::Invalid)?;
    let items = value.as_array().ok_or(HistoryError::Invalid)?;
    let mut entries: Vec<_> = items
        .iter()
        .map(|item| {
            let track = parse_track(item).ok_or(HistoryError::Invalid)?;
            Ok(Entry {
                track,
                played_at: field(item, "playedAt").cloned(),
                played_at_ms: field(item, "playedAtUnixMs").and_then(Value::as_u64),
            })
        })
        .collect::<Result<_, HistoryError>>()?;
    entries.sort_by_cached_key(|entry| {
        std::cmp::Reverse(
            entry
                .played_at_ms
                .and_then(|value| i64::try_from(value).ok())
                .or_else(|| {
                    entry
                        .played_at
                        .as_ref()
                        .and_then(Value::as_str)
                        .and_then(legacy_time_ms)
                }),
        )
    });
    let mut seen = HashSet::new();
    entries.retain(|entry| seen.insert(entry.track.id));
    entries.truncate(CAPACITY);
    Ok(entries)
}

fn encode(entries: &[Entry]) -> Result<Vec<u8>, HistoryError> {
    let values: Vec<_> = entries
        .iter()
        .map(|entry| {
            let mut value = track_value(&entry.track);
            let object = value.as_object_mut().expect("track encoding is an object");
            object.remove("Id");
            object.insert("MusicId".into(), json!(entry.track.id));
            if let Some(played_at) = &entry.played_at {
                object.insert("PlayedAt".into(), played_at.clone());
            }
            if let Some(played_at_ms) = entry.played_at_ms {
                object.insert("PlayedAtUnixMs".into(), json!(played_at_ms));
            }
            value
        })
        .collect();
    serde_json::to_vec_pretty(&values).map_err(|_| HistoryError::Invalid)
}

fn backup(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".bak");
    name.into()
}

pub struct HistoryStore {
    paths: DataPaths,
    gate: Mutex<()>,
}

impl HistoryStore {
    pub fn new(paths: DataPaths) -> Self {
        Self {
            paths,
            gate: Mutex::new(()),
        }
    }
    fn path(&self, scope: &str) -> Result<PathBuf, HistoryError> {
        Ok(self.paths.account_dir(scope)?.join("history.json"))
    }
    fn read(path: &Path) -> Result<Vec<Entry>, HistoryError> {
        match atomic::read_limited(path) {
            Ok(bytes) => match parse(&bytes) {
                Ok(entries) => Ok(entries),
                Err(_) => Self::read_backup(path, false),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::read_backup(path, true),
            Err(error) => Err(error.into()),
        }
    }
    fn read_backup(path: &Path, missing: bool) -> Result<Vec<Entry>, HistoryError> {
        match atomic::read_limited(&backup(path)) {
            Ok(bytes) => parse(&bytes),
            Err(error) if missing && error.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Err(HistoryError::Invalid),
            Err(error) => Err(error.into()),
        }
    }
    pub fn load(&self, scope: &str) -> Result<Vec<Track>, HistoryError> {
        let _guard = self.gate.lock().map_err(|_| HistoryError::Poisoned)?;
        Ok(Self::read(&self.path(scope)?)?
            .into_iter()
            .map(|entry| entry.track)
            .collect())
    }
    pub fn record(&self, scope: &str, track: &Track) -> Result<(), HistoryError> {
        if track.id <= 0 || track.title.is_empty() {
            return Err(HistoryError::Invalid);
        }
        let _guard = self.gate.lock().map_err(|_| HistoryError::Poisoned)?;
        let path = self.path(scope)?;
        let mut entries = Self::read(&path)?;
        entries.retain(|entry| entry.track.id != track.id);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|time| u64::try_from(time.as_millis()).ok());
        entries.insert(
            0,
            Entry {
                track: track.clone(),
                played_at: None,
                played_at_ms: now,
            },
        );
        entries.truncate(CAPACITY);
        let bytes = encode(&entries)?;
        match atomic::read_limited(&path) {
            Ok(previous) => {
                // Refuse to overwrite a corrupt original even when its backup can be read.
                parse(&previous)?;
                atomic::replace(&backup(&path), &previous)?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        atomic::replace(&path, &bytes)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
