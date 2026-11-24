use super::*;

impl Render for MusicApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let compact = window.viewport_size().width < px(1050.);
        let background = self.appearance_background(window, cx);
        let rgb = appearance::paint(cx);
        if let Some(video) = self.render_mv(window, cx) {
            return video;
        }
        if self.lyric_page.open {
            return self.render_immersive_lyrics(window, cx);
        }
        let show_search_assist = !self.search.assist_dismissed
            && self.playlist_editor.dialog == PlaylistDialog::Closed
            && !self.auth.busy
            && !self.show_account
            && (self.search.show_hot_words
                || self.search.show_history
                || (!self.search.suggestions.is_empty()
                    && self
                        .search
                        .input
                        .read(cx)
                        .focus_handle(cx)
                        .is_focused(window)));
        let body = if self.shortcuts.open {
            self.render_shortcuts(cx)
        } else if self.appearance_page.open {
            self.render_appearance(cx)
        } else {
            self.render_shell_content(compact, cx)
        };
        let header = self.render_shell_header(show_search_assist, cx);
        let center = div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .border_1()
            .border_color(rgb(0xd8dce6))
            .rounded_tl_lg()
            .bg(rgb(0xffffff))
            .child(header);
        let center = self.render_shell_overlays(center, cx).child(body);
        let modal = self.render_playlist_modal(cx);
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(background)
            .text_color(rgb(0x20293a))
            .text_size(px(14.))
            .when_some(
                appearance::font(appearance::FontTarget::Interface, cx),
                |view, font| view.font_family(font),
            )
            .on_action(cx.listener(Self::search))
            .track_focus(&self.playback_focus)
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    if this.playlist_editor.dialog != PlaylistDialog::Closed {
                        if !this.playlist_editor.busy {
                            this.close_playlist_dialog(cx);
                            window.focus(&this.playback_focus);
                        }
                        cx.stop_propagation();
                        return;
                    }
                    if this.show_account {
                        this.close_account_menu(cx);
                        window.focus(&this.playback_focus);
                    }
                    this.dismiss_search_assist(cx);
                    this.panels.player_tools_open = false;
                    this.row_actions = None;
                    this.panels.side_panel_open = false;
                    this.close_comments(cx);
                    cx.notify();
                } else {
                    this.playback_shortcut(event, window, cx);
                }
            }))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .pt_2()
                    .child(self.render_navigation(compact, cx))
                    .child(center),
            )
            .when(
                ["失败", "无法", "不可用", "需要登录"]
                    .iter()
                    .any(|word| self.status.contains(word)),
                |root| {
                    root.child(
                        div()
                            .px_6()
                            .py_2()
                            .bg(rgb(0xe9edff))
                            .flex()
                            .items_center()
                            .gap_3()
                            .text_sm()
                            .child(div().flex_1().child(self.status.clone()))
                            .child(ui::icon_button("dismiss-status", "close", cx).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.status.clear();
                                    cx.notify();
                                }),
                            )),
                    )
                },
            )
            .child(self.render_player_bar(compact, cx))
            .when(self.show_account, |root| {
                root.child(self.render_account_dropdown(cx))
            })
            .when(
                self.playlist_editor.dialog != PlaylistDialog::Closed,
                |root| root.child(modal),
            )
            .into_any_element()
    }
}

impl MusicApp {
    pub(super) fn render_side_panel(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        if self.panels.show_comments && self.comment_panel.image.is_open() {
            self.render_comment_image(cx)
        } else if self.panels.show_comments {
            self.render_comments_panel(cx)
        } else if self.panels.show_quality {
            self.render_quality_panel(cx)
        } else if self.panels.show_effects {
            self.render_effects_panel(false, cx)
        } else if self.panels.show_lyrics {
            self.render_lyrics_panel(cx)
        } else {
            self.render_queue_panel(cx)
        }
    }
}
