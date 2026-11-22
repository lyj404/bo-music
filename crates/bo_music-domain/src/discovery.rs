use crate::{catalog::Playlist, track::Track};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Chart {
    pub id: i64,
    pub name: String,
    pub cover_url: Option<String>,
    pub update_text: String,
    pub preview_tracks: Vec<Track>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChartSection {
    pub title: String,
    pub charts: Vec<Chart>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HomeModule {
    pub id: i32,
    pub kind: i32,
    pub name: String,
    pub delay_ms: i32,
}

impl HomeModule {
    pub fn is_supported(&self) -> bool {
        matches!(self.kind, 3 | 4 | 5 | 10 | 11)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AiPlaylist {
    pub title: String,
    pub subtitle: String,
    pub big_title: String,
    pub tracks: Vec<Track>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AiPreview {
    pub title: String,
    pub index: usize,
    pub pass_rec_name: String,
    pub tracks: Vec<Track>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HomeContent {
    Tracks(Vec<Track>),
    AiPreviews {
        groups: Vec<AiPreview>,
        mosaic: bool,
    },
    Playlists(Vec<Playlist>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HomeFeed {
    pub title: String,
    pub content: HomeContent,
}
