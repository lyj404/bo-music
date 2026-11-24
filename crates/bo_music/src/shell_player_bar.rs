use super::*;

impl MusicApp {
    pub(super) fn render_player_bar(
        &self,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        let palette = appearance::palette(cx);
        let player = self.player.as_ref();
        let menu_open = self.panels.player_tools_open;
        let playing = player.is_some_and(|p| p.state == PlaybackState::Playing);
        let position = player.map_or(0, |p| p.position_ms);
        let duration = player.map_or(0, |p| p.duration_ms);
        let progress = if duration == 0 {
            0.
        } else {
            position as f64 / duration as f64
        };
        let stamp = self.lyric_seek_stamp();
        let entity = cx.weak_entity();
        let volume_entity = cx.weak_entity();
        let volume = player.map_or(100, |p| p.volume);
        let track = player.and_then(|p| p.track_id).filter(|id| *id > 0);
        let like = track.is_some()
            && self.like.track_id == track
            && self.like.state == LikeState::Ready(true);
        let can_like = self.auth.account.is_some()
            && track.is_some()
            && self.like.track_id == track
            && matches!(
                self.like.state,
                LikeState::Ready(_) | LikeState::Unknown | LikeState::NoPlaylist
            );
        let title = player
            .map_or("选择一首音乐，开始聆听", |p| p.title.as_str())
            .to_owned();
        let artist = player.map_or("bo-music", |p| p.artist.as_str()).to_owned();
        let left = div()
            .w(px(if compact { 180. } else { 230. }))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_3()
            .child(self.cover(player.and_then(|p| p.cover_url.as_deref()), 55., cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(13.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_size(px(12.))
                            .text_color(rgb(0x788196))
                            .child(artist),
                    ),
            )
            .child(
                ui::icon_button("like-current-track", "heart", cx)
                    .when(like, |item| item.bg(rgb(0xe9edff)))
                    .when(self.auth.account.is_none(), |item| {
                        item.on_click(cx.listener(|this, _, _, cx| {
                            this.show_account = true;
                            let _ = this.service.commands.send(Command::RefreshAccounts);
                            cx.notify();
                        }))
                    })
                    .when(can_like, |item| {
                        item.on_click(cx.listener(move |this, _, _, _| {
                            if let Some(id) = track {
                                let command = if matches!(this.like.state, LikeState::Ready(_)) {
                                    Command::ToggleLike(id)
                                } else {
                                    Command::RetryLike(id)
                                };
                                let _ = this.service.commands.send(command);
                            }
                        }))
                    }),
            );
        let transport = div()
            .flex_1()
            .max_w(px(390.))
            .min_w(px(220.))
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .child(ui::icon_button("mode", "shuffle", cx).on_click(cx.listener(
                        |this, _, window, _| {
                            if !this.lyric_page.open {
                                window.focus(&this.playback_focus);
                            }
                            let _ = this.service.commands.send(Command::CycleMode);
                        },
                    )))
                    .child(
                        ui::icon_button("previous", "previous", cx).on_click(cx.listener(
                            |this, _, window, _| {
                                if !this.lyric_page.open {
                                    window.focus(&this.playback_focus);
                                }
                                let _ = this.service.commands.send(Command::Previous);
                            },
                        )),
                    )
                    .child(
                        div()
                            .id("toggle")
                            .size(px(40.))
                            .rounded_full()
                            .bg(rgb(0x4d66e5))
                            .hover(|button| button.bg(rgb(0x3854d5)))
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(ui::icon(
                                if playing { "pause" } else { "play" },
                                gpui::rgb(0xffffff),
                            ))
                            .on_click(cx.listener(|this, _, window, _| {
                                if !this.lyric_page.open {
                                    window.focus(&this.playback_focus);
                                }
                                let _ = this.service.commands.send(Command::Toggle);
                            })),
                    )
                    .child(ui::icon_button("next", "next", cx).on_click(cx.listener(
                        |this, _, window, _| {
                            if !this.lyric_page.open {
                                window.focus(&this.playback_focus);
                            }
                            let _ = this.service.commands.send(Command::Next);
                        },
                    )))
                    .child(ui::icon_button("player-more", "more", cx).on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.panels.player_tools_open = !menu_open;
                            cx.notify();
                        }),
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(30.))
                            .text_size(px(10.))
                            .text_color(rgb(0x788196))
                            .child(clock(position / 1000)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .child(ui::slider(progress, rgb, move |fraction, cx| {
                                if duration > 0 {
                                    let _ = entity.update(cx, |this, _| {
                                        this.seek_lyric(stamp, fraction * duration as f64 / 1000.)
                                    });
                                }
                            })),
                    )
                    .child(
                        div()
                            .w(px(30.))
                            .text_right()
                            .text_size(px(10.))
                            .text_color(rgb(0x788196))
                            .child(clock(duration / 1000)),
                    ),
            );
        let tools = div()
            .w(px(if compact {
                200.
            } else if cfg!(windows) {
                420.
            } else {
                400.
            }))
            .flex_shrink_0()
            .flex()
            .when(compact, |tools| tools.flex_col().items_end().gap_1())
            .when(!compact, |tools| tools.items_center().justify_end().gap_5())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .children(
                        [(false, "equalizer", "音效"), (true, "quality", "音质")]
                            .into_iter()
                            .map(|(quality, icon, label)| {
                                let open = !self.panels.side_panel_open
                                    && if quality {
                                        self.panels.show_quality
                                    } else {
                                        self.panels.show_effects
                                    };
                                ui::icon_button_labeled(label, icon, label, cx)
                                    .when(open, |button| button.bg(rgb(0xe9edff)))
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        if this.lyric_page.open {
                                            this.close_immersive_lyrics(window, cx);
                                        }
                                        this.close_comments(cx);
                                        this.panels.side_panel_open = false;
                                        this.panels.player_tools_open = false;
                                        this.panels.show_lyrics = false;
                                        this.panels.show_quality = quality && !open;
                                        this.panels.show_effects = !quality && !open;
                                        cx.notify();
                                    }))
                            }),
                    )
                    .child(ui::icon("volume", rgb(0x687083)))
                    .child(
                        div()
                            .w(px(if compact { 45. } else { 65. }))
                            .child(ui::slider(
                                f64::from(volume) / 100.,
                                rgb,
                                move |fraction, cx| {
                                    let _ = volume_entity.update(cx, |this, _| {
                                        let _ = this
                                            .service
                                            .commands
                                            .send(Command::SetVolume((fraction * 100.).round()));
                                    });
                                },
                            )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .when(compact, |group| group.gap_1())
                    .when(!compact, |group| {
                        group
                            .gap_3()
                            .pl_3()
                            .border_l_1()
                            .border_color(palette.border)
                    })
                    .child(
                        ui::icon_button_labeled(
                            "toggle-lyrics-panel",
                            "lyrics",
                            if self.lyric_page.open {
                                "收起歌词"
                            } else {
                                "展开歌词"
                            },
                            cx,
                        )
                        .when(self.lyric_page.open, |button| button.bg(rgb(0xe9edff)))
                        .on_click(cx.listener(|this, _, window, cx| {
                            if this.lyric_page.open {
                                this.close_immersive_lyrics(window, cx);
                            } else {
                                this.open_immersive_lyrics(window, cx);
                            }
                        })),
                    )
                    .child(
                        ui::icon_button_labeled(
                            "mini-player-open",
                            "mini-player",
                            "迷你播放器",
                            cx,
                        )
                        .when(self.mini_player.is_some(), |button| {
                            button.bg(rgb(0xe9edff))
                        })
                        .on_click(
                            cx.listener(|this, _, window, cx| this.open_mini_player(window, cx)),
                        ),
                    )
                    .child(self.desktop_lyrics_buttons(cx))
                    .child(
                        ui::icon_button("toggle-queue-panel", "list", cx)
                            .when(self.panels.side_panel_open, |button| {
                                button.bg(rgb(0xe9edff))
                            })
                            .on_click(cx.listener(|this, _, window, cx| {
                                if this.lyric_page.open {
                                    this.close_immersive_lyrics(window, cx);
                                }
                                this.close_comments(cx);
                                this.panels.side_panel_open = !this.panels.side_panel_open;
                                this.panels.show_lyrics = false;
                                this.panels.show_effects = false;
                                this.panels.show_quality = false;
                                cx.notify();
                            })),
                    ),
            );
        div()
            .relative()
            .id("player-bar")
            .h(px(94.))
            .flex_shrink_0()
            .px_6()
            .border_t_1()
            .border_color(palette.border)
            .bg(palette.elevated)
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .child(left)
            .child(transport)
            .child(tools)
            .when(
                !self.panels.side_panel_open
                    && (self.panels.show_effects || self.panels.show_quality),
                |root| root.child(self.render_player_settings(cx)),
            )
            .when(self.panels.player_tools_open, |root| {
                root.child(self.render_player_menu(cx))
            })
            .into_any_element()
    }

    fn render_player_settings(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let colors = appearance::palette(cx);
        let quality = self.panels.show_quality;
        div()
            .id("player-settings-popover")
            .absolute()
            .bottom(px(100.))
            .right(px(16.))
            .w(px(340.))
            .max_h(px(360.))
            .overflow_y_scroll()
            .occlude()
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .p_4()
            .rounded_lg()
            .bg(colors.elevated)
            .border_1()
            .border_color(colors.border)
            .shadow_lg()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.panels.show_effects = false;
                this.panels.show_quality = false;
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .mb_3()
                    .child(if quality { "音质" } else { "音效" })
                    .child(
                        ui::icon_button("close-player-settings", "close", cx).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.panels.show_effects = false;
                                this.panels.show_quality = false;
                                cx.notify();
                            }),
                        ),
                    ),
            )
            .child(if quality {
                self.render_quality_panel(cx)
            } else {
                self.render_effects_panel(false, cx)
            })
            .into_any_element()
    }

    fn render_player_menu(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        let stamp = self
            .player
            .as_ref()
            .map(|p| (p.queue_account_revision, p.queue_revision));
        let track = self
            .player
            .as_ref()
            .and_then(|p| p.track_id)
            .filter(|id| *id > 0);
        div()
            .absolute()
            .bottom(px(90.))
            .right(px(20.))
            .w(px(250.))
            .p_3()
            .rounded_lg()
            .bg(rgb(0xffffff))
            .border_1()
            .border_color(rgb(0xd8dce6))
            .shadow_lg()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.panels.player_tools_open = false;
                cx.notify();
            }))
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .text_sm()
                    .text_color(rgb(0x788196))
                    .px_2()
                    .pb_2()
                    .child("播放选项"),
            )
            .children(
                [(-10, "后退 10 秒"), (10, "快进 10 秒")]
                    .into_iter()
                    .enumerate()
                    .map(|(index, (delta, label))| {
                        div()
                            .id(("player-seek", index))
                            .px_3()
                            .py_2()
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|item| item.bg(rgb(0xe9edf5)))
                            .child(label)
                            .on_click(cx.listener(move |this, _, _, _| {
                                let _ = this.service.commands.send(Command::SeekRelative(delta));
                            }))
                    }),
            )
            .child(
                ui::icon_button_labeled("player-comments", "list", "评论", cx).on_click(
                    cx.listener(|this, _, window, cx| {
                        if this.lyric_page.open {
                            this.close_immersive_lyrics(window, cx);
                        }
                        this.close_comments(cx);
                        this.panels.side_panel_open = true;
                        this.panels.show_lyrics = false;
                        this.panels.show_effects = false;
                        this.panels.show_quality = false;
                        this.panels.show_comments = true;
                        this.panels.player_tools_open = false;
                        let _ = this.service.commands.send(Command::OpenComments);
                        cx.notify();
                    }),
                ),
            )
            .when_some(track.filter(|_| !self.auth.busy), |menu, id| {
                menu.child(
                    div()
                        .id("open-current-mv")
                        .px_3()
                        .py_2()
                        .cursor_pointer()
                        .child("观看 MV")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some((account, _)) = stamp {
                                let _ = this.service.commands.send(Command::OpenMv(account, id));
                            }
                            this.panels.player_tools_open = false;
                            cx.notify();
                        })),
                )
                .child(
                    ui::icon_button_labeled("share-current-track", "share", "分享歌曲", cx)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some((account, revision)) = stamp {
                                let _ = this
                                    .service
                                    .commands
                                    .send(Command::ShareCurrent(account, revision, id));
                            }
                            this.panels.player_tools_open = false;
                            cx.notify();
                        })),
                )
            })
            .into_any_element()
    }
}
