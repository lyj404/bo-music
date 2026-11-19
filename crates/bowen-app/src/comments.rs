//! Account- and track-bound song comments, independent of GPUI.
use crate::desktop_service::Command;
use bowen_api::{
    comments::{self, PublishComment},
    transport::Transport,
};
use bowen_domain::comments::{CommentPage, CommentSort, SongComment};
use std::{collections::HashSet, sync::Arc};
use tokio::{sync::mpsc, task::JoinHandle};

#[derive(Clone)]
pub struct CommentsView {
    pub open: bool,
    pub track_id: Option<i64>,
    pub account_revision: Option<u64>,
    pub generation: u64,
    pub sort: CommentSort,
    pub root: Option<SongComment>,
    pub rows: Vec<SongComment>,
    pub total: i64,
    pub loading: bool,
    pub has_more: bool,
    pub error: Option<String>,
    pub posting: bool,
    pub pending_likes: Vec<i64>,
    pub uncertain_likes: Vec<i64>,
    pub reply_target: Option<(i64, String)>,
}

#[derive(Default)]
struct PageState {
    rows: Vec<SongComment>,
    seen: HashSet<i64>,
    next: i32,
    total: i64,
    loading: bool,
    more: bool,
    error: Option<String>,
}
impl PageState {
    fn fresh() -> Self {
        Self {
            next: 1,
            more: true,
            ..Default::default()
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommentContext {
    revision: u64,
    track: i64,
    pub generation: u64,
}

#[derive(Clone, Copy)]
pub struct CommentLike {
    context: CommentContext,
    id: i64,
    parent: i64,
    desired: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublishOutcome {
    Verified,
    AcceptedUnverified,
    Failed,
}

struct PublishVerification {
    context: CommentContext,
    parent: i64,
    published_id: Option<i64>,
    uid: String,
    text: String,
    previous_ids: HashSet<i64>,
}

async fn verify_published(transport: &Transport, verification: &PublishVerification) -> bool {
    let context = verification.context;
    let first = if verification.parent > 0 {
        comments::replies_at_revision(
            transport,
            context.track,
            verification.parent,
            1,
            context.revision,
        )
        .await
    } else {
        comments::list_at_revision(
            transport,
            context.track,
            CommentSort::Latest,
            1,
            context.revision,
        )
        .await
    };
    let Ok(page) = first else {
        return false;
    };
    let matches = |row: &SongComment| match verification.published_id {
        Some(id) => row.id == id,
        None => {
            !verification.previous_ids.contains(&row.id)
                && row.content == verification.text
                && row.user_id.to_string() == verification.uid
        }
    };
    if page.items.iter().any(matches) {
        return true;
    }
    if verification.parent <= 0 || page.total_count <= 30 {
        return false;
    }
    let last_page = (page.total_count.saturating_add(29) / 30).clamp(1, i64::from(i32::MAX)) as i32;
    comments::replies_at_revision(
        transport,
        context.track,
        verification.parent,
        last_page,
        context.revision,
    )
    .await
    .is_ok_and(|page| page.items.iter().any(matches))
}

pub struct Comments {
    open: bool,
    context: Option<CommentContext>,
    generation: u64,
    fetch_generation: u64,
    fetch: Option<JoinHandle<()>>,
    sort: CommentSort,
    list: PageState,
    root: Option<SongComment>,
    replies: PageState,
    reply_target: Option<(i64, String)>,
    likes: HashSet<(u64, i64, i64)>,
    uncertain: HashSet<i64>,
    posting: Option<CommentContext>,
}
impl Default for Comments {
    fn default() -> Self {
        Self {
            open: false,
            context: None,
            generation: 0,
            fetch_generation: 0,
            fetch: None,
            sort: CommentSort::Recommended,
            list: PageState::fresh(),
            root: None,
            replies: PageState::fresh(),
            reply_target: None,
            likes: HashSet::new(),
            uncertain: HashSet::new(),
            posting: None,
        }
    }
}
impl Comments {
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub(crate) fn visible_comment(
        &self,
        generation: u64,
        revision: u64,
        id: i64,
    ) -> Option<&SongComment> {
        if !self.open || self.generation != generation || self.context?.revision != revision {
            return None;
        }
        self.root
            .iter()
            .chain(&self.page().rows)
            .find(|row| row.id == id)
    }

    pub(crate) fn track_id(&self) -> Option<i64> {
        self.context.map(|context| context.track)
    }
    pub fn view(&self) -> CommentsView {
        let page = self.page();
        let pending_likes = self.context.map_or_else(Vec::new, |context| {
            self.likes
                .iter()
                .filter_map(|&(revision, track, id)| {
                    (revision == context.revision && track == context.track).then_some(id)
                })
                .collect()
        });
        CommentsView {
            open: self.open,
            track_id: self.context.map(|context| context.track),
            account_revision: self.context.map(|context| context.revision),
            generation: self.generation,
            sort: self.sort,
            root: self.root.clone(),
            rows: page.rows.clone(),
            total: page.total,
            loading: page.loading,
            has_more: page.more,
            error: page.error.clone(),
            posting: self.posting.is_some(),
            pending_likes,
            uncertain_likes: self.uncertain.iter().copied().collect(),
            reply_target: self.reply_target.clone(),
        }
    }
    fn page(&self) -> &PageState {
        if self.root.is_some() {
            &self.replies
        } else {
            &self.list
        }
    }
    fn page_mut(&mut self) -> &mut PageState {
        if self.root.is_some() {
            &mut self.replies
        } else {
            &mut self.list
        }
    }
    fn cancel_fetch(&mut self) {
        self.fetch_generation = self.fetch_generation.wrapping_add(1);
        if let Some(task) = self.fetch.take() {
            task.abort();
        }
        self.list.loading = false;
        self.replies.loading = false;
    }
    fn change_context(&mut self) {
        self.cancel_fetch();
        self.generation = self.generation.wrapping_add(1);
        if let Some(context) = &mut self.context {
            context.generation = self.generation;
        }
        self.uncertain.clear();
        self.reply_target = None;
    }
    pub fn close(&mut self) {
        self.change_context();
        self.open = false;
        self.context = None;
    }
    pub fn reset_account(&mut self) {
        self.change_context();
        self.context = None;
        self.list = PageState::fresh();
        self.root = None;
        self.replies = PageState::fresh();
    }
    pub fn open(
        &mut self,
        track: Option<i64>,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
    ) {
        self.change_context();
        self.open = true;
        self.context = None;
        self.list = PageState::fresh();
        self.root = None;
        self.replies = PageState::fresh();
        self.select(track, transport, sender);
    }
    pub fn select(
        &mut self,
        track: Option<i64>,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
    ) {
        if !self.open {
            return;
        }
        let revision = transport.session().snapshot().revision;
        let track = track.filter(|id| *id > 0);
        if match (self.context, track) {
            (None, None) => true,
            (Some(context), Some(id)) => context.track == id && context.revision == revision,
            _ => false,
        } {
            return;
        }
        self.change_context();
        self.list = PageState::fresh();
        self.replies = PageState::fresh();
        self.root = None;
        self.context = track.map(|track| CommentContext {
            revision,
            track,
            generation: self.generation,
        });
        if let Some(track) = track {
            if track > i64::from(i32::MAX) {
                self.list.error = Some("这首歌暂不支持评论".into());
                self.list.more = false;
            } else {
                self.load_more(transport, sender);
            }
        }
    }
    pub fn sort(
        &mut self,
        sort: CommentSort,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
    ) {
        if !self.open || self.sort == sort {
            return;
        }
        self.sort = sort;
        self.root = None;
        self.change_context();
        self.list = PageState::fresh();
        self.load_more(transport, sender);
    }
    pub fn open_thread(
        &mut self,
        id: i64,
        generation: u64,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
    ) {
        if !self.open || self.root.is_some() || generation != self.generation {
            return;
        }
        let Some(root) = self.list.rows.iter().find(|row| row.id == id).cloned() else {
            return;
        };
        self.change_context();
        self.root = Some(root);
        self.replies = PageState::fresh();
        self.load_more(transport, sender);
    }
    pub fn close_thread(&mut self) {
        if self.root.is_some() {
            self.change_context();
            self.root = None;
            self.replies = PageState::fresh();
        }
    }
    pub fn reply_to(&mut self, id: i64, generation: u64) {
        if !self.open
            || generation != self.generation
            || self.root.is_none()
            || self.posting.is_some()
        {
            return;
        }
        self.reply_target = self
            .replies
            .rows
            .iter()
            .find(|row| row.id == id)
            .map(|row| (row.id, row.nickname.clone()));
    }
    pub fn clear_reply(&mut self) {
        if self.posting.is_none() {
            self.reply_target = None;
        }
    }
    pub fn refresh(&mut self, transport: Arc<Transport>, sender: mpsc::UnboundedSender<Command>) {
        if !self.open || self.context.is_none() {
            return;
        }
        self.change_context();
        *self.page_mut() = PageState::fresh();
        self.load_more(transport, sender);
    }
    pub fn load_more(&mut self, transport: Arc<Transport>, sender: mpsc::UnboundedSender<Command>) {
        let Some(context) = self.context else {
            return;
        };
        if !self.open || self.page().loading || !self.page().more {
            return;
        }
        if context.track > i64::from(i32::MAX) {
            let page = self.page_mut();
            page.more = false;
            page.error = Some("这首歌暂不支持评论".into());
            return;
        }
        if transport.session().snapshot().revision != context.revision {
            return;
        }
        self.cancel_fetch();
        let generation = self.fetch_generation;
        let parent = self.root.as_ref().map(|root| root.id);
        let sort = self.sort;
        let page_number = self.page().next;
        self.page_mut().loading = true;
        self.page_mut().error = None;
        self.fetch = Some(tokio::spawn(async move {
            let result = match parent {
                Some(parent) => {
                    comments::replies_at_revision(
                        &transport,
                        context.track,
                        parent,
                        page_number,
                        context.revision,
                    )
                    .await
                }
                None => {
                    comments::list_at_revision(
                        &transport,
                        context.track,
                        sort,
                        page_number,
                        context.revision,
                    )
                    .await
                }
            }
            .map_err(|_| ());
            let _ = sender.send(Command::CommentsPageDone(
                generation, context, parent, result,
            ));
        }));
    }
    pub fn finish_page(
        &mut self,
        generation: u64,
        context: CommentContext,
        parent: Option<i64>,
        current: u64,
        result: Result<CommentPage, ()>,
    ) -> bool {
        if !self.open
            || self.context != Some(context)
            || current != context.revision
            || generation != self.fetch_generation
            || self.root.as_ref().map(|root| root.id) != parent
        {
            return false;
        }
        self.fetch = None;
        match result {
            Ok(mut result) => {
                if let Some(parent) = parent {
                    for row in &mut result.items {
                        row.parent_id = parent;
                    }
                    if let Some(root) = self.root.as_mut() {
                        root.reply_count = result.total_count;
                    }
                    if let Some(root) = self.list.rows.iter_mut().find(|row| row.id == parent) {
                        root.reply_count = result.total_count;
                    }
                }
                let page = self.page_mut();
                page.loading = false;
                let had_items = !result.items.is_empty();
                for item in result.items {
                    if page.seen.insert(item.id) {
                        page.rows.push(item);
                    }
                }
                page.total = result.total_count;
                page.more = result.has_more && had_items;
                page.next = page.next.saturating_add(1);
                page.error = None;
            }
            Err(()) => {
                let page = self.page_mut();
                page.loading = false;
                page.error = Some("评论加载失败，请重试".into());
            }
        }
        true
    }
    fn find(&self, id: i64) -> Option<&SongComment> {
        self.root
            .as_ref()
            .filter(|row| row.id == id)
            .or_else(|| self.page().rows.iter().find(|row| row.id == id))
    }
    pub fn like(
        &mut self,
        id: i64,
        generation: u64,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
    ) -> Result<(), &'static str> {
        let Some(context) = self.context else {
            return Err("没有可操作的评论");
        };
        let identity = transport.session().snapshot();
        if !self.open || context.generation != generation || identity.revision != context.revision {
            return Err("评论或账号已变化，请重试");
        }
        if !identity.is_authenticated() {
            return Err("登录后可以点赞");
        }
        if self.uncertain.contains(&id) {
            return Err("请先刷新评论确认点赞状态");
        }
        let Some(row) = self.find(id) else {
            return Err("评论已变化，请刷新");
        };
        let action = CommentLike {
            context,
            id,
            parent: row.parent_id,
            desired: !row.is_liked,
        };
        if !self.likes.insert((context.revision, context.track, id)) {
            return Ok(());
        }
        tokio::spawn(async move {
            let success = comments::set_liked_at_revision(
                &transport,
                context.track,
                action.id,
                action.desired,
                action.parent,
                context.revision,
            )
            .await
            .is_ok();
            let _ = sender.send(Command::CommentLikeDone(action, success));
        });
        Ok(())
    }
    pub fn finish_like(&mut self, action: CommentLike, current: u64, success: bool) -> bool {
        self.likes
            .remove(&(action.context.revision, action.context.track, action.id));
        let Some(context) = self.context else {
            return false;
        };
        if !self.open
            || context.revision != current
            || context.revision != action.context.revision
            || context.track != action.context.track
        {
            return false;
        }
        if context.generation != action.context.generation || !success {
            self.uncertain.insert(action.id);
            return true;
        }
        for row in self
            .list
            .rows
            .iter_mut()
            .chain(self.replies.rows.iter_mut())
            .chain(self.root.iter_mut())
            .filter(|row| row.id == action.id)
        {
            if row.is_liked != action.desired {
                row.like_count = if action.desired {
                    row.like_count.saturating_add(1)
                } else {
                    row.like_count.saturating_sub(1).max(0)
                };
                row.is_liked = action.desired;
            }
        }
        true
    }
    pub fn publish(
        &mut self,
        generation: u64,
        text: String,
        anonymous: bool,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
    ) -> Result<(), &'static str> {
        let Some(context) = self.context else {
            return Err("没有可评论的歌曲");
        };
        let identity = transport.session().snapshot();
        if !self.open || context.generation != generation || identity.revision != context.revision {
            return Err("歌曲或账号已变化，请重试");
        }
        if !identity.is_authenticated() {
            return Err("登录后可以评论");
        }
        if context.track > i64::from(i32::MAX) {
            return Err("这首歌暂不支持评论");
        }
        if text.trim().is_empty() {
            return Err("请输入评论内容");
        }
        if text.encode_utf16().count() > 1000 {
            return Err("评论不能超过 1000 个字符");
        }
        if self.posting.is_some() {
            return Err("正在发送评论");
        }
        let parent = self.root.as_ref().map_or(0, |root| root.id);
        let reply = self.reply_target.as_ref().map_or(0, |target| target.0);
        let previous_ids = self.page().rows.iter().map(|row| row.id).collect();
        let uid = identity.uid;
        self.posting = Some(context);
        tokio::spawn(async move {
            let comment = PublishComment {
                music_id: context.track,
                content: &text,
                parent_id: parent,
                reply_id: reply,
                anonymous,
            };
            let outcome =
                match comments::publish_at_revision(&transport, comment, context.revision).await {
                    Err(_) => PublishOutcome::Failed,
                    Ok(published_id) => {
                        let verification = PublishVerification {
                            context,
                            parent,
                            published_id,
                            uid,
                            text: text.trim().to_owned(),
                            previous_ids,
                        };
                        if verify_published(&transport, &verification).await {
                            PublishOutcome::Verified
                        } else {
                            PublishOutcome::AcceptedUnverified
                        }
                    }
                };
            let _ = sender.send(Command::CommentPublishDone(context, outcome));
        });
        Ok(())
    }
    pub fn finish_publish(
        &mut self,
        context: CommentContext,
        current: u64,
        outcome: PublishOutcome,
        transport: Arc<Transport>,
        sender: mpsc::UnboundedSender<Command>,
    ) -> bool {
        if self.posting != Some(context) {
            return false;
        }
        self.posting = None;
        if !self.open || self.context != Some(context) || current != context.revision {
            return false;
        }
        if outcome != PublishOutcome::Failed {
            let target = self.reply_target.clone();
            if self.root.is_none() {
                self.sort = CommentSort::Latest;
            }
            self.refresh(transport, sender);
            self.reply_target = target;
        }
        true
    }
}

#[cfg(test)]
#[path = "comments_tests.rs"]
mod tests;
