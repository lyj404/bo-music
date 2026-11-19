use super::*;

impl Render for MusicApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let background = self.appearance_background(window, cx);
        let rgb = appearance::paint(cx);
        if self.appearance_page.open {
            return self.render_appearance(cx);
        }
        if let Some(video) = self.render_mv(window, cx) {
            return video;
        }
        if self.shortcuts.open {
            return self.render_shortcuts(cx);
        }
        if self.lyric_page.open {
            return self.render_immersive_lyrics(window, cx);
        }
        let show_search_assist = !self.auth.busy
            && !self.show_account
            && (self.show_hot_words
                || self.show_search_history
                || (!self.search_suggestions.is_empty()
                    && self.input.read(cx).focus_handle(cx).is_focused(window)));
        let root = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(background)
            .text_color(rgb(0x20293a))
            .when_some(
                appearance::font(appearance::FontTarget::Interface, cx),
                |view, family| view.font_family(family),
            )
            .on_action(cx.listener(Self::search))
            .track_focus(&self.playback_focus)
            .capture_any_mouse_down(
                cx.listener(|this, _, window, _| window.focus(&this.playback_focus)),
            )
            .on_key_down(cx.listener(Self::playback_shortcut))
            .child(self.render_shell_header(cx));
        self.render_shell_overlays(root, show_search_assist, cx)
            .child(self.render_shell_content(cx))
            .child(self.render_player_bar(cx))
            .into_any_element()
    }
}

impl MusicApp {
    pub(super) fn render_side_panel(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        if self.show_comments && self.comment_image.is_open() {
            self.render_comment_image(cx)
        } else if self.show_comments {
            self.render_comments_panel(cx)
        } else if self.show_quality {
            self.render_quality_panel(cx)
        } else if self.show_effects {
            self.render_effects_panel(cx)
        } else if self.show_lyrics {
            self.render_lyrics_panel(cx)
        } else {
            self.render_queue_panel(cx)
        }
    }
}
