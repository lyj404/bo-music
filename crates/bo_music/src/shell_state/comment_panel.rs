//! State owned by the comment panel.

use crate::{comment_image, input::TextInput};
use bo_music_app::comments::CommentsView;
use gpui::Entity;
use std::collections::HashMap;

pub(crate) type CommentDraftKey = (u64, i64, i64, i64);

pub(crate) fn comment_draft_key(view: &CommentsView) -> Option<CommentDraftKey> {
    Some((
        view.account_revision?,
        view.track_id?,
        view.root.as_ref().map_or(0, |root| root.id),
        view.reply_target.as_ref().map_or(0, |target| target.0),
    ))
}

pub(crate) struct CommentPanelState {
    pub(crate) input: Entity<TextInput>,
    pub(crate) image: comment_image::CommentImage,
    pub(crate) view: Option<CommentsView>,
    pub(crate) drafts: HashMap<CommentDraftKey, String>,
    pub(crate) submitted: Option<(u64, CommentDraftKey, String)>,
    pub(crate) anonymous: bool,
}

impl CommentPanelState {
    pub(crate) fn new(input: Entity<TextInput>) -> Self {
        Self {
            input,
            image: comment_image::CommentImage::default(),
            view: None,
            drafts: HashMap::new(),
            submitted: None,
            anonymous: false,
        }
    }
}
