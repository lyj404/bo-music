//! Page state is grouped by ownership; rendering and service dispatch stay in the shell.
mod comment_panel;
mod navigation;
mod panels;
mod playlist_editor;
mod search;

pub(crate) use comment_panel::{CommentPanelState, comment_draft_key};
pub(crate) use navigation::{NavigationState, Page};
pub(crate) use panels::PanelState;
pub(crate) use playlist_editor::{PlaylistDialog, PlaylistEditorState};
pub(crate) use search::SearchState;
