//! State owned by the panels.

pub(crate) struct PanelState {
    pub(crate) player_tools_open: bool,
    pub(crate) side_panel_open: bool,
    pub(crate) show_lyrics: bool,
    pub(crate) show_effects: bool,
    pub(crate) show_quality: bool,
    pub(crate) show_comments: bool,
}

impl Default for PanelState {
    fn default() -> Self {
        Self {
            player_tools_open: false,
            side_panel_open: false,
            show_lyrics: true,
            show_effects: false,
            show_quality: false,
            show_comments: false,
        }
    }
}
