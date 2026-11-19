//! Catalog search and detail navigation, independent of the desktop toolkit.
use crate::desktop_service::Command;
use crate::playback::PlaybackOrigin;
use bowen_api::{
    catalog::{self, service_cursor},
    collection, discovery, library, playlist,
    search::{search_albums, search_artists, search_cursor, search_playlists},
    sharing::ShareTarget,
    telemetry::CollectionKind,
    transport::Transport,
};
use bowen_domain::{
    catalog::{Album, Artist, CategoryGroup, Playlist},
    discovery::{Chart, ChartSection},
    paging::PagedCursor,
    track::Track,
};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::mpsc;

#[path = "browse_collection.rs"]
mod collection_actions;
#[path = "browse_navigation.rs"]
mod navigation;

#[derive(Clone)]
pub enum BrowseContent {
    Charts(Vec<ChartSection>),
    Playlists(Vec<Playlist>),
    Albums(Vec<Album>),
    Artists(Vec<Artist>),
    Tracks(Vec<Track>),
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum SearchKind {
    #[default]
    Playlist,
    Album,
    Artist,
}

#[derive(Clone, Copy)]
enum TrackSource {
    Playlist(i64, i32),
    Album(i64),
    Artist(i64),
    Chart(i64),
}

enum Selection {
    Playlist(Playlist),
    Album(Album),
    Artist(Artist),
    Chart(Chart),
}

impl Selection {
    fn title(&self) -> &str {
        match self {
            Self::Playlist(item) => &item.name,
            Self::Album(item) => &item.name,
            Self::Artist(item) => &item.name,
            Self::Chart(item) => &item.name,
        }
    }
    fn source(&self) -> TrackSource {
        match self {
            Self::Playlist(item) => TrackSource::Playlist(
                item.id,
                if item.source_type > 0 {
                    item.source_type
                } else {
                    4
                },
            ),
            Self::Album(item) => TrackSource::Album(item.id),
            Self::Artist(item) => TrackSource::Artist(item.id),
            Self::Chart(item) => TrackSource::Chart(item.id),
        }
    }
}

async fn fetch_tracks(
    transport: &Transport,
    source: TrackSource,
    cursor: &mut PagedCursor,
) -> Result<Vec<Track>, ()> {
    match source {
        TrackSource::Playlist(id, source) => playlist::tracks(transport, id, source, cursor)
            .await
            .map(|page| page.items)
            .map_err(|_| ()),
        TrackSource::Album(id) => catalog::album_tracks(transport, id, cursor)
            .await
            .map(|page| page.items)
            .map_err(|_| ()),
        TrackSource::Artist(id) => catalog::artist_tracks(transport, id, cursor)
            .await
            .map(|page| page.items)
            .map_err(|_| ()),
        TrackSource::Chart(id) => discovery::chart_tracks(transport, id, cursor)
            .await
            .map(|page| page.items)
            .map_err(|_| ()),
    }
}

#[derive(Clone)]
pub enum DetailView {
    Album(Album),
    Artist(Artist),
}

pub enum DetailResult {
    Album(Option<Album>),
    Artist(Option<Artist>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum CollectionTarget {
    Playlist(i64, i32),
    Album(i64),
    Artist(i64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CollectionState {
    Hidden,
    LoginRequired,
    Loading,
    Owned,
    Ready(bool),
    Writing,
    Unknown,
}

#[derive(Clone)]
pub struct BrowseView {
    pub square: Option<PlaylistSquareView>,
    pub generation: u64,
    pub title: String,
    pub cover_url: Option<String>,
    pub content: BrowseContent,
    pub loading: bool,
    pub has_more: bool,
    pub message: String,
    pub error: bool,
    pub is_detail: bool,
    pub artist_page: bool,
    pub artist_albums_active: bool,
    pub detail: Option<DetailView>,
    pub detail_loading: bool,
    pub detail_error: Option<String>,
    pub collection_state: CollectionState,
    pub my_playlists: bool,
    pub editable_playlist: Option<i64>,
    pub selected_playlist_id: Option<i64>,
    pub can_share: bool,
}

#[derive(Clone, Default)]
pub struct PlaylistSquareView {
    pub groups: Vec<CategoryGroup>,
    pub category: Option<i64>,
}

pub enum BrowseResult {
    PlaylistCategories(Vec<CategoryGroup>),
    Charts(Vec<ChartSection>),
    Search(PagedCursor, Vec<Playlist>),
    Albums(PagedCursor, Vec<Album>),
    Artists(PagedCursor, Vec<Artist>),
    ArtistAlbums(PagedCursor, Vec<Album>),
    Mine(Vec<Playlist>, Option<String>, Option<HashSet<i64>>),
    Following(Vec<Artist>),
    Tracks(PagedCursor, Vec<Track>),
    AllTracks(PagedCursor, Vec<Track>, Option<String>),
}

enum Request {
    PlaylistCategories,
    CategoryPlaylists(i64, PagedCursor),
    Charts,
    Search(SearchKind, String, PagedCursor),
    Mine,
    Collection(SearchKind, PagedCursor),
    ArtistAlbums(i64, PagedCursor),
    Tracks(TrackSource, PagedCursor),
    AllTracks(TrackSource, PagedCursor),
}

pub struct Browser {
    square: Option<PlaylistSquareView>,
    generation: u64,
    task: Option<tokio::task::JoinHandle<()>>,
    detail_generation: u64,
    detail_task: Option<tokio::task::JoinHandle<()>>,
    detail_error: Option<String>,
    collection_generation: u64,
    collection_task: Option<tokio::task::JoinHandle<()>>,
    collection_state: CollectionState,
    collection_state_revision: Option<u64>,
    collection_writes: HashSet<(u64, CollectionTarget)>,
    query: Option<String>,
    kind: SearchKind,
    mine: bool,
    mine_revision: Option<u64>,
    editable_playlists: Option<HashSet<i64>>,
    collection: bool,
    charts_page: bool,
    charts: Vec<ChartSection>,
    playlists: Vec<Playlist>,
    albums: Vec<Album>,
    artists: Vec<Artist>,
    list_cursor: PagedCursor,
    selected: Option<Selection>,
    tracks: Vec<Track>,
    track_cursor: PagedCursor,
    artist_albums: Vec<Album>,
    artist_albums_cursor: PagedCursor,
    artist_albums_active: bool,
    artist_parent: Option<ArtistReturn>,
    loading: bool,
    error: Option<String>,
    play_all_pending: bool,
    play_all_ready: Option<Vec<Track>>,
}

struct ArtistReturn {
    artist: Artist,
    tracks: Vec<Track>,
    track_cursor: PagedCursor,
    albums: Vec<Album>,
    albums_cursor: PagedCursor,
}

impl Default for Browser {
    fn default() -> Self {
        Self {
            square: None,
            generation: 0,
            task: None,
            detail_generation: 0,
            detail_task: None,
            detail_error: None,
            collection_generation: 0,
            collection_task: None,
            collection_state: CollectionState::Hidden,
            collection_state_revision: None,
            collection_writes: HashSet::new(),
            query: None,
            kind: SearchKind::default(),
            mine: false,
            mine_revision: None,
            editable_playlists: None,
            collection: false,
            charts_page: false,
            charts: Vec::new(),
            playlists: Vec::new(),
            albums: Vec::new(),
            artists: Vec::new(),
            list_cursor: search_cursor(30),
            selected: None,
            tracks: Vec::new(),
            track_cursor: service_cursor(30),
            artist_albums: Vec::new(),
            artist_albums_cursor: service_cursor(30),
            artist_albums_active: false,
            artist_parent: None,
            loading: false,
            error: None,
            play_all_pending: false,
            play_all_ready: None,
        }
    }
}

impl Browser {
    pub fn share_target(&self, generation: u64) -> Option<(ShareTarget, i64)> {
        if generation != self.generation {
            return None;
        }
        let (target, id) = match self.selected.as_ref()? {
            Selection::Playlist(item) => (
                ShareTarget::Playlist {
                    source: if item.source_type > 0 {
                        item.source_type
                    } else {
                        4
                    },
                },
                item.id,
            ),
            Selection::Album(item) => (ShareTarget::Album, item.id),
            Selection::Artist(item) => (ShareTarget::Artist, item.id),
            Selection::Chart(_) => return None,
        };
        (id > 0).then_some((target, id))
    }
    pub fn is_mine_list(&self, revision: u64) -> bool {
        self.showing_mine_list(revision) && !self.loading
    }
    pub fn showing_mine_list(&self, revision: u64) -> bool {
        self.mine && self.selected.is_none() && self.mine_revision == Some(revision)
    }
    pub fn add_created_playlist(
        &mut self,
        revision: u64,
        id: i64,
        name: &str,
        private: bool,
    ) -> bool {
        if id <= 0 || !self.is_mine_list(revision) {
            return false;
        }
        if self.playlists.iter().any(|item| item.id == id) {
            return false;
        }
        self.playlists.push(Playlist {
            id,
            name: name.into(),
            is_private: private,
            source_type: 5,
            ..Default::default()
        });
        if let Some(editable) = self.editable_playlists.as_mut() {
            editable.insert(id);
        }
        true
    }
    pub fn editable_playlist(&self, id: i64, revision: u64) -> bool {
        self.mine
            && !self.loading
            && self.mine_revision == Some(revision)
            && matches!(&self.selected, Some(Selection::Playlist(item)) if item.id == id)
            && self
                .editable_playlists
                .as_ref()
                .is_some_and(|ids| ids.contains(&id))
    }
    pub fn remove_deleted_playlist(&mut self, id: i64, revision: u64) -> bool {
        if !self.mine || self.mine_revision != Some(revision) {
            return false;
        }
        let before = self.playlists.len();
        self.playlists.retain(|item| item.id != id);
        if let Some(ids) = self.editable_playlists.as_mut() {
            ids.remove(&id);
        }
        if matches!(&self.selected, Some(Selection::Playlist(item)) if item.id == id) {
            self.back();
        }
        before != self.playlists.len()
    }
    /// Refresh the current playlist after a write, since deleting an item shifts pages.
    pub fn reload_playlist(
        &mut self,
        id: i64,
        revision: u64,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
    ) -> bool {
        if transport.session().snapshot().revision != revision
            || !matches!(&self.selected, Some(Selection::Playlist(item)) if item.id == id)
        {
            return false;
        }
        let Some(selected) = self.selected.as_ref() else {
            return false;
        };
        let source = selected.source();
        self.tracks.clear();
        self.track_cursor = service_cursor(30);
        self.request(
            transport,
            sender,
            Request::Tracks(source, self.track_cursor.clone()),
        );
        true
    }
    pub fn update_edited_playlist(
        &mut self,
        id: i64,
        revision: u64,
        name: &str,
        description: &str,
        cover_url: Option<&str>,
    ) -> bool {
        if !self.mine || self.mine_revision != Some(revision) {
            return false;
        }
        let mut changed = false;
        for item in &mut self.playlists {
            if item.id == id {
                item.name = name.into();
                item.description = description.into();
                if let Some(cover) = cover_url {
                    item.cover_raw_url = cover.into();
                    if let Ok(url) = reqwest::Url::parse(cover)
                        && matches!(url.scheme(), "http" | "https")
                        && url.username().is_empty()
                        && !url
                            .as_str()
                            .split('/')
                            .nth(2)
                            .is_some_and(|authority| authority.contains('@'))
                    {
                        item.cover_url = Some(url.into());
                    }
                }
                changed = true;
            }
        }
        if let Some(Selection::Playlist(item)) = &mut self.selected
            && item.id == id
        {
            item.name = name.into();
            item.description = description.into();
            if let Some(cover) = cover_url {
                item.cover_raw_url = cover.into();
                if let Ok(url) = reqwest::Url::parse(cover)
                    && matches!(url.scheme(), "http" | "https")
                    && url.username().is_empty()
                    && !url
                        .as_str()
                        .split('/')
                        .nth(2)
                        .is_some_and(|authority| authority.contains('@'))
                {
                    item.cover_url = Some(url.into());
                }
            }
            changed = true;
        }
        changed
    }
    pub fn playback_origin(&self) -> Option<PlaybackOrigin> {
        let origin = match &self.selected {
            Some(Selection::Playlist(playlist)) => PlaybackOrigin {
                kind: CollectionKind::Playlist,
                id: playlist.id,
                source_type: if playlist.source_type > 0 {
                    playlist.source_type
                } else {
                    4
                },
            },
            Some(Selection::Album(album)) => PlaybackOrigin {
                kind: CollectionKind::Album,
                id: album.id,
                source_type: album.source_type,
            },
            _ => return None,
        };
        (origin.id > 0 && origin.source_type > 0).then_some(origin)
    }
    pub fn playable_track(&self, index: usize, expected_id: i64, generation: u64) -> Option<Track> {
        if generation != self.generation || self.selected.is_none() || self.artist_albums_active {
            return None;
        }
        self.tracks
            .get(index)
            .filter(|track| track.id == expected_id && expected_id > 0)
            .cloned()
    }
    fn cancel_details(&mut self) {
        self.detail_generation = self.detail_generation.wrapping_add(1);
        if let Some(task) = self.detail_task.take() {
            task.abort();
        }
        self.detail_error = None;
    }
    fn cancel_collection(&mut self) {
        self.collection_generation = self.collection_generation.wrapping_add(1);
        if let Some(task) = self.collection_task.take() {
            task.abort();
        }
        self.collection_state = CollectionState::Hidden;
        self.collection_state_revision = None;
    }
    fn cancel(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        if let Some(task) = self.task.take() {
            task.abort();
        }
        self.loading = false;
        self.play_all_pending = false;
        self.play_all_ready = None;
    }
    /// Cancel outstanding account-bound requests while keeping visible data.
    pub fn cancel_pending(&mut self) {
        let was_loading = self.loading;
        let details_were_loading = self.detail_task.is_some();
        self.cancel();
        self.cancel_details();
        self.cancel_collection();
        if was_loading {
            self.error = Some("加载已取消，请重试".into());
        }
        if details_were_loading {
            self.detail_error = Some("详情加载已取消，请重试".into());
        }
    }
    pub fn clear(&mut self) {
        self.cancel();
        self.cancel_details();
        self.cancel_collection();
        let collection_writes = std::mem::take(&mut self.collection_writes);
        *self = Self {
            generation: self.generation,
            detail_generation: self.detail_generation,
            collection_generation: self.collection_generation,
            collection_writes,
            ..Default::default()
        };
    }

    pub fn load_details(
        &mut self,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
    ) {
        let request = match &self.selected {
            Some(Selection::Album(album)) => (false, album.id),
            Some(Selection::Artist(artist)) => (true, artist.id),
            _ => return,
        };
        self.cancel_details();
        let generation = self.detail_generation;
        let revision = transport.session().snapshot().revision;
        self.detail_task = Some(tokio::spawn(async move {
            let result = if request.0 {
                catalog::artist(&transport, request.1)
                    .await
                    .map(DetailResult::Artist)
            } else {
                catalog::album(&transport, request.1)
                    .await
                    .map(DetailResult::Album)
            }
            .map_err(|_| "详情加载失败，请重试".to_owned());
            let _ = sender.send(Command::DetailDone(generation, revision, result));
        }));
    }

    pub fn finish_details(
        &mut self,
        generation: u64,
        revision: u64,
        current_revision: u64,
        result: Result<DetailResult, String>,
    ) -> bool {
        if generation != self.detail_generation || revision != current_revision {
            return false;
        }
        self.detail_task = None;
        match (result, &mut self.selected) {
            (Ok(DetailResult::Album(Some(details))), Some(Selection::Album(album)))
                if details.id == album.id =>
            {
                *album = details
            }
            (Ok(DetailResult::Artist(Some(details))), Some(Selection::Artist(artist)))
                if details.id == artist.id =>
            {
                *artist = details
            }
            (Ok(DetailResult::Album(None) | DetailResult::Artist(None)), _) => {
                self.detail_error = Some("暂无详情资料".into())
            }
            (Err(error), _) => self.detail_error = Some(error),
            _ => self.detail_error = Some("详情资料不匹配，请重试".into()),
        }
        true
    }

    pub fn next(&mut self, transport: Arc<Transport>, sender: mpsc::UnboundedSender<Command>) {
        if self.loading {
            return;
        }
        let request = if self.artist_albums_active {
            let Some(Selection::Artist(artist)) = &self.selected else {
                return;
            };
            if self.artist_albums_cursor.exhausted() {
                return;
            }
            Request::ArtistAlbums(artist.id, self.artist_albums_cursor.clone())
        } else if let Some(selected) = &self.selected {
            if self.track_cursor.exhausted() {
                return;
            }
            Request::Tracks(selected.source(), self.track_cursor.clone())
        } else if self.charts_page {
            if self.error.is_none() {
                return;
            }
            Request::Charts
        } else if let Some(square) = &self.square {
            match square.category {
                Some(id) if !self.list_cursor.exhausted() => {
                    Request::CategoryPlaylists(id, self.list_cursor.clone())
                }
                None if self.error.is_some() => Request::PlaylistCategories,
                _ => return,
            }
        } else if self.mine {
            if self.error.is_none() {
                return;
            }
            Request::Mine
        } else if self.collection {
            if self.kind == SearchKind::Artist && self.error.is_none() {
                return;
            }
            if self.kind != SearchKind::Artist && self.list_cursor.exhausted() {
                return;
            }
            Request::Collection(self.kind, self.list_cursor.clone())
        } else if let Some(query) = &self.query {
            if self.list_cursor.exhausted() {
                return;
            }
            Request::Search(self.kind, query.clone(), self.list_cursor.clone())
        } else {
            return;
        };
        self.request(transport, sender, request);
    }
    pub fn finish(
        &mut self,
        generation: u64,
        revision: u64,
        current_revision: u64,
        result: Result<BrowseResult, String>,
    ) -> bool {
        if generation != self.generation || revision != current_revision {
            return false;
        }
        self.task = None;
        self.loading = false;
        match result {
            Ok(BrowseResult::PlaylistCategories(groups)) => {
                if let Some(square) = &mut self.square {
                    square.groups = groups;
                }
            }
            Ok(BrowseResult::Charts(items)) => self.charts = items,
            Ok(BrowseResult::Search(cursor, items)) => {
                self.list_cursor = cursor;
                self.playlists.extend(items);
            }
            Ok(BrowseResult::Albums(cursor, items)) => {
                self.list_cursor = cursor;
                self.albums.extend(items);
            }
            Ok(BrowseResult::Artists(cursor, items)) => {
                self.list_cursor = cursor;
                self.artists.extend(items);
            }
            Ok(BrowseResult::ArtistAlbums(cursor, items)) => {
                self.artist_albums_cursor = cursor;
                self.artist_albums.extend(items);
            }
            Ok(BrowseResult::Mine(items, warning, editable)) => {
                self.playlists = items;
                self.error = warning;
                self.mine_revision = Some(revision);
                self.editable_playlists = editable;
            }
            Ok(BrowseResult::Following(items)) => self.artists = items,
            Ok(BrowseResult::Tracks(cursor, items)) => {
                self.track_cursor = cursor;
                self.tracks.extend(items);
            }
            Ok(BrowseResult::AllTracks(cursor, items, warning)) => {
                self.track_cursor = cursor;
                self.tracks.extend(items);
                self.error = warning;
                self.play_all_pending = false;
                if !self.tracks.is_empty() {
                    self.play_all_ready = Some(self.tracks.clone());
                }
            }
            Err(error) => {
                self.play_all_pending = false;
                self.error = Some(error);
            }
        }
        true
    }
    pub fn view(&self) -> BrowseView {
        let artist_page = matches!(self.selected, Some(Selection::Artist(_)));
        let is_detail = self.selected.is_some();
        let (title, content, has_more) = if artist_page && self.artist_albums_active {
            (
                self.selected
                    .as_ref()
                    .expect("artist selected")
                    .title()
                    .to_owned(),
                BrowseContent::Albums(self.artist_albums.clone()),
                !self.artist_albums_cursor.exhausted(),
            )
        } else if let Some(selected) = &self.selected {
            (
                selected.title().to_owned(),
                BrowseContent::Tracks(self.tracks.clone()),
                !self.track_cursor.exhausted(),
            )
        } else if self.charts_page {
            (
                "排行榜".into(),
                BrowseContent::Charts(self.charts.clone()),
                self.error.is_some(),
            )
        } else {
            (
                if self.square.is_some() {
                    "歌单广场".into()
                } else if self.mine {
                    "我的歌单".into()
                } else if self.collection {
                    match self.kind {
                        SearchKind::Playlist => "收藏的歌单",
                        SearchKind::Album => "收藏的专辑",
                        SearchKind::Artist => "关注的歌手",
                    }
                    .into()
                } else {
                    match self.kind {
                        SearchKind::Playlist => "歌单搜索",
                        SearchKind::Album => "专辑搜索",
                        SearchKind::Artist => "歌手搜索",
                    }
                    .into()
                },
                match self.kind {
                    SearchKind::Playlist => BrowseContent::Playlists(self.playlists.clone()),
                    SearchKind::Album => BrowseContent::Albums(self.albums.clone()),
                    SearchKind::Artist => BrowseContent::Artists(self.artists.clone()),
                },
                if let Some(square) = &self.square {
                    self.error.is_some()
                        || square.category.is_some() && !self.list_cursor.exhausted()
                } else if self.mine {
                    self.error.is_some()
                } else if self.collection {
                    if self.kind == SearchKind::Artist {
                        self.error.is_some()
                    } else {
                        !self.list_cursor.exhausted()
                    }
                } else {
                    self.query.is_some() && !self.list_cursor.exhausted()
                },
            )
        };
        let message = if self.loading {
            "正在加载…".into()
        } else if let Some(error) = &self.error {
            error.clone()
        } else if !is_detail
            && let Some(square) = &self.square
            && square.category.is_none()
        {
            if square.groups.is_empty() {
                "暂无歌单分类".into()
            } else {
                "选择分类浏览歌单".into()
            }
        } else {
            match &content {
                BrowseContent::Charts(items) if items.is_empty() => "暂无榜单".into(),
                BrowseContent::Playlists(items) if items.is_empty() => "暂无歌单".into(),
                BrowseContent::Albums(items) if items.is_empty() => "暂无专辑".into(),
                BrowseContent::Artists(items) if items.is_empty() => "暂无歌手".into(),
                BrowseContent::Tracks(tracks) if tracks.is_empty() => "暂无歌曲".into(),
                BrowseContent::Tracks(tracks) if has_more => format!("已加载 {} 首", tracks.len()),
                _ => String::new(),
            }
        };
        BrowseView {
            square: self.square.clone(),
            generation: self.generation,
            title,
            cover_url: match &self.selected {
                Some(Selection::Playlist(item)) => item.cover_url.clone(),
                Some(Selection::Album(item)) => item.cover_url.clone(),
                Some(Selection::Artist(item)) => item.cover_url.clone(),
                Some(Selection::Chart(item)) => item.cover_url.clone(),
                None => None,
            },
            content,
            loading: self.loading,
            has_more,
            message,
            error: self.error.is_some(),
            is_detail,
            artist_page,
            artist_albums_active: self.artist_albums_active,
            detail: match &self.selected {
                Some(Selection::Album(album)) => Some(DetailView::Album(album.clone())),
                Some(Selection::Artist(artist)) => Some(DetailView::Artist(artist.clone())),
                _ => None,
            },
            detail_loading: self.detail_task.is_some(),
            detail_error: self.detail_error.clone(),
            collection_state: self.collection_state,
            my_playlists: self.mine && self.selected.is_none(),
            editable_playlist: match &self.selected {
                Some(Selection::Playlist(item))
                    if self.mine
                        && !self.loading
                        && self
                            .editable_playlists
                            .as_ref()
                            .is_some_and(|ids| ids.contains(&item.id)) =>
                {
                    Some(item.id)
                }
                _ => None,
            },
            selected_playlist_id: match &self.selected {
                Some(Selection::Playlist(item)) => Some(item.id),
                _ => None,
            },
            can_share: self.share_target(self.generation).is_some(),
        }
    }
}

#[cfg(test)]
#[path = "browse_tests.rs"]
mod tests;
