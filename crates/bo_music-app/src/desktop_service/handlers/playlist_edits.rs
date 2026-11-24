//! Command routes for playlist edits.

macro_rules! playlist_edits_arms {
    ($command:ident [$($arms:tt)*] $rest:tt;
        $playlist_edit_data:ident, $cover_preview_task:ident, $cover_preview_slot:ident,
        $events:ident, $sender:ident, $transport:ident,
        $query_generation:ident, $search_results:ident, $browser:ident,
        $liked_songs:ident, $playlist_mutation:ident, $batch_cancel:ident,
        $playlist_edit_generation:ident, $picker_generation:ident, $picker:ident,
        $picker_task:ident, $history_generation:ident, $history_results:ident,
        $history_revision:ident, $open_track_picker:ident, $auth_busy:ident $(,)?
    ) => {
        collect_command_arms!($command [$($arms)*
            Command::OpenTrackPicker(track) => {
                let identity = $transport.session().snapshot();
                if !identity.is_authenticated() || track.id <= 0 {
                    let _ = $events.send(Event::Status("请登录后选择有效歌曲".into()));
                    continue;
                }
                $open_track_picker!(vec![track], identity.revision);
            }
            Command::OpenBatchPicker(source, selected) => {
                let identity = $transport.session().snapshot();
                let tracks = selected_tracks(
                    source,
                    &selected,
                    &$browser,
                    ($query_generation, &$search_results),
                    ($history_generation, $history_revision, &$history_results),
                    identity.revision,
                );
                if $auth_busy || !identity.is_authenticated() || tracks.is_none() {
                    let _ = $events.send(Event::Status("歌曲或账号已变化，请重新选择".into()));
                    continue;
                }
                $open_track_picker!(tracks.expect("validated tracks"), identity.revision);
            }
            Command::LikeSelected(source, selected) => {
                let identity = $transport.session().snapshot();
                let tracks = selected_tracks(
                    source,
                    &selected,
                    &$browser,
                    ($query_generation, &$search_results),
                    ($history_generation, $history_revision, &$history_results),
                    identity.revision,
                );
                if $auth_busy
                    || $playlist_mutation.is_some()
                    || $liked_songs.has_pending_writes(identity.revision)
                    || !identity.is_authenticated()
                    || tracks.is_none()
                {
                    let _ = $events.send(Event::Status("歌曲或账号状态已变化，请重新选择".into()));
                    continue;
                }
                let ids = batch_playlist::normalize(
                    tracks
                        .expect("validated tracks")
                        .into_iter()
                        .map(|track| track.id),
                );
                $playlist_mutation = Some(identity.revision);
                $liked_songs.reset();
                let _ = $events.send(Event::Like($liked_songs.view()));
                let _ = $events.send(Event::PlaylistMutationBusy(true));
                let _ = $events.send(Event::BatchProgress(Some(BatchProgress {
                    total: ids.len(),
                    ..Default::default()
                })));
                $batch_cancel = Some(start_liked_batch(
                    Arc::clone(&$transport),
                    $sender.clone(),
                    ids,
                    identity.revision,
                ));
            }
            Command::CloseTrackPicker => {
                $picker_generation = $picker_generation.wrapping_add(1);
                if let Some(task) = $picker_task.take() {
                    task.abort();
                }
                $picker = None;
                let _ = $events.send(Event::TrackPicker(None));
            }
            Command::TrackPickerLoaded(generation, revision, result) => {
                let Some(state) = $picker.as_mut() else {
                    continue;
                };
                if $auth_busy
                    || state.generation != generation
                    || state.revision != revision
                    || revision != $transport.session().snapshot().revision
                {
                    continue;
                }
                $picker_task = None;
                state.view.loading = false;
                match result {
                    Ok(items) => {
                        let mut seen = std::collections::HashSet::new();
                        state.view.playlists = items
                            .into_iter()
                            .filter(|item| item.id > 0 && seen.insert(item.id))
                            .collect();
                        state.view.error = None;
                    }
                    Err(()) => state.view.error = Some("加载歌单失败，请关闭后重试".into()),
                }
                let _ = $events.send(Event::TrackPicker(Some(state.view.clone())));
            }
            Command::AddTrackToPlaylist(track_id, playlist_id) => {
                let identity = $transport.session().snapshot();
                let Some(state) = $picker.as_mut() else {
                    continue;
                };
                if $playlist_mutation.is_some()
                    || !identity.is_authenticated()
                    || state.revision != identity.revision
                    || state.track_ids.len() != 1
                    || state.view.track_id != track_id
                    || state.view.loading
                    || state.view.error.is_some()
                    || !state
                        .view
                        .playlists
                        .iter()
                        .any(|item| item.id == playlist_id)
                {
                    let _ = $events.send(Event::Status("歌单或账号状态已变化，请重新选择".into()));
                    continue;
                }
                let mutation = TrackMutation {
                    playlist_id,
                    track_id,
                    add: true,
                    $picker_generation: Some(state.generation),
                };
                state.view.busy = true;
                let _ = $events.send(Event::TrackPicker(Some(state.view.clone())));
                $playlist_mutation = Some(identity.revision);
                let _ = $events.send(Event::PlaylistMutationBusy(true));
                let $sender = $sender.clone();
                let $transport = Arc::clone(&$transport);
                tokio::spawn(async move {
                    let success = playlist::set_tracks_at_revision(
                        &$transport,
                        playlist_id,
                        &[track_id],
                        true,
                        identity.revision,
                    )
                    .await
                    .is_ok();
                    let _ = $sender.send(Command::TrackMutationDone(
                        identity.revision,
                        mutation,
                        success,
                    ));
                });
            }
            Command::AddPickedTracks(generation, playlist_id) => {
                let identity = $transport.session().snapshot();
                let Some(state) = $picker.as_mut() else {
                    continue;
                };
                if $auth_busy
                    || $playlist_mutation.is_some()
                    || $liked_songs.has_pending_writes(identity.revision)
                    || !identity.is_authenticated()
                    || state.revision != identity.revision
                    || state.generation != generation
                    || state.view.loading
                    || state.view.error.is_some()
                    || !state
                        .view
                        .playlists
                        .iter()
                        .any(|item| item.id == playlist_id)
                    || state.track_ids.is_empty()
                {
                    let _ = $events.send(Event::Status("歌单或账号状态已变化，请重新选择".into()));
                    continue;
                }
                let ids = state.track_ids.clone();
                state.view.busy = true;
                let _ = $events.send(Event::TrackPicker(Some(state.view.clone())));
                $playlist_mutation = Some(identity.revision);
                $liked_songs.reset();
                let _ = $events.send(Event::Like($liked_songs.view()));
                let _ = $events.send(Event::PlaylistMutationBusy(true));
                let _ = $events.send(Event::BatchProgress(Some(BatchProgress {
                    total: ids.len(),
                    ..Default::default()
                })));
                $batch_cancel = Some(start_batch(
                    Arc::clone(&$transport),
                    $sender.clone(),
                    BatchJob {
                        playlist_id,
                        track_ids: ids,
                        add: true,
                        revision: identity.revision,
                    },
                    Some(generation),
                ));
            }
            Command::RemoveSelectedTracks(playlist_id, generation, selected) => {
                let identity = $transport.session().snapshot();
                let valid = !selected.is_empty()
                    && selected.iter().all(|&(index, id)| {
                        $browser.playable_track(index, id, generation).is_some()
                    });
                if $auth_busy
                    || $playlist_mutation.is_some()
                    || $liked_songs.has_pending_writes(identity.revision)
                    || !identity.is_authenticated()
                    || !$browser.editable_playlist(playlist_id, identity.revision)
                    || !valid
                {
                    let _ =
                        $events.send(Event::Status("歌曲或歌单状态已变化，请刷新后重试".into()));
                    continue;
                }
                let ids = batch_playlist::normalize(selected.into_iter().map(|(_, id)| id));
                $playlist_mutation = Some(identity.revision);
                $liked_songs.reset();
                let _ = $events.send(Event::Like($liked_songs.view()));
                let _ = $events.send(Event::PlaylistMutationBusy(true));
                let _ = $events.send(Event::BatchProgress(Some(BatchProgress {
                    total: ids.len(),
                    ..Default::default()
                })));
                $batch_cancel = Some(start_batch(
                    Arc::clone(&$transport),
                    $sender.clone(),
                    BatchJob {
                        playlist_id,
                        track_ids: ids,
                        add: false,
                        revision: identity.revision,
                    },
                    None,
                ));
            }
            Command::CancelBatch => {
                if let Some(cancel) = &$batch_cancel {
                    cancel.store(true, Ordering::Release);
                    let _ = $events.send(Event::Status("正在停止，当前请求完成后结束".into()));
                }
            }
            Command::BatchProgress(revision, progress) => {
                if $batch_cancel.is_some()
                    && !$auth_busy
                    && $transport.session().snapshot().revision == revision
                {
                    let _ = $events.send(Event::BatchProgress(Some(progress)));
                }
            }
            Command::BatchPreparationFailed(revision, message) => {
                if $playlist_mutation != Some(revision) || $batch_cancel.is_none() {
                    continue;
                }
                $batch_cancel = None;
                $playlist_mutation = None;
                let _ = $events.send(Event::PlaylistMutationBusy(false));
                let _ = $events.send(Event::BatchProgress(None));
                if let Some(state) = $picker.as_mut() {
                    state.view.busy = false;
                    let _ = $events.send(Event::TrackPicker(Some(state.view.clone())));
                }
                let _ = $events.send(Event::Status(
                    if $auth_busy || revision != $transport.session().snapshot().revision {
                        "账号已变化，请重新检查歌单"
                    } else {
                        message
                    }
                    .into(),
                ));
            }
            Command::BatchDone(revision, playlist_id, action, $picker_generation, result) => {
                if $playlist_mutation != Some(revision) || $batch_cancel.is_none() {
                    continue;
                }
                $batch_cancel = None;
                $playlist_mutation = None;
                let _ = $events.send(Event::PlaylistMutationBusy(false));
                let _ = $events.send(Event::BatchProgress(None));
                if $auth_busy
                    || revision != $transport.session().snapshot().revision
                    || result.account_changed
                {
                    let _ = $events.send(Event::Status("账号已变化，请重新检查歌单".into()));
                    continue;
                }
                if result.processed > 0 {
                    $liked_songs.reset();
                    let _ = $events.send(Event::Like($liked_songs.view()));
                    if $browser.reload_playlist(
                        playlist_id,
                        revision,
                        Arc::clone(&$transport),
                        $sender.clone(),
                    ) {
                        let _ = $events.send(Event::Browse($browser.view()));
                    }
                }
                if let Some(state) = $picker.as_mut() {
                    state.view.busy = false;
                    if Some(state.generation) == $picker_generation && state.revision == revision {
                        if result.unconfirmed == 0
                            && !result.canceled
                            && result.processed == result.total
                        {
                            $picker = None;
                            let _ = $events.send(Event::TrackPicker(None));
                        } else {
                            state.view.playlists.clear();
                            state.view.error =
                                Some("部分结果未确认或操作已停止，请检查歌单后再试".into());
                            let _ = $events.send(Event::TrackPicker(Some(state.view.clone())));
                        }
                    } else {
                        let _ = $events.send(Event::TrackPicker(Some(state.view.clone())));
                    }
                }
                let verb = match action {
                    BatchAction::Add => "加入",
                    BatchAction::Remove => "移除",
                    BatchAction::Like => "喜欢",
                };
                let suffix = if result.canceled { "，已停止" } else { "" };
                let skipped = if result.skipped > 0 {
                    format!("，{} 首已喜欢", result.skipped)
                } else {
                    String::new()
                };
                let _ = $events.send(Event::Status(format!(
                    "{verb}完成：{} 首成功，{} 首结果未确认{skipped}{suffix}",
                    result.succeeded, result.unconfirmed
                )));
            }
            Command::RemovePlaylistTrack(playlist_id, index, track_id, generation) => {
                let identity = $transport.session().snapshot();
                if $playlist_mutation.is_some()
                    || !identity.is_authenticated()
                    || !$browser.editable_playlist(playlist_id, identity.revision)
                    || $browser
                        .playable_track(index, track_id, generation)
                        .is_none()
                {
                    let _ =
                        $events.send(Event::Status("歌曲或歌单状态已变化，请刷新后重试".into()));
                    continue;
                }
                let mutation = TrackMutation {
                    playlist_id,
                    track_id,
                    add: false,
                    $picker_generation: None,
                };
                $playlist_mutation = Some(identity.revision);
                let _ = $events.send(Event::PlaylistMutationBusy(true));
                let $sender = $sender.clone();
                let $transport = Arc::clone(&$transport);
                tokio::spawn(async move {
                    let success = playlist::set_tracks_at_revision(
                        &$transport,
                        playlist_id,
                        &[track_id],
                        false,
                        identity.revision,
                    )
                    .await
                    .is_ok();
                    let _ = $sender.send(Command::TrackMutationDone(
                        identity.revision,
                        mutation,
                        success,
                    ));
                });
            }
            Command::TrackMutationDone(revision, mutation, success) => {
                if $playlist_mutation != Some(revision) {
                    continue;
                }
                $playlist_mutation = None;
                let _ = $events.send(Event::PlaylistMutationBusy(false));
                if $auth_busy || revision != $transport.session().snapshot().revision {
                    let _ = $events.send(Event::Status("账号已变化，请重新检查歌单".into()));
                    continue;
                }
                $liked_songs.reset();
                let _ = $events.send(Event::Like($liked_songs.view()));
                if $browser.reload_playlist(
                    mutation.playlist_id,
                    revision,
                    Arc::clone(&$transport),
                    $sender.clone(),
                ) {
                    let _ = $events.send(Event::Browse($browser.view()));
                }
                if let Some(state) = $picker.as_mut() {
                    state.view.busy = false;
                    let same_picker = Some(state.generation) == mutation.$picker_generation
                        && state.revision == revision
                        && state.view.track_id == mutation.track_id;
                    if same_picker && success {
                        $picker = None;
                        let _ = $events.send(Event::TrackPicker(None));
                    } else {
                        if same_picker {
                            state.view.playlists.clear();
                            state.view.error = Some("操作结果未确认，请检查歌单后再试".into());
                        }
                        let _ = $events.send(Event::TrackPicker(Some(state.view.clone())));
                    }
                }
                let message = if success {
                    if mutation.add {
                        "已加入歌单"
                    } else {
                        "已从歌单移除"
                    }
                } else {
                    "操作结果未确认，请检查歌单"
                };
                let _ = $events.send(Event::Status(message.into()));
            }
            Command::StartPlaylistEdit(id) => {
                let revision = $transport.session().snapshot().revision;
                if $playlist_mutation.is_some() || !$browser.editable_playlist(id, revision) {
                    let _ = $events.send(Event::PlaylistEditFailed);
                    continue;
                }
                $playlist_edit_generation = $playlist_edit_generation.wrapping_add(1);
                let generation = $playlist_edit_generation;
                $playlist_edit_data = None;
                let $sender = $sender.clone();
                let $transport = Arc::clone(&$transport);
                tokio::spawn(async move {
                    let (result, categories) = tokio::join!(
                        playlist::info_at_revision(&$transport, id, 5, revision),
                        library_api::categories(&$transport),
                    );
                    let _ = $sender.send(Command::PlaylistEditLoaded(
                        generation,
                        revision,
                        id,
                        result.map_err(|_| ()),
                        categories.ok(),
                    ));
                });
            }
            Command::LoadPlaylistCover(request, path) => {
                if $auth_busy || $playlist_edit_data.is_none() {
                    let _ = $events.send(Event::PlaylistCoverPreview(request, Err(())));
                    continue;
                }
                if let Some(task) = $cover_preview_task.take() {
                    task.abort();
                }
                let $events = $events.clone();
                let slot = Arc::clone(&$cover_preview_slot);
                $cover_preview_task = Some(tokio::spawn(async move {
                    let result = match slot.acquire_owned().await {
                        Ok(permit) => tokio::task::spawn_blocking(move || {
                            let _permit = permit;
                            crate::playlist_cover::load(&path).map(Arc::new)
                        })
                        .await
                        .unwrap_or(Err(())),
                        Err(_) => Err(()),
                    };
                    let _ = $events.send(Event::PlaylistCoverPreview(request, result));
                }));
            }
            Command::PlaylistEditLoaded(generation, revision, id, result, categories) => {
                if generation != $playlist_edit_generation {
                    continue;
                }
                if $auth_busy
                    || revision != $transport.session().snapshot().revision
                    || !$browser.editable_playlist(id, revision)
                {
                    let _ = $events.send(Event::PlaylistEditFailed);
                    continue;
                }
                match result {
                    Ok(Some(info)) if info.id == id => {
                        $playlist_edit_data = Some((revision, info.clone(), categories.clone()));
                        let _ = $events.send(Event::PlaylistEditReady(info, categories));
                    }
                    _ => {
                        let _ = $events.send(Event::PlaylistEditFailed);
                        let _ = $events.send(Event::Status("无法读取歌单详情，请重试".into()));
                    }
                }
            }
            Command::CreatePlaylist(name, private) => {
                let identity = $transport.session().snapshot();
                let name = name.trim().to_owned();
                if $playlist_mutation.is_some()
                    || !identity.is_authenticated()
                    || !$browser.is_mine_list(identity.revision)
                    || name.is_empty()
                {
                    let _ = $events.send(Event::Status("请登录并输入歌单名称后重试".into()));
                    continue;
                }
                $playlist_mutation = Some(identity.revision);
                let _ = $events.send(Event::PlaylistMutationBusy(true));
                let $sender = $sender.clone();
                let $transport = Arc::clone(&$transport);
                tokio::spawn(async move {
                    let result = playlist::create_at_revision(
                        &$transport,
                        &name,
                        private,
                        identity.revision,
                    )
                    .await
                    .map(|id| PlaylistMutationResult::Created { id, name, private })
                    .map_err(|_| PlaylistMutationError::Uncertain);
                    let _ = $sender.send(Command::PlaylistMutationDone(identity.revision, result));
                });
            }
            Command::SavePlaylist(id, name, description, categories, cover_edit) => {
                let revision = $transport.session().snapshot().revision;
                let name = name.trim().to_owned();
                let Some((prepared_revision, info, choices)) = $playlist_edit_data.as_ref() else {
                    let _ = $events.send(Event::Status("请先重新加载歌单详情".into()));
                    continue;
                };
                if $playlist_mutation.is_some()
                    || *prepared_revision != revision
                    || info.id != id
                    || !$browser.editable_playlist(id, revision)
                    || name.is_empty()
                {
                    let _ = $events.send(Event::Status("歌单状态已变化或名称为空，请重试".into()));
                    continue;
                }
                let original: Result<Vec<i32>, _> = info
                    .categories
                    .iter()
                    .map(|category| i32::try_from(category.id))
                    .collect();
                let Ok(original) = original else {
                    let _ = $events.send(Event::Status("歌单标签无法安全保存".into()));
                    continue;
                };
                let mut unique = HashSet::new();
                let allowed = |id: i32| {
                    original.contains(&id)
                        || choices.as_ref().is_some_and(|groups| {
                            groups.iter().any(|group| {
                                group.categories.iter().any(|item| item.id == i64::from(id))
                            })
                        })
                };
                if (categories.len() > 3 && categories != original)
                    || categories
                        .iter()
                        .any(|&id| id <= 0 || !unique.insert(id) || !allowed(id))
                    || (choices.is_none() && categories != original)
                {
                    let _ = $events.send(Event::Status("标签已变化或超过三个，请重新选择".into()));
                    continue;
                }
                let cover = info.cover_raw_url.clone();
                $playlist_mutation = Some(revision);
                let _ = $events.send(Event::PlaylistMutationBusy(true));
                let $sender = $sender.clone();
                let $transport = Arc::clone(&$transport);
                tokio::spawn(async move {
                    let result = async {
                        let uploaded = if let Some(edit) = cover_edit {
                            let bytes = tokio::task::spawn_blocking(move || {
                                crate::playlist_cover::render(&edit)
                            })
                            .await
                            .map_err(|_| PlaylistMutationError::InvalidCover)?
                            .map_err(|_| PlaylistMutationError::InvalidCover)?;
                            Some(
                                playlist::upload_cover_at_revision(
                                    &$transport,
                                    id,
                                    bytes,
                                    "cover.png",
                                    "image/png",
                                    revision,
                                )
                                .await
                                .map_err(|_| PlaylistMutationError::Uncertain)?,
                            )
                        } else {
                            None
                        };
                        let chosen_cover = uploaded.as_deref().unwrap_or(&cover);
                        let edit = PlaylistEdit {
                            id,
                            name: &name,
                            description: &description,
                            cover_raw_url: chosen_cover,
                            category_ids: &categories,
                        };
                        playlist::update_at_revision(&$transport, edit, revision)
                            .await
                            .map_err(|_| PlaylistMutationError::Uncertain)?;
                        Ok(PlaylistMutationResult::Edited {
                            id,
                            name,
                            description,
                            cover_url: uploaded,
                        })
                    }
                    .await;
                    let _ = $sender.send(Command::PlaylistMutationDone(revision, result));
                });
            }
            Command::DeletePlaylist(id) => {
                let revision = $transport.session().snapshot().revision;
                if $playlist_mutation.is_some() || !$browser.editable_playlist(id, revision) {
                    let _ = $events.send(Event::Status("歌单状态已变化，请刷新后重试".into()));
                    continue;
                }
                $playlist_mutation = Some(revision);
                let _ = $events.send(Event::PlaylistMutationBusy(true));
                let $sender = $sender.clone();
                let $transport = Arc::clone(&$transport);
                tokio::spawn(async move {
                    let result = playlist::delete_at_revision(&$transport, id, revision)
                        .await
                        .map(|()| PlaylistMutationResult::Deleted { id })
                        .map_err(|_| PlaylistMutationError::Uncertain);
                    let _ = $sender.send(Command::PlaylistMutationDone(revision, result));
                });
            }
            Command::PlaylistMutationDone(revision, result) => {
                if $playlist_mutation != Some(revision) {
                    continue;
                }
                $playlist_mutation = None;
                let _ = $events.send(Event::PlaylistMutationBusy(false));
                if let Some(state) = $picker.as_mut() {
                    state.view.busy = false;
                    let _ = $events.send(Event::TrackPicker(Some(state.view.clone())));
                }
                if revision != $transport.session().snapshot().revision || $auth_busy {
                    let _ = $events.send(Event::PlaylistMutationFinished(None));
                    let _ = $events.send(Event::Status("账号已变化，请重新检查歌单".into()));
                    continue;
                }
                match result {
                    Ok(outcome) => {
                        let inserted = match &outcome {
                            PlaylistMutationResult::Created { id, name, private } => {
                                $browser.add_created_playlist(revision, *id, name, *private)
                            }
                            PlaylistMutationResult::Edited {
                                id,
                                name,
                                description,
                                cover_url,
                            } => {
                                $browser.update_edited_playlist(
                                    *id,
                                    revision,
                                    name,
                                    description,
                                    cover_url.as_deref(),
                                );
                                $playlist_edit_data = None;
                                false
                            }
                            PlaylistMutationResult::Deleted { id } => {
                                $browser.remove_deleted_playlist(*id, revision);
                                $playlist_edit_data = None;
                                false
                            }
                        };
                        if !inserted && $browser.showing_mine_list(revision) {
                            $browser.mine(Arc::clone(&$transport), $sender.clone());
                        }
                        let _ = $events.send(Event::Browse($browser.view()));
                        let _ = $events.send(Event::PlaylistMutationFinished(Some(outcome)));
                        let _ = $events.send(Event::Status("歌单操作已完成".into()));
                    }
                    Err(PlaylistMutationError::InvalidCover) => {
                        let _ = $events.send(Event::Status(
                            "无法读取封面，请选择不超过 12 MB 的 JPG、PNG、BMP 或 WebP 图片".into(),
                        ));
                    }
                    Err(PlaylistMutationError::Uncertain) => {
                        $playlist_edit_data = None;
                        let _ = $events.send(Event::PlaylistMutationFinished(None));
                        let _ =
                            $events.send(Event::Status("操作结果未确认，请刷新歌单后检查".into()));
                    }
                }
            }
        ] $rest)
    };
}
