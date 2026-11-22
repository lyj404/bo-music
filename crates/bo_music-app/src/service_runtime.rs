//! Owns the desktop command loop, tasks and shutdown order.

use super::*;

impl Service {
    pub fn start() -> (Self, StartupPlacement) {
        let startup_paths = initialize_paths(None);
        let placement = startup_paths
            .as_ref()
            .ok()
            .and_then(|paths| SettingsStore::open(paths.config()).ok())
            .map_or_else(StartupPlacement::default, |store| StartupPlacement {
                main: store.main_window_bounds().unwrap_or_default(),
                #[cfg(windows)]
                legacy_main: store
                    .legacy_floating_windows()
                    .ok()
                    .and_then(|windows| windows.main),
            });
        let service = Self::start_with(ServiceOptions {
            credentials: Some(Arc::new(OsSecretBackend)),
            startup_paths: Some(startup_paths),
            ..Default::default()
        });
        (service, placement)
    }

    pub(super) fn start_with(options: ServiceOptions) -> Self {
        let (commands, mut receiver) = mpsc::unbounded_channel();
        let (events, event_receiver) = std_mpsc::channel();
        let sender = commands.clone();
        let worker = std::thread::Builder::new().name("bo_music-service".into()).spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(runtime) => runtime,
                Err(_) => { let _ = events.send(Event::Status("无法启动后台服务".into())); return; }
            };
            runtime.block_on(async move {
                #[cfg(any(target_os = "linux", test))]
                let register_desktop_integration = options.paths.is_none();
                #[cfg(windows)]
                let legacy_credential_source = if options.paths.is_none() {
                    std::env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty()).map(|value| PathBuf::from(value).join("Bowen"))
                } else { None };
                let paths = match options.startup_paths.unwrap_or_else(|| initialize_paths(options.paths)) {
                    Ok(paths) => paths,
                    Err(error) => { let _ = events.send(Event::Status(error)); return; }
                };
                let id = match device::get_or_create(&paths.device_id()) {
                    Ok(id) => id,
                    Err(_) => { let _ = events.send(Event::Status("无法读取设备标识".into())); return; }
                };
                let mut transport_options = TransportOptions::default();
                if let Some(base_url) = options.base_url { transport_options.base_url = base_url; }
                let transport = match Transport::new(transport_options, Session::default(), id) {
                    Ok(value) => Arc::new(value),
                    Err(_) => { let _ = events.send(Event::Status("无法初始化网络服务".into())); return; }
                };
                let settings = match SettingsStore::open(paths.config()) {
                    Ok(settings) => settings,
                    Err(_) => { let _ = events.send(Event::Status("无法读取设置或备份，请检查数据目录".into())); return; }
                };
                let preferences = match settings.playback() {
                    Ok(value) => value,
                    Err(_) => { let _ = events.send(Event::Status("无法读取播放设置".into())); return; }
                };
                let mut volume = settings.volume().unwrap_or(100.0);
                let mut muted = false;
                let mut floating_windows = settings.floating_windows().unwrap_or_default();
                let mut floating_save_at = None;
                let mut main_placement: Option<FloatingBounds> = None;
                let mut main_save_at = None;
                let _ = events.send(Event::FloatingWindows(floating_windows));
                let _ = events.send(Event::Appearance(settings.appearance().unwrap_or_default()));
                let _ = events.send(Event::Shortcuts(settings.shortcuts().unwrap_or_default()));
                let _ = events.send(Event::LyricPosterStyle(settings.lyric_poster_style().unwrap_or_default()));
                #[cfg(windows)]
                let _ = events.send(Event::LegacyFloatingWindows(settings.legacy_floating_windows().unwrap_or_default()));
                let preferred_quality = settings.preferred_quality().unwrap_or(AudioQuality::Lossless);
                let mut audio_effects = settings.audio_effects().unwrap_or_default();
                let ambient_root = paths.root().join("ambient").join("v9");
                let mut current_filter = None;
                let (history_tx, history_thread) = history_worker(paths.clone(), sender.clone(), events.clone());
                #[cfg(windows)]
                let legacy_credential_target = paths.clone();
                let queue_store = QueueStore::new(paths);
                let mut persist_enabled = preferences.restore_queue;
                let library = options.library.unwrap_or_else(mpv_library);
                let video_library = options.video_library.unwrap_or_else(|| video_library(&library));
                let mut mv = MvController::new(Arc::clone(&transport), sender.clone(), events.clone(), video_library, options.audio_output.clone());
                let mut player = MpvPlayer::open(library, MpvOptions { audio_output: options.audio_output }).ok()
                    .map(|mpv| PlaybackCoordinator::new(Arc::clone(&transport), mpv));
                if let Some(player) = player.as_mut() {
                    player.set_mode(preferences.mode);
                    player.set_quality(Some(preferred_quality));
                    if player.volume(volume).is_err() {
                        let _ = events.send(Event::Status("音量设置失败".into()));
                    }
                    if player.restore_account_queue(&queue_store, preferences.restore_queue).is_err() {
                        persist_enabled = false;
                        let _ = events.send(Event::Status("播放队列无法恢复，原文件已保留".into()));
                    } else { let _ = events.send(Event::Status("准备就绪".into())); }
                    match audio_filter(&audio_effects, &ambient_root) {
                        Ok(filter) if player.audio_filter(filter.as_deref()).is_ok() => current_filter = filter,
                        _ => { let _ = events.send(Event::Status("音效恢复失败，请检查素材和 libmpv 滤镜支持".into())); }
                    }
                } else {
                    let _ = events.send(Event::Status("播放器不可用，请检查 libmpv；仍可搜索".into()));
                }
                let mut query_generation = 0_u64;
                let mut overview_task: Option<tokio::task::JoinHandle<()>> = None;
                let mut account_overview_task: Option<tokio::task::JoinHandle<()>> = None;
                let mut month_report_task: Option<tokio::task::JoinHandle<()>> = None;
                let mut month_poster_task: Option<tokio::task::JoinHandle<()>> = None;
                let mut month_poster_request = None;
                let mut month_poster_png: Option<Arc<Vec<u8>>> = None;
                let mut month_poster_saving = None;
                let mut month_probe_task: Option<tokio::task::JoinHandle<()>> = None;
                let mut month_probe_request = None;
                let mut month_reports = ReportCache::default();
                let mut month_track_task: Option<tokio::task::JoinHandle<()>> = None;
                let mut month_track_request = 0_u64;
                let mut month_report_request = None;
                let mut active_month_report: Option<Box<MonthReport>> = None;
                let mut month_playlist_writes = HashSet::new();
                let mut created_month_playlists = HashSet::new();
                let mut account_overview_request = None;
                let mut weekly_tracks: Vec<PlayRankingEntry> = Vec::new();
                let mut overview_request = None;
                let mut overview_sections: Vec<SearchSection> = Vec::new();
                let mut query_text = String::new();
                let mut cursor = search_cursor(30);
                let mut search_results = Vec::new();
                let mut search_loading = false;
                let mut search_task: Option<tokio::task::JoinHandle<()>> = None;
                let mut suggestion_task: Option<tokio::task::JoinHandle<()>> = None;
                let mut hot_words_task: Option<tokio::task::JoinHandle<()>> = None;
                let mut lyrics_key = None;
                let mut lyrics_generation = 0_u64;
                let mut lyrics_task: Option<tokio::task::JoinHandle<()>> = None;
                let mut resolution_task: Option<tokio::task::JoinHandle<()>> = None;
                let mut quality_task: Option<tokio::task::JoinHandle<()>> = None;
                let mut recovery_task: Option<tokio::task::JoinHandle<()>> = None;
                let mut prefetch = Prefetcher::default();
                let mut report_tasks = tokio::task::JoinSet::new();
                let mut browser = Browser::default();
                let artwork_fetcher = ArtworkFetcher::new().ok();
                #[cfg(any(target_os = "linux", test))]
                let mut mpris = register_desktop_integration.then(|| crate::mpris::Mpris::start(sender.clone(), artwork_fetcher.clone()));
                #[cfg(any(target_os = "linux", test))]
                if register_desktop_integration {
                    crate::linux_tray::start(sender.clone(), events.clone());
                }
                let mut home = Home::default();
                let mut music_library = MusicLibrary::default();
                let mut comments = Comments::default();
                let mut comment_image_task: Option<tokio::task::JoinHandle<()>> = None;
                let mut comment_image = None;
                let mut comment_image_request = None;
                let mut share_reports = HashSet::new();
                let mut liked_songs = LikedSongs::default();
                let mut playlist_mutation: Option<u64> = None;
                let mut batch_cancel: Option<Arc<AtomicBool>> = None;
                let mut playlist_edit_generation = 0_u64;
        let mut playlist_edit_data: Option<(u64, Playlist, Option<Vec<CategoryGroup>>)> = None;
        let mut cover_preview_task: Option<tokio::task::JoinHandle<()>> = None;
        let cover_preview_slot = Arc::new(tokio::sync::Semaphore::new(1));
                let mut picker_generation = 0_u64;
                let mut picker: Option<PlaylistPickerState> = None;
                let mut picker_task: Option<tokio::task::JoinHandle<()>> = None;
                let mut qr_task: Option<tokio::task::JoinHandle<()>> = None;
                let mut qr_generation = 0_u64;
                let qr_interval = options.qr_interval.unwrap_or(Duration::from_secs(2));
                let qr_lifetime = options.qr_lifetime.unwrap_or(Duration::from_secs(300));
                let mut last_view = None;
                let mut last_queue_revision = player.as_ref().map(|p| p.queue().revision()).unwrap_or(0);
                let mut last_checkpoint = std::time::Instant::now();
                let mut history_generation = 0_u64;
                let mut history_results = Vec::new();
                let mut history_revision = u64::MAX;
                let mut account_list_generation = 0_u64;
                macro_rules! cancel_account_requests {
                    () => {{
                        account_list_generation = account_list_generation.wrapping_add(1);
                        history_generation = history_generation.wrapping_add(1);
                        history_results.clear();
                        history_revision = u64::MAX;
                        let _ = events.send(Event::History(HistoryView { generation: history_generation, tracks: Vec::new(), loading: false, error: None }));
                        browser.cancel_pending();
                        home.invalidate();
                        let _ = events.send(Event::Home(home.view()));
                        music_library.invalidate();
                        let _ = events.send(Event::Library(music_library.view()));
                        comments.reset_account();
                        if let Some(task) = comment_image_task.take() { task.abort(); }
                        comment_image = None;
                        comment_image_request = None;
                        if let Some(cancel) = &batch_cancel { cancel.store(true, Ordering::Release); }
                        let _ = events.send(Event::BatchProgress(None));
                        let _ = events.send(Event::Comments(Box::new(comments.view())));
                        playlist_edit_generation = playlist_edit_generation.wrapping_add(1);
                        playlist_edit_data = None;
                        picker_generation = picker_generation.wrapping_add(1);
                        if let Some(task) = picker_task.take() { task.abort(); }
                        picker = None;
                        let _ = events.send(Event::TrackPicker(None));
                        liked_songs.reset();
                        let _ = events.send(Event::Like(liked_songs.view()));
                        let _ = events.send(Event::Browse(browser.view()));
                        query_generation = query_generation.wrapping_add(1);
                        if let Some(task) = search_task.take() { task.abort(); }
                        if let Some(task) = account_overview_task.take() { task.abort(); }
                        if let Some(task) = month_report_task.take() { task.abort(); }
                        if let Some(task) = month_poster_task.take() { task.abort(); }
                        month_poster_request = None;
                        month_poster_png = None;
                        if let Some(task) = month_probe_task.take() { task.abort(); }
                        month_probe_request = None;
                        month_reports.clear();
                        if let Some(task) = month_track_task.take() { task.abort(); }
                        month_track_request = month_track_request.wrapping_add(1);
                        month_report_request = None;
                        active_month_report = None;
                        created_month_playlists.clear();
                        account_overview_request = None;
                        weekly_tracks.clear();
                        search_loading = false;
                        let _ = events.send(Event::Search(SearchView { generation: query_generation, tracks: search_results.clone(), has_more: !query_text.is_empty() && !cursor.exhausted(), loading: false }));
                        if let Some(task) = lyrics_task.take() { task.abort(); }
                        lyrics_key = None;
                        lyrics_generation = lyrics_generation.wrapping_add(1);
                        if let Some(task) = resolution_task.take() { task.abort(); }
                        if let Some(task) = quality_task.take() { task.abort(); }
                        if let Some(task) = recovery_task.take() { task.abort(); }
                        if let Some(player) = player.as_mut() { player.cancel_recovery_refresh(); player.cancel_quality_request(); }
                        prefetch.cancel();
                    }};
                }
                macro_rules! cancel_qr {
                    () => {{
                        qr_generation = qr_generation.wrapping_add(1);
                        if let Some(task) = qr_task.take() { task.abort(); }
                        let _ = events.send(Event::Qr(QrView::Idle));
                    }};
                }
                macro_rules! comments_current {
                    ($generation:expr) => {{
                        let track = player.as_ref().and_then(|player| player.queue().current()).map(|track| track.id);
                        comments.select(track, Arc::clone(&transport), sender.clone());
                        let current = comments.generation() == $generation;
                        if !current { let _ = events.send(Event::Comments(Box::new(comments.view()))); }
                        current
                    }};
                }
                macro_rules! open_track_picker {
                    ($tracks:expr, $revision:expr) => {{
                        let tracks: Vec<Track> = $tracks;
                        let revision: u64 = $revision;
                        picker_generation = picker_generation.wrapping_add(1);
                        if let Some(task) = picker_task.take() { task.abort(); }
                        let generation = picker_generation;
                        let track_ids = batch_playlist::normalize(tracks.iter().map(|track| track.id));
                        let title = if track_ids.len() == 1 { tracks[0].title.clone() } else { format!("{} 首歌曲", track_ids.len()) };
                        picker = Some(PlaylistPickerState {
                            revision, generation, track_ids: track_ids.clone(),
                            view: PlaylistPickerView { track_id: track_ids[0], track_title: title, generation, track_count: track_ids.len(), playlists: Vec::new(), loading: true, busy: playlist_mutation.is_some(), error: None },
                        });
                        let _ = events.send(Event::TrackPicker(picker.as_ref().map(|picker| picker.view.clone())));
                        let sender = sender.clone(); let transport = Arc::clone(&transport);
                        picker_task = Some(tokio::spawn(async move {
                            let result = playlist::created_at_revision(&transport, revision).await.map_err(|_| ());
                            let _ = sender.send(Command::TrackPickerLoaded(generation, revision, result));
                        }));
                    }};
                }
                let secrets = options.credentials;
                let mut account: Option<Account> = None;
                let mut auth_busy = secrets.is_some();
                let mut sms_busy = false;
                let mut last_sms: Option<std::time::Instant> = None;
                if let Some(backend) = secrets.clone() {
                    let session = transport.session();
                    let sender = sender.clone();
                    auth_view(&events, &account, true, "正在恢复登录…");
                    std::thread::spawn(move || {
                        let store = CredentialStore::new(backend);
                        #[cfg(windows)]
                        let migration_failed = legacy_credential_source.as_ref().is_some_and(|source| {
                            let revision = session.snapshot().revision;
                            session.with_revision_for_store(revision, || bo_music_storage::legacy_credentials::import_once(source, &legacy_credential_target, &store))
                                .is_some_and(|result| result.is_err())
                        });
                        let result = auth::restore(&session, &store);
                        let _ = sender.send(Command::AuthDone(AuthOperation::Restore, result));
                        #[cfg(windows)]
                        if migration_failed { let _ = sender.send(Command::LegacyCredentialNotice); }
                    });
                } else {
                    auth_view(&events, &account, false, "未登录");
                    let _ = sender.send(Command::RefreshAccounts);
                }
                let mut timer = tokio::time::interval(Duration::from_millis(250));
                loop {
                    tokio::select! {
                        _ = timer.tick() => {
                            if main_save_at.is_some_and(|deadline| std::time::Instant::now() >= deadline)
                                && let Some(bounds) = main_placement {
                                main_save_at = if settings.set_main_window_bounds(bounds).is_ok() { None }
                                    else { Some(std::time::Instant::now() + Duration::from_secs(2)) };
                            }
                            if floating_save_at.is_some_and(|deadline| std::time::Instant::now() >= deadline) {
                                if settings.set_floating_windows(floating_windows).is_err() {
                                    floating_save_at = Some(std::time::Instant::now() + Duration::from_secs(2));
                                    let _ = events.send(Event::Status("小窗设置保存失败".into()));
                                } else {
                                    floating_save_at = None;
                                }
                            }
                            if auth_busy { continue; }
                            if account.is_some() && !transport.session().snapshot().is_authenticated()
                                && let Some(backend) = secrets.clone() {
                                cancel_qr!();
                                cancel_account_requests!();
                                account = None;
                                browser.clear();
                                let _ = events.send(Event::Browse(browser.view()));
                                if let Some(player) = player.as_mut() {
                                    if player.restore_account_queue(&queue_store, preferences.restore_queue).is_err() {
                                        persist_enabled = false;
                                    } else { persist_enabled = preferences.restore_queue; }
                                }
                                search_results.clear(); query_text.clear();
                                let _ = events.send(Event::Search(SearchView { generation: query_generation, tracks: Vec::new(), has_more: false, loading: false }));
                                let _ = events.send(Event::Lyrics(LyricsView::Empty));
                                auth_busy = true;
                                let session = transport.session();
                                let revision = session.snapshot().revision;
                                let sender = sender.clone();
                                auth_view(&events, &account, true, "登录已失效，正在清理凭据…");
                                std::thread::spawn(move || {
                                    let store = CredentialStore::new(backend);
                                    let result = session.clear_after(revision, || store.expire_active())
                                        .ok_or(AuthError::SessionChanged)
                                        .and_then(|result| result.map_err(AuthError::Storage))
                                        .map(|_| None);
                                    let _ = sender.send(Command::AuthDone(AuthOperation::Expired, result));
                                });
                                continue;
                            }
                            if let Some(player) = player.as_mut() {
                                let failed_before_poll = player.snapshot().state == PlaybackState::Failed;
                                match player.poll() {
                                    Ok(ticket) => {
                                        if failed_before_poll && ticket.is_some() {
                                            let _ = events.send(Event::Status("预取音源加载失败，正在重新获取…".into()));
                                            if let Some(ticket) = &ticket { prefetch.forget_for(ticket); }
                                        }
                                        launch(ticket, &sender, &mut resolution_task, &mut prefetch);
                                    }
                                    Err(_) => { let _ = events.send(Event::Status("播放状态更新失败".into())); }
                                }
                                if let Some(outcome) = player.take_quality_outcome() {
                                    let message = match outcome {
                                        QualityOutcome::Changed => "音质已切换",
                                        QualityOutcome::Restored => "音质切换失败，已恢复原音源",
                                        QualityOutcome::Failed => "音质切换及原音源恢复失败",
                                        _ => "音质切换失败",
                                    };
                                    let _ = events.send(Event::Status(message.into()));
                                }
                                if !player.snapshot().recovering && let Some(task) = recovery_task.take() { task.abort(); }
                                if let Some(ticket) = player.recovery_ticket() {
                                    if let Some(task) = recovery_task.take() { task.abort(); }
                                    let sender = sender.clone();
                                    recovery_task = Some(tokio::spawn(async move {
                                        let _ = sender.send(Command::RecoveryResolved(ticket.resolve().await));
                                    }));
                                }
                                let identity = transport.session().snapshot().revision;
                                let track = player.queue().current().filter(|track| track.id > 0);
                                let previous_comments = comments.generation();
                                comments.select(track.map(|track| track.id), Arc::clone(&transport), sender.clone());
                                if previous_comments != comments.generation() { let _ = events.send(Event::Comments(Box::new(comments.view()))); }
                                let prior_like = liked_songs.view();
                                liked_songs.select(track.map(|track| track.id), Arc::clone(&transport), sender.clone());
                                if prior_like != liked_songs.view() { let _ = events.send(Event::Like(liked_songs.view())); }
                                let next_key = track.map(|track| (track.id, identity));
                                if next_key != lyrics_key {
                                    lyrics_generation = lyrics_generation.wrapping_add(1);
                                    if let Some(task) = lyrics_task.take() { task.abort(); }
                                    lyrics_key = next_key;
                                    if let Some(track) = track {
                                        let _ = events.send(Event::Lyrics(LyricsView::Loading));
                                        let track = track.clone();
                                        let transport = Arc::clone(&transport);
                                        let sender = sender.clone();
                                        let endpoint = options.lyrics_endpoint.clone();
                                        let generation = lyrics_generation;
                                        lyrics_task = Some(tokio::spawn(async move {
                                            let result = match endpoint {
                                                Some(endpoint) => lyrics::get_at(&transport, &track, identity, &endpoint).await,
                                                None => lyrics::get_for_revision(&transport, &track, identity).await,
                                            }.map_err(|_| "歌词加载失败".to_owned());
                                            let _ = sender.send(Command::LyricsDone(generation, result));
                                        }));
                                    } else {
                                        let _ = events.send(Event::Lyrics(LyricsView::Empty));
                                    }
                                }
                                let view = player_view(player, if muted { 0. } else { volume }, &audio_effects);
                                #[cfg(any(target_os = "linux", test))]
                                if let Some(mpris) = &mut mpris {
                                    mpris.publish(&view);
                                }
                                if last_view.as_ref() != Some(&view) {
                                    if view.audio_filter_failed && last_view.as_ref().is_none_or(|previous: &PlayerView| !previous.audio_filter_failed) {
                                        let _ = events.send(Event::Status("音效滤镜加载失败，请检查 libmpv 滤镜支持".into()));
                                    }
                                    if view.recovering && last_view.as_ref().is_none_or(|previous: &PlayerView| !previous.recovering) {
                                        let _ = events.send(Event::Status("网络中断，正在重新连接…".into()));
                                    } else if !view.recovering && last_view.as_ref().is_some_and(|previous: &PlayerView| previous.recovering)
                                        && matches!(view.state, PlaybackState::Playing | PlaybackState::Paused) {
                                        let _ = events.send(Event::Status("播放已恢复".into()));
                                    }
                                    if view.state == PlaybackState::Failed && !player.is_resolving()
                                        && last_view.as_ref().is_none_or(|previous: &PlayerView| previous.state != PlaybackState::Failed) {
                                        let _ = events.send(Event::Status("音频播放失败，请重试".into()));
                                    }
                                    let _ = events.send(Event::Player(view.clone()));
                                    last_view = Some(view);
                                }
                                let revision = player.queue().revision();
                                if persist_enabled && (revision != last_queue_revision || last_checkpoint.elapsed() >= Duration::from_secs(30)) {
                                    if player.save_account_queue(&queue_store).is_err() {
                                        let _ = events.send(Event::Status("播放队列保存失败".into()));
                                    }
                                    last_queue_revision = revision;
                                    last_checkpoint = std::time::Instant::now();
                                }
                            }
                        }
                        Some(command) = receiver.recv() => {
                            if auth_busy && matches!(command, Command::Search(_) | Command::SearchNext | Command::ShowHistory | Command::SearchPlaylists(_) | Command::SearchAlbums(_) | Command::SearchArtists(_) | Command::MyPlaylists | Command::Collections(_) | Command::LoadCollectionState | Command::ToggleCollection | Command::ToggleLike(_) | Command::RetryLike(_) | Command::StartPlaylistEdit(_) | Command::CreatePlaylist(..) | Command::SavePlaylist(..) | Command::DeletePlaylist(_) | Command::OpenTrackPicker(_) | Command::AddTrackToPlaylist(..) | Command::RemovePlaylistTrack(..) | Command::OpenComments | Command::CommentSort(..) | Command::CommentsNext(_) | Command::CommentsRefresh(_) | Command::OpenCommentThread(..) | Command::CloseCommentThread(_) | Command::ReplyToComment(..) | Command::ClearCommentReply(_) | Command::ToggleCommentLike(..) | Command::PublishComment(..) | Command::Charts | Command::OpenChart(_) | Command::OpenPlaylist(_) | Command::OpenAlbum(_) | Command::OpenArtist(_) | Command::ArtistTracks | Command::ArtistAlbums | Command::LoadDetails | Command::BrowseBack | Command::BrowseNext | Command::PlayAllPlaylist | Command::Play(..) | Command::PlaySingle(_) | Command::PlayBrowse(..) | Command::Toggle | Command::SetPlaying(_) | Command::Stop | Command::Next | Command::Previous | Command::EditQueue(..) | Command::EditBrowseQueue(..) | Command::SeekRelative(_) | Command::SeekTo(..) | Command::SeekFromMedia(..)) {
                                if let Command::PublishComment(generation, ..) = &command { let _ = events.send(Event::CommentPostResult(*generation, false)); }
                                auth_view(&events, &account, true, "正在切换账号，请稍候");
                                continue;
                            }
                            let search_history_action = match &command {
                                Command::LoadSearchHistory => Some(SearchHistoryAction::Load),
                                Command::ClearSearchHistory => Some(SearchHistoryAction::Clear),
                                Command::Search(query) | Command::SearchPlaylists(query)
                                | Command::SearchAlbums(query) | Command::SearchArtists(query)
                                | Command::SearchOverview(_, query)
                                    if !query.trim().is_empty() => Some(SearchHistoryAction::Record(query.trim().to_owned())),
                                _ => None,
                            };
                            if !auth_busy && let Some(action) = search_history_action {
                                let identity = transport.session().snapshot();
                                let scope = if identity.is_authenticated() { identity.uid } else { "anonymous".into() };
                                if history_tx.send(HistoryWork::Search { scope, revision: identity.revision, action }).is_err() {
                                    let _ = events.send(Event::SearchHistory(Err("搜索历史服务不可用".into())));
                                }
                            }
                            collect_command_arms!(command [] [
                                settings_and_reports_arms [
                                    events, sender, transport, settings, volume, muted, main_placement,
                                    main_save_at, mv, player, overview_task, account_overview_task,
                                    month_report_task, month_poster_task, month_poster_request,
                                    month_poster_png, month_poster_saving, month_probe_task,
                                    month_probe_request, month_reports, month_track_task,
                                    month_track_request, month_report_request, active_month_report,
                                    month_playlist_writes, created_month_playlists,
                                    account_overview_request, weekly_tracks, overview_request,
                                    overview_sections, suggestion_task, hot_words_task, resolution_task,
                                    prefetch, browser, artwork_fetcher, comments, comment_image_task,
                                    comment_image, comment_image_request, account, auth_busy,
                                ]
                                accounts_arms [
                                    events, sender, transport, preferences, queue_store, persist_enabled,
                                    player, query_generation, query_text, search_results, search_loading,
                                    search_task, lyrics_key, lyrics_generation, lyrics_task, browser,
                                    qr_task, qr_generation, qr_interval, qr_lifetime,
                                    account_list_generation, cancel_account_requests, cancel_qr, secrets,
                                    account, auth_busy, sms_busy, last_sms,
                                ]
                                search_and_comments_arms [
                                    history_tx, events, sender, transport, player, query_generation,
                                    query_text, cursor, search_results, search_loading, search_task,
                                    browser, comments, comment_image_task, comment_image,
                                    comment_image_request, share_reports, liked_songs, batch_cancel,
                                    history_generation, history_results, history_revision,
                                    comments_current, auth_busy,
                                ]
                                playlist_edits_arms [
                                    playlist_edit_data, cover_preview_task, cover_preview_slot, events,
                                    sender, transport, query_generation, search_results, browser,
                                    liked_songs, playlist_mutation, batch_cancel, playlist_edit_generation,
                                    picker_generation, picker, picker_task, history_generation,
                                    history_results, history_revision, open_track_picker, auth_busy,
                                ]
                                catalog_arms [
                                    events, sender, transport, player, query_generation, query_text,
                                    cursor, search_results, search_loading, search_task, resolution_task,
                                    prefetch, browser, home, music_library, auth_busy,
                                ]
                                playback_arms [
                                    events, sender, transport, settings, preferences, volume, muted,
                                    floating_windows, floating_save_at, main_placement, main_save_at,
                                    audio_effects, ambient_root, current_filter, queue_store,
                                    persist_enabled, mv, player, lyrics_key, lyrics_generation,
                                    lyrics_task, resolution_task, quality_task, prefetch, browser, mpris,
                                    auth_busy,
                                ]
                            ]);
                        }
                        else => break,
                    }
                    if auth_busy { mv.close(false); }
                    mv.reconcile(transport.session().snapshot().revision, player.as_ref());
                    if let Some(player) = player.as_mut() {
                        if !player.quality_request_pending() && let Some(task) = quality_task.take() { task.abort(); }
                        for (track, revision) in player.take_recent_plays() {
                            let identity = transport.session().snapshot();
                            if identity.revision == revision {
                                let scope = if identity.is_authenticated() { identity.uid } else { "anonymous".into() };
                                if history_tx.send(HistoryWork::Record { scope, track: Box::new(track), revision }).is_err() {
                                    let _ = events.send(Event::Status("最近播放服务不可用".into()));
                                }
                            }
                        }
                        let reports = player.take_reports();
                        if reports.iter().any(|report| matches!(report, PlaybackReport::Started { .. })) {
                            let _ = events.send(Event::Status("正在播放".into()));
                        }
                        schedule_reports(&transport, reports, &mut report_tasks);
                        prefetch.retain_revision(transport.session().snapshot().revision);
                        prefetch.reconcile(if auth_busy { None } else { player.prefetch_candidate() });
                    }
                }
                drop(history_tx);
                let _ = history_thread.join();
            });
        }).expect("failed to start background service");
        Self {
            commands,
            events: event_receiver,
            worker: Some(worker),
        }
    }
}

#[derive(Default)]
pub(super) struct ServiceOptions {
    pub(super) startup_paths: Option<Result<DataPaths, String>>,
    pub(super) paths: Option<DataPaths>,
    pub(super) base_url: Option<String>,
    pub(super) library: Option<PathBuf>,
    pub(super) video_library: Option<PathBuf>,
    pub(super) audio_output: Option<String>,
    pub(super) lyrics_endpoint: Option<String>,
    pub(super) credentials: Option<SharedSecrets>,
    pub(super) qr_interval: Option<Duration>,
    pub(super) qr_lifetime: Option<Duration>,
}

impl Drop for Service {
    fn drop(&mut self) {
        let _ = self.commands.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
