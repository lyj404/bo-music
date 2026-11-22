//! A single in-process writer for versioned global settings.
use bo_music_domain::queue::PlayMode;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    io,
    path::{Path, PathBuf},
    sync::Mutex,
};
use thiserror::Error;

use crate::atomic;
use crate::shortcuts::Shortcuts;
use bo_music_domain::audio_effects::{AudioEffects, CustomPreset};
use bo_music_domain::track::AudioQuality;

const VERSION: u64 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
pub struct FloatingBounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl FloatingBounds {
    fn valid(self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && (1.0..=16_384.0).contains(&self.width)
            && (1.0..=16_384.0).contains(&self.height)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FloatingWindows {
    pub mini_player: Option<FloatingBounds>,
    pub desktop_lyrics: Option<FloatingBounds>,
    pub lyric_font_size: f32,
    pub lyric_second_line: bool,
    pub lyric_highlight: u32,
    pub lyric_staggered: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhysicalWindowBounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LegacyFloatingWindows {
    pub main: Option<PhysicalWindowBounds>,
    pub mini_player: Option<PhysicalWindowBounds>,
    pub desktop_lyrics: Option<PhysicalWindowBounds>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppearanceSettings {
    pub theme: ThemeMode,
    pub acrylic: bool,
    pub ui_font: String,
    pub lyric_font: String,
    pub mini_lyric_font: String,
    pub desktop_lyric_font: String,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: ThemeMode::System,
            acrylic: false,
            ui_font: String::new(),
            lyric_font: String::new(),
            mini_lyric_font: String::new(),
            desktop_lyric_font: String::new(),
        }
    }
}

impl AppearanceSettings {
    fn normalized(mut self) -> Self {
        for font in [
            &mut self.ui_font,
            &mut self.lyric_font,
            &mut self.mini_lyric_font,
            &mut self.desktop_lyric_font,
        ] {
            *font = font
                .trim()
                .chars()
                .filter(|ch| !ch.is_control())
                .take(128)
                .collect();
        }
        self
    }

    fn from_legacy(state: &Map<String, Value>) -> Self {
        let field = |section, key| {
            case_field(state, section)
                .and_then(Value::as_object)
                .and_then(|section| case_field(section, key))
        };
        let font = |section, key| {
            field(section, key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let theme = match field("appearance", "theme") {
            Some(Value::String(name)) if name.eq_ignore_ascii_case("light") => ThemeMode::Light,
            Some(Value::String(name)) if name.eq_ignore_ascii_case("dark") => ThemeMode::Dark,
            Some(Value::Number(number)) if number.as_u64() == Some(1) => ThemeMode::Light,
            Some(Value::Number(number)) if number.as_u64() == Some(2) => ThemeMode::Dark,
            _ => ThemeMode::System,
        };
        let acrylic = case_field(state, "appearance").is_some_and(|value| value.is_object())
            && field("appearance", "acrylicEffects")
                .and_then(Value::as_bool)
                .unwrap_or(true);
        Self {
            theme,
            acrylic,
            ui_font: font("fonts", "fontFamily"),
            lyric_font: font("lyrics", "fontFamily"),
            mini_lyric_font: font("lyrics", "miniPlayerFontFamily"),
            desktop_lyric_font: font("desktopLyrics", "fontFamily"),
        }
        .normalized()
    }
}

#[derive(Clone, Copy, Default, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PosterTheme {
    #[default]
    Cover,
    Dark,
    Light,
    Warm,
}

#[derive(Clone, Default, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LyricPosterStyle {
    pub theme: PosterTheme,
    pub font_family: String,
    pub background_rgb: Option<u32>,
    pub text_rgb: Option<u32>,
}

impl LyricPosterStyle {
    fn normalized(mut self) -> Self {
        self.background_rgb = self.background_rgb.filter(|rgb| *rgb <= 0xffffff);
        self.text_rgb = self.text_rgb.filter(|rgb| *rgb <= 0xffffff);
        self.font_family = self
            .font_family
            .trim()
            .chars()
            .filter(|ch| !ch.is_control())
            .take(128)
            .collect();
        self
    }

    fn from_legacy(section: &Map<String, Value>) -> Self {
        let is_mode = |key, number, name: &str| {
            case_field(section, key).is_some_and(|value| {
                value.as_u64() == Some(number)
                    || value
                        .as_str()
                        .is_some_and(|value| value.eq_ignore_ascii_case(name))
            })
        };
        let color = |key| {
            case_field(section, key)
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .map(|argb| argb & 0xffffff)
        };
        let background_rgb = if is_mode("backgroundSource", 1, "Preset")
            || is_mode("backgroundSource", 2, "Custom")
        {
            color("backgroundColorArgb")
        } else {
            None
        };
        let text_rgb = if is_mode("textColorMode", 1, "White") {
            Some(0xffffff)
        } else if is_mode("textColorMode", 2, "Black") {
            Some(0x171717)
        } else if is_mode("textColorMode", 3, "Custom") {
            color("textColorArgb")
        } else {
            None
        };
        Self {
            theme: PosterTheme::Cover,
            font_family: case_field(section, "fontFamily")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            background_rgb,
            text_rgb,
        }
        .normalized()
    }
}

impl Default for FloatingWindows {
    fn default() -> Self {
        Self {
            mini_player: None,
            desktop_lyrics: None,
            lyric_font_size: 32.,
            lyric_second_line: true,
            lyric_highlight: 0x00e5bf,
            lyric_staggered: false,
        }
    }
}

impl FloatingWindows {
    fn normalized(mut self) -> Self {
        self.mini_player = self.mini_player.filter(|bounds| bounds.valid());
        self.desktop_lyrics = self.desktop_lyrics.filter(|bounds| bounds.valid());
        self.lyric_font_size = if self.lyric_font_size.is_finite() {
            self.lyric_font_size.clamp(18., 48.)
        } else {
            Self::default().lyric_font_size
        };
        if self.lyric_highlight >= 0xffffff {
            self.lyric_highlight = Self::default().lyric_highlight;
        }
        self
    }

    fn from_legacy(section: &Map<String, Value>) -> Self {
        let defaults = Self::default();
        Self {
            lyric_font_size: case_field(section, "fontSize")
                .and_then(Value::as_f64)
                .map_or(defaults.lyric_font_size, |value| value as f32),
            lyric_second_line: case_field(section, "dualLine")
                .and_then(Value::as_bool)
                .unwrap_or(defaults.lyric_second_line),
            lyric_highlight: case_field(section, "textColorArgb")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .map_or(defaults.lyric_highlight, |argb| argb & 0xffffff),
            lyric_staggered: case_field(section, "alignment").is_some_and(|value| {
                value.as_u64() == Some(1)
                    || value
                        .as_str()
                        .is_some_and(|value| value.eq_ignore_ascii_case("staggered"))
            }),
            ..defaults
        }
    }
}

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("settings I/O failed")]
    Io(#[from] io::Error),
    #[error("settings contain invalid JSON or have an invalid schema")]
    Invalid,
    #[error("settings were written by a newer version (schema {0})")]
    FutureVersion(u64),
    #[error("settings writer lock poisoned")]
    Poisoned,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LoadedFrom {
    Default,
    Primary,
    Backup,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlaybackSettings {
    pub mode: PlayMode,
    pub restore_queue: bool,
}
impl Default for PlaybackSettings {
    fn default() -> Self {
        Self {
            mode: PlayMode::Sequential,
            restore_queue: true,
        }
    }
}

// WinUI's unified settings accept JSON comments and trailing commas. Strip
// those outside strings before handing the result to the strict JSON parser.
fn normalize_legacy_json(bytes: &[u8]) -> Result<Vec<u8>, SettingsError> {
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            output.push(byte);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
        } else if byte == b'"' {
            in_string = true;
            output.push(byte);
            index += 1;
        } else if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            output.push(b' ');
            index += 2;
            while index < bytes.len() && !matches!(bytes[index], b'\n' | b'\r') {
                index += 1;
            }
        } else if byte == b'/' && bytes.get(index + 1) == Some(&b'*') {
            output.push(b' ');
            index += 2;
            while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/') {
                index += 1;
            }
            if index + 1 >= bytes.len() {
                return Err(SettingsError::Invalid);
            }
            index += 2;
        } else {
            output.push(byte);
            index += 1;
        }
    }
    let mut clean = Vec::with_capacity(output.len());
    let mut in_string = false;
    let mut escaped = false;
    for (index, &byte) in output.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
        } else if byte == b'"' {
            in_string = true;
        } else if byte == b',' {
            let next = output[index + 1..]
                .iter()
                .find(|byte| !byte.is_ascii_whitespace());
            if matches!(next, Some(b'}' | b']')) {
                continue;
            }
        }
        clean.push(byte);
    }
    Ok(clean)
}

