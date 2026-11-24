//! Search results, request generations and suggestion panel state.

use crate::input::TextInput;
use bo_music_app::desktop_service::SearchSection;
use bo_music_domain::track::Track;
use gpui::Entity;

pub(crate) struct SearchState {
    pub(crate) input: Entity<TextInput>,
    pub(crate) overview_generation: u64,
    pub(crate) overview_query: String,
    pub(crate) overview_result: Option<Result<Vec<SearchSection>, ()>>,
    pub(crate) suggestion_query: Option<String>,
    pub(crate) suggestion_generation: u64,
    pub(crate) suggestions: Vec<String>,
    pub(crate) assist_dismissed: bool,
    pub(crate) show_hot_words: bool,
    pub(crate) show_history: bool,
    pub(crate) history: Option<Result<Vec<String>, String>>,
    pub(crate) confirm_clear_history: bool,
    pub(crate) hot_words: Option<Vec<String>>,
    pub(crate) hot_words_loading: bool,
    pub(crate) generation: u64,
    pub(crate) tracks: Vec<Track>,
    pub(crate) has_more: bool,
    pub(crate) loading: bool,
}

impl SearchState {
    pub(crate) fn new(input: Entity<TextInput>) -> Self {
        Self {
            input,
            overview_generation: 0,
            overview_query: String::new(),
            overview_result: None,
            suggestion_query: Some(String::new()),
            suggestion_generation: 0,
            suggestions: Vec::new(),
            assist_dismissed: false,
            show_hot_words: false,
            show_history: false,
            history: None,
            confirm_clear_history: false,
            hot_words: None,
            hot_words_loading: false,
            generation: 0,
            tracks: Vec::new(),
            has_more: false,
            loading: false,
        }
    }

    pub(crate) fn dismiss_assist(&mut self) {
        self.assist_dismissed = true;
        self.hide_assist_panels();
    }

    pub(crate) fn invalidate_suggestions(&mut self) {
        self.suggestion_generation = self.suggestion_generation.wrapping_add(1);
        self.suggestions.clear();
    }

    pub(crate) fn update_suggestion_query(&mut self, query: Option<String>) -> bool {
        if self.suggestion_query == query {
            return false;
        }
        self.assist_dismissed = false;
        self.suggestion_query = query;
        self.invalidate_suggestions();
        self.hide_assist_panels();
        true
    }

    fn hide_assist_panels(&mut self) {
        self.show_hot_words = false;
        self.show_history = false;
        self.confirm_clear_history = false;
    }
}
