use super::*;

impl MusicApp {
    pub(super) fn render_shell_content(
        &self,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        let queue_stamp = self
            .player
            .as_ref()
            .map(|p| (p.queue_account_revision, p.queue_revision));
        let list_source = if self.navigation.page == Page::Recent {
            BatchSource::History(self.history_generation)
        } else {
            BatchSource::Search(self.search.generation)
        };
        let rows = if self.navigation.page == Page::Recent {
            &self.recent_tracks
        } else {
            &self.search.tracks
        }
        .iter()
        .enumerate()
        .map(|(index, track)| {
            let picker_track = track.clone();
            let append_track = track.clone();
            let next_track = track.clone();
            let valid_track = track.id > 0;
            let track_id = track.id;
            let checked = self.batch_generation == Some(list_source)
                && self.batch_selection.contains(&(index, track_id));
            let show_actions = self.row_actions == Some((list_source, index, track_id));
            let title = track.title.clone();
            let artist = track.artist_text.clone();
            let album = track.album_name.clone().unwrap_or_default();
            div()
                .id(("track", index))
                .group("track-row")
                .flex()
                .flex_wrap()
                .items_center()
                .gap_3()
                .p_3()
                .rounded_md()
                .hover(|style| style.bg(rgb(0xe9edf5)).cursor_pointer())
                .on_click(cx.listener(move |this, _, _, _| {
                    let tracks = if this.navigation.page == Page::Recent {
                        &this.recent_tracks
                    } else {
                        &this.search.tracks
                    };
                    let _ = this
                        .service
                        .commands
                        .send(Command::Play(tracks.clone(), index));
                }))
                .when(valid_track, |row| {
                    row.child(
                        div()
                            .id(("list-batch-select-track", index))
                            .cursor_pointer()
                            .text_color(rgb(0x415de5))
                            .child(if checked { "☑" } else { "☐" })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                if this.batch_generation != Some(list_source) {
                                    this.batch_selection.clear();
                                    this.batch_generation = Some(list_source);
                                }
                                if !this.batch_selection.insert((index, track_id)) {
                                    this.batch_selection.remove(&(index, track_id));
                                }
                                this.batch_remove_confirm = false;
                                cx.notify();
                            })),
                    )
                })
                .child(self.cover(track.cover_url.as_deref(), 40.0, cx))
                .child(
                    div()
                        .w(px(36.))
                        .text_color(rgb(0x8a91a2))
                        .child(format!("{:02}", index + 1)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(div().truncate().child(title))
                        .child(
                            div()
                                .truncate()
                                .text_size(px(12.))
                                .text_color(rgb(0x788196))
                                .child(artist.clone()),
                        ),
                )
                .when(!compact, |row| {
                    row.child(
                        div()
                            .w(px(140.))
                            .truncate()
                            .text_sm()
                            .text_color(rgb(0x788196))
                            .child(album),
                    )
                })
                .child(
                    div()
                        .w(px(40.))
                        .flex_shrink_0()
                        .text_size(px(11.))
                        .text_color(rgb(0x788196))
                        .child(clock(track.duration_seconds.max(0) as u64)),
                )
                .child(
                    ui::icon_button(("track-more", index), "more", cx).on_click(cx.listener(
                        move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.row_actions = if show_actions {
                                None
                            } else {
                                Some((list_source, index, track_id))
                            };
                            cx.notify();
                        },
                    )),
                )
                .when(show_actions, |row| {
                    row.child(
                        div()
                            .w_full()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_4()
                            .px_3()
                            .py_2()
                            .bg(rgb(0xf5f7fb))
                            .rounded_md()
                            .child(
                                div()
                                    .id(("track-add-playlist", index))
                                    .text_sm()
                                    .text_color(rgb(0x415de5))
                                    .cursor_pointer()
                                    .child("加入歌单")
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        cx.stop_propagation();
                                        let _ = this
                                            .service
                                            .commands
                                            .send(Command::OpenTrackPicker(picker_track.clone()));
                                    })),
                            )
                            .when(queue_stamp.is_some() && valid_track, |row| {
                                let (account, revision) = queue_stamp.expect("queue checked");
                                row.child(
                                    div()
                                        .id(("track-append-queue", index))
                                        .text_sm()
                                        .text_color(rgb(0x415de5))
                                        .cursor_pointer()
                                        .child("排队")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            let _ = this.service.commands.send(Command::EditQueue(
                                                account,
                                                revision,
                                                QueueEdit::Append(append_track.clone()),
                                            ));
                                        })),
                                )
                                .child(
                                    div()
                                        .id(("track-next-queue", index))
                                        .text_sm()
                                        .text_color(rgb(0x415de5))
                                        .cursor_pointer()
                                        .child("下一首")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            let _ = this.service.commands.send(Command::EditQueue(
                                                account,
                                                revision,
                                                QueueEdit::PlayNext(next_track.clone()),
                                            ));
                                        })),
                                )
                            }),
                    )
                })
        })
        .collect::<Vec<_>>();
        let list_tracks = if self.navigation.page == Page::Recent {
            &self.recent_tracks
        } else {
            &self.search.tracks
        };
        let mut list_selected = if self.batch_generation == Some(list_source) {
            self.batch_selection.iter().copied().collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        list_selected.sort_unstable();
        let all_list_tracks = list_tracks
            .iter()
            .enumerate()
            .filter_map(|(index, track)| (track.id > 0).then_some((index, track.id)))
            .collect::<HashSet<_>>();
        let list_count = list_selected.len();
        let list_add_selection = list_selected.clone();
        let list_batch_toolbar = div()
            .flex()
            .items_center()
            .gap_3()
            .pb_3()
            .text_sm()
            .child(format!("已选 {list_count} 首"))
            .child(
                div()
                    .id("list-batch-all")
                    .cursor_pointer()
                    .text_color(rgb(0x415de5))
                    .child("全选已加载")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.batch_selection = all_list_tracks.clone();
                        this.batch_generation = Some(list_source);
                        cx.notify();
                    })),
            )
            .when(list_count > 0, |bar| {
                bar.child(
                    div()
                        .id("list-batch-clear")
                        .cursor_pointer()
                        .text_color(rgb(0x415de5))
                        .child("清空选择")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.batch_selection.clear();
                            this.batch_generation = None;
                            cx.notify();
                        })),
                )
            })
            .when(
                list_count > 0 && !self.playlist_editor.busy && !self.auth.busy,
                |bar| {
                    bar.child(
                        div()
                            .id("list-batch-add")
                            .cursor_pointer()
                            .text_color(rgb(0x415de5))
                            .child("批量加入歌单")
                            .on_click(cx.listener(move |this, _, _, _| {
                                let _ = this.service.commands.send(Command::OpenBatchPicker(
                                    list_source,
                                    list_add_selection.clone(),
                                ));
                            })),
                    )
                },
            )
            .when(
                list_count > 0 && !self.playlist_editor.busy && !self.auth.busy,
                |bar| {
                    bar.child(
                        div()
                            .id("list-batch-like")
                            .cursor_pointer()
                            .text_color(rgb(0x415de5))
                            .child("批量喜欢")
                            .on_click(cx.listener(move |this, _, _, _| {
                                let _ = this.service.commands.send(Command::LikeSelected(
                                    list_source,
                                    list_selected.clone(),
                                ));
                            })),
                    )
                },
            );
        let browse_body = self.render_browse_content(queue_stamp, compact, cx);
        let home_body = if self.navigation.page == Page::Home && !self.navigation.home_playlist_open
        {
            self.render_home(compact, cx)
        } else {
            div().into_any_element()
        };
        let library_body =
            if self.navigation.page == Page::Library && !self.navigation.library_album_open {
                self.render_library(cx)
            } else {
                div().into_any_element()
            };
        let side_body = self.render_side_panel(cx);
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .gap_3()
            .px_6()
            .pb_4()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .bg(rgb(0xffffff))
                    .rounded_lg()
                    .px_2()
                    .py_4()
                    .when(
                        matches!(
                            self.navigation.page,
                            Page::Overview
                                | Page::Tracks
                                | Page::PlaylistSearch
                                | Page::AlbumSearch
                                | Page::ArtistSearch
                        ),
                        |panel| {
                            panel
                                .child(ui::eyebrow("SEARCH RESULTS", cx))
                                .child(ui::heading("搜索结果", cx).mt_1().mb_2())
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(0x788196))
                                        .mb_4()
                                        .child(self.search.input.read(cx).text().to_owned()),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .gap_5()
                                        .pb_2()
                                        .mb_4()
                                        .border_b_1()
                                        .border_color(rgb(0xd8dce6))
                                        .children(
                                            [
                                                (Page::Overview, "综合"),
                                                (Page::Tracks, "歌曲"),
                                                (Page::PlaylistSearch, "歌单"),
                                                (Page::AlbumSearch, "专辑"),
                                                (Page::ArtistSearch, "歌手"),
                                            ]
                                            .into_iter()
                                            .enumerate()
                                            .map(
                                                |(index, (page, label))| {
                                                    div()
                                                        .id(("search-kind", index))
                                                        .py_2()
                                                        .cursor_pointer()
                                                        .text_color(rgb(
                                                            if self.navigation.page == page {
                                                                0x20293a
                                                            } else {
                                                                0x788196
                                                            },
                                                        ))
                                                        .when(self.navigation.page == page, |tab| {
                                                            tab.border_b_2()
                                                                .border_color(rgb(0x415de5))
                                                        })
                                                        .child(label)
                                                        .on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                this.navigation.page = page;
                                                                this.run_search(cx);
                                                            },
                                                        ))
                                                },
                                            ),
                                        ),
                                )
                        },
                    )
                    .when(self.navigation.page == Page::Recent, |panel| {
                        panel
                            .child(ui::eyebrow("LISTENING HISTORY", cx))
                            .child(ui::heading("最近播放", cx).mt_1().mb_5())
                    })
                    .when(
                        self.navigation.page == Page::Collection
                            && !self
                                .navigation
                                .browse
                                .as_ref()
                                .is_some_and(|view| view.is_detail),
                        |panel| {
                            panel.child(
                                div().flex().gap_4().pb_3().children(
                                    [
                                        (SearchKind::Playlist, "歌单"),
                                        (SearchKind::Album, "专辑"),
                                        (SearchKind::Artist, "歌手"),
                                    ]
                                    .into_iter()
                                    .enumerate()
                                    .map(|(index, (kind, label))| {
                                        div()
                                            .id(("collection-kind", index))
                                            .cursor_pointer()
                                            .text_color(
                                                if self.navigation.collection_kind == kind {
                                                    rgb(0x415de5)
                                                } else {
                                                    rgb(0x687083)
                                                },
                                            )
                                            .child(label)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.navigation.collection_kind = kind;
                                                let _ = this
                                                    .service
                                                    .commands
                                                    .send(Command::Collections(kind));
                                                cx.notify();
                                            }))
                                    })
                                    .collect::<Vec<_>>(),
                                ),
                            )
                        },
                    )
                    .when(
                        self.navigation.page == Page::Home && !self.navigation.home_playlist_open,
                        |panel| panel.child(home_body),
                    )
                    .when(
                        self.navigation.page == Page::Library
                            && !self.navigation.library_album_open,
                        |panel| panel.child(library_body),
                    )
                    .when(
                        self.navigation.page == Page::Overview
                            && !self.navigation.overview_detail_open,
                        |panel| panel.child(self.render_search_overview(cx)),
                    )
                    .when(self.navigation.page == Page::AccountOverview, |panel| {
                        panel.child(self.render_account_overview(compact, cx))
                    })
                    .when(self.navigation.browse_is_visible(), |panel| {
                        panel.child(browse_body)
                    })
                    .when(
                        self.navigation.page == Page::Tracks
                            || self.navigation.page == Page::Recent,
                        |panel| {
                            panel.child(list_batch_toolbar).child(
                                div()
                                    .id("search-results")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .when(rows.is_empty(), |view| {
                                        view.child(div().p_6().text_color(rgb(0x8a91a2)).child(
                                            if self.navigation.page == Page::Recent {
                                                if self.recent_loading {
                                                    "正在加载最近播放…".to_owned()
                                                } else {
                                                    self.recent_error.clone().unwrap_or_else(|| {
                                                        "暂无最近播放".to_owned()
                                                    })
                                                }
                                            } else {
                                                "输入关键词开始搜索".to_owned()
                                            },
                                        ))
                                    })
                                    .children(rows)
                                    .when(
                                        self.navigation.page == Page::Recent
                                            && self.recent_error.is_some(),
                                        |view| {
                                            view.child(
                                                div()
                                                    .id("history-retry")
                                                    .p_3()
                                                    .text_color(rgb(0x415de5))
                                                    .cursor_pointer()
                                                    .child("重试")
                                                    .on_click(cx.listener(|this, _, _, _| {
                                                        let _ = this
                                                            .service
                                                            .commands
                                                            .send(Command::ShowHistory);
                                                    })),
                                            )
                                        },
                                    )
                                    .when(
                                        self.navigation.page == Page::Tracks
                                            && (self.search.has_more || self.search.loading),
                                        |view| {
                                            view.child(
                                                ui::icon_button_labeled(
                                                    "more",
                                                    "refresh",
                                                    if self.search.loading {
                                                        "正在加载…"
                                                    } else {
                                                        "加载更多"
                                                    },
                                                    cx,
                                                )
                                                .when(self.search.loading, |button| {
                                                    button.opacity(0.45).cursor_default()
                                                })
                                                .when(!self.search.loading, |button| {
                                                    button.cursor_pointer().on_click(cx.listener(
                                                        |this, _, _, _| {
                                                            let _ = this
                                                                .service
                                                                .commands
                                                                .send(Command::SearchNext);
                                                        },
                                                    ))
                                                }),
                                            )
                                        },
                                    ),
                            )
                        },
                    ),
            )
            .when(self.panels.side_panel_open, |root| {
                root.child(
                    div()
                        .absolute()
                        .right_0()
                        .top_0()
                        .bottom_0()
                        .shadow_lg()
                        .border_l_1()
                        .border_color(rgb(0xd8dce6))
                        .w(px(if compact { 300. } else { 340. }))
                        .flex_shrink_0()
                        .flex()
                        .flex_col()
                        .bg(rgb(0xffffff))
                        .rounded_lg()
                        .p_4()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .pb_3()
                                .child(
                                    div()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .child("播放面板"),
                                )
                                .child(ui::icon_button("close-side-panel", "close", cx).on_click(
                                    cx.listener(|this, _, _, cx| {
                                        this.panels.side_panel_open = false;
                                        this.close_comments(cx);
                                        cx.notify();
                                    }),
                                )),
                        )
                        .when_some(self.player.as_ref(), |panel, player| {
                            panel.child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .pb_4()
                                    .child(self.cover(player.cover_url.as_deref(), 48., cx))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .text_color(rgb(0x788196))
                                                    .child("正在播放"),
                                            )
                                            .child(
                                                div()
                                                    .truncate()
                                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                                    .child(player.title.clone()),
                                            )
                                            .child(
                                                div()
                                                    .truncate()
                                                    .text_sm()
                                                    .text_color(rgb(0x788196))
                                                    .child(player.artist.clone()),
                                            ),
                                    ),
                            )
                        })
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_2()
                                .pb_3()
                                .child(
                                    div()
                                        .id("lyrics-tab")
                                        .px_2()
                                        .py_1()
                                        .rounded_md()
                                        .when(
                                            self.panels.show_lyrics
                                                && !self.panels.show_effects
                                                && !self.panels.show_quality
                                                && !self.panels.show_comments,
                                            |item| {
                                                item.bg(rgb(0xe9edff))
                                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                            },
                                        )
                                        .cursor_pointer()
                                        .text_color(
                                            if self.panels.show_lyrics
                                                && !self.panels.show_effects
                                                && !self.panels.show_quality
                                                && !self.panels.show_comments
                                            {
                                                rgb(0x415de5)
                                            } else {
                                                rgb(0x8a91a2)
                                            },
                                        )
                                        .child("歌词")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.close_comments(cx);
                                            this.panels.show_lyrics = true;
                                            this.panels.show_effects = false;
                                            this.panels.show_quality = false;
                                            if let Some(index) = this.lyric_line {
                                                this.lyric_scroll.scroll_to_item(index);
                                            }
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    div()
                                        .id("queue-tab")
                                        .px_2()
                                        .py_1()
                                        .rounded_md()
                                        .when(
                                            !self.panels.show_lyrics
                                                && !self.panels.show_effects
                                                && !self.panels.show_quality
                                                && !self.panels.show_comments,
                                            |item| {
                                                item.bg(rgb(0xe9edff))
                                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                            },
                                        )
                                        .cursor_pointer()
                                        .text_color(
                                            if self.panels.show_lyrics
                                                || self.panels.show_effects
                                                || self.panels.show_quality
                                                || self.panels.show_comments
                                            {
                                                rgb(0x8a91a2)
                                            } else {
                                                rgb(0x415de5)
                                            },
                                        )
                                        .child("队列")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.close_comments(cx);
                                            this.panels.show_lyrics = false;
                                            this.panels.show_effects = false;
                                            this.panels.show_quality = false;
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    div()
                                        .id("effects-tab")
                                        .px_2()
                                        .py_1()
                                        .rounded_md()
                                        .when(self.panels.show_effects, |item| {
                                            item.bg(rgb(0xe9edff))
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                        })
                                        .cursor_pointer()
                                        .text_color(if self.panels.show_effects {
                                            rgb(0x415de5)
                                        } else {
                                            rgb(0x8a91a2)
                                        })
                                        .child("音效")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.close_comments(cx);
                                            this.panels.show_effects = true;
                                            this.panels.show_quality = false;
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    div()
                                        .id("quality-tab")
                                        .px_2()
                                        .py_1()
                                        .rounded_md()
                                        .when(self.panels.show_quality, |item| {
                                            item.bg(rgb(0xe9edff))
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                        })
                                        .cursor_pointer()
                                        .text_color(if self.panels.show_quality {
                                            rgb(0x415de5)
                                        } else {
                                            rgb(0x8a91a2)
                                        })
                                        .child("音质")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.close_comments(cx);
                                            this.panels.show_quality = true;
                                            this.panels.show_effects = false;
                                            cx.notify();
                                        })),
                                )
                                .child(
                                    div()
                                        .id("comments-tab")
                                        .px_2()
                                        .py_1()
                                        .rounded_md()
                                        .when(self.panels.show_comments, |item| {
                                            item.bg(rgb(0xe9edff))
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                        })
                                        .cursor_pointer()
                                        .text_color(if self.panels.show_comments {
                                            rgb(0x415de5)
                                        } else {
                                            rgb(0x8a91a2)
                                        })
                                        .child("评论")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            if !this.panels.show_comments {
                                                this.panels.show_comments = true;
                                                this.panels.show_effects = false;
                                                this.panels.show_quality = false;
                                                this.panels.show_lyrics = false;
                                                let _ = this
                                                    .service
                                                    .commands
                                                    .send(Command::OpenComments);
                                            }
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_2()
                                .text_sm()
                                .pb_3()
                                .child(
                                    div()
                                        .id("immersive-lyrics-open")
                                        .cursor_pointer()
                                        .text_color(rgb(0x415de5))
                                        .child("展开歌词")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.open_immersive_lyrics(window, cx)
                                        })),
                                )
                                .child(
                                    div()
                                        .id("mini-player-open")
                                        .cursor_pointer()
                                        .text_color(rgb(0x415de5))
                                        .child("迷你播放器")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.open_mini_player(window, cx)
                                        })),
                                )
                                .child(self.desktop_lyrics_buttons(cx)),
                        )
                        .child(side_body),
                )
            })
            .into_any_element()
    }
}