fn parse(bytes: &[u8]) -> Result<Map<String, Value>, SettingsError> {
    let value: Value = serde_json::from_slice(&normalize_legacy_json(bytes)?)
        .map_err(|_| SettingsError::Invalid)?;
    let object = value.as_object().ok_or(SettingsError::Invalid)?;
    let version = object
        .get("schemaVersion")
        .or_else(|| object.get("SchemaVersion"))
        .and_then(Value::as_u64)
        .filter(|version| *version > 0)
        .unwrap_or(VERSION);
    if version > VERSION {
        return Err(SettingsError::FutureVersion(version));
    }
    Ok(object.clone())
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".bak");
    PathBuf::from(name)
}

fn read(path: &Path) -> Result<Option<Map<String, Value>>, SettingsError> {
    match atomic::read_limited(path) {
        Ok(bytes) => parse(&bytes).map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) if error.kind() == io::ErrorKind::InvalidData => Err(SettingsError::Invalid),
        Err(error) => Err(error.into()),
    }
}

fn apply_standalone_settings(path: &Path, state: &mut Map<String, Value>) {
    use crate::migration::{STANDALONE_SETTINGS, STANDALONE_WINDOWS, regular_file};
    let Some(parent) = path.parent() else { return };
    let root = parent.join("legacy-backup");
    if !std::fs::symlink_metadata(&root).is_ok_and(|meta| meta.file_type().is_dir()) {
        return;
    }
    let read_file = |name| {
        let path = root.join(name);
        if !regular_file(&path).ok()? {
            return None;
        }
        let bytes = atomic::read_limited(&path).ok()?;
        serde_json::from_slice::<Value>(&normalize_legacy_json(&bytes).ok()?).ok()
    };
    for (name, section) in STANDALONE_SETTINGS {
        if case_field(state, section).is_some() {
            continue;
        }
        if let Some(value) = read_file(name) {
            let value = match *section {
                "audioQuality" if value.is_string() || value.is_number() => {
                    json!({ "preferred": value })
                }
                "playback" if value.is_string() || value.is_number() => json!({ "mode": value }),
                _ if value.is_object() => value,
                _ => continue,
            };
            state.insert((*section).into(), value);
        }
    }
    let mut windows = case_field(state, "windows")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let mut changed = false;
    for (name, key) in STANDALONE_WINDOWS {
        if case_field(&windows, key).is_none()
            && let Some(value) = read_file(name).filter(Value::is_object)
        {
            windows.insert((*key).into(), value);
            changed = true;
        }
    }
    if changed {
        state.retain(|key, _| !key.eq_ignore_ascii_case("windows"));
        state.insert("windows".into(), Value::Object(windows));
    }
}

