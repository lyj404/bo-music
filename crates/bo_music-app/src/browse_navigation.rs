//! Navigation and request scheduling for the browser.
use super::*;

impl Browser {
    pub(super) fn request(
        &mut self,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
        request: Request,
    ) {
        self.cancel();
        self.loading = true;
        self.error = None;
        let generation = self.generation;
        let revision = transport.session().snapshot().revision;
        self.task = Some(tokio::spawn(async move {
            let result = match request {
                Request::PlaylistCategories => {
                    library::categories_at_revision(&transport, revision)
                        .await
                        .map(BrowseResult::PlaylistCategories)
                        .map_err(|_| "加载歌单分类失败，请重试".to_owned())
                }
                Request::CategoryPlaylists(id, mut cursor) => {
                    library::category_playlists_at_revision(&transport, id, &mut cursor, revision)
                        .await
                        .map(|page| BrowseResult::Search(cursor, page.items))
                        .map_err(|_| "加载分类歌单失败，请重试".to_owned())
                }
                Request::Charts => discovery::chart_sections(&transport)
                    .await
                    .map(BrowseResult::Charts)
                    .map_err(|_| "加载排行榜失败，请重试".to_owned()),
                Request::Search(kind, query, mut cursor) => match kind {
                    SearchKind::Playlist => search_playlists(&transport, &query, &mut cursor)
                        .await
                        .map(|page| BrowseResult::Search(cursor, page.items))
                        .map_err(|_| "搜索歌单失败，请重试".to_owned()),
                    SearchKind::Album => search_albums(&transport, &query, &mut cursor)
                        .await
                        .map(|page| BrowseResult::Albums(cursor, page.items))
                        .map_err(|_| "搜索专辑失败，请重试".to_owned()),
                    SearchKind::Artist => search_artists(&transport, &query, &mut cursor)
                        .await
                        .map(|page| BrowseResult::Artists(cursor, page.items))
                        .map_err(|_| "搜索歌手失败，请重试".to_owned()),
                },
                Request::Mine => {
                    async {
                        let (liked, created) = tokio::join!(
                            playlist::liked(&transport),
                            playlist::created(&transport)
                        );
                        if liked.is_err() && created.is_err() {
                            return Err(
                                if matches!(liked, Err(playlist::PlaylistError::NotAuthenticated)) {
                                    "请先登录后查看我的歌单".to_owned()
                                } else {
                                    "加载我的歌单失败，请重试".to_owned()
                                },
                            );
                        }
                        let warning = (liked.is_err() || created.is_err())
                            .then(|| "部分歌单加载失败，请重试".to_owned());
                        let liked_id = liked
                            .as_ref()
                            .ok()
                            .and_then(|item| item.as_ref())
                            .map(|item| item.id);
                        let can_edit = liked.is_ok() && created.is_ok();
                        let mut items = Vec::new();
                        for mut item in created.unwrap_or_default() {
                            item.source_type = 5;
                            if Some(item.id) != liked_id
                                && !items
                                    .iter()
                                    .any(|existing: &Playlist| existing.id == item.id)
                            {
                                items.push(item);
                            }
                        }
                        let editable = can_edit.then(|| {
                            items
                                .iter()
                                .filter_map(|item| (item.id > 0).then_some(item.id))
                                .collect()
                        });
                        Ok(BrowseResult::Mine(items, warning, editable))
                    }
                    .await
                }
                Request::Collection(kind, mut cursor) => match kind {
                    SearchKind::Playlist => collection::playlists(&transport, &mut cursor)
                        .await
                        .map(|page| BrowseResult::Search(cursor, page.items))
                        .map_err(|_| "加载收藏歌单失败，请登录后重试".to_owned()),
                    SearchKind::Album => collection::albums(&transport, &mut cursor)
                        .await
                        .map(|page| BrowseResult::Albums(cursor, page.items))
                        .map_err(|_| "加载收藏专辑失败，请登录后重试".to_owned()),
                    SearchKind::Artist => collection::followed_artists(&transport)
                        .await
                        .map(BrowseResult::Following)
                        .map_err(|_| "加载关注歌手失败，请登录后重试".to_owned()),
                },
                Request::ArtistAlbums(id, mut cursor) => {
                    catalog::artist_albums(&transport, id, &mut cursor)
                        .await
                        .map(|page| BrowseResult::ArtistAlbums(cursor, page.items))
                        .map_err(|_| "加载歌手专辑失败，请重试".to_owned())
                }
                Request::Tracks(source, mut cursor) => {
                    fetch_tracks(&transport, source, &mut cursor)
                        .await
                        .map(|items| BrowseResult::Tracks(cursor, items))
                        .map_err(|_| "加载歌曲失败，请重试".to_owned())
                }
                Request::AllTracks(source, mut cursor) => {
                    let mut tracks = Vec::new();
                    let mut warning = None;
                    for _ in 0..20 {
                        if cursor.exhausted() {
                            break;
                        }
                        if transport.session().snapshot().revision != revision {
                            warning = Some("账号已变化，歌曲加载已取消".into());
                            break;
                        }
                        match fetch_tracks(&transport, source, &mut cursor).await {
                            Ok(items) => tracks.extend(items),
                            Err(_) => {
                                warning = Some("部分歌曲加载失败，将使用已加载的歌曲".into());
                                break;
                            }
                        }
                    }
                    if !cursor.exhausted() && warning.is_none() {
                        warning = Some("歌曲尚未加载完，将使用已加载的歌曲".into());
                    }
                    Ok(BrowseResult::AllTracks(cursor, tracks, warning))
                }
            };
            let _ = sender.send(Command::BrowseDone(generation, revision, result));
        }));
    }

