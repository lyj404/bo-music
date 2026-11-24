use super::*;

impl MusicApp {
    pub(super) fn render_browse_content(
        &self,
        queue_stamp: Option<(u64, u64)>,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        if let Some(view) = self.navigation.browse.as_ref().filter(|view| {
            if self.navigation.page == Page::PlaylistSquare {
                !self.auth.busy && view.square.is_some()
            } else {
                view.square.is_none()
            }
        }) {
            let mut selected =
                if self.batch_generation == Some(BatchSource::Browse(view.generation)) {
                    self.batch_selection.iter().copied().collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
            selected.sort_unstable();
            let browse_rows = self.render_browse_rows(view, &selected, queue_stamp, compact, cx);
            let is_detail = view.is_detail;
            let can_play_all = matches!(&view.content, BrowseContent::Tracks(tracks) if !tracks.is_empty())
                && !view.loading;
            let message = view.message.clone();
            let retry = view.error && !view.loading;
            let (detail_line, description) = match &view.detail {
                Some(DetailView::Album(album)) => (
                    format!(
                        "{}  ·  {}  ·  {} 首",
                        album.artist_text, album.release_date, album.music_count
                    ),
                    album.description.clone(),
                ),
                Some(DetailView::Artist(artist)) => (
                    format!(
                        "{}  ·  {} 首  ·  {} 张专辑  ·  {} 粉丝",
                        artist.alias, artist.song_count, artist.album_count, artist.fans_count
                    ),
                    artist.description.clone(),
                ),
                None => (String::new(), String::new()),
            };
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .when(is_detail, |body| {
                    body.child(
                        div()
                            .flex()
                            .items_center()
                            .gap_6()
                            .pb_5()
                            .child(self.cover(view.cover_url.as_deref(), 170., cx))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(ui::eyebrow(
                                        if view.artist_page {
                                            "ARTIST"
                                        } else {
                                            "MUSIC COLLECTION"
                                        },
                                        cx,
                                    ))
                                    .child(ui::heading(view.title.clone(), cx))
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(rgb(0x788196))
                                            .child(detail_line.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(rgb(0x788196))
                                            .max_h(px(48.))
                                            .overflow_hidden()
                                            .child(description.clone()),
                                    ),
                            ),
                    )
                })
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .gap_3()
                        .pb_4()
                        .when(is_detail, |row| {
                            row.child(
                                ui::icon_button("browse-back", "back", cx)
                                    .on_click(cx.listener(|this, _, _, cx| this.go_back(cx))),
                            )
                        })
                        .when(!is_detail, |row| {
                            row.child(ui::heading(view.title.clone(), cx))
                        })
                        .when(view.can_share && !self.auth.busy, |row| {
                            let generation = view.generation;
                            row.child(
                                ui::icon_button_labeled("share-detail", "share", "分享", cx)
                                    .on_click(cx.listener(move |this, _, _, _| {
                                        let _ = this
                                            .service
                                            .commands
                                            .send(Command::ShareDetail(generation));
                                    })),
                            )
                        })
                        .when(
                            view.my_playlists
                                && self.auth.account.is_some()
                                && !view.loading
                                && !self.playlist_editor.busy,
                            |row| {
                                row.child(
                                    ui::icon_button_labeled(
                                        "playlist-create",
                                        "add",
                                        "新建歌单",
                                        cx,
                                    )
                                    .on_click(cx.listener(
                                        |this, _, window, cx| {
                                            this.playlist_editor
                                                .name_input
                                                .update(cx, |input, cx| input.clear(cx));
                                            this.playlist_editor.private = false;
                                            this.open_playlist_dialog(
                                                PlaylistDialog::Create,
                                                window,
                                                cx,
                                            );
                                            cx.notify();
                                        },
                                    )),
                                )
                            },
                        )
                        .when(
                            view.editable_playlist.is_some() && !self.playlist_editor.busy,
                            |row| {
                                let id = view.editable_playlist.expect("checked playlist");
                                row.child(
                                    ui::icon_button_labeled(
                                        "playlist-edit",
                                        "edit",
                                        "编辑歌单",
                                        cx,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, window, cx| {
                                            this.open_playlist_dialog(
                                                PlaylistDialog::EditLoading(id),
                                                window,
                                                cx,
                                            );
                                            let _ = this
                                                .service
                                                .commands
                                                .send(Command::StartPlaylistEdit(id));
                                            cx.notify();
                                        },
                                    )),
                                )
                                .child(
                                    ui::icon_button_labeled(
                                        "playlist-delete",
                                        "delete",
                                        "删除歌单",
                                        cx,
                                    )
                                    .on_click(cx.listener(
                                        move |this, _, window, cx| {
                                            this.open_playlist_dialog(
                                                PlaylistDialog::Delete(id),
                                                window,
                                                cx,
                                            );
                                            cx.notify();
                                        },
                                    )),
                                )
                            },
                        )
                        .when(
                            view.is_detail
                                && view.collection_state != CollectionState::Hidden
                                && view.collection_state != CollectionState::Owned,
                            |row| {
                                let state = view.collection_state;
                                let artist = view.artist_page;
                                let label = match state {
                                    CollectionState::LoginRequired => "登录后可收藏",
                                    CollectionState::Loading => "正在查询…",
                                    CollectionState::Writing => "正在保存…",
                                    CollectionState::Unknown => "状态未知 · 重试",
                                    CollectionState::Ready(true) if artist => "取消关注",
                                    CollectionState::Ready(false) if artist => "关注",
                                    CollectionState::Ready(true) => "取消收藏",
                                    CollectionState::Ready(false) => "收藏",
                                    CollectionState::Hidden | CollectionState::Owned => "",
                                };
                                row.child(
                                    div()
                                        .id("collection-toggle")
                                        .px_3()
                                        .py_2()
                                        .rounded_md()
                                        .bg(rgb(0xe9edf5))
                                        .child(label)
                                        .when(
                                            matches!(
                                                state,
                                                CollectionState::Ready(_)
                                                    | CollectionState::Unknown
                                            ),
                                            |button| {
                                                button.cursor_pointer().on_click(cx.listener(
                                                    move |this, _, _, _| {
                                                        let command =
                                                            if state == CollectionState::Unknown {
                                                                Command::LoadCollectionState
                                                            } else {
                                                                Command::ToggleCollection
                                                            };
                                                        let _ = this.service.commands.send(command);
                                                    },
                                                ))
                                            },
                                        ),
                                )
                            },
                        )
                        .when(matches!(view.content, BrowseContent::Tracks(_)), |row| {
                            row.child(
                                ui::primary_icon_button(
                                    "playlist-play-all",
                                    "play",
                                    if view.loading {
                                        "正在加载…"
                                    } else if can_play_all {
                                        "播放全部"
                                    } else {
                                        "暂无可播放歌曲"
                                    },
                                    can_play_all,
                                    cx,
                                )
                                .when(can_play_all, |button| {
                                    button.cursor_pointer().on_click(cx.listener(
                                        |this, _, _, _| {
                                            let _ = this
                                                .service
                                                .commands
                                                .send(Command::PlayAllPlaylist);
                                        },
                                    ))
                                }),
                            )
                        }),
                )
                .when(
                    matches!(&view.content, BrowseContent::Tracks(_)) && !view.loading,
                    |body| {
                        let generation = view.generation;
                        let all = match &view.content {
                            BrowseContent::Tracks(tracks) => tracks
                                .iter()
                                .enumerate()
                                .filter_map(|(index, track)| {
                                    (track.id > 0).then_some((index, track.id))
                                })
                                .collect::<HashSet<_>>(),
                            _ => HashSet::new(),
                        };
                        let count = selected.len();
                        let add_selection = selected.clone();
                        let like_selection = selected.clone();
                        let remove_selection = selected.clone();
                        let editable = view.editable_playlist;
                        let confirm = self.batch_remove_confirm;
                        body.child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .pb_3()
                                .text_sm()
                                .child(format!("已选 {count} 首"))
                                .child(
                                    div()
                                        .id("batch-select-all")
                                        .cursor_pointer()
                                        .text_color(rgb(0x415de5))
                                        .child("全选已加载")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.batch_selection = all.clone();
                                            this.batch_generation =
                                                Some(BatchSource::Browse(generation));
                                            this.batch_remove_confirm = false;
                                            cx.notify();
                                        })),
                                )
                                .when(count > 0, |bar| {
                                    bar.child(
                                        div()
                                            .id("batch-clear")
                                            .cursor_pointer()
                                            .text_color(rgb(0x415de5))
                                            .child("清空选择")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.batch_selection.clear();
                                                this.batch_generation = None;
                                                this.batch_remove_confirm = false;
                                                cx.notify();
                                            })),
                                    )
                                })
                                .when(
                                    count > 0 && !self.playlist_editor.busy && !self.auth.busy,
                                    |bar| {
                                        bar.child(
                                            div()
                                                .id("batch-add")
                                                .cursor_pointer()
                                                .text_color(rgb(0x415de5))
                                                .child("批量加入歌单")
                                                .on_click(cx.listener(move |this, _, _, _| {
                                                    let _ = this.service.commands.send(
                                                        Command::OpenBatchPicker(
                                                            BatchSource::Browse(generation),
                                                            add_selection.clone(),
                                                        ),
                                                    );
                                                })),
                                        )
                                    },
                                )
                                .when(
                                    count > 0 && !self.playlist_editor.busy && !self.auth.busy,
                                    |bar| {
                                        bar.child(
                                            div()
                                                .id("batch-like")
                                                .cursor_pointer()
                                                .text_color(rgb(0x415de5))
                                                .child("批量喜欢")
                                                .on_click(cx.listener(move |this, _, _, _| {
                                                    let _ = this.service.commands.send(
                                                        Command::LikeSelected(
                                                            BatchSource::Browse(generation),
                                                            like_selection.clone(),
                                                        ),
                                                    );
                                                })),
                                        )
                                    },
                                )
                                .when(
                                    count > 0
                                        && editable.is_some()
                                        && !self.playlist_editor.busy
                                        && !self.auth.busy,
                                    |bar| {
                                        let playlist_id =
                                            editable.expect("editable playlist checked");
                                        bar.child(
                                            div()
                                                .id("batch-remove")
                                                .cursor_pointer()
                                                .text_color(rgb(0xc23b46))
                                                .child(if confirm {
                                                    "确认移除所选"
                                                } else {
                                                    "批量移除"
                                                })
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    if this.batch_remove_confirm {
                                                        let _ = this.service.commands.send(
                                                            Command::RemoveSelectedTracks(
                                                                playlist_id,
                                                                generation,
                                                                remove_selection.clone(),
                                                            ),
                                                        );
                                                        this.batch_remove_confirm = false;
                                                    } else {
                                                        this.batch_remove_confirm = true;
                                                    }
                                                    cx.notify();
                                                })),
                                        )
                                    },
                                ),
                        )
                    },
                )
                .when(view.artist_page, |body| {
                    body.child(
                        div()
                            .flex()
                            .gap_4()
                            .pb_3()
                            .child(
                                div()
                                    .id("artist-tracks")
                                    .cursor_pointer()
                                    .text_color(if !view.artist_albums_active {
                                        rgb(0x415de5)
                                    } else {
                                        rgb(0x687083)
                                    })
                                    .child("歌曲")
                                    .on_click(cx.listener(|this, _, _, _| {
                                        let _ = this.service.commands.send(Command::ArtistTracks);
                                    })),
                            )
                            .child(
                                div()
                                    .id("artist-albums")
                                    .cursor_pointer()
                                    .text_color(if view.artist_albums_active {
                                        rgb(0x415de5)
                                    } else {
                                        rgb(0x687083)
                                    })
                                    .child("专辑")
                                    .on_click(cx.listener(|this, _, _, _| {
                                        let _ = this.service.commands.send(Command::ArtistAlbums);
                                    })),
                            ),
                    )
                })
                .child(
                    div()
                        .id("browse-results")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .when(!description.is_empty(), |body| {
                            body.child(
                                div()
                                    .id("toggle-description")
                                    .text_size(px(12.))
                                    .text_color(rgb(0x788196))
                                    .px_3()
                                    .py_2()
                                    .cursor_pointer()
                                    .child(if self.navigation.browse_description_open {
                                        "收起介绍 ⌃"
                                    } else {
                                        "展开介绍 ⌄"
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.navigation.browse_description_open =
                                            !this.navigation.browse_description_open;
                                        cx.notify();
                                    })),
                            )
                            .when(
                                self.navigation.browse_description_open,
                                |body| {
                                    body.child(
                                        div()
                                            .p_3()
                                            .text_sm()
                                            .text_color(rgb(0x687083))
                                            .child(description),
                                    )
                                },
                            )
                        })
                        .when(view.detail_loading, |body| {
                            body.child(
                                div()
                                    .p_3()
                                    .text_sm()
                                    .text_color(rgb(0x788196))
                                    .child("正在加载详情…"),
                            )
                        })
                        .when_some(view.detail_error.as_ref(), |body, error| {
                            body.child(
                                div()
                                    .id("detail-retry")
                                    .p_3()
                                    .text_sm()
                                    .cursor_pointer()
                                    .text_color(rgb(0x415de5))
                                    .child(format!("{error} · 重试"))
                                    .on_click(cx.listener(|this, _, _, _| {
                                        let _ = this.service.commands.send(Command::LoadDetails);
                                    })),
                            )
                        })
                        .when(view.square.is_some() && !view.is_detail, |body| {
                            body.child(self.render_playlist_categories(view, cx))
                        })
                        .when(!message.is_empty(), |body| {
                            body.child(div().p_3().text_color(rgb(0x8a91a2)).child(message))
                        })
                        .child(
                            div()
                                .when(
                                    !matches!(
                                        view.content,
                                        BrowseContent::Tracks(_) | BrowseContent::Charts(_)
                                    ),
                                    |body| body.flex().flex_wrap(),
                                )
                                .children(browse_rows),
                        )
                        .when(view.has_more || view.loading, |body| {
                            body.child(
                                ui::icon_button_labeled(
                                    "browse-more",
                                    "refresh",
                                    if view.loading {
                                        "正在加载…"
                                    } else if retry {
                                        "重试加载"
                                    } else {
                                        "加载更多"
                                    },
                                    cx,
                                )
                                .when(view.loading, |button| button.opacity(0.45).cursor_default())
                                .when(!view.loading, |button| {
                                    button.cursor_pointer().on_click(cx.listener(
                                        |this, _, _, _| {
                                            let _ = this.service.commands.send(Command::BrowseNext);
                                        },
                                    ))
                                }),
                            )
                        }),
                )
                .into_any_element()
        } else {
            div()
                .flex_1()
                .p_4()
                .text_color(rgb(0x8a91a2))
                .child(if self.navigation.page == Page::PlaylistSquare {
                    "正在加载歌单分类…"
                } else {
                    "输入关键词开始搜索"
                })
                .into_any_element()
        }
    }
}