/// Keep one instance per data root. All mutations through this instance are serialized.
pub struct SettingsStore {
    path: PathBuf,
    state: Mutex<Map<String, Value>>,
    loaded_from: LoadedFrom,
}

impl SettingsStore {
    pub fn appearance(&self) -> Result<AppearanceSettings, SettingsError> {
        let state = self.state.lock().map_err(|_| SettingsError::Poisoned)?;
        Ok(state
            .get("rustAppearance")
            .cloned()
            .and_then(|value| serde_json::from_value::<AppearanceSettings>(value).ok())
            .map(AppearanceSettings::normalized)
            .unwrap_or_else(|| AppearanceSettings::from_legacy(&state)))
    }

    pub fn set_appearance(&self, appearance: AppearanceSettings) -> Result<(), SettingsError> {
        let value =
            serde_json::to_value(appearance.normalized()).map_err(|_| SettingsError::Invalid)?;
        self.update(|state| {
            state.insert("rustAppearance".into(), value);
        })
    }
    pub fn main_window_bounds(&self) -> Result<Option<FloatingBounds>, SettingsError> {
        let state = self.state.lock().map_err(|_| SettingsError::Poisoned)?;
        Ok(state
            .get("mainWindowBounds")
            .cloned()
            .and_then(|value| serde_json::from_value::<FloatingBounds>(value).ok())
            .filter(|bounds| bounds.valid()))
    }

