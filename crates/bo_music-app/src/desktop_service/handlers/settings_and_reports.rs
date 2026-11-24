//! Command routes for settings and reports.

macro_rules! settings_and_reports_arms {
    ($command:ident [$($arms:tt)*] $rest:tt;
        $events:ident, $sender:ident, $transport:ident,
        $settings:ident, $volume:ident, $muted:ident,
        $main_placement:ident, $main_save_at:ident, $mv:ident,
        $player:ident, $overview_task:ident, $account_overview_task:ident,
        $month_report_task:ident, $month_poster_task:ident, $month_poster_request:ident,
        $month_poster_png:ident, $month_poster_saving:ident, $month_probe_task:ident,
        $month_probe_request:ident, $month_reports:ident, $month_track_task:ident,
        $month_track_request:ident, $month_report_request:ident, $active_month_report:ident,
        $month_playlist_writes:ident, $created_month_playlists:ident, $account_overview_request:ident,
        $weekly_tracks:ident, $overview_request:ident, $overview_sections:ident,
        $suggestion_task:ident, $hot_words_task:ident, $resolution_task:ident,
        $prefetch:ident, $browser:ident, $artwork_fetcher:ident,
        $comments:ident, $comment_image_task:ident, $comment_image:ident,
        $comment_image_request:ident, $account:ident, $auth_busy:ident $(,)?
    ) => {
        collect_command_arms!($command [$($arms)*
            Command::SaveAppearance(value) => {
                let _ = $events.send(Event::AppearanceSaved(
                    $settings.set_appearance(value).is_ok(),
                ));
            }
            Command::SaveMainWindow(bounds) => {
                $main_placement = Some(bounds);
                $main_save_at = Some(std::time::Instant::now() + Duration::from_millis(400));
            }
            Command::SaveLyricPosterStyle(style) => {
                if $settings.set_lyric_poster_style(style).is_err() {
                    let _ = $events.send(Event::Status("歌词海报样式未能保存".into()));
                }
            }
            Command::SaveShortcuts(value) => {
                let _ = $events.send(Event::ShortcutsSaved(
                    $settings.set_shortcuts(value).is_ok(),
                ));
            }
            Command::ToggleMute => {
                if let Some($player) = $player.as_ref() {
                    let output = if $muted { $volume } else { 0. };
                    if $player.$volume(output).is_ok() {
                        $muted = !$muted;
                    } else {
                        let _ = $events.send(Event::Status("音量设置失败".into()));
                    }
                }
            }
            Command::OpenMv(revision, id) => {
                if !$auth_busy {
                    $mv.open(
                        revision,
                        id,
                        if $muted { 0. } else { $volume },
                        $player.as_ref(),
                    );
                }
            }
            Command::ControlMv(action) => {
                if !$auth_busy {
                    $mv.control(action);
                }
            }
            Command::CloseMv => $mv.close(!$auth_busy),
            Command::MvLoaded(request, result) => {
                if !$auth_busy {
                    $mv.loaded(request, result, $player.as_mut());
                }
            }
            Command::MvResumeAudio(resume) => {
                if !$auth_busy {
                    $mv.resume_audio(resume, $player.as_mut());
                }
            }
            Command::LoadMonthReport(generation, period) => {
                if let Some(task) = $month_report_task.take() {
                    task.abort();
                }
                if let Some(task) = $month_poster_task.take() {
                    task.abort();
                }
                $month_poster_request = None;
                $month_poster_png = None;
                if let Some(task) = $month_track_task.take() {
                    task.abort();
                }
                $month_track_request = $month_track_request.wrapping_add(1);
                $month_report_request = None;
                $active_month_report = None;
                if $month_probe_request.is_some_and(|(_, _, year)| year != period.year()) {
                    if let Some(task) = $month_probe_task.take() {
                        task.abort();
                    }
                    $month_probe_request = None;
                }
                let identity = $transport.session().snapshot();
                if $auth_busy || !identity.is_authenticated() {
                    let _ = $events.send(Event::MonthReport(generation, Err(())));
                    continue;
                }
                let revision = identity.revision;
                $month_report_request = Some((generation, revision, period));
                if let Some(result) = $month_reports.get(period) {
                    $active_month_report = result.as_ref().ok().and_then(Clone::clone);
                    let _ = $events.send(Event::MonthReport(generation, result));
                    continue;
                }
                let $transport = Arc::clone(&$transport);
                let $sender = $sender.clone();
                $month_report_task = Some(tokio::spawn(async move {
                    let result =
                        $month_reports::fetch_report(&$transport, period, revision).await;
                    let _ = $sender.send(Command::MonthReportDone(
                        generation, revision, period, result,
                    ));
                }));
            }
            Command::MonthReportDone(generation, revision, period, result) => {
                if $auth_busy
                    || $month_report_request != Some((generation, revision, period))
                    || revision != $transport.session().snapshot().revision
                {
                    continue;
                }
                $month_reports.remember(period, &result);
                $active_month_report = result.as_ref().ok().and_then(Clone::clone);
                let _ = $events.send(Event::MonthReport(generation, result));
            }
            Command::ProbeMonthReports(generation, first, last) => {
                let revision = $transport.session().snapshot().revision;
                let Some((active_generation, active_revision, selected)) = $month_report_request
                else {
                    continue;
                };
                if $auth_busy
                    || active_generation != generation
                    || active_revision != revision
                    || first.year() != selected.year()
                    || last.year() != selected.year()
                    || first > last
                {
                    continue;
                }
                if let Some(task) = $month_probe_task.take() {
                    task.abort();
                }
                $month_probe_request = Some((generation, revision, selected.year()));
                let mut pending = Vec::new();
                for month in first.month()..=last.month() {
                    let Some(period) = MonthPeriod::new(selected.year(), month) else {
                        continue;
                    };
                    if let Some(availability) = $month_reports.availability(period) {
                        let _ = $events.send(Event::MonthAvailability(
                            generation,
                            period,
                            availability,
                        ));
                    } else if period != selected {
                        pending.push(period);
                    }
                }
                let $transport = Arc::clone(&$transport);
                let $sender = $sender.clone();
                $month_probe_task = Some(tokio::spawn(async move {
                    $month_reports::probe_months(
                        $transport,
                        revision,
                        pending,
                        |period, report| {
                            let _ = $sender.send(Command::ProbeMonthReportResult(
                                generation, revision, period, report,
                            ));
                        },
                    )
                    .await;
                    let _ = $sender.send(Command::ProbeMonthReportsFinished(generation, revision));
                }));
            }
            Command::ProbeMonthReportResult(generation, revision, period, result) => {
                if $auth_busy
                    || revision != $transport.session().snapshot().revision
                    || $month_probe_request != Some((generation, revision, period.year()))
                {
                    continue;
                }
                let availability = if let Some(known) = $month_reports.availability(period) {
                    known
                } else {
                    $month_reports.remember(period, &result);
                    $month_reports::availability(&result)
                };
                let _ = $events.send(Event::MonthAvailability(generation, period, availability));
            }
            Command::ProbeMonthReportsFinished(generation, revision) => {
                if !$auth_busy
                    && revision == $transport.session().snapshot().revision
                    && $month_probe_request.is_some_and(|(request, $account, _)| {
                        request == generation && $account == revision
                    })
                {
                    $month_probe_request = None;
                    let _ = $events.send(Event::MonthAvailabilityFinished(generation));
                }
            }
            Command::CloseMonthReport(generation) => {
                if $month_report_request.is_some_and(|(request, _, _)| request == generation) {
                    if let Some(task) = $month_report_task.take() {
                        task.abort();
                    }
                    if let Some(task) = $month_poster_task.take() {
                        task.abort();
                    }
                    $month_poster_request = None;
                    $month_poster_png = None;
                    if let Some(task) = $month_probe_task.take() {
                        task.abort();
                    }
                    $month_probe_request = None;
                    if let Some(task) = $month_track_task.take() {
                        task.abort();
                    }
                    $month_report_request = None;
                    $active_month_report = None;
                }
            }
            Command::RenderMonthPoster(generation, request, style) => {
                let revision = $transport.session().snapshot().revision;
                if $auth_busy
                    || $month_report_request.is_none_or(|(active, $account, _)| {
                        active != generation || $account != revision
                    })
                {
                    continue;
                }
                let Some(report) = $active_month_report.as_deref().cloned() else {
                    continue;
                };
                if let Some(task) = $month_poster_task.take() {
                    task.abort();
                }
                $month_poster_request = Some((generation, request, revision));
                $month_poster_png = None;
                let nickname = $account
                    .as_ref()
                    .and_then(|$account| $account.nickname.clone());
                let fetcher = $artwork_fetcher.clone();
                let $sender = $sender.clone();
                $month_poster_task = Some(tokio::spawn(async move {
                    let result =
                        crate::month_poster::render(report, nickname, style, fetcher).await;
                    let _ = $sender.send(Command::MonthPosterDone(
                        generation, request, revision, result,
                    ));
                }));
            }
            Command::MonthPosterDone(generation, request, revision, result) => {
                if $auth_busy
                    || revision != $transport.session().snapshot().revision
                    || $month_poster_request != Some((generation, request, revision))
                {
                    continue;
                }
                let result = result.map(Arc::new);
                $month_poster_png = result.as_ref().ok().cloned();
                let _ = $events.send(Event::MonthPoster(generation, request, result));
            }
            Command::SaveMonthPoster(generation, request, path) => {
                let revision = $transport.session().snapshot().revision;
                if $auth_busy || $month_poster_request != Some((generation, request, revision)) {
                    continue;
                }
                if $month_poster_saving == Some((generation, request, revision)) {
                    continue;
                }
                let Some(png) = $month_poster_png.clone() else {
                    continue;
                };
                $month_poster_saving = Some((generation, request, revision));
                let $sender = $sender.clone();
                tokio::task::spawn_blocking(move || {
                    let saved = atomic::replace(&path, &png).is_ok();
                    let _ = $sender.send(Command::MonthPosterSaved(
                        generation, request, revision, saved,
                    ));
                });
            }
            Command::MonthPosterSaved(generation, request, revision, saved) => {
                if $month_poster_saving == Some((generation, request, revision)) {
                    $month_poster_saving = None;
                }
                if !$auth_busy && revision == $transport.session().snapshot().revision {
                    let _ = $events.send(Event::MonthPosterSaved(generation, request, saved));
                }
            }
            Command::CreateMonthPlaylist(generation, period) => {
                let revision = $transport.session().snapshot().revision;
                if $auth_busy
                    || $month_report_request != Some((generation, revision, period))
                    || $active_month_report.is_none()
                {
                    continue;
                }
                if $created_month_playlists.contains(&period) {
                    let _ = $events.send(Event::MonthPlaylist(period, true));
                    continue;
                }
                if !$month_playlist_writes.insert((revision, period)) {
                    continue;
                }
                let $transport = Arc::clone(&$transport);
                let $sender = $sender.clone();
                tokio::spawn(async move {
                    let success =
                        create_report_playlist_at_revision(&$transport, period, revision)
                            .await
                            .unwrap_or(false);
                    let _ = $sender.send(Command::MonthPlaylistDone(revision, period, success));
                });
            }
            Command::MonthPlaylistDone(revision, period, success) => {
                if !$month_playlist_writes.remove(&(revision, period))
                    || $auth_busy
                    || revision != $transport.session().snapshot().revision
                {
                    continue;
                }
                if success {
                    $created_month_playlists.insert(period);
                    if $browser.showing_mine_list(revision) {
                        $browser.mine(Arc::clone(&$transport), $sender.clone());
                        let _ = $events.send(Event::Browse($browser.view()));
                    }
                }
                let _ = $events.send(Event::MonthPlaylist(period, success));
            }
            Command::PlayMonthTrack(generation, id) => {
                let revision = $transport.session().snapshot().revision;
                if $auth_busy
                    || !$month_report_request.is_some_and(|(request, $account, _)| {
                        request == generation && $account == revision
                    })
                    || !$active_month_report
                        .as_ref()
                        .is_some_and(|report| report.top_tracks.iter().any(|track| track.id == id))
                {
                    continue;
                }
                let Some($player) = $player.as_ref() else {
                    continue;
                };
                let queue_revision = $player.queue().revision();
                if let Some(task) = $month_track_task.take() {
                    task.abort();
                }
                $month_track_request = $month_track_request.wrapping_add(1);
                let request = $month_track_request;
                let $transport = Arc::clone(&$transport);
                let $sender = $sender.clone();
                let _ = $events.send(Event::Status("正在读取歌曲信息…".into()));
                $month_track_task = Some(tokio::spawn(async move {
                    let track = track_at_revision(&$transport, id, revision)
                        .await
                        .ok()
                        .flatten()
                        .filter(|track| track.id == id)
                        .map(Box::new);
                    let _ = $sender.send(Command::MonthTrackDone(request, queue_revision, track));
                }));
            }
            Command::MonthTrackDone(request, queue_revision, track) => {
                if $auth_busy
                    || request != $month_track_request
                    || $month_report_request.is_none_or(|(_, revision, _)| {
                        revision != $transport.session().snapshot().revision
                    })
                {
                    continue;
                }
                let Some($player) = $player
                    .as_mut()
                    .filter(|$player| $player.queue().revision() == queue_revision)
                else {
                    continue;
                };
                let Some(track) = track else {
                    let _ = $events.send(Event::Status("无法读取歌曲信息，请重试".into()));
                    continue;
                };
                if $browser.cancel_play_all() {
                    let _ = $events.send(Event::Browse($browser.view()));
                }
                match $player.enqueue_and_play(*track) {
                    Ok(ticket) => {
                        let _ = $events.send(Event::Status("正在获取音源…".into()));
                        launch(ticket, &$sender, &mut $resolution_task, &mut $prefetch);
                    }
                    Err(_) => {
                        let _ = $events.send(Event::Status("无法开始播放".into()));
                    }
                }
            }
            Command::LoadAccountOverview(generation) => {
                if let Some(task) = $account_overview_task.take() {
                    task.abort();
                }
                $account_overview_request = None;
                $weekly_tracks.clear();
                let identity = $transport.session().snapshot();
                if $auth_busy || !identity.is_authenticated() {
                    let _ = $events.send(Event::AccountOverview(generation, Box::new(Err(()))));
                    continue;
                }
                let revision = identity.revision;
                $account_overview_request = Some((generation, revision));
                let $transport = Arc::clone(&$transport);
                let $sender = $sender.clone();
                $account_overview_task = Some(tokio::spawn(async move {
                    let result = overview_at_revision(&$transport, revision)
                        .await
                        .map_err(|_| ());
                    let _ = $sender.send(Command::AccountOverviewDone(
                        generation,
                        revision,
                        Box::new(result),
                    ));
                }));
            }
            Command::AccountOverviewDone(generation, revision, result) => {
                if $auth_busy
                    || $account_overview_request != Some((generation, revision))
                    || revision != $transport.session().snapshot().revision
                {
                    continue;
                }
                if let Ok(overview) = result.as_ref()
                    && let Some(weekly) = &overview.weekly
                {
                    $weekly_tracks.clone_from(weekly);
                }
                let _ = $events.send(Event::AccountOverview(generation, result));
            }
            Command::PlayWeeklyTrack(generation, index, id) => {
                let revision = $transport.session().snapshot().revision;
                if $auth_busy || $account_overview_request != Some((generation, revision)) {
                    continue;
                }
                if let Some(entry) = $weekly_tracks
                    .get(index)
                    .filter(|entry| entry.track.id == id)
                    && let Some($player) = $player.as_mut()
                {
                    if $browser.cancel_play_all() {
                        let _ = $events.send(Event::Browse($browser.view()));
                    }
                    match $player.enqueue_and_play(entry.track.clone()) {
                        Ok(ticket) => {
                            let _ = $events.send(Event::Status("正在获取音源…".into()));
                            launch(ticket, &$sender, &mut $resolution_task, &mut $prefetch);
                        }
                        Err(_) => {
                            let _ = $events.send(Event::Status("无法开始播放".into()));
                        }
                    }
                }
            }
            Command::SearchOverview(generation, query) => {
                if let Some(task) = $overview_task.take() {
                    task.abort();
                }
                $overview_sections.clear();
                $overview_request = None;
                if $auth_busy || query.trim().is_empty() {
                    continue;
                }
                let revision = $transport.session().snapshot().revision;
                $overview_request = Some((generation, revision));
                let $transport = Arc::clone(&$transport);
                let $sender = $sender.clone();
                $overview_task = Some(tokio::spawn(async move {
                    let result = comprehensive_at_revision(&$transport, query.trim(), revision)
                        .await
                        .map_err(|_| ());
                    let _ =
                        $sender.send(Command::SearchOverviewDone(generation, revision, result));
                }));
            }
            Command::SearchOverviewDone(generation, revision, result) => {
                if $auth_busy
                    || $overview_request != Some((generation, revision))
                    || revision != $transport.session().snapshot().revision
                {
                    continue;
                }
                if let Ok(sections) = &result {
                    $overview_sections.clone_from(sections);
                }
                let _ = $events.send(Event::SearchOverview(generation, result));
            }
            Command::SelectSearchOverview(generation, section, index) => {
                let revision = $transport.session().snapshot().revision;
                if $auth_busy || $overview_request != Some((generation, revision)) {
                    continue;
                }
                match $overview_sections.get(section) {
                    Some(SearchSection::Tracks(items)) => {
                        if let Some(track) = items.get(index).filter(|item| item.id > 0).cloned()
                            && let Some($player) = $player.as_mut()
                        {
                            if $browser.cancel_play_all() {
                                let _ = $events.send(Event::Browse($browser.view()));
                            }
                            match $player.enqueue_and_play(track) {
                                Ok(ticket) => {
                                    let _ = $events.send(Event::Status("正在获取音源…".into()));
                                    launch(
                                        ticket,
                                        &$sender,
                                        &mut $resolution_task,
                                        &mut $prefetch,
                                    );
                                }
                                Err(_) => {
                                    let _ = $events.send(Event::Status("无法开始播放".into()));
                                }
                            }
                        }
                        continue;
                    }
                    Some(SearchSection::Playlists(items)) => {
                        let Some(item) = items.get(index).filter(|item| item.id > 0).cloned()
                        else {
                            continue;
                        };
                        $browser.open_external(Arc::clone(&$transport), $sender.clone(), item);
                    }
                    Some(SearchSection::Albums(items)) => {
                        let Some(item) = items.get(index).filter(|item| item.id > 0).cloned()
                        else {
                            continue;
                        };
                        $browser.open_external_album(
                            Arc::clone(&$transport),
                            $sender.clone(),
                            item,
                        );
                    }
                    Some(SearchSection::Artists(items)) => {
                        let Some(item) = items.get(index).filter(|item| item.id > 0).cloned()
                        else {
                            continue;
                        };
                        $browser.open_external_artist(
                            Arc::clone(&$transport),
                            $sender.clone(),
                            item,
                        );
                    }
                    None => continue,
                }
                $browser.load_collection_state(Arc::clone(&$transport), $sender.clone());
                $browser.load_details(Arc::clone(&$transport), $sender.clone());
                let _ = $events.send(Event::Browse($browser.view()));
                let _ = $events.send(Event::SearchOverviewOpened(generation));
            }
            Command::LoadSearchHistory | Command::ClearSearchHistory => {}
            Command::SearchHistoryDone(revision, result) => {
                if !$auth_busy && revision == $transport.session().snapshot().revision {
                    let _ = $events.send(Event::SearchHistory(result));
                }
            }
            Command::SearchSuggestions(generation, query) => {
                if let Some(task) = $suggestion_task.take() {
                    task.abort();
                }
                if $auth_busy || query.trim().is_empty() || query.len() > 512 {
                    continue;
                }
                let $transport = Arc::clone(&$transport);
                let $events = $events.clone();
                let revision = $transport.session().snapshot().revision;
                $suggestion_task = Some(tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                    let words = suggestions_at_revision(&$transport, &query, revision)
                        .await
                        .unwrap_or_default();
                    if $transport.session().snapshot().revision == revision {
                        let _ = $events.send(Event::SearchSuggestions(
                            generation,
                            words.into_iter().take(8).collect(),
                        ));
                    }
                }));
            }
            Command::LoadSearchHotWords => {
                if $hot_words_task
                    .as_ref()
                    .is_some_and(|task| !task.is_finished())
                {
                    continue;
                }
                let $transport = Arc::clone(&$transport);
                let $events = $events.clone();
                $hot_words_task = Some(tokio::spawn(async move {
                    let words = hot_words(&$transport).await.ok().map(|words| {
                        words
                            .into_iter()
                            .take(12)
                            .map(|word| word.keyword)
                            .collect()
                    });
                    let _ = $events.send(Event::SearchHotWords(words));
                }));
            }
            Command::LoadCommentImage(request, generation, id, poster) => {
                if let Some(task) = $comment_image_task.take() {
                    task.abort();
                }
                $comment_image = None;
                $comment_image_request = None;
                let revision = $transport.session().snapshot().revision;
                let comment = $comments
                    .visible_comment(generation, revision, id)
                    .cloned();
                let track = $player
                    .as_ref()
                    .and_then(|$player| $player.queue().current())
                    .filter(|track| $comments.track_id() == Some(track.id))
                    .cloned();
                if !$auth_busy && let (Some(comment), Some(track)) = (comment, track) {
                    $comment_image_request = Some((request, generation, revision, id));
                    let $sender = $sender.clone();
                    let fetcher = $artwork_fetcher.clone();
                    let link = sharing::link(ShareTarget::Track, track.id, "").ok();
                    $comment_image_task = Some(tokio::spawn(async move {
                        let result = if poster {
                            crate::comment_poster::render(comment, track, link, fetcher).await
                        } else if let (Some(address), Some(fetcher)) =
                            (comment.image_url.filter(|url| url.len() <= 4096), fetcher)
                        {
                            fetcher.fetch_comment_preview(&address).await
                        } else {
                            Err(())
                        };
                        let _ = $sender.send(Command::CommentImageDone(
                            request, generation, revision, id, result,
                        ));
                    }));
                } else {
                    let _ = $events.send(Event::CommentImage(request, Err(())));
                }
            }
            Command::CommentImageDone(request, generation, revision, id, result) => {
                if $auth_busy
                    || $comment_image_request != Some((request, generation, revision, id))
                    || $comments
                        .visible_comment(generation, revision, id)
                        .is_none()
                    || $transport.session().snapshot().revision != revision
                {
                    continue;
                }
                let result = result.map(Arc::new);
                $comment_image = result.as_ref().ok().cloned();
                let _ = $events.send(Event::CommentImage(request, result));
            }
            Command::SaveCommentImage(request, path) => {
                let result = $comment_image_request
                    .filter(|&(active, generation, revision, id)| {
                        !$auth_busy
                            && active == request
                            && $comments
                                .visible_comment(generation, revision, id)
                                .is_some()
                            && revision == $transport.session().snapshot().revision
                    })
                    .and($comment_image.as_ref())
                    .cloned();
                let $events = $events.clone();
                tokio::task::spawn_blocking(move || {
                    let saved = result.is_some_and(|bytes| atomic::replace(&path, &bytes).is_ok());
                    let _ = $events.send(Event::CommentImageSaved(request, saved));
                });
            }
            Command::LoadArtwork(address) => {
                if address.len() > 4096 {
                    let _ = $events.send(Event::Artwork(address, Err(())));
                    continue;
                }
                let Some(fetcher) = $artwork_fetcher.clone() else {
                    let _ = $events.send(Event::Artwork(address, Err(())));
                    continue;
                };
                let $events = $events.clone();
                tokio::spawn(async move {
                    let result = fetcher.fetch_thumbnail(&address).await;
                    let _ = $events.send(Event::Artwork(address, result));
                });
            }
        ] $rest)
    };
}
