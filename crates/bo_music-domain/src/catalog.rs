use crate::track::TrackArtist;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Album {
    pub id: i64,
    pub source_type: i32,
    pub name: String,
    pub artist_text: String,
    pub artist_id: i64,
    pub artists: Vec<TrackArtist>,
    pub cover_url: Option<String>,
    pub music_count: i32,
    pub artist_cover_url: Option<String>,
    pub release_date: String,
    pub description: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Artist {
    pub id: i64,
    pub name: String,
    pub cover_url: Option<String>,
    pub song_count: i32,
    pub album_count: i32,
    pub alias: String,
    pub fans_count: i64,
    pub description: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub music_count: i32,
    pub cover_url: Option<String>,
    pub source_type: i32,
    pub cover_raw_url: String,
    pub is_private: bool,
    pub creator_id: i64,
    pub creator_name: String,
    pub creator_cover_url: Option<String>,
    pub description: String,
    pub play_count: i64,
    pub collected_count: i64,
    pub collect_time: String,
    pub categories: Vec<PlaylistCategory>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlaylistCategory {
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LibraryGroup {
    pub id: String,
    pub name: String,
    pub title: String,
    pub description: String,
    pub cover_url: Option<String>,
    pub album_count: i32,
    pub children: Vec<LibraryChild>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LibraryChild {
    pub id: String,
    pub name: String,
    pub title: String,
    pub description: String,
    pub sample_album: Option<Album>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CategoryGroup {
    pub name: String,
    pub categories: Vec<PlaylistCategory>,
}
