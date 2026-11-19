use super::*;

impl MusicApp {
    pub(super) fn render_home(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        let Some(view) = &self.home else {
            return div().p_4().child("正在加载发现页…").into_any_element();
        };
        let generation = view.generation;
        let queue_stamp = self
            .player
            .as_ref()
            .map(|player| (player.queue_account_revision, player.queue_revision));
        if let Some(ai) = &view.ai {
            let tracks = ai
                .playlist
                .as_ref()
                .map(|playlist| playlist.tracks.as_slice())
                .unwrap_or(&[]);
            let rows = tracks
                .iter()
                .enumerate()
                .map(|(index, track)| {
                    let id = track.id;
                    div()
                        .id(("home-ai-track", index))
                        .flex()
                        .items_center()
                        .gap_3()
                        .p_3()
                        .rounded_md()
                        .hover(|style| style.bg(rgb(0xe9edf5)))
                        .child(self.cover(track.cover_url.as_deref(), 42.0, cx))
                        .child(format!("{} · {}", track.title, track.artist_text))
                        .when(id > 0, |row| {
                            row.cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, _| {
                                    let _ = this
                                        .service
                                        .commands
                                        .send(Command::HomePlay(None, index, id, generation));
                                }))
                        })
                        .into_any_element()
                })
                .collect::<Vec<_>>();
            return div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .pb_3()
                        .child(
                            div()
                                .id("home-ai-back")
                                .cursor_pointer()
                                .text_color(rgb(0x415de5))
                                .child("← 返回发现")
                                .on_click(cx.listener(move |this, _, _, _| {
                                    let _ = this
                                        .service
                                        .commands
                                        .send(Command::HomeCloseAi(generation));
                                })),
                        )
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::BOLD)
                                .child(ai.title.clone()),
                        )
                        .when(!tracks.is_empty(), |row| {
                            row.child(
                                div()
                                    .id("home-ai-play-all")
                                    .cursor_pointer()
                                    .text_color(rgb(0x415de5))
                                    .child("播放全部")
                                    .on_click(cx.listener(move |this, _, _, _| {
                                        let _ = this
                                            .service
                                            .commands
                                            .send(Command::HomePlayAll(generation));
                                    })),
                            )
                        }),
                )
                .child(
                    div()
                        .id("home-ai-results")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .when(ai.loading, |body| body.child("正在加载歌单…"))
                        .when(ai.error, |body| body.child("加载失败，请返回后重试"))
                        .when(!ai.loading && !ai.error && tracks.is_empty(), |body| {
                            body.child("暂无歌曲")
                        })
                        .children(rows),
                )
                .into_any_element();
        }
        let items = view
            .items
            .iter()
            .enumerate()
            .map(|(item_index, item)| {
                let title = item
                    .feed
                    .as_ref()
                    .map(|feed| feed.title.as_str())
                    .filter(|title| !title.is_empty())
                    .unwrap_or(&item.module.name)
                    .to_owned();
                let contents = match item.feed.as_ref().map(|feed| &feed.content) {
                    Some(HomeContent::Tracks(tracks)) => tracks
                        .iter()
                        .enumerate()
                        .map(|(index, track)| {
                            let id = track.id;
                            div()
                                .id(("home-track", item_index * 10000 + index))
                                .flex()
                                .items_center()
                                .gap_3()
                                .p_2()
                                .rounded_md()
                                .child(self.cover(track.cover_url.as_deref(), 42.0, cx))
                                .child(format!("{} · {}", track.title, track.artist_text))
                                .when(id > 0, |row| {
                                    row.cursor_pointer()
                                        .hover(|style| style.bg(rgb(0xe9edf5)))
                                        .on_click(cx.listener(move |this, _, _, _| {
                                            let _ = this.service.commands.send(Command::HomePlay(
                                                Some(item_index),
                                                index,
                                                id,
                                                generation,
                                            ));
                                        }))
                                })
                                .into_any_element()
                        })
                        .collect::<Vec<_>>(),
                    Some(HomeContent::Playlists(playlists)) => playlists
                        .iter()
                        .enumerate()
                        .map(|(index, playlist)| {
                            let id = playlist.id;
                            div()
                                .id(("home-playlist", item_index * 10000 + index))
                                .flex()
                                .items_center()
                                .gap_3()
                                .p_2()
                                .rounded_md()
                                .child(self.cover(playlist.cover_url.as_deref(), 42.0, cx))
                                .child(playlist.name.clone())
                                .when(id > 0, |row| {
                                    row.cursor_pointer()
                                        .hover(|style| style.bg(rgb(0xe9edf5)))
                                        .on_click(cx.listener(move |this, _, _, _| {
                                            let _ = this.service.commands.send(
                                                Command::HomeOpenPlaylist(
                                                    item_index, index, id, generation,
                                                ),
                                            );
                                        }))
                                })
                                .into_any_element()
                        })
                        .collect::<Vec<_>>(),
                    Some(HomeContent::AiPreviews { groups, .. }) => groups
                        .iter()
                        .enumerate()
                        .map(|(index, group)| {
                            let preview = group
                                .tracks
                                .iter()
                                .take(3)
                                .map(|track| track.title.as_str())
                                .collect::<Vec<_>>()
                                .join(" · ");
                            div()
                                .id(("home-ai-group", item_index * 10000 + index))
                                .flex()
                                .items_center()
                                .gap_3()
                                .p_2()
                                .rounded_md()
                                .cursor_pointer()
                                .hover(|style| style.bg(rgb(0xe9edf5)))
                                .child(
                                    self.cover(
                                        group
                                            .tracks
                                            .first()
                                            .and_then(|track| track.cover_url.as_deref()),
                                        42.0,
                                        cx,
                                    ),
                                )
                                .child(div().flex().flex_col().child(group.title.clone()).child(
                                    div().text_sm().text_color(rgb(0x788196)).child(preview),
                                ))
                                .on_click(cx.listener(move |this, _, _, _| {
                                    let _ = this
                                        .service
                                        .commands
                                        .send(Command::HomeOpenAi(item_index, index, generation));
                                }))
                                .into_any_element()
                        })
                        .collect::<Vec<_>>(),
                    None => Vec::new(),
                };
                div()
                    .mb_4()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(div().font_weight(gpui::FontWeight::BOLD).child(title))
                            .when(
                                (item.error || matches!(item.module.kind, 3 | 10)) && !view.loading,
                                |row| {
                                    row.child(
                                        div()
                                            .id(("home-refresh-module", item_index))
                                            .cursor_pointer()
                                            .text_sm()
                                            .text_color(rgb(0x415de5))
                                            .child(if item.error { "重试" } else { "换一批" })
                                            .on_click(cx.listener(move |this, _, _, _| {
                                                let _ = this.service.commands.send(
                                                    Command::HomeRefresh(item_index, generation),
                                                );
                                            })),
                                    )
                                },
                            )
                            .when(
                                matches!(item.module.kind, 3 | 10)
                                    && !item.error
                                    && !view.loading
                                    && queue_stamp.is_some(),
                                |row| {
                                    let (account, queue) = queue_stamp.expect("queue checked");
                                    row.child(
                                        div()
                                            .id(("home-queue-module", item_index))
                                            .cursor_pointer()
                                            .text_sm()
                                            .text_color(rgb(0x415de5))
                                            .child("全部加入队列")
                                            .on_click(cx.listener(move |this, _, _, _| {
                                                let _ = this.service.commands.send(
                                                    Command::HomeQueueModule(
                                                        item_index, generation, account, queue,
                                                    ),
                                                );
                                            })),
                                    )
                                },
                            ),
                    )
                    .when(item.error, |body| {
                        body.child(
                            div()
                                .text_sm()
                                .text_color(rgb(0xc23b46))
                                .child("模块加载失败"),
                        )
                    })
                    .children(contents)
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        div()
            .id("home-results")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .pb_3()
                    .child(div().font_weight(gpui::FontWeight::BOLD).child("发现音乐"))
                    .child(
                        div()
                            .id("home-reload")
                            .cursor_pointer()
                            .text_sm()
                            .text_color(rgb(0x415de5))
                            .child("重新加载")
                            .on_click(cx.listener(|this, _, _, _| {
                                let _ = this.service.commands.send(Command::LoadHome);
                            })),
                    ),
            )
            .children(items)
            .when(view.loading, |body| body.child("正在加载…"))
            .when(view.layout_error, |body| {
                body.child("发现页加载失败，请重新加载")
            })
            .when(
                !view.loading && !view.layout_error && view.items.is_empty() && !view.has_more,
                |body| body.child("暂无发现内容"),
            )
            .when(view.has_more && !view.loading, |body| {
                body.child(
                    div()
                        .id("home-next")
                        .p_3()
                        .cursor_pointer()
                        .text_color(rgb(0x415de5))
                        .child("加载更多")
                        .on_click(cx.listener(|this, _, _, _| {
                            let _ = this.service.commands.send(Command::HomeNext);
                        })),
                )
            })
            .into_any_element()
    }

    pub(super) fn render_library(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        let Some(view) = &self.library else {
            return div().p_4().child("正在加载乐库…").into_any_element();
        };
        let generation = view.generation;
        if let Some(group) = view.selected_group.and_then(|index| view.groups.get(index)) {
            let children = group
                .children
                .iter()
                .enumerate()
                .map(|(index, child)| {
                    div()
                        .id(("library-child", index))
                        .cursor_pointer()
                        .p_2()
                        .text_color(if view.selected_child == Some(index) {
                            rgb(0x415de5)
                        } else {
                            rgb(0x687083)
                        })
                        .child(child.name.clone())
                        .on_click(cx.listener(move |this, _, _, _| {
                            let _ = this
                                .service
                                .commands
                                .send(Command::LibrarySelectChild(index, generation));
                        }))
                        .into_any_element()
                })
                .collect::<Vec<_>>();
            let albums =
                view.albums
                    .iter()
                    .enumerate()
                    .map(|(index, album)| {
                        let id = album.id;
                        div()
                            .id(("library-album", index))
                            .flex()
                            .items_center()
                            .gap_3()
                            .p_3()
                            .rounded_md()
                            .child(self.cover(album.cover_url.as_deref(), 48.0, cx))
                            .child(format!("{} · {}", album.name, album.artist_text))
                            .when(id > 0, |row| {
                                row.cursor_pointer()
                                    .hover(|style| style.bg(rgb(0xe9edf5)))
                                    .on_click(cx.listener(move |this, _, _, _| {
                                        let _ = this.service.commands.send(
                                            Command::LibraryOpenAlbum(Some(index), id, generation),
                                        );
                                    }))
                            })
                            .into_any_element()
                    })
                    .collect::<Vec<_>>();
            let sample = view
                .selected_child
                .and_then(|index| group.children.get(index))
                .and_then(|child| child.sample_album.as_ref())
                .filter(|album| album.id > 0)
                .cloned();
            return div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .pb_3()
                        .child(
                            div()
                                .id("library-back")
                                .cursor_pointer()
                                .text_color(rgb(0x415de5))
                                .child("← 返回乐库")
                                .on_click(cx.listener(move |this, _, _, _| {
                                    let _ = this
                                        .service
                                        .commands
                                        .send(Command::LibraryBack(generation));
                                })),
                        )
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::BOLD)
                                .child(group.name.clone()),
                        ),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(rgb(0x788196))
                        .child(group.description.clone()),
                )
                .child(div().flex().flex_wrap().gap_2().children(children))
                .when(view.selected_child.is_some(), |body| {
                    body.child(
                        div().flex().gap_3().pb_2().children(
                            [
                                (LibrarySort::Curated, "精品"),
                                (LibrarySort::Newest, "最新"),
                            ]
                            .into_iter()
                            .enumerate()
                            .map(|(index, (sort, label))| {
                                div()
                                    .id(("library-sort", index))
                                    .cursor_pointer()
                                    .text_color(if view.sort == sort {
                                        rgb(0x415de5)
                                    } else {
                                        rgb(0x687083)
                                    })
                                    .child(label)
                                    .on_click(cx.listener(move |this, _, _, _| {
                                        let _ = this
                                            .service
                                            .commands
                                            .send(Command::LibrarySort(sort, generation));
                                    }))
                            })
                            .collect::<Vec<_>>(),
                        ),
                    )
                })
                .child(
                    div()
                        .id("library-albums")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .when(sample.is_some(), |body| {
                            let album = sample.expect("sample checked");
                            let id = album.id;
                            body.child(
                                div()
                                    .id("library-sample")
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .p_3()
                                    .cursor_pointer()
                                    .text_color(rgb(0x415de5))
                                    .child(self.cover(album.cover_url.as_deref(), 48.0, cx))
                                    .child(format!(
                                        "推荐专辑：{} · {}",
                                        album.name, album.artist_text
                                    ))
                                    .on_click(cx.listener(move |this, _, _, _| {
                                        let _ = this
                                            .service
                                            .commands
                                            .send(Command::LibraryOpenAlbum(None, id, generation));
                                    })),
                            )
                        })
                        .children(albums)
                        .when(view.loading, |body| {
                            body.child(div().p_3().child("正在加载专辑…"))
                        })
                        .when(view.error.is_some(), |body| {
                            body.child(
                                div()
                                    .p_3()
                                    .text_color(rgb(0xc23b46))
                                    .child(view.error.clone().unwrap_or_default()),
                            )
                        })
                        .when(
                            !view.loading && view.error.is_none() && view.albums.is_empty(),
                            |body| body.child(div().p_3().child("暂无专辑")),
                        )
                        .when(!view.loading && view.has_more, |body| {
                            body.child(
                                div()
                                    .id("library-next")
                                    .p_3()
                                    .cursor_pointer()
                                    .text_color(rgb(0x415de5))
                                    .child(if view.error.is_some() {
                                        "重试"
                                    } else {
                                        "加载更多"
                                    })
                                    .on_click(cx.listener(move |this, _, _, _| {
                                        let _ = this
                                            .service
                                            .commands
                                            .send(Command::LibraryNext(generation));
                                    })),
                            )
                        }),
                )
                .into_any_element();
        }
        let groups = view
            .groups
            .iter()
            .enumerate()
            .map(|(index, group)| {
                div()
                    .id(("library-group", index))
                    .flex()
                    .items_center()
                    .gap_3()
                    .p_3()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(0xe9edf5)))
                    .child(self.cover(group.cover_url.as_deref(), 48.0, cx))
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::BOLD)
                            .child(group.name.clone()),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x788196))
                            .child(group.title.clone()),
                    )
                    .on_click(cx.listener(move |this, _, _, _| {
                        let _ = this
                            .service
                            .commands
                            .send(Command::LibrarySelectGroup(index, generation));
                    }))
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        div()
            .id("library-groups")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .gap_3()
                    .pb_3()
                    .child(div().font_weight(gpui::FontWeight::BOLD).child("乐库"))
                    .child(
                        div()
                            .id("library-reload")
                            .cursor_pointer()
                            .text_color(rgb(0x415de5))
                            .child("重新加载")
                            .on_click(cx.listener(|this, _, _, _| {
                                let _ = this.service.commands.send(Command::LoadLibrary);
                            })),
                    ),
            )
            .when(view.loading, |body| body.child("正在加载乐库…"))
            .when(view.error.is_some(), |body| {
                body.child(view.error.clone().unwrap_or_default())
            })
            .when(
                !view.loading && view.error.is_none() && view.groups.is_empty(),
                |body| body.child("暂无分类"),
            )
            .children(groups)
            .into_any_element()
    }
}
