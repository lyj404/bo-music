//! Command routes for search and comments.

macro_rules! search_and_comments_arms {
    ($command:ident [$($arms:tt)*] $rest:tt;
        $history_tx:ident, $events:ident, $sender:ident,
        $transport:ident, $player:ident, $query_generation:ident,
        $query_text:ident, $cursor:ident, $search_results:ident,
        $search_loading:ident, $search_task:ident, $browser:ident,
        $comments:ident, $comment_image_task:ident, $comment_image:ident,
        $comment_image_request:ident, $share_reports:ident, $liked_songs:ident,
        $batch_cancel:ident, $history_generation:ident, $history_results:ident,
        $history_revision:ident, $comments_current:ident, $auth_busy:ident $(,)?
    ) => {
        collect_command_arms!($command [$($arms)*
            Command::ShowHistory => {
                $history_generation = $history_generation.wrapping_add(1);
                $history_results.clear();
                $history_revision = u64::MAX;
                let identity = $transport.session().snapshot();
                let scope = if identity.is_authenticated() {
                    identity.uid
                } else {
                    "anonymous".into()
                };
                let _ = $events.send(Event::History(HistoryView {
                    generation: $history_generation,
                    tracks: Vec::new(),
                    loading: true,
                    error: None,
                }));
                if $history_tx
                    .send(HistoryWork::Load {
                        scope,
                        generation: $history_generation,
                        revision: identity.revision,
                    })
                    .is_err()
                {
                    let _ = $events.send(Event::History(HistoryView {
                        generation: $history_generation,
                        tracks: Vec::new(),
                        loading: false,
                        error: Some("最近播放服务不可用".into()),
                    }));
                }
            }
            Command::HistoryDone(generation, revision, result) => {
                if !$auth_busy
                    && generation == $history_generation
                    && revision == $transport.session().snapshot().revision
                {
                    let (tracks, error) = match result {
                        Ok(tracks) => (tracks, None),
                        Err(error) => (Vec::new(), Some(error)),
                    };
                    $history_results = tracks.clone();
                    $history_revision = revision;
                    let _ = $events.send(Event::History(HistoryView {
                        generation: $history_generation,
                        tracks,
                        loading: false,
                        error,
                    }));
                }
            }
            Command::HistoryRecorded(revision) => {
                if !$auth_busy && revision == $transport.session().snapshot().revision {
                    let _ = $events.send(Event::HistoryChanged);
                }
            }
            Command::Search(query) => {
                $query_generation = $query_generation.wrapping_add(1);
                if let Some(task) = $search_task.take() {
                    task.abort();
                }
                $query_text = query.trim().to_owned();
                $cursor = search_cursor(30);
                $search_results.clear();
                $search_loading = false;
                if query.trim().is_empty() {
                    let _ = $events.send(Event::Status("请输入搜索内容".into()));
                    let _ = $events.send(Event::Search(SearchView {
                        generation: $query_generation,
                        tracks: Vec::new(),
                        has_more: false,
                        loading: false,
                    }));
                    continue;
                }
                $search_loading = true;
                let _ = $events.send(Event::Search(SearchView {
                    generation: $query_generation,
                    tracks: Vec::new(),
                    has_more: false,
                    loading: true,
                }));
                let _ = $events.send(Event::Status("正在搜索…".into()));
                $search_task = Some(launch_search(
                    Arc::clone(&$transport),
                    $query_text.clone(),
                    $cursor.clone(),
                    $query_generation,
                    $sender.clone(),
                ));
            }
            Command::SearchPlaylists(query) => {
                $browser.search(Arc::clone(&$transport), $sender.clone(), query);
                let _ = $events.send(Event::Browse($browser.view()));
            }
            Command::SearchAlbums(query) => {
                $browser.search_kind(
                    Arc::clone(&$transport),
                    $sender.clone(),
                    SearchKind::Album,
                    query,
                );
                let _ = $events.send(Event::Browse($browser.view()));
            }
            Command::SearchArtists(query) => {
                $browser.search_kind(
                    Arc::clone(&$transport),
                    $sender.clone(),
                    SearchKind::Artist,
                    query,
                );
                let _ = $events.send(Event::Browse($browser.view()));
            }
            Command::PlaylistSquare => {
                if !$auth_busy {
                    $browser.playlist_square(Arc::clone(&$transport), $sender.clone());
                    let _ = $events.send(Event::Browse($browser.view()));
                }
            }
            Command::SelectPlaylistCategory(id, generation) => {
                if !$auth_busy
                    && $browser.select_playlist_category(
                        Arc::clone(&$transport),
                        $sender.clone(),
                        id,
                        generation,
                    )
                {
                    let _ = $events.send(Event::Browse($browser.view()));
                }
            }
            Command::MyPlaylists => {
                $browser.mine(Arc::clone(&$transport), $sender.clone());
                let _ = $events.send(Event::Browse($browser.view()));
            }
            Command::Collections(kind) => {
                $browser.collection(Arc::clone(&$transport), $sender.clone(), kind);
                let _ = $events.send(Event::Browse($browser.view()));
            }
            Command::LoadCollectionState => {
                $browser.load_collection_state(Arc::clone(&$transport), $sender.clone());
                let _ = $events.send(Event::Browse($browser.view()));
            }
            Command::ToggleCollection => {
                $browser.write_collection(Arc::clone(&$transport), $sender.clone());
                let _ = $events.send(Event::Browse($browser.view()));
            }
            Command::CollectionStateDone(generation, revision, target, state) => {
                if !$auth_busy
                    && $browser.finish_collection_state(
                        generation,
                        revision,
                        $transport.session().snapshot().revision,
                        target,
                        state,
                    )
                {
                    let _ = $events.send(Event::Browse($browser.view()));
                }
            }
            Command::CollectionWriteDone(revision, target, desired, success) => {
                if $browser.finish_collection_write(
                    revision,
                    $transport.session().snapshot().revision,
                    target,
                    desired,
                    success,
                ) {
                    let _ = $events.send(Event::Browse($browser.view()));
                    let _ = $events.send(Event::Status(
                        if success {
                            "收藏状态已更新"
                        } else {
                            "操作结果未确认，请重新检查收藏状态"
                        }
                        .into(),
                    ));
                }
            }
            Command::ToggleLike(id) => {
                if $batch_cancel.is_some() {
                    let _ = $events.send(Event::Status("批量处理期间请稍后再修改喜欢状态".into()));
                    continue;
                }
                if $player
                    .as_ref()
                    .and_then(|$player| $player.queue().current())
                    .is_some_and(|track| track.id == id)
                {
                    $liked_songs.toggle(id, Arc::clone(&$transport), $sender.clone());
                    let _ = $events.send(Event::Like($liked_songs.view()));
                }
            }
            Command::RetryLike(id) => {
                if $player
                    .as_ref()
                    .and_then(|$player| $player.queue().current())
                    .is_some_and(|track| track.id == id)
                    && $liked_songs.view().track_id == Some(id)
                {
                    $liked_songs.refresh(Arc::clone(&$transport), $sender.clone(), true);
                    let _ = $events.send(Event::Like($liked_songs.view()));
                }
            }
            Command::LikedLoadDone(generation, revision, result) => {
                if !$auth_busy
                    && $liked_songs.finish_load(
                        generation,
                        revision,
                        $transport.session().snapshot().revision,
                        result,
                    )
                {
                    let _ = $events.send(Event::Like($liked_songs.view()));
                }
            }
            Command::LikedWriteDone(revision, id, desired, success) => {
                if $liked_songs.finish_write(
                    revision,
                    $transport.session().snapshot().revision,
                    (id, desired, success),
                    Arc::clone(&$transport),
                    $sender.clone(),
                ) {
                    let _ = $events.send(Event::Like($liked_songs.view()));
                    let _ = $events.send(Event::Status(
                        if success {
                            "喜欢状态已更新"
                        } else {
                            "操作结果未确认，请重新查询喜欢状态"
                        }
                        .into(),
                    ));
                }
            }
            Command::OpenComments => {
                let track = $player
                    .as_ref()
                    .and_then(|$player| $player.queue().current())
                    .map(|track| track.id);
                $comments.open(track, Arc::clone(&$transport), $sender.clone());
                let _ = $events.send(Event::Comments(Box::new($comments.view())));
            }
            Command::CloseComments => {
                $comments.close();
                if let Some(task) = $comment_image_task.take() {
                    task.abort();
                }
                $comment_image = None;
                $comment_image_request = None;
                let _ = $events.send(Event::Comments(Box::new($comments.view())));
            }
            Command::CommentSort(sort, generation) => {
                if $comments_current!(generation) {
                    $comments.sort(sort, Arc::clone(&$transport), $sender.clone());
                    let _ = $events.send(Event::Comments(Box::new($comments.view())));
                }
            }
            Command::CommentsNext(generation) => {
                if $comments_current!(generation) {
                    $comments.load_more(Arc::clone(&$transport), $sender.clone());
                    let _ = $events.send(Event::Comments(Box::new($comments.view())));
                }
            }
            Command::CommentsRefresh(generation) => {
                if $comments_current!(generation) {
                    $comments.refresh(Arc::clone(&$transport), $sender.clone());
                    let _ = $events.send(Event::Comments(Box::new($comments.view())));
                }
            }
            Command::OpenCommentThread(id, generation) => {
                if $comments_current!(generation) {
                    $comments.open_thread(
                        id,
                        generation,
                        Arc::clone(&$transport),
                        $sender.clone(),
                    );
                    let _ = $events.send(Event::Comments(Box::new($comments.view())));
                }
            }
            Command::CloseCommentThread(generation) => {
                if $comments_current!(generation) {
                    $comments.close_thread();
                    let _ = $events.send(Event::Comments(Box::new($comments.view())));
                }
            }
            Command::ReplyToComment(id, generation) => {
                if $comments_current!(generation) {
                    $comments.reply_to(id, generation);
                    let _ = $events.send(Event::Comments(Box::new($comments.view())));
                }
            }
            Command::ClearCommentReply(generation) => {
                if $comments_current!(generation) {
                    $comments.clear_reply();
                    let _ = $events.send(Event::Comments(Box::new($comments.view())));
                }
            }
            Command::ToggleCommentLike(id, generation) => {
                if $comments_current!(generation) {
                    if let Err(message) =
                        $comments.like(id, generation, Arc::clone(&$transport), $sender.clone())
                    {
                        let _ = $events.send(Event::Status(message.into()));
                    }
                    let _ = $events.send(Event::Comments(Box::new($comments.view())));
                }
            }
            Command::PublishComment(generation, text, anonymous) => {
                if !$comments_current!(generation) {
                    let _ = $events.send(Event::CommentPostResult(generation, false));
                    continue;
                }
                if let Err(message) = $comments.publish(
                    generation,
                    text,
                    anonymous,
                    Arc::clone(&$transport),
                    $sender.clone(),
                ) {
                    let _ = $events.send(Event::Status(message.into()));
                    let _ = $events.send(Event::CommentPostResult(generation, false));
                }
                let _ = $events.send(Event::Comments(Box::new($comments.view())));
            }
            Command::CommentsPageDone(generation, context, parent, result) => {
                if !$auth_busy {
                    let previous = $comments.generation();
                    let _ = $comments_current!(previous);
                }
                if !$auth_busy
                    && $comments.finish_page(
                        generation,
                        context,
                        parent,
                        $transport.session().snapshot().revision,
                        result,
                    )
                {
                    let _ = $events.send(Event::Comments(Box::new($comments.view())));
                }
            }
            Command::CommentLikeDone(action, success) => {
                if !$auth_busy {
                    let previous = $comments.generation();
                    let _ = $comments_current!(previous);
                }
                if $comments.finish_like(
                    action,
                    $transport.session().snapshot().revision,
                    success,
                ) && !success
                {
                    let _ = $events.send(Event::Status("点赞结果未确认，请刷新评论后检查".into()));
                }
                let _ = $events.send(Event::Comments(Box::new($comments.view())));
            }
            Command::CommentPublishDone(context, outcome) => {
                if !$auth_busy {
                    let previous = $comments.generation();
                    let _ = $comments_current!(previous);
                }
                let accepted = $comments.finish_publish(
                    context,
                    $transport.session().snapshot().revision,
                    outcome,
                    Arc::clone(&$transport),
                    $sender.clone(),
                );
                let _ = $events.send(Event::CommentPostResult(
                    context.generation,
                    accepted && outcome == PublishOutcome::Verified,
                ));
                if accepted {
                    let _ = $events.send(Event::Status(
                        match outcome {
                            PublishOutcome::Verified => "评论已发送",
                            PublishOutcome::AcceptedUnverified => {
                                "请求已受理，暂未确认显示，请刷新检查"
                            }
                            PublishOutcome::Failed => "发送结果未确认，请刷新检查后再试",
                        }
                        .into(),
                    ));
                }
                let _ = $events.send(Event::Comments(Box::new($comments.view())));
            }
            Command::ShareCurrent(account_revision, queue_revision, id) => {
                let identity = $transport.session().snapshot();
                let valid = $player.as_ref().is_some_and(|$player| {
                    id > 0
                        && $player.queue_account_revision() == account_revision
                        && $player.queue().revision() == queue_revision
                        && $player
                            .queue()
                            .current()
                            .is_some_and(|track| track.id == id)
                });
                if $auth_busy || identity.revision != account_revision || !valid {
                    let _ = $events.send(Event::Status("歌曲或账号已变化，请重试分享".into()));
                    continue;
                }
                if let Ok(link) = sharing::link(ShareTarget::Track, id, &identity.uid) {
                    let _ = $events.send(Event::CopyShareLink(link));
                    if $share_reports.len() < 8 && $share_reports.insert((identity.revision, id))
                    {
                        let $transport = Arc::clone(&$transport);
                        let $sender = $sender.clone();
                        tokio::spawn(async move {
                            let result = sharing::report_track_at_revision(
                                &$transport,
                                id,
                                identity.revision,
                            )
                            .await
                            .map_err(|_| ());
                            let _ = $sender.send(Command::ShareReportDone(
                                identity.revision,
                                id,
                                result,
                            ));
                        });
                    }
                }
            }
            Command::ShareDetail(generation) => {
                if $auth_busy {
                    let _ = $events.send(Event::Status("正在切换账号，请稍候".into()));
                    continue;
                }
                let identity = $transport.session().snapshot();
                if let Some((target, id)) = $browser.share_target(generation)
                    && let Ok(link) = sharing::link(target, id, &identity.uid)
                {
                    let _ = $events.send(Event::CopyShareLink(link));
                } else {
                    let _ = $events.send(Event::Status("页面已变化，请重试分享".into()));
                }
            }
            Command::ShareReportDone(revision, id, result) => {
                $share_reports.remove(&(revision, id));
                if !$auth_busy
                    && $transport.session().snapshot().revision == revision
                    && $player
                        .as_ref()
                        .and_then(|$player| $player.queue().current())
                        .is_some_and(|track| track.id == id)
                {
                    let message = match result {
                        Ok(ShareOutcome::Succeeded) => None,
                        Ok(ShareOutcome::Unsupported) => Some("该歌曲暂不支持分享计数，链接已复制"),
                        Err(()) => Some("链接已复制，分享计数未确认"),
                    };
                    if let Some(message) = message {
                        let _ = $events.send(Event::Status(message.into()));
                    }
                }
            }
        ] $rest)
    };
}
