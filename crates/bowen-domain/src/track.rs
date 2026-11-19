//! Track information independent of a particular API response or UI framework.

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum AudioQuality {
    Standard,
    High,
    Lossless,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AudioVariant {
    pub quality: AudioQuality,
    pub level: String,
    pub format: String,
    pub bitrate_kbps: u32,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrackArtist {
    pub id: i64,
    pub name: String,
    pub image_url: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LyricAvailability {
    pub line: bool,
    pub word: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Track {
    pub id: i64,
    pub title: String,
    pub artist_text: String,
    pub artists: Vec<TrackArtist>,
    pub album_name: Option<String>,
    pub album_id: i64,
    pub cover_url: Option<String>,
    pub duration_seconds: i32,
    /// Quality-only entries are retained for restoring older queue snapshots without variants.
    pub available_qualities: Vec<AudioQuality>,
    pub audio_variants: Vec<AudioVariant>,
    /// These flags are for display only; playback authorization is decided by the service.
    pub requires_vip: bool,
    pub requires_purchase: bool,
    pub lyrics: Option<LyricAvailability>,
    pub has_mv: bool,
    pub comment_count: Option<i64>,
    pub favorite_count: Option<i64>,
    pub share_count: Option<i64>,
}
