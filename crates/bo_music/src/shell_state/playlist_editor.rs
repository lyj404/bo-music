//! State owned by the playlist editor.

use crate::input::TextInput;
use bo_music_app::playlist_cover::{CoverCrop, CoverImage};
use bo_music_domain::catalog::{CategoryGroup, PlaylistCategory};
use gpui::{Entity, FocusHandle, Image, Pixels, Point};
use std::{path::PathBuf, sync::Arc};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlaylistDialog {
    Closed,
    Create,
    EditLoading(i64),
    Edit(i64),
    Delete(i64),
}

pub(crate) struct PlaylistEditorState {
    pub(crate) name_input: Entity<TextInput>,
    pub(crate) description_input: Entity<TextInput>,
    pub(crate) dialog: PlaylistDialog,
    pub(crate) modal_focus: gpui::FocusHandle,
    pub(crate) private: bool,
    pub(crate) busy: bool,
    pub(crate) category_groups: Option<Vec<CategoryGroup>>,
    pub(crate) original_categories: Vec<PlaylistCategory>,
    pub(crate) selected_categories: Vec<i32>,
    pub(crate) cover_path: Option<PathBuf>,
    pub(crate) cover_image: Option<Arc<CoverImage>>,
    pub(crate) cover_preview: Option<Arc<Image>>,
    pub(crate) cover_crop: CoverCrop,
    pub(crate) cover_loading: bool,
    pub(crate) cover_request: u64,
    pub(crate) cover_drag: Option<Point<Pixels>>,
    pub(crate) generation: u64,
}

impl PlaylistEditorState {
    pub(crate) fn new(
        name_input: Entity<TextInput>,
        description_input: Entity<TextInput>,
        modal_focus: FocusHandle,
    ) -> Self {
        Self {
            name_input,
            description_input,
            dialog: PlaylistDialog::Closed,
            modal_focus,
            private: false,
            busy: false,
            category_groups: None,
            original_categories: Vec::new(),
            selected_categories: Vec::new(),
            cover_path: None,
            cover_image: None,
            cover_preview: None,
            cover_crop: CoverCrop::default(),
            cover_loading: false,
            cover_request: 0,
            cover_drag: None,
            generation: 0,
        }
    }
}
