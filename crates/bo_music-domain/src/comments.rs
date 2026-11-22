#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SongComment {
    pub id: i64,
    pub user_id: i64,
    pub parent_id: i64,
    pub reply_id: i64,
    pub reply_nickname: String,
    pub nickname: String,
    pub avatar_url: Option<String>,
    pub content: String,
    pub image_url: Option<String>,
    pub publish_time: String,
    pub location: String,
    pub like_count: i64,
    pub reply_count: i64,
    pub is_liked: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommentPage {
    pub items: Vec<SongComment>,
    pub total_count: i64,
    pub has_more: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommentSort {
    Recommended,
    Latest,
}