    pub fn search(
        &mut self,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
        query: String,
    ) {
        self.search_kind(transport, sender, SearchKind::Playlist, query);
    }
    pub fn search_kind(
        &mut self,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
        kind: SearchKind,
        query: String,
    ) {
        self.clear();
        self.kind = kind;
        let query = query.trim().to_owned();
        if query.is_empty() {
            self.error = Some("请输入搜索内容".into());
            return;
        }
        self.query = Some(query.clone());
        self.request(
            transport,
            sender,
            Request::Search(kind, query, self.list_cursor.clone()),
        );
    }
    pub fn mine(&mut self, transport: Arc<Transport>, sender: mpsc::UnboundedSender<Command>) {
        self.clear();
        self.mine = true;
        self.request(transport, sender, Request::Mine);
    }
    pub fn collection(
        &mut self,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
        kind: SearchKind,
    ) {
        self.clear();
        self.kind = kind;
        self.collection = true;
        self.list_cursor = service_cursor(30);
        self.request(
            transport,
            sender,
            Request::Collection(kind, self.list_cursor.clone()),
        );
    }
    pub fn playlist_square(
        &mut self,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
    ) {
        self.clear();
        self.square = Some(PlaylistSquareView::default());
        self.request(transport, sender, Request::PlaylistCategories);
    }

    pub fn select_playlist_category(
        &mut self,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
        id: i64,
        generation: u64,
    ) -> bool {
        if generation != self.generation || self.selected.is_some() {
            return false;
        }
        let Some(square) = &mut self.square else {
            return false;
        };
        if square.category == Some(id)
            || !square
                .groups
                .iter()
                .flat_map(|group| &group.categories)
                .any(|tag| tag.id == id)
        {
            return false;
        }
        square.category = Some(id);
        self.playlists.clear();
        self.list_cursor = service_cursor(30);
        self.request(
            transport,
            sender,
            Request::CategoryPlaylists(id, self.list_cursor.clone()),
        );
        true
    }

    pub fn charts(&mut self, transport: Arc<Transport>, sender: mpsc::UnboundedSender<Command>) {
        self.clear();
        self.charts_page = true;
        self.request(transport, sender, Request::Charts);
    }
    pub fn open_chart(
        &mut self,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
        chart: Chart,
    ) {
        if self.selected.is_some()
            || !self.charts_page
            || !self
                .charts
                .iter()
                .flat_map(|section| &section.charts)
                .any(|item| item.id == chart.id)
        {
            self.error = Some("榜单不可用".into());
            return;
        }
        self.open_selection(transport, sender, Selection::Chart(chart));
    }
    pub fn open(
        &mut self,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
        playlist: Playlist,
    ) {
        if playlist.id <= 0
            || !self
                .playlists
                .iter()
                .any(|entry| entry.id == playlist.id && entry.source_type == playlist.source_type)
        {
            self.error = Some("歌单不可用".into());
            return;
        }
        self.cancel();
        self.cancel_details();
        self.tracks.clear();
        self.track_cursor = service_cursor(30);
        self.selected = Some(Selection::Playlist(playlist));
        let source = self.selected.as_ref().expect("selection exists").source();
        self.request(
            transport,
            sender,
            Request::Tracks(source, self.track_cursor.clone()),
        );
    }