    pub fn set_main_window_bounds(&self, bounds: FloatingBounds) -> Result<(), SettingsError> {
        if !bounds.valid() {
            return Err(SettingsError::Invalid);
        }
        self.update(|state| {
            state.insert("mainWindowBounds".into(), json!(bounds));
        })
    }
    pub fn lyric_poster_style(&self) -> Result<LyricPosterStyle, SettingsError> {
        let state = self.state.lock().map_err(|_| SettingsError::Poisoned)?;
        Ok(state
            .get("rustLyricPoster")
            .cloned()
            .and_then(|value| serde_json::from_value::<LyricPosterStyle>(value).ok())
            .map(LyricPosterStyle::normalized)
            .or_else(|| {
                case_field(&state, "lyricPoster")
                    .and_then(Value::as_object)
                    .map(LyricPosterStyle::from_legacy)
            })
            .unwrap_or_default())
    }

    pub fn set_lyric_poster_style(&self, style: LyricPosterStyle) -> Result<(), SettingsError> {
        let value = serde_json::to_value(style.normalized()).map_err(|_| SettingsError::Invalid)?;
        self.update(|state| {
            state.insert("rustLyricPoster".into(), value);
        })
    }
    pub fn legacy_floating_windows(&self) -> Result<LegacyFloatingWindows, SettingsError> {
        let state = self.state.lock().map_err(|_| SettingsError::Poisoned)?;
        let section = case_field(&state, "windows").and_then(Value::as_object);
        let read = |key| {
            let entry = case_field(section?, key)?.as_object()?;
            let bound = |key| i32::try_from(case_field(entry, key)?.as_i64()?).ok();
            let value = PhysicalWindowBounds {
                x: bound("x")?,
                y: bound("y")?,
                width: bound("width")?,
                height: bound("height")?,
            };
            (value.width > 0
                && value.height > 0
                && value.x.checked_add(value.width).is_some()
                && value.y.checked_add(value.height).is_some())
            .then_some(value)
        };
        Ok(LegacyFloatingWindows {
            main: read("main"),
            mini_player: read("miniPlayer"),
            desktop_lyrics: read("desktopLyrics"),
        })
    }

    pub fn shortcuts(&self) -> Result<Shortcuts, SettingsError> {
        let state = self.state.lock().map_err(|_| SettingsError::Poisoned)?;
        Ok(state
            .get("rustShortcuts")
            .and_then(|value| serde_json::from_value::<Shortcuts>(value.clone()).ok())
            .map(Shortcuts::normalized)
            .or_else(|| case_field(&state, "shortcuts").and_then(Shortcuts::from_legacy))
            .unwrap_or_default())
    }

