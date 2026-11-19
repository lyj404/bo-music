use super::*;

impl MusicApp {
    pub(super) fn accept_event(&mut self, event: Event, cx: &mut Context<Self>) {
        let this = self;
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
                this.comment_image.accept(request, result);
            }
            Event::CommentImageSaved(request, saved) => {
                this.comment_image.saved(request, saved);
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
                if generation == this.overview_generation && !this.auth.busy {
                    this.overview_result = Some(result);
                }
            }
            Event::SearchOverviewOpened(generation) => {
                if generation == this.overview_generation
                    && this.page == Page::Overview
                    && !this.auth.busy
                {
                    this.overview_detail_open = true;
                }
            }
            Event::SearchSuggestions(generation, words) => {
                if generation == this.suggestion_generation && !this.auth.busy {
                    this.search_suggestions = words;
                }
            }
            Event::SearchHotWords(words) => {
                this.hot_words_loading = false;
                this.hot_words = words;
            }
            Event::SearchHistory(result) => {
                this.search_history = Some(result);
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
                if this.page == Page::Library {
                    this.library_album_open = true;
                }
            }
            Event::HomePlaylistOpened => {
                if this.page == Page::Home {
                    this.home_playlist_open = true;
                }
            }
            Event::CopyShareLink(link) => {
                cx.write_to_clipboard(ClipboardItem::new_string(link));
                this.status = "链接已复制".into();
            }
            Event::Browse(view) => {
                if !view.is_detail {
                    this.overview_detail_open = false;
                }
                if matches!(this.batch_generation, Some(BatchSource::Browse(generation)) if generation != view.generation)
                {
                    this.batch_selection.clear();
                    this.batch_generation = None;
                    this.batch_remove_confirm = false;
                }
                if matches!(this.playlist_dialog, PlaylistDialog::Create) && !view.my_playlists {
                    this.playlist_dialog = PlaylistDialog::Closed;
                    this.clear_playlist_cover(cx);
                }
                if let PlaylistDialog::EditLoading(id)
                | PlaylistDialog::Edit(id)
                | PlaylistDialog::Delete(id) = this.playlist_dialog
                    && view.selected_playlist_id != Some(id)
                {
                    this.playlist_dialog = PlaylistDialog::Closed;
                    this.clear_playlist_cover(cx);
                }
                this.browse = Some(view);
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
                    this.comment_image.clear(cx);
                    this.account_overview_generation =
                        this.account_overview_generation.wrapping_add(1);
                    this.account_overview = None;
                    this.month_page.reset(cx);
                }
                if (changed || was_busy)
                    && !this.auth.busy
                    && this.page == Page::AccountOverview
                    && this.auth.account.is_some()
                {
                    this.load_account_overview(cx);
                }
                if changed || this.auth.busy {
                    this.overview_generation = this.overview_generation.wrapping_add(1);
                    this.overview_query.clear();
                    this.overview_result = None;
                    this.overview_detail_open = false;
                }
                if this.show_comments && !this.auth.busy && (was_busy || changed) {
                    let _ = this.service.commands.send(Command::OpenComments);
                }
                if this.auth.busy {
                    this.show_search_history = false;
                    this.search_history = None;
                    this.confirm_clear_search_history = false;
                    this.suggestion_generation = this.suggestion_generation.wrapping_add(1);
                    this.search_suggestions.clear();
                    this.home_playlist_open = false;
                    this.library_album_open = false;
                    this.batch_selection.clear();
                    this.batch_generation = None;
                    this.batch_remove_confirm = false;
                    this.forget_pending = None;
                    this.playlist_dialog = PlaylistDialog::Closed;
                    this.clear_playlist_cover(cx);
                    this.clear_queue_pending = None;
                }
                if !this.auth.busy && (changed || this.auth.message == "登录成功") {
                    this.adding_account = false;
                }
                if changed && !this.auth.busy && this.page == Page::Mine {
                    let _ = this.service.commands.send(Command::MyPlaylists);
                }
                if changed && !this.auth.busy && this.page == Page::Collection {
                    let _ = this
                        .service
                        .commands
                        .send(Command::Collections(this.collection_kind));
                }
                if changed && !this.auth.busy && this.page == Page::Charts {
                    let _ = this.service.commands.send(Command::Charts);
                }
                if (changed || was_busy) && !this.auth.busy && this.page == Page::PlaylistSquare {
                    this.browse = None;
                    let _ = this.service.commands.send(Command::PlaylistSquare);
                }
                if changed && !this.auth.busy && this.page == Page::Recent {
                    let _ = this.service.commands.send(Command::ShowHistory);
                }
                if (changed || was_busy) && !this.auth.busy && this.page == Page::Home {
                    let _ = this.service.commands.send(Command::LoadHome);
                }
                if (changed || was_busy) && !this.auth.busy && this.page == Page::Library {
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
                this.search_generation = search.generation;
                this.tracks = search.tracks;
                this.has_more = search.has_more;
                this.search_loading = search.loading;
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
                if this.page == Page::Recent && !this.auth.busy {
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
                if this.comments.as_ref().is_some_and(|old| {
                    old.generation != view.generation || old.track_id != view.track_id
                }) {
                    this.comment_image.clear(cx);
                }
                let old_key = this.comments.as_ref().and_then(comment_draft_key);
                let new_key = comment_draft_key(&view);
                if old_key != new_key {
                    if let Some(key) = old_key {
                        let draft = this.comment_input.read(cx).text().to_owned();
                        if draft.trim().is_empty() {
                            this.comment_drafts.remove(&key);
                        } else {
                            this.comment_drafts.insert(key, draft);
                        }
                        if this.comment_drafts.len() > 32
                            && let Some(key) = this
                                .comment_drafts
                                .keys()
                                .copied()
                                .find(|key| Some(*key) != new_key)
                        {
                            this.comment_drafts.remove(&key);
                        }
                    }
                    let draft = new_key
                        .and_then(|key| this.comment_drafts.get(&key).cloned())
                        .unwrap_or_default();
                    this.comment_input
                        .update(cx, |input, cx| input.set_text(draft, cx));
                }
                this.comments = Some(*view);
            }
            Event::CommentPostResult(generation, success) => {
                if this
                    .submitted_comment
                    .as_ref()
                    .is_some_and(|pending| pending.0 == generation)
                {
                    let (_, key, text) = this.submitted_comment.take().expect("pending checked");
                    if success {
                        if this
                            .comment_drafts
                            .get(&key)
                            .is_some_and(|draft| draft == &text)
                        {
                            this.comment_drafts.remove(&key);
                        }
                        if this.comments.as_ref().and_then(comment_draft_key) == Some(key)
                            && this.comment_input.read(cx).text() == text
                        {
                            this.comment_input.update(cx, |input, cx| input.clear(cx));
                        }
                    }
                }
            }
            Event::PlaylistEditReady(playlist, groups) => {
                if this.playlist_dialog == PlaylistDialog::EditLoading(playlist.id) {
                    this.playlist_editor_generation =
                        this.playlist_editor_generation.wrapping_add(1);
                    this.playlist_category_groups = groups;
                    this.playlist_selected_categories = playlist
                        .categories
                        .iter()
                        .filter_map(|category| i32::try_from(category.id).ok())
                        .collect();
                    this.playlist_original_categories = playlist.categories.clone();
                    this.clear_playlist_cover(cx);
                    this.playlist_name_input
                        .update(cx, |input, cx| input.set_text(playlist.name, cx));
                    this.playlist_description_input
                        .update(cx, |input, cx| input.set_text(playlist.description, cx));
                    this.playlist_dialog = PlaylistDialog::Edit(playlist.id);
                }
            }
            Event::PlaylistEditFailed => {
                if matches!(this.playlist_dialog, PlaylistDialog::EditLoading(_)) {
                    this.playlist_dialog = PlaylistDialog::Closed;
                }
            }
            Event::PlaylistCoverPreview(request, result) => {
                if this.playlist_cover_request == request
                    && this.playlist_cover_loading
                    && matches!(this.playlist_dialog, PlaylistDialog::Edit(_))
                {
                    this.playlist_cover_loading = false;
                    match result {
                        Ok(image) => {
                            this.playlist_cover_preview = Some(Arc::new(Image::from_bytes(
                                ImageFormat::Png,
                                image.preview_png.clone(),
                            )));
                            this.playlist_cover_image = Some(image);
                            this.playlist_cover_crop = CoverCrop::default();
                        }
                        Err(()) => {
                            this.playlist_cover_path = None;
                            this.status =
                                "无法读取封面，请选择不超过 12 MB 的 JPG、PNG、BMP 或 WebP 图片"
                                    .into();
                        }
                    }
                }
            }
            Event::PlaylistMutationBusy(busy) => this.playlist_busy = busy,
            Event::TrackPicker(picker) => this.track_picker = picker,
            Event::BatchProgress(progress) => this.batch_progress = progress,
            Event::PlaylistMutationFinished(outcome) => {
                match (this.playlist_dialog, outcome) {
                    (PlaylistDialog::Create, Some(PlaylistMutationResult::Created { .. })) => {
                        this.playlist_dialog = PlaylistDialog::Closed
                    }
                    (
                        PlaylistDialog::Edit(id),
                        Some(PlaylistMutationResult::Edited { id: done, .. }),
                    ) if id == done => this.playlist_dialog = PlaylistDialog::Closed,
                    (
                        PlaylistDialog::Delete(id),
                        Some(PlaylistMutationResult::Deleted { id: done }),
                    ) if id == done => this.playlist_dialog = PlaylistDialog::Closed,
                    (PlaylistDialog::Edit(_), None) => {
                        this.playlist_dialog = PlaylistDialog::Closed
                    }
                    _ => {}
                }
                if this.playlist_dialog == PlaylistDialog::Closed {
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
    }
}