    /// Open a playlist validated by another account-scoped catalog (for example discovery).
    pub fn open_external(
        &mut self,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
        playlist: Playlist,
    ) {
        if playlist.id <= 0 {
            return;
        }
        self.clear();
        self.playlists.push(playlist.clone());
        self.open(transport, sender, playlist);
    }
    pub fn open_album(
        &mut self,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
        album: Album,
    ) {
        let from_artist =
            matches!(self.selected, Some(Selection::Artist(_))) && self.artist_albums_active;
        let valid = if from_artist {
            self.artist_albums.iter().any(|item| item.id == album.id)
        } else {
            self.selected.is_none() && self.albums.iter().any(|item| item.id == album.id)
        };
        if album.id <= 0 || !valid {
            self.error = Some("专辑不可用".into());
            return;
        }
        if from_artist {
            let Some(Selection::Artist(artist)) = self.selected.take() else {
                return;
            };
            self.artist_parent = Some(ArtistReturn {
                artist,
                tracks: std::mem::take(&mut self.tracks),
                track_cursor: self.track_cursor.clone(),
                albums: std::mem::take(&mut self.artist_albums),
                albums_cursor: self.artist_albums_cursor.clone(),
            });
        }
        self.open_selection(transport, sender, Selection::Album(album));
    }

    /// Open an album already validated by another catalog view.
    pub fn open_external_album(
        &mut self,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
        album: Album,
    ) {
        if album.id <= 0 {
            return;
        }
        self.clear();
        self.albums.push(album.clone());
        self.open_album(transport, sender, album);
    }
    pub fn open_artist(
        &mut self,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
        artist: Artist,
    ) {
        if artist.id <= 0
            || self.selected.is_some()
            || !self.artists.iter().any(|item| item.id == artist.id)
        {
            self.error = Some("歌手不可用".into());
            return;
        }
        self.open_selection(transport, sender, Selection::Artist(artist));
    }

    pub fn open_external_artist(
        &mut self,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
        artist: Artist,
    ) {
        if artist.id > 0 {
            self.clear();
            self.artists.push(artist.clone());
            self.open_artist(transport, sender, artist);
        }
    }
    fn open_selection(
        &mut self,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
        selected: Selection,
    ) {
        self.cancel();
        self.cancel_details();
        self.tracks.clear();
        self.track_cursor = service_cursor(30);
        self.artist_albums.clear();
        self.artist_albums_cursor = service_cursor(30);
        self.artist_albums_active = false;
        let source = selected.source();
        self.selected = Some(selected);
        self.request(
            transport,
            sender,
            Request::Tracks(source, self.track_cursor.clone()),
        );
    }
    pub fn back(&mut self) {
        self.cancel();
        self.cancel_details();
        if let Some(parent) = self.artist_parent.take() {
            self.selected = Some(Selection::Artist(parent.artist));
            self.tracks = parent.tracks;
            self.track_cursor = parent.track_cursor;
            self.artist_albums = parent.albums;
            self.artist_albums_cursor = parent.albums_cursor;
            self.artist_albums_active = true;
        } else {
            self.selected = None;
            self.tracks.clear();
            self.artist_albums.clear();
            self.artist_albums_cursor = service_cursor(30);
            self.artist_albums_active = false;
        }
        self.error = None;
    }

    pub fn show_artist_albums(
        &mut self,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
    ) {
        let Some(Selection::Artist(artist)) = &self.selected else {
            return;
        };
        let id = artist.id;
        if self.artist_albums_active {
            return;
        }
        self.cancel();
        self.artist_albums_active = true;
        self.error = None;
        if self.artist_albums.is_empty() && !self.artist_albums_cursor.exhausted() {
            self.request(
                transport,
                sender,
                Request::ArtistAlbums(id, self.artist_albums_cursor.clone()),
            );
        }
    }

    pub fn show_artist_tracks(
        &mut self,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
    ) {
        let Some(Selection::Artist(artist)) = &self.selected else {
            return;
        };
        let id = artist.id;
        if !self.artist_albums_active {
            return;
        }
        self.cancel();
        self.artist_albums_active = false;
        self.error = None;
        if self.tracks.is_empty() && !self.track_cursor.exhausted() {
            self.request(
                transport,
                sender,
                Request::Tracks(TrackSource::Artist(id), self.track_cursor.clone()),
            );
        }
    }
    /// Load remaining pages before replacing the queue; failed pages leave a retryable cursor.
    pub fn play_all(&mut self, transport: Arc<Transport>, sender: mpsc::UnboundedSender<Command>) {
        if self.loading || self.artist_albums_active || self.tracks.is_empty() {
            return;
        }
        let Some(selected) = &self.selected else {
            return;
        };
        if self.track_cursor.exhausted() {
            self.play_all_ready = Some(self.tracks.clone());
            return;
        }
        let request = Request::AllTracks(selected.source(), self.track_cursor.clone());
        self.request(transport, sender, request);
        self.play_all_pending = true;
    }
    pub fn cancel_play_all(&mut self) -> bool {
        if !self.play_all_pending {
            return false;
        }
        self.cancel_pending();
        true
    }
    pub fn take_play_all(&mut self) -> Option<Vec<Track>> {
        self.play_all_ready.take()
    }
}