    pub fn set_shortcuts(&self, value: Shortcuts) -> Result<(), SettingsError> {
        let value = serde_json::to_value(value.normalized()).map_err(|_| SettingsError::Invalid)?;
        self.update(|state| {
            state.insert("rustShortcuts".into(), value);
        })
    }
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, SettingsError> {
        let path = path.into();
        let primary = read(&path);
        let missing_primary = matches!(&primary, Ok(None));
        let (mut state, loaded_from) = match primary {
            Ok(Some(value)) => (value, LoadedFrom::Primary),
            Err(SettingsError::FutureVersion(version)) => {
                return Err(SettingsError::FutureVersion(version));
            }
            Err(SettingsError::Io(error)) => return Err(SettingsError::Io(error)),
            Ok(None) | Err(SettingsError::Invalid) => match read(&backup_path(&path)) {
                Ok(Some(value)) => (value, LoadedFrom::Backup),
                Ok(None) if missing_primary => (
                    Map::from_iter([("schemaVersion".into(), json!(VERSION))]),
                    LoadedFrom::Default,
                ),
                Ok(None) | Err(SettingsError::Invalid) => return Err(SettingsError::Invalid),
                Err(error) => return Err(error),
            },
            Err(error) => return Err(error),
        };
        apply_standalone_settings(&path, &mut state);
        Ok(Self {
            path,
            state: Mutex::new(state),
            loaded_from,
        })
    }

    pub fn loaded_from(&self) -> LoadedFrom {
        self.loaded_from
    }

    pub fn snapshot(&self) -> Result<Map<String, Value>, SettingsError> {
        Ok(self
            .state
            .lock()
            .map_err(|_| SettingsError::Poisoned)?
            .clone())
    }

    /// Apply and persist a complete transaction; failed writes leave the in-memory value unchanged.
    pub fn update(
        &self,
        change: impl FnOnce(&mut Map<String, Value>),
    ) -> Result<(), SettingsError> {
        let mut state = self.state.lock().map_err(|_| SettingsError::Poisoned)?;
        let mut next = state.clone();
        change(&mut next);
        next.insert("schemaVersion".into(), json!(VERSION));
        next.remove("SchemaVersion");
        let bytes = serde_json::to_vec_pretty(&next).map_err(|_| SettingsError::Invalid)?;
        match atomic::read_limited(&self.path) {
            Ok(previous) => match parse(&previous) {
                Ok(_) => atomic::replace(&backup_path(&self.path), &previous)?,
                Err(SettingsError::FutureVersion(version)) => {
                    return Err(SettingsError::FutureVersion(version));
                }
                Err(SettingsError::Invalid) => {}
                Err(error) => return Err(error),
            },
            Err(error)
                if error.kind() == io::ErrorKind::NotFound
                    || error.kind() == io::ErrorKind::InvalidData => {}
            Err(error) => return Err(error.into()),
        }
        atomic::replace(&self.path, &bytes)?;
        *state = next;
        Ok(())
    }

