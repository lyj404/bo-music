use super::*;

impl MusicApp {
    pub(super) fn accept_event(&mut self, event: Event, cx: &mut Context<Self>) {
        let this = self;
        let modal_was_open = this.playlist_editor.dialog != PlaylistDialog::Closed;
        match event {
            Event::Appearance(value) => this.apply_appearance(value, cx),
            Event::AppearanceSaved(saved) => this.appearance_page.error = !saved,
            Event::LyricPosterStyle(style) => this.lyric_poster.apply_style(style, cx),
            Event::Shortcuts(value) => this.shortcuts.bindings = value,
            Event::ShortcutsSaved(false) => {
                this.shortcuts.message = Some("快捷键未能写入配置".into())
            }
            Event::ShortcutsSaved(true) => {}
            Event::Mv(view) => this.accept_mv(view, cx),
            Event::CommentImage(request, result) => {
                this.comment_panel.image.accept(request, result);
            }
            Event::CommentImageSaved(request, saved) => {
                this.comment_panel.image.saved(request, saved);
            }
            Event::MonthPoster(generation, request, image) => {
                this.month_page.accept_poster(generation, request, image);
            }
            Event::MonthPosterSaved(generation, request, saved) => {
                this.month_page
                    .accept_poster_saved(generation, request, saved);
            }
            Event::MonthReport(generation, result) => {
                if !this.auth.busy && this.auth.account.is_some() {
                    this.month_page.accept_report(generation, result);
                }
            }
            Event::MonthAvailability(generation, period, status) => {
                this.month_page
                    .accept_availability(generation, period, status);
            }
            Event::MonthAvailabilityFinished(generation) => {
                this.month_page.finish_probe(generation);
            }
            Event::MonthPlaylist(period, created) => {
                if !this.auth.busy && this.auth.account.is_some() {
                    this.month_page.accept_playlist(period, created);
                }
            }
            Event::AccountOverview(generation, result) => {
                if generation == this.account_overview_generation
                    && !this.auth.busy
                    && this.auth.account.is_some()
                {
                    this.account_overview = Some(result);
                }
            }
            Event::SearchOverview(generation, result) => {
                if generation == this.search.overview_generation && !this.auth.busy {
                    this.search.overview_result = Some(result);
                }
            }
            Event::SearchOverviewOpened(generation) => {
                if generation == this.search.overview_generation
                    && this.navigation.page == Page::Overview
                    && !this.auth.busy
                {
                    this.navigation.open_detail(Page::Overview);
                }
            }
            Event::SearchSuggestions(generation, words) => {
                if generation == this.search.suggestion_generation && !this.auth.busy {
                    this.search.suggestions = words;
                }
            }
            Event::SearchHotWords(words) => {
                this.search.hot_words_loading = false;
                this.search.hot_words = words;
            }
            Event::SearchHistory(result) => {
                this.search.history = Some(result);
            }
            Event::FloatingWindows(settings) => this.floating_windows.set(Some(settings)),
            #[cfg(windows)]
            Event::LegacyFloatingWindows(settings) => this.legacy_floating_windows = settings,
            Event::ActivateMainWindow => cx.defer(|cx| {
                if let Some(window) = cx
                    .windows()
                    .iter()
                    .find_map(|window| window.downcast::<MusicApp>())
                {
                    let _ = window.update(cx, |_, window, _| activate_main_window(window));
                }
            }),
            Event::ExitApplication => cx.defer(|cx| {
                if let Some(window) = cx
                    .windows()
                    .iter()
                    .find_map(|window| window.downcast::<MusicApp>())
                {
                    let _ = window.update(cx, |_, window, _| window.remove_window());
                }
            }),
            Event::LinuxTrayAvailable(available) => {
                #[cfg(target_os = "linux")]
                this.linux_tray_available.set(available);
                #[cfg(not(target_os = "linux"))]
                let _ = available;
            }
            Event::Artwork(address, result) => {
                this.artwork.borrow_mut().finish(&address, result, cx);
                #[cfg(windows)]
                this.update_media_artwork(cx);
            }
            Event::Home(view) => this.home = Some(view),
            Event::Library(view) => this.library = Some(view),
            Event::LibraryAlbumOpened => {
                this.navigation.open_detail(Page::Library);
            }
            Event::HomePlaylistOpened => {
                this.navigation.open_detail(Page::Home);
            }
            Event::CopyShareLink(link) => {
                cx.write_to_clipboard(ClipboardItem::new_string(link));
                this.status = "链接已复制".into();
            }
            Event::Browse(view) => {
                if this
                    .navigation
                    .browse
                    .as_ref()
                    .is_none_or(|old| old.generation != view.generation)
                {
                    this.row_actions = None;
                }
                if matches!(this.batch_generation, Some(BatchSource::Browse(generation)) if generation != view.generation)
                {
                    this.batch_selection.clear();
                    this.batch_generation = None;
                    this.batch_remove_confirm = false;
                }
                if matches!(this.playlist_editor.dialog, PlaylistDialog::Create)
                    && !view.my_playlists
                {
                    this.playlist_editor.dialog = PlaylistDialog::Closed;
                    this.clear_playlist_cover(cx);
                }
                if let PlaylistDialog::EditLoading(id)
                | PlaylistDialog::Edit(id)
                | PlaylistDialog::Delete(id) = this.playlist_editor.dialog
                    && view.selected_playlist_id != Some(id)
                {
                    this.playlist_editor.dialog = PlaylistDialog::Closed;
                    this.clear_playlist_cover(cx);
                }
                this.navigation.accept_browse(view);
            }
            Event::Qr(qr) => {
                this.qr = match qr {
                    QrView::Idle => QrDisplay::Idle,
                    QrView::Loading => QrDisplay::Loading,
                    QrView::Expired => QrDisplay::Expired,
                    QrView::Failed => QrDisplay::Failed,
                    QrView::Ready(url) => match qrcode::QrCode::new(url.as_bytes()) {
                        Ok(code) => {
                            let width = code.width();
                            let modules = code
                                .to_colors()
                                .into_iter()
                                .map(|color| color == qrcode::Color::Dark)
                                .collect();
                            QrDisplay::Ready { width, modules }
                        }
                        Err(_) => QrDisplay::Failed,
                    },
                };
            }
            Event::Auth(auth) => {
                let was_busy = this.auth.busy;
                let changed = this.auth.account.as_ref().map(|account| &account.uid)
                    != auth.account.as_ref().map(|account| &account.uid);
                if auth.account.is_some() && !auth.busy {
                    this.code_input.update(cx, |input, cx| input.clear(cx));
                }
                this.auth = auth;
                if changed || this.auth.busy {
                    this.comment_panel.image.clear(cx);
                    this.account_overview_generation =
                        this.account_overview_generation.wrapping_add(1);
                    this.account_overview = None;
                    this.month_page.reset(cx);
                }
                if (changed || was_busy)
                    && !this.auth.busy
                    && this.navigation.page == Page::AccountOverview
                    && this.auth.account.is_some()
                {
                    this.load_account_overview(cx);
                }
                if changed || this.auth.busy {
                    this.search.overview_generation =
                        this.search.overview_generation.wrapping_add(1);
                    this.search.overview_query.clear();
                    this.search.overview_result = None;
                    this.navigation.overview_detail_open = false;
                }
                if this.panels.show_comments && !this.auth.busy && (was_busy || changed) {
                    let _ = this.service.commands.send(Command::OpenComments);
                }
                if this.auth.busy {
                    this.search.show_history = false;
                    this.search.history = None;
                    this.search.confirm_clear_history = false;
                    this.search.invalidate_suggestions();
                    this.navigation.home_playlist_open = false;
                    this.navigation.library_album_open = false;
                    this.batch_selection.clear();
                    this.batch_generation = None;
                    this.batch_remove_confirm = false;
                    this.forget_pending = None;
                    this.playlist_editor.dialog = PlaylistDialog::Closed;
                    this.clear_playlist_cover(cx);
                    this.clear_queue_pending = None;
                }
                if !this.auth.busy && (changed || this.auth.message == "登录成功") {
                    this.adding_account = false;
                }
                if changed && !this.auth.busy && this.navigation.page == Page::Mine {
                    let _ = this.service.commands.send(Command::MyPlaylists);
                }
                if changed && !this.auth.busy && this.navigation.page == Page::Collection {
                    let _ = this
                        .service
                        .commands
                        .send(Command::Collections(this.navigation.collection_kind));
                }
                if changed && !this.auth.busy && this.navigation.page == Page::Charts {
                    let _ = this.service.commands.send(Command::Charts);
                }
                if (changed || was_busy)
                    && !this.auth.busy
                    && this.navigation.page == Page::PlaylistSquare
                {
                    this.navigation.browse = None;
                    let _ = this.service.commands.send(Command::PlaylistSquare);
                }
                if changed && !this.auth.busy && this.navigation.page == Page::Recent {
                    let _ = this.service.commands.send(Command::ShowHistory);
                }
                if (changed || was_busy) && !this.auth.busy && this.navigation.page == Page::Home {
                    let _ = this.service.commands.send(Command::LoadHome);
                }
                if (changed || was_busy) && !this.auth.busy && this.navigation.page == Page::Library
                {
                    let _ = this.service.commands.send(Command::LoadLibrary);
                }
            }
            Event::Accounts(accounts) => {
                this.remembered_accounts = accounts.entries;
                this.account_cleanup_pending = accounts.cleanup_pending;
            }
            Event::Status(status) => this.status = status,
            Event::Search(search) => {
                if matches!(this.batch_generation, Some(BatchSource::Search(generation)) if generation != search.generation)
                {
                    this.batch_selection.clear();
                    this.batch_generation = None;
                }
                this.search.generation = search.generation;
                this.search.tracks = search.tracks;
                this.search.has_more = search.has_more;
                this.search.loading = search.loading;
            }
            Event::History(history) => {
                if matches!(this.batch_generation, Some(BatchSource::History(generation)) if generation != history.generation)
                {
                    this.batch_selection.clear();
                    this.batch_generation = None;
                }
                this.history_generation = history.generation;
                this.recent_tracks = history.tracks;
                this.recent_loading = history.loading;
                this.recent_error = history.error;
            }
            Event::HistoryChanged => {
                if this.navigation.page == Page::Recent && !this.auth.busy {
                    let _ = this.service.commands.send(Command::ShowHistory);
                }
            }
            Event::Player(player) => {
                if this.clear_queue_pending.is_some_and(|stamp| {
                    stamp != (player.queue_account_revision, player.queue_revision)
                }) {
                    this.clear_queue_pending = None;
                }
                this.player = Some(player);
                #[cfg(windows)]
                this.update_windows_media(cx);
            }
            Event::Like(like) => this.like = like,
            Event::Comments(view) => {
                if this.comment_panel.view.as_ref().is_some_and(|old| {
                    old.generation != view.generation || old.track_id != view.track_id
                }) {
                    this.comment_panel.image.clear(cx);
                }
                let old_key = this.comment_panel.view.as_ref().and_then(comment_draft_key);
                let new_key = comment_draft_key(&view);
                if old_key != new_key {
                    if let Some(key) = old_key {
                        let draft = this.comment_panel.input.read(cx).text().to_owned();
                        if draft.trim().is_empty() {
                            this.comment_panel.drafts.remove(&key);
                        } else {
                            this.comment_panel.drafts.insert(key, draft);
                        }
                        if this.comment_panel.drafts.len() > 32
                            && let Some(key) = this
                                .comment_panel
                                .drafts
                                .keys()
                                .copied()
                                .find(|key| Some(*key) != new_key)
                        {
                            this.comment_panel.drafts.remove(&key);
                        }
                    }
                    let draft = new_key
                        .and_then(|key| this.comment_panel.drafts.get(&key).cloned())
                        .unwrap_or_default();
                    this.comment_panel
                        .input
                        .update(cx, |input, cx| input.set_text(draft, cx));
                }
                this.comment_panel.view = Some(*view);
            }
            Event::CommentPostResult(generation, success) => {
                if this
                    .comment_panel
                    .submitted
                    .as_ref()
                    .is_some_and(|pending| pending.0 == generation)
                {
                    let (_, key, text) = this
                        .comment_panel
                        .submitted
                        .take()
                        .expect("pending checked");
                    if success {
                        if this
                            .comment_panel
                            .drafts
                            .get(&key)
                            .is_some_and(|draft| draft == &text)
                        {
                            this.comment_panel.drafts.remove(&key);
                        }
                        if this.comment_panel.view.as_ref().and_then(comment_draft_key) == Some(key)
                            && this.comment_panel.input.read(cx).text() == text
                        {
                            this.comment_panel
                                .input
                                .update(cx, |input, cx| input.clear(cx));
                        }
                    }
                }
            }
            Event::PlaylistEditReady(playlist, groups) => {
                if this.playlist_editor.dialog == PlaylistDialog::EditLoading(playlist.id) {
                    this.playlist_editor.generation =
                        this.playlist_editor.generation.wrapping_add(1);
                    this.playlist_editor.category_groups = groups;
                    this.playlist_editor.selected_categories = playlist
                        .categories
                        .iter()
                        .filter_map(|category| i32::try_from(category.id).ok())
                        .collect();
                    this.playlist_editor.original_categories = playlist.categories.clone();
                    this.clear_playlist_cover(cx);
                    this.playlist_editor
                        .name_input
                        .update(cx, |input, cx| input.set_text(playlist.name, cx));
                    this.playlist_editor
                        .description_input
                        .update(cx, |input, cx| input.set_text(playlist.description, cx));
                    this.playlist_editor.dialog = PlaylistDialog::Edit(playlist.id);
                }
            }
            Event::PlaylistEditFailed => {
                if matches!(this.playlist_editor.dialog, PlaylistDialog::EditLoading(_)) {
                    this.playlist_editor.dialog = PlaylistDialog::Closed;
                }
            }
            Event::PlaylistCoverPreview(request, result) => {
                if this.playlist_editor.cover_request == request
                    && this.playlist_editor.cover_loading
                    && matches!(this.playlist_editor.dialog, PlaylistDialog::Edit(_))
                {
                    this.playlist_editor.cover_loading = false;
                    match result {
                        Ok(image) => {
                            this.playlist_editor.cover_preview = Some(Arc::new(Image::from_bytes(
                                ImageFormat::Png,
                                image.preview_png.clone(),
                            )));
                            this.playlist_editor.cover_image = Some(image);
                            this.playlist_editor.cover_crop = CoverCrop::default();
                        }
                        Err(()) => {
                            this.playlist_editor.cover_path = None;
                            this.status =
                                "无法读取封面，请选择不超过 12 MB 的 JPG、PNG、BMP 或 WebP 图片"
                                    .into();
                        }
                    }
                }
            }
            Event::PlaylistMutationBusy(busy) => this.playlist_editor.busy = busy,
            Event::TrackPicker(picker) => this.track_picker = picker,
            Event::BatchProgress(progress) => this.batch_progress = progress,
            Event::PlaylistMutationFinished(outcome) => {
                match (this.playlist_editor.dialog, outcome) {
                    (PlaylistDialog::Create, Some(PlaylistMutationResult::Created { .. })) => {
                        this.playlist_editor.dialog = PlaylistDialog::Closed
                    }
                    (
                        PlaylistDialog::Edit(id),
                        Some(PlaylistMutationResult::Edited { id: done, .. }),
                    ) if id == done => this.playlist_editor.dialog = PlaylistDialog::Closed,
                    (
                        PlaylistDialog::Delete(id),
                        Some(PlaylistMutationResult::Deleted { id: done }),
                    ) if id == done => this.playlist_editor.dialog = PlaylistDialog::Closed,
                    (PlaylistDialog::Edit(_), None) => {
                        this.playlist_editor.dialog = PlaylistDialog::Closed
                    }
                    _ => {}
                }
                if this.playlist_editor.dialog == PlaylistDialog::Closed {
                    this.clear_playlist_cover(cx);
                }
            }
            Event::Lyrics(lyrics) => {
                this.lyrics = lyrics;
                this.lyric_line = None;
                this.manual_scroll_until = None;
                this.lyric_page.reset_follow();
            }
        }
        if modal_was_open && this.playlist_editor.dialog == PlaylistDialog::Closed {
            this.restore_playlist_focus(cx);
        }
    }
}
