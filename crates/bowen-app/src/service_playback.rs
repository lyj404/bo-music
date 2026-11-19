//! Command routes for playback.

macro_rules! playback_arms {
    ($command:ident [$($arms:tt)*] $rest:tt;
        $events:ident, $sender:ident, $transport:ident,
        $settings:ident, $preferences:ident, $volume:ident,
        $muted:ident, $floating_windows:ident, $floating_save_at:ident,
        $main_placement:ident, $main_save_at:ident, $audio_effects:ident,
        $ambient_root:ident, $current_filter:ident, $queue_store:ident,
        $persist_enabled:ident, $mv:ident, $player:ident,
        $lyrics_key:ident, $lyrics_generation:ident, $lyrics_task:ident,
        $resolution_task:ident, $quality_task:ident, $prefetch:ident,
        $browser:ident, $mpris:ident, $auth_busy:ident $(,)?
    ) => {
        collect_command_arms!($command [$($arms)*
            Command::LyricsDone(generation, result)
                if !$auth_busy
                    && generation == $lyrics_generation
                    && $lyrics_key
                        == $player
                            .as_ref()
                            .and_then(|$player| $player.queue().current())
                            .filter(|track| track.id > 0)
                            .map(|track| (track.id, $transport.session().snapshot().revision)) =>
            {
                $lyrics_task = None;
                let view = match result {
                    Ok(document) if document.lines.is_empty() => LyricsView::Empty,
                    Ok(document) => LyricsView::Ready(document),
                    Err(_) => LyricsView::Failed,
                };
                let _ = $events.send(Event::Lyrics(view));
            }
            Command::LyricsDone(..) => {}
            Command::Resolved(mut result) => {
                if !$auth_busy && let Some($player) = $player.as_mut() {
                    $prefetch.remember(&mut result, $transport.session().snapshot().revision);
                    let message = match $player.apply(result) {
                        Ok(ApplyOutcome::Started) => "正在加载音源…",
                        Ok(ApplyOutcome::Audition) => "正在加载试听片段…",
                        Ok(ApplyOutcome::Denied(_)) => "当前歌曲无法播放",
                        Ok(ApplyOutcome::Stale) => continue,
                        Ok(ApplyOutcome::RetryFresh) => match $player.play_current() {
                            Ok(ticket) => {
                                if let Some(ticket) = &ticket {
                                    $prefetch.forget_for(ticket);
                                }
                                launch(ticket, &$sender, &mut $resolution_task, &mut $prefetch);
                                "预取音源加载失败，正在重新获取…"
                            }
                            Err(_) => "无法重新获取音源",
                        },
                        Err(_) => "音源解析或加载失败",
                    };
                    let _ = $events.send(Event::Status(message.into()));
                }
            }
            Command::RecoveryResolved(result) => {
                if !$auth_busy && let Some($player) = $player.as_mut() {
                    match $player.apply_recovery(result) {
                        Ok(RecoveryOutcome::Denied(_)) => {
                            let _ =
                                $events.send(Event::Status("播放权限已失效，无法继续播放".into()));
                        }
                        Ok(
                            RecoveryOutcome::Refreshed
                            | RecoveryOutcome::Stale
                            | RecoveryOutcome::Unavailable,
                        ) => {}
                        Err(_) => {
                            let _ = $events.send(Event::Status("更新音源失败，正在重试…".into()));
                        }
                    }
                }
            }
            Command::Play(tracks, index) => {
                if let Some($player) = $player.as_mut() {
                    $mv.close(false);
                    if $browser.cancel_play_all() {
                        let _ = $events.send(Event::Browse($browser.view()));
                    }
                    match $player.replace(tracks, index) {
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
            Command::PlaySingle(track) => {
                if let Some($player) = $player.as_mut() {
                    $mv.close(false);
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
                }
            }
            Command::PlayBrowse(index, expected_id, generation) => {
                if let Some($player) = $player.as_mut()
                    && let Some(track) = $browser.playable_track(index, expected_id, generation)
                {
                    $mv.close(false);
                    if $browser.cancel_play_all() {
                        let _ = $events.send(Event::Browse($browser.view()));
                    }
                    match $player.enqueue_and_play_with_origin(track, $browser.playback_origin())
                    {
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
            Command::Toggle | Command::SetPlaying(_) if $mv.active() => {
                let requested = if let Command::SetPlaying(playing) = $command {
                    Some(playing)
                } else {
                    None
                };
                $mv.control(MvAction::Playing(requested));
            }
            Command::Toggle | Command::SetPlaying(_) => {
                if let Some($player) = $player.as_mut() {
                    let requested = if let Command::SetPlaying(playing) = $command {
                        Some(playing)
                    } else {
                        None
                    };
                    if $browser.cancel_play_all() {
                        let _ = $events.send(Event::Browse($browser.view()));
                    }
                    match $player.snapshot().state {
                        PlaybackState::Playing if requested != Some(true) => {
                            let _ = $player.pause(true);
                        }
                        PlaybackState::Paused if requested != Some(false) => {
                            let _ = $player.pause(false);
                        }
                        PlaybackState::Idle | PlaybackState::Stopped | PlaybackState::Failed
                            if requested != Some(false) =>
                        {
                            if let Ok(ticket) = $player.play_current() {
                                launch(ticket, &$sender, &mut $resolution_task, &mut $prefetch);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Command::Stop => {
                if let Some($player) = $player.as_mut() {
                    $mv.close(false);
                    if $browser.cancel_play_all() {
                        let _ = $events.send(Event::Browse($browser.view()));
                    }
                    if $player.stop().is_ok() {
                        if let Some(task) = $resolution_task.take() {
                            task.abort();
                        }
                        $prefetch.cancel();
                    }
                }
            }
            Command::Next => {
                if let Some($player) = $player.as_mut() {
                    $mv.close(true);
                    if $browser.cancel_play_all() {
                        let _ = $events.send(Event::Browse($browser.view()));
                    }
                    if let Ok(ticket) = $player.next_track() {
                        launch(ticket, &$sender, &mut $resolution_task, &mut $prefetch);
                    }
                }
            }
            Command::Previous => {
                if let Some($player) = $player.as_mut() {
                    $mv.close(true);
                    if $browser.cancel_play_all() {
                        let _ = $events.send(Event::Browse($browser.view()));
                    }
                    if let Ok(ticket) = $player.previous() {
                        launch(ticket, &$sender, &mut $resolution_task, &mut $prefetch);
                    }
                }
            }
            Command::EditBrowseQueue(
                account_revision,
                queue_revision,
                index,
                track_id,
                generation,
                play_next,
            ) => {
                if let Some(track) = $browser.playable_track(index, track_id, generation) {
                    let edit = if play_next {
                        QueueEdit::PlayNext(track)
                    } else {
                        QueueEdit::Append(track)
                    };
                    let _ =
                        $sender.send(Command::EditQueue(account_revision, queue_revision, edit));
                } else {
                    let _ = $events.send(Event::Status("歌曲列表已变化，请重试".into()));
                }
            }
            Command::EditQueue(account_revision, queue_revision, edit) => {
                let unchanged = if matches!(&edit, QueueEdit::Append(_) | QueueEdit::AppendMany(_))
                {
                    "歌曲已在队列中"
                } else {
                    "队列未发生变化"
                };
                let message = match &edit {
                    QueueEdit::Append(_) | QueueEdit::AppendMany(_) => "已加入播放队列",
                    QueueEdit::PlayNext(_) => "已设为下一首",
                    QueueEdit::Select(_) => "正在获取音源…",
                    QueueEdit::Remove(_) => "已从播放队列移除",
                    QueueEdit::Move(..) => "播放队列已调整",
                    QueueEdit::Clear => "播放队列已清空",
                };
                let Some($player) = $player.as_mut() else {
                    let _ = $events.send(Event::Status("播放器不可用".into()));
                    continue;
                };
                match $player.edit_queue(account_revision, queue_revision, edit) {
                    Ok(outcome) if outcome.changed => {
                        if $browser.cancel_play_all() {
                            let _ = $events.send(Event::Browse($browser.view()));
                        }
                        if outcome.affects_playback && outcome.ticket.is_none() {
                            if let Some(task) = $resolution_task.take() {
                                task.abort();
                            }
                            $prefetch.cancel();
                        }
                        launch(
                            outcome.ticket,
                            &$sender,
                            &mut $resolution_task,
                            &mut $prefetch,
                        );
                        let _ = $events.send(Event::Status(message.into()));
                    }
                    Ok(_) => {
                        let _ = $events.send(Event::Status(unchanged.into()));
                    }
                    Err(_) => {
                        let _ = $events.send(Event::Status("队列或账号已变化，请重试".into()));
                    }
                }
            }
            Command::SeekRelative(seconds) => {
                if let Some($player) = $player.as_mut() {
                    let state = $player.snapshot();
                    let target = (state.position_seconds + f64::from(seconds)).max(0.0);
                    let target = state
                        .duration_seconds
                        .map_or(target, |duration| target.min(duration));
                    if $player.seek(target).is_err() {
                        let _ = $events.send(Event::Status("无法调整播放进度".into()));
                    } else {
                        #[cfg(any(target_os = "linux", test))]
                        if let Some($mpris) = &mut $mpris {
                            $mpris.note_seek();
                        }
                    }
                }
            }
            Command::SeekTo(account_revision, track_id, position) => {
                if let Some($player) = $player.as_mut()
                    && $player.queue_account_revision() == account_revision
                    && $player
                        .queue()
                        .current()
                        .is_some_and(|track| track.id == track_id)
                {
                    if $player.seek(position).is_err() {
                        let _ = $events.send(Event::Status("无法调整播放进度".into()));
                    } else {
                        #[cfg(any(target_os = "linux", test))]
                        if let Some($mpris) = &mut $mpris {
                            $mpris.note_seek();
                        }
                    }
                }
            }
            Command::SeekFromMedia(account_revision, track_id, offset_us) => {
                if let Some($player) = $player.as_mut()
                    && $player.queue_account_revision() == account_revision
                    && $player
                        .queue()
                        .current()
                        .is_some_and(|track| track.id == track_id)
                {
                    let state = $player.snapshot();
                    let target = (state.position_seconds + offset_us as f64 / 1_000_000.0).max(0.0);
                    if state
                        .duration_seconds
                        .is_some_and(|duration| target > duration)
                    {
                        if $browser.cancel_play_all() {
                            let _ = $events.send(Event::Browse($browser.view()));
                        }
                        if let Ok(ticket) = $player.next_track() {
                            launch(ticket, &$sender, &mut $resolution_task, &mut $prefetch);
                        }
                    } else if $player.seek(target).is_ok() {
                        #[cfg(any(target_os = "linux", test))]
                        if let Some($mpris) = &mut $mpris {
                            $mpris.note_seek();
                        }
                    }
                }
            }
            Command::VolumeDelta(_) | Command::SetVolume(_) => {
                if let Some($player) = $player.as_ref() {
                    let next = match $command {
                        Command::VolumeDelta(delta) => $volume + f64::from(delta),
                        Command::SetVolume(next) if next.is_finite() => next,
                        _ => continue,
                    }
                    .clamp(0.0, 100.0);
                    if $player.$volume(next).is_ok() {
                        $muted = false;
                        $volume = next;
                        if $settings.set_volume($volume).is_err() {
                            let _ = $events.send(Event::Status("音量保存失败".into()));
                        }
                    } else {
                        let _ = $events.send(Event::Status("音量设置失败".into()));
                    }
                }
            }
            Command::SetPreferredQuality(quality) => {
                if let Some($player) = $player.as_mut() {
                    if $settings.set_preferred_quality(quality).is_ok() {
                        $player.set_quality(Some(quality));
                        let _ = $events.send(Event::Status("默认音质已更新".into()));
                    } else {
                        let _ = $events.send(Event::Status("默认音质保存失败".into()));
                    }
                }
            }
            Command::SwitchQuality(quality) => {
                if let Some($player) = $player.as_mut() {
                    match $player.request_quality(quality) {
                        Ok(Some(ticket)) => {
                            if let Some(task) = $quality_task.take() {
                                task.abort();
                            }
                            let $sender = $sender.clone();
                            $quality_task = Some(tokio::spawn(async move {
                                let _ =
                                    $sender.send(Command::QualityResolved(ticket.resolve().await));
                            }));
                            let _ = $events.send(Event::Status("正在获取所选音质…".into()));
                        }
                        Ok(None) => {
                            let _ = $events.send(Event::Status("当前音质无需切换或不可用".into()));
                        }
                        Err(_) => {
                            let _ = $events.send(Event::Status("音质切换失败".into()));
                        }
                    }
                }
            }
            Command::QualityResolved(result) => {
                if !$auth_busy && let Some($player) = $player.as_mut() {
                    match $player.apply_quality(result) {
                        Ok(QualityOutcome::Loading) => {
                            let _ = $events.send(Event::Status("正在切换音源…".into()));
                        }
                        Ok(QualityOutcome::Unavailable) | Err(_) => {
                            let _ =
                                $events.send(Event::Status("音质切换失败，继续播放原音源".into()));
                        }
                        Ok(QualityOutcome::Failed) => {
                            let _ = $events.send(Event::Status("音质切换及原音源恢复失败".into()));
                        }
                        _ => {}
                    }
                }
            }
            Command::AudioEffect(action) => {
                if let Some($player) = $player.as_ref() {
                    let volume_only = matches!(action, AudioEffectAction::AdjustAmbientVolume(_));
                    let next = match changed_audio_effects(&$audio_effects, action) {
                        Ok(next) => next,
                        Err(error) => {
                            let _ = $events.send(Event::Status(error.into()));
                            continue;
                        }
                    };
                    let filter = match audio_filter(&next, &$ambient_root) {
                        Ok(filter) => filter,
                        Err(_) => {
                            let _ = $events.send(Event::Status("环境音素材无法读取".into()));
                            continue;
                        }
                    };
                    let applied = if volume_only && (!next.enabled || next.ambient_id.is_empty()) {
                        Ok(())
                    } else if volume_only {
                        let has_texture = PRESETS
                            .iter()
                            .any(|preset| preset.id == next.preset_id && preset.texture.is_some());
                        let weights = bowen_playback::effects::mix_weights(&next, has_texture);
                        match filter.as_deref() {
                            Some(graph)
                                if $player.audio_filter_weights(&weights, graph).is_ok() =>
                            {
                                Ok(())
                            }
                            _ => $player.audio_filter(filter.as_deref()),
                        }
                    } else {
                        $player.audio_filter(filter.as_deref())
                    };
                    if applied.is_err() {
                        let _ = $events
                            .send(Event::Status("音效无法应用，请检查 libmpv 滤镜支持".into()));
                    } else if $settings.set_audio_effects(&next).is_err() {
                        let _ = $player.audio_filter($current_filter.as_deref());
                        let _ = $events.send(Event::Status("音效设置保存失败".into()));
                    } else {
                        $audio_effects = next;
                        $current_filter = filter;
                        let _ = $events.send(Event::Status("音效设置已更新".into()));
                    }
                }
            }
            Command::CycleMode | Command::SetPlayMode(_) => {
                if let Some($player) = $player.as_mut() {
                    let mode = if let Command::SetPlayMode(mode) = $command {
                        mode
                    } else {
                        match $player.queue().mode() {
                            PlayMode::Sequential => PlayMode::ListLoop,
                            PlayMode::ListLoop => PlayMode::SingleLoop,
                            PlayMode::SingleLoop => PlayMode::Shuffle,
                            PlayMode::Shuffle => PlayMode::Sequential,
                        }
                    };
                    $player.set_mode(mode);
                    if $settings
                        .set_playback(bowen_storage::$settings::PlaybackSettings {
                            mode,
                            restore_queue: $preferences.restore_queue,
                        })
                        .is_err()
                    {
                        let _ = $events.send(Event::Status("播放模式保存失败".into()));
                    }
                }
            }
            Command::RaiseMainWindow => {
                let _ = $events.send(Event::ActivateMainWindow);
            }
            Command::ExitApplication => {
                let _ = $events.send(Event::ExitApplication);
            }
            Command::SaveFloatingWindows(value) => {
                if value != $floating_windows {
                    $floating_windows = value;
                    $floating_save_at =
                        Some(std::time::Instant::now() + Duration::from_millis(400));
                }
            }
            Command::Shutdown => {
                if $main_save_at.is_some()
                    && let Some(bounds) = $main_placement
                {
                    let _ = $settings.set_main_window_bounds(bounds);
                }
                $mv.close(false);
                if $floating_save_at.is_some() {
                    let _ = $settings.set_floating_windows($floating_windows);
                }
                if !$auth_busy
                    && $persist_enabled
                    && let Some($player) = $player.as_ref()
                {
                    let _ = $player.save_account_queue(&$queue_store);
                }
                break;
            }
        ] $rest)
    };
}
