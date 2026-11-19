//! Command routes for catalog.

macro_rules! catalog_arms {
    ($command:ident [$($arms:tt)*] $rest:tt;
        $events:ident, $sender:ident, $transport:ident,
        $player:ident, $query_generation:ident, $query_text:ident,
        $cursor:ident, $search_results:ident, $search_loading:ident,
        $search_task:ident, $resolution_task:ident, $prefetch:ident,
        $browser:ident, $home:ident, $music_library:ident,
        $auth_busy:ident $(,)?
    ) => {
        collect_command_arms!($command [$($arms)*
            Command::LoadLibrary => {
                if !$auth_busy {
                    $music_library.reload(Arc::clone(&$transport), $sender.clone());
                    let _ = $events.send(Event::Library($music_library.view()));
                }
            }
            Command::LibrarySelectGroup(index, generation) => {
                if !$auth_busy {
                    $music_library.select_group(
                        index,
                        generation,
                        Arc::clone(&$transport),
                        $sender.clone(),
                    );
                    let _ = $events.send(Event::Library($music_library.view()));
                }
            }
            Command::LibrarySelectChild(index, generation) => {
                if !$auth_busy {
                    $music_library.select_child(
                        index,
                        generation,
                        Arc::clone(&$transport),
                        $sender.clone(),
                    );
                    let _ = $events.send(Event::Library($music_library.view()));
                }
            }
            Command::LibrarySort(sort, generation) => {
                if !$auth_busy {
                    $music_library.sort(
                        sort,
                        generation,
                        Arc::clone(&$transport),
                        $sender.clone(),
                    );
                    let _ = $events.send(Event::Library($music_library.view()));
                }
            }
            Command::LibraryBack(generation) => {
                if !$auth_busy {
                    $music_library.back(generation, $transport.session().snapshot().revision);
                    let _ = $events.send(Event::Library($music_library.view()));
                }
            }
            Command::LibraryNext(generation) => {
                if !$auth_busy && generation == $music_library.generation() {
                    $music_library.next(Arc::clone(&$transport), $sender.clone());
                    let _ = $events.send(Event::Library($music_library.view()));
                }
            }
            Command::LibraryOpenAlbum(index, id, generation) => {
                if !$auth_busy
                    && let Some(album) = $music_library.album(
                        index,
                        id,
                        generation,
                        $transport.session().snapshot().revision,
                    )
                {
                    $browser.open_external_album(
                        Arc::clone(&$transport),
                        $sender.clone(),
                        album,
                    );
                    $browser.load_collection_state(Arc::clone(&$transport), $sender.clone());
                    $browser.load_details(Arc::clone(&$transport), $sender.clone());
                    let _ = $events.send(Event::Browse($browser.view()));
                    let _ = $events.send(Event::LibraryAlbumOpened);
                } else {
                    let _ = $events.send(Event::Status("乐库内容已变化，请重试".into()));
                }
            }
            Command::LibraryNavigationDone(epoch, revision, result) => {
                if !$auth_busy
                    && $music_library.finish_navigation(
                        epoch,
                        revision,
                        result,
                        $transport.session().snapshot().revision,
                    )
                {
                    let _ = $events.send(Event::Library($music_library.view()));
                }
            }
            Command::LibraryAlbumsDone(epoch, revision, $cursor, result) => {
                if !$auth_busy
                    && $music_library.finish_albums(
                        epoch,
                        revision,
                        $cursor,
                        result,
                        $transport.session().snapshot().revision,
                    )
                {
                    let _ = $events.send(Event::Library($music_library.view()));
                }
            }
            Command::LoadHome => {
                if !$auth_busy {
                    $home.reload(Arc::clone(&$transport), $sender.clone());
                    let _ = $events.send(Event::Home($home.view()));
                }
            }
            Command::HomeNext => {
                if !$auth_busy {
                    $home.load_more(3, Arc::clone(&$transport), $sender.clone());
                    let _ = $events.send(Event::Home($home.view()));
                }
            }
            Command::HomeRefresh(index, generation) => {
                if !$auth_busy {
                    $home.refresh(
                        index,
                        generation,
                        Arc::clone(&$transport),
                        $sender.clone(),
                    );
                    let _ = $events.send(Event::Home($home.view()));
                }
            }
            Command::HomeOpenAi(item, group, generation) => {
                if !$auth_busy {
                    $home.open_ai(
                        item,
                        group,
                        generation,
                        Arc::clone(&$transport),
                        $sender.clone(),
                    );
                    let _ = $events.send(Event::Home($home.view()));
                }
            }
            Command::HomeCloseAi(generation) => {
                if !$auth_busy {
                    $home.close_ai(generation, $transport.session().snapshot().revision);
                    let _ = $events.send(Event::Home($home.view()));
                }
            }
            Command::HomeOpenPlaylist(item, index, id, generation) => {
                if !$auth_busy
                    && let Some(playlist) = $home.playlist(
                        item,
                        index,
                        id,
                        generation,
                        $transport.session().snapshot().revision,
                    )
                {
                    $browser.open_external(Arc::clone(&$transport), $sender.clone(), playlist);
                    $browser.load_collection_state(Arc::clone(&$transport), $sender.clone());
                    let _ = $events.send(Event::Browse($browser.view()));
                    let _ = $events.send(Event::HomePlaylistOpened);
                } else {
                    let _ = $events.send(Event::Status("发现内容已变化，请重试".into()));
                }
            }
            Command::HomePlay(item, index, id, generation) => {
                if !$auth_busy
                    && let Some(track) = $home.track(
                        item,
                        index,
                        id,
                        generation,
                        $transport.session().snapshot().revision,
                    )
                    && let Some($player) = $player.as_mut()
                {
                    if $browser.cancel_play_all() {
                        let _ = $events.send(Event::Browse($browser.view()));
                    }
                    match $player.enqueue_and_play(track) {
                        Ok(ticket) => {
                            let _ = $events.send(Event::Status("正在获取音源…".into()));
                            launch(ticket, &$sender, &mut $resolution_task, &mut $prefetch);
                        }
                        Err(_) => {
                            let _ = $events.send(Event::Status("无法开始播放".into()));
                        }
                    }
                } else {
                    let _ = $events.send(Event::Status("歌曲或账号已变化，请重试".into()));
                }
            }
            Command::HomePlayAll(generation) => {
                if !$auth_busy
                    && let Some(tracks) =
                        $home.ai_tracks(generation, $transport.session().snapshot().revision)
                    && let Some($player) = $player.as_mut()
                {
                    if $browser.cancel_play_all() {
                        let _ = $events.send(Event::Browse($browser.view()));
                    }
                    match $player.replace(tracks, 0) {
                        Ok(ticket) => {
                            let _ = $events.send(Event::Status("正在获取音源…".into()));
                            launch(ticket, &$sender, &mut $resolution_task, &mut $prefetch);
                        }
                        Err(_) => {
                            let _ = $events.send(Event::Status("无法开始播放".into()));
                        }
                    }
                } else {
                    let _ = $events.send(Event::Status("歌单或账号已变化，请重试".into()));
                }
            }
            Command::HomeQueueModule(item, generation, account_revision, queue_revision) => {
                if !$auth_busy
                    && let Some(tracks) = $home.module_tracks(
                        item,
                        generation,
                        $transport.session().snapshot().revision,
                    )
                {
                    let _ = $sender.send(Command::EditQueue(
                        account_revision,
                        queue_revision,
                        QueueEdit::AppendMany(tracks),
                    ));
                } else {
                    let _ = $events.send(Event::Status("发现内容已变化，请重试".into()));
                }
            }
            Command::HomeLayoutDone(epoch, revision, result) => {
                if !$auth_busy
                    && $home.finish_layout(
                        epoch,
                        revision,
                        result,
                        Arc::clone(&$transport),
                        $sender.clone(),
                    )
                {
                    let _ = $events.send(Event::Home($home.view()));
                }
            }
            Command::HomeBatchDone(epoch, revision, start, result) => {
                if !$auth_busy
                    && $home.finish_batch(
                        epoch,
                        revision,
                        start,
                        result,
                        $transport.session().snapshot().revision,
                    )
                {
                    let _ = $events.send(Event::Home($home.view()));
                }
            }
            Command::HomeRefreshDone(epoch, revision, index, result) => {
                if !$auth_busy
                    && $home.finish_refresh(
                        epoch,
                        revision,
                        index,
                        result,
                        $transport.session().snapshot().revision,
                    )
                {
                    let _ = $events.send(Event::Home($home.view()));
                }
            }
            Command::HomeAiDone(epoch, revision, generation, result) => {
                if !$auth_busy
                    && $home.finish_ai(
                        epoch,
                        revision,
                        generation,
                        result,
                        $transport.session().snapshot().revision,
                    )
                {
                    let _ = $events.send(Event::Home($home.view()));
                }
            }
            Command::Charts => {
                $browser.charts(Arc::clone(&$transport), $sender.clone());
                let _ = $events.send(Event::Browse($browser.view()));
            }
            Command::OpenChart(chart) => {
                $browser.open_chart(Arc::clone(&$transport), $sender.clone(), chart);
                $browser.load_collection_state(Arc::clone(&$transport), $sender.clone());
                let _ = $events.send(Event::Browse($browser.view()));
            }
            Command::OpenPlaylist(playlist) => {
                $browser.open(Arc::clone(&$transport), $sender.clone(), playlist);
                $browser.load_collection_state(Arc::clone(&$transport), $sender.clone());
                let _ = $events.send(Event::Browse($browser.view()));
            }
            Command::OpenAlbum(album) => {
                $browser.open_album(Arc::clone(&$transport), $sender.clone(), album);
                $browser.load_collection_state(Arc::clone(&$transport), $sender.clone());
                let _ = $events.send(Event::Browse($browser.view()));
            }
            Command::OpenArtist(artist) => {
                $browser.open_artist(Arc::clone(&$transport), $sender.clone(), artist);
                $browser.load_collection_state(Arc::clone(&$transport), $sender.clone());
                let _ = $events.send(Event::Browse($browser.view()));
            }
            Command::ArtistTracks => {
                $browser.show_artist_tracks(Arc::clone(&$transport), $sender.clone());
                let _ = $events.send(Event::Browse($browser.view()));
            }
            Command::ArtistAlbums => {
                $browser.show_artist_albums(Arc::clone(&$transport), $sender.clone());
                let _ = $events.send(Event::Browse($browser.view()));
            }
            Command::LoadDetails => {
                $browser.load_details(Arc::clone(&$transport), $sender.clone());
                let _ = $events.send(Event::Browse($browser.view()));
            }
            Command::DetailDone(generation, revision, result) => {
                if !$auth_busy
                    && $browser.finish_details(
                        generation,
                        revision,
                        $transport.session().snapshot().revision,
                        result,
                    )
                {
                    let _ = $events.send(Event::Browse($browser.view()));
                }
            }
            Command::BrowseBack => {
                $browser.back();
                $browser.load_collection_state(Arc::clone(&$transport), $sender.clone());
                let _ = $events.send(Event::Browse($browser.view()));
            }
            Command::BrowseNext => {
                $browser.next(Arc::clone(&$transport), $sender.clone());
                let _ = $events.send(Event::Browse($browser.view()));
            }
            Command::PlayAllPlaylist => {
                if $player.is_none() {
                    let _ = $events.send(Event::Status("播放器不可用".into()));
                    continue;
                }
                $browser.play_all(Arc::clone(&$transport), $sender.clone());
                let _ = $events.send(Event::Browse($browser.view()));
                if let Some(tracks) = $browser.take_play_all()
                    && let Some($player) = $player.as_mut()
                {
                    match $player.replace_with_origin(tracks, 0, $browser.playback_origin()) {
                        Ok(ticket) => {
                            launch(ticket, &$sender, &mut $resolution_task, &mut $prefetch)
                        }
                        Err(_) => {
                            let _ = $events.send(Event::Status("无法开始播放".into()));
                        }
                    }
                }
            }
            Command::BrowseDone(generation, revision, result) => {
                if !$auth_busy
                    && $browser.finish(
                        generation,
                        revision,
                        $transport.session().snapshot().revision,
                        result,
                    )
                {
                    let _ = $events.send(Event::Browse($browser.view()));
                    if let Some(tracks) = $browser.take_play_all()
                        && let Some($player) = $player.as_mut()
                    {
                        match $player.replace_with_origin(tracks, 0, $browser.playback_origin()) {
                            Ok(ticket) => {
                                launch(ticket, &$sender, &mut $resolution_task, &mut $prefetch)
                            }
                            Err(_) => {
                                let _ = $events.send(Event::Status("无法开始播放".into()));
                            }
                        }
                    }
                }
            }
            Command::SearchNext
                if !$query_text.is_empty() && !$search_loading && !$cursor.exhausted() =>
            {
                $search_loading = true;
                let _ = $events.send(Event::Search(SearchView {
                    generation: $query_generation,
                    tracks: $search_results.clone(),
                    has_more: true,
                    loading: true,
                }));
                $search_task = Some(launch_search(
                    Arc::clone(&$transport),
                    $query_text.clone(),
                    $cursor.clone(),
                    $query_generation,
                    $sender.clone(),
                ));
            }
            Command::SearchNext => {}
            Command::SearchDone(generation, next_cursor, result)
                if generation == $query_generation =>
            {
                $search_task = None;
                $search_loading = false;
                match result {
                    Ok(tracks) => {
                        $cursor = next_cursor;
                        $search_results.extend(tracks);
                        let _ = $events.send(Event::Status(if $search_results.is_empty() {
                            "没有找到歌曲".into()
                        } else {
                            format!("已加载 {} 首歌曲", $search_results.len())
                        }));
                    }
                    Err(error) => {
                        let _ = $events.send(Event::Status(error));
                    }
                }
                let _ = $events.send(Event::Search(SearchView {
                    generation: $query_generation,
                    tracks: $search_results.clone(),
                    has_more: !$cursor.exhausted(),
                    loading: false,
                }));
            }
            Command::SearchDone(..) => {}
        ] $rest)
    };
}