    pub fn playback(&self) -> Result<PlaybackSettings, SettingsError> {
        let state = self.state.lock().map_err(|_| SettingsError::Poisoned)?;
        let section = state
            .get("playback")
            .or_else(|| state.get("Playback"))
            .and_then(Value::as_object);
        let get = |name: &str| {
            section.and_then(|s| {
                s.iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case(name))
                    .map(|(_, v)| v)
            })
        };
        let mode = match get("mode") {
            Some(Value::String(value)) if value.eq_ignore_ascii_case("ListLoop") => {
                PlayMode::ListLoop
            }
            Some(Value::String(value)) if value.eq_ignore_ascii_case("SingleLoop") => {
                PlayMode::SingleLoop
            }
            Some(Value::String(value)) if value.eq_ignore_ascii_case("Shuffle") => {
                PlayMode::Shuffle
            }
            Some(Value::Number(value)) if value.as_u64() == Some(1) => PlayMode::ListLoop,
            Some(Value::Number(value)) if value.as_u64() == Some(2) => PlayMode::Shuffle,
            Some(Value::Number(value)) if value.as_u64() == Some(3) => PlayMode::SingleLoop,
            _ => PlayMode::Sequential,
        };
        Ok(PlaybackSettings {
            mode,
            restore_queue: get("restoreQueue").and_then(Value::as_bool).unwrap_or(true),
        })
    }

    pub fn set_playback(&self, value: PlaybackSettings) -> Result<(), SettingsError> {
        let name = match value.mode {
            PlayMode::Sequential => "Sequential",
            PlayMode::ListLoop => "ListLoop",
            PlayMode::SingleLoop => "SingleLoop",
            PlayMode::Shuffle => "Shuffle",
        };
        self.update(|state| {
            let existing = state
                .remove("playback")
                .or_else(|| state.remove("Playback"));
            let mut section = existing
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default();
            section.retain(|key, _| {
                !key.eq_ignore_ascii_case("mode") && !key.eq_ignore_ascii_case("restoreQueue")
            });
            section.insert("mode".into(), json!(name));
            section.insert("restoreQueue".into(), json!(value.restore_queue));
            state.insert("playback".into(), Value::Object(section));
        })
    }

    pub fn volume(&self) -> Result<f64, SettingsError> {
        let state = self.state.lock().map_err(|_| SettingsError::Poisoned)?;
        let section = state
            .get("volume")
            .or_else(|| state.get("Volume"))
            .and_then(Value::as_object);
        Ok(section
            .and_then(|s| s.iter().find(|(key, _)| key.eq_ignore_ascii_case("level")))
            .and_then(|(_, value)| value.as_f64())
            .filter(|value| value.is_finite())
            .unwrap_or(100.0)
            .clamp(0.0, 100.0))
    }

    pub fn floating_windows(&self) -> Result<FloatingWindows, SettingsError> {
        let state = self.state.lock().map_err(|_| SettingsError::Poisoned)?;
        let saved = state
            .get("floatingWindows")
            .cloned()
            .and_then(|value| serde_json::from_value::<FloatingWindows>(value).ok())
            .or_else(|| {
                case_field(&state, "desktopLyrics")
                    .and_then(Value::as_object)
                    .map(FloatingWindows::from_legacy)
            })
            .unwrap_or_default();
        Ok(saved.normalized())
    }

    pub fn set_floating_windows(&self, value: FloatingWindows) -> Result<(), SettingsError> {
        let Value::Object(fields) =
            serde_json::to_value(value.normalized()).map_err(|_| SettingsError::Invalid)?
        else {
            return Err(SettingsError::Invalid);
        };
        self.update(|state| {
            let mut section = state
                .remove("floatingWindows")
                .and_then(|section| section.as_object().cloned())
                .unwrap_or_default();
            section.extend(fields);
            state.insert("floatingWindows".into(), Value::Object(section));
        })
    }

    pub fn preferred_quality(&self) -> Result<AudioQuality, SettingsError> {
        let state = self.state.lock().map_err(|_| SettingsError::Poisoned)?;
        let value = case_field(&state, "audioQuality")
            .and_then(Value::as_object)
            .and_then(|section| case_field(section, "preferred"));
        Ok(match value {
            Some(Value::String(value)) if value.eq_ignore_ascii_case("standard") => {
                AudioQuality::Standard
            }
            Some(Value::String(value)) if value.eq_ignore_ascii_case("high") => AudioQuality::High,
            Some(Value::Number(value)) if value.as_u64() == Some(0) => AudioQuality::Standard,
            Some(Value::Number(value)) if value.as_u64() == Some(1) => AudioQuality::High,
            _ => AudioQuality::Lossless,
        })
    }

    pub fn set_preferred_quality(&self, quality: AudioQuality) -> Result<(), SettingsError> {
        let name = match quality {
            AudioQuality::Standard => "Standard",
            AudioQuality::High => "High",
            AudioQuality::Lossless => "Lossless",
        };
        self.update(|state| {
            let key = state
                .keys()
                .find(|key| key.eq_ignore_ascii_case("audioQuality"))
                .cloned();
            let mut section = key
                .and_then(|key| state.remove(&key))
                .and_then(|value| value.as_object().cloned())
                .unwrap_or_default();
            section.retain(|key, _| !key.eq_ignore_ascii_case("preferred"));
            section.insert("preferred".into(), json!(name));
            state.insert("audioQuality".into(), Value::Object(section));
        })
    }

    pub fn set_volume(&self, volume: f64) -> Result<(), SettingsError> {
        if !volume.is_finite() {
            return Err(SettingsError::Invalid);
        }
        let volume = volume.clamp(0.0, 100.0);
        self.update(|state| {
            let existing = state.remove("volume").or_else(|| state.remove("Volume"));
            let mut section = existing
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default();
            section.retain(|key, _| !key.eq_ignore_ascii_case("level"));
            section.insert("level".into(), json!(volume));
            state.insert("volume".into(), Value::Object(section));
        })
    }

    pub fn audio_effects(&self) -> Result<AudioEffects, SettingsError> {
        let state = self.state.lock().map_err(|_| SettingsError::Poisoned)?;
        let section = case_field(&state, "audioEffects").and_then(Value::as_object);
        let get = |name: &str| section.and_then(|section| case_field(section, name));
        let mut effects = AudioEffects {
            enabled: get("enabled").and_then(Value::as_bool).unwrap_or(false),
            preset_id: get("presetId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            ambient_id: get("ambientId")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            ambient_volume: get("ambientVolume").and_then(Value::as_f64).unwrap_or(30.0),
            custom_presets: get("customPresets")
                .and_then(Value::as_array)
                .map(|presets| {
                    presets
                        .iter()
                        .filter_map(|value| {
                            let item = value.as_object()?;
                            let field = |name| case_field(item, name);
                            let mut bands = [0.0; 10];
                            if let Some(values) = field("bands").and_then(Value::as_array) {
                                for (band, value) in bands.iter_mut().zip(values) {
                                    *band = value.as_f64().unwrap_or(0.0);
                                }
                            }
                            Some(CustomPreset {
                                id: field("id")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .into(),
                                name: field("name")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .into(),
                                description: field("description")
                                    .and_then(Value::as_str)
                                    .unwrap_or_default()
                                    .into(),
                                bands,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
        };
        effects.normalize();
        Ok(effects)
    }

    pub fn set_audio_effects(&self, effects: &AudioEffects) -> Result<(), SettingsError> {
        let mut effects = effects.clone();
        effects.normalize();
        self.update(|state| {
            let key = state.keys().find(|key| key.eq_ignore_ascii_case("audioEffects")).cloned();
            let mut section = key.and_then(|key| state.remove(&key)).and_then(|value| value.as_object().cloned()).unwrap_or_default();
            section.retain(|key, _| !["enabled", "presetId", "ambientId", "ambientVolume", "customPresets"].iter().any(|name| key.eq_ignore_ascii_case(name)));
            section.insert("enabled".into(), json!(effects.enabled));
            section.insert("presetId".into(), json!(effects.preset_id));
            section.insert("ambientId".into(), json!(effects.ambient_id));
            section.insert("ambientVolume".into(), json!(effects.ambient_volume));
            section.insert("customPresets".into(), Value::Array(effects.custom_presets.iter().map(|preset| json!({
                "id": preset.id, "name": preset.name, "description": preset.description, "bands": preset.bands,
            })).collect()));
            state.insert("audioEffects".into(), Value::Object(section));
        })
    }
}

fn case_field<'a>(object: &'a Map<String, Value>, name: &str) -> Option<&'a Value> {
    object
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value)
}
