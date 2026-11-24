//! Active tab, detail visibility and return navigation.

use bo_music_app::browse::{BrowseView, SearchKind};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Page {
    Home,
    Library,
    Overview,
    Tracks,
    PlaylistSearch,
    PlaylistSquare,
    AlbumSearch,
    ArtistSearch,
    Mine,
    Collection,
    Charts,
    Recent,
    AccountOverview,
}

pub(crate) struct NavigationState {
    pub(crate) page: Page,
    pub(crate) collection_kind: SearchKind,
    pub(crate) browse: Option<BrowseView>,
    pub(crate) home_playlist_open: bool,
    pub(crate) library_album_open: bool,
    pub(crate) overview_detail_open: bool,
    pub(crate) browse_description_open: bool,
}

impl Default for NavigationState {
    fn default() -> Self {
        Self {
            page: Page::Home,
            collection_kind: SearchKind::Playlist,
            browse: None,
            home_playlist_open: false,
            library_album_open: false,
            overview_detail_open: false,
            browse_description_open: false,
        }
    }
}

impl NavigationState {
    /// Sidebar tabs start at their own root without retaining another tab's detail.
    pub(crate) fn enter(&mut self, page: Page) {
        self.page = page;
        self.browse = None;
        self.home_playlist_open = false;
        self.library_album_open = false;
        self.overview_detail_open = false;
        self.browse_description_open = false;
    }

    pub(crate) fn browse_is_visible(&self) -> bool {
        match self.page {
            Page::Home => self.home_playlist_open,
            Page::Library => self.library_album_open,
            Page::Overview => self.overview_detail_open,
            Page::Tracks | Page::Recent | Page::AccountOverview => false,
            _ => true,
        }
    }

    pub(crate) fn can_go_back(&self) -> bool {
        self.browse_is_visible() && self.browse.as_ref().is_some_and(|view| view.is_detail)
    }

    pub(crate) fn open_detail(&mut self, origin: Page) {
        if self.page != origin {
            return;
        }
        match origin {
            Page::Home => self.home_playlist_open = true,
            Page::Library => self.library_album_open = true,
            Page::Overview => self.overview_detail_open = true,
            _ => {}
        }
    }

    pub(crate) fn accept_browse(&mut self, view: BrowseView) {
        if self
            .browse
            .as_ref()
            .is_none_or(|old| old.generation != view.generation)
        {
            self.browse_description_open = false;
        }
        if !view.is_detail {
            self.home_playlist_open = false;
            self.library_album_open = false;
            self.overview_detail_open = false;
        }
        self.browse = Some(view);
    }
}

#[cfg(test)]
mod tests;
