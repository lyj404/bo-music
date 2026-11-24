use super::*;
use bo_music_app::browse::Browser;

fn detail() -> BrowseView {
    let mut view = Browser::default().view();
    view.is_detail = true;
    view
}

#[test]
fn sidebar_tabs_do_not_inherit_another_tabs_back_destination() {
    let mut state = NavigationState::default();
    assert!(!state.can_go_back());
    state.open_detail(Page::Home);
    state.accept_browse(detail());
    assert!(state.can_go_back());

    for page in [
        Page::Library,
        Page::Overview,
        Page::Charts,
        Page::Mine,
        Page::Collection,
        Page::PlaylistSquare,
        Page::Recent,
        Page::AccountOverview,
        Page::Home,
    ] {
        state.enter(page);
        assert!(!state.can_go_back());
        // A late detail snapshot alone must not open Home, Library or Overview details.
        if page != Page::Home {
            state.open_detail(Page::Home);
        }
        state.accept_browse(detail());
        if matches!(
            page,
            Page::Home | Page::Library | Page::Overview | Page::Recent | Page::AccountOverview
        ) {
            assert!(!state.can_go_back());
        }
    }
}

#[test]
fn returning_from_details_keeps_the_active_tab() {
    for page in [
        Page::Home,
        Page::Library,
        Page::Overview,
        Page::PlaylistSearch,
        Page::AlbumSearch,
        Page::ArtistSearch,
        Page::Mine,
        Page::Collection,
        Page::Charts,
    ] {
        let mut state = NavigationState::default();
        state.enter(page);
        state.open_detail(page);
        state.accept_browse(detail());
        assert!(state.can_go_back());

        // Nested details retain their origin until the service returns its root.
        state.accept_browse(detail());
        assert!(state.can_go_back());
        state.accept_browse(Browser::default().view());
        assert!(!state.can_go_back());
        if matches!(page, Page::Home | Page::Library | Page::Overview) {
            assert!(!state.browse_is_visible());
        }
        assert!(state.page == page);
    }
}
