use super::*;

impl MusicApp {
    pub(super) fn render_home(&self, compact: bool, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        let Some(view) = &self.home else {
            return ui::empty_state("正在加载发现页", "好音乐即将呈现", cx).into_any_element();
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
                            ui::icon_button_labeled("home-ai-back", "back", "返回发现", cx)
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
                                ui::primary_icon_button(
                                    "home-ai-play-all",
                                    "play",
                                    "播放全部",
                                    true,
                                    cx,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, _| {
                                        let _ = this
                                            .service
                                            .commands
                                            .send(Command::HomePlayAll(generation));
                                    },
                                )),
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
                                .w_full()
                                .min_w_0()
                                .h(px(68.))
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .gap_3()
                                .px_3()
                                .py_2()
                                .rounded_lg()
                                .child(self.cover(track.cover_url.as_deref(), 48.0, cx))
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
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .child(track.title.clone()),
                                        )
                                        .child(
                                            div()
                                                .truncate()
                                                .text_size(px(12.))
                                                .text_color(rgb(0x788196))
                                                .child(track.artist_text.clone()),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .text_size(px(11.))
                                        .text_color(rgb(0x788196))
                                        .child(clock(track.duration_seconds.max(0) as u64)),
                                )
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
                                .w(px(156.))
                                .flex()
                                .flex_col()
                                .gap_2()
                                .p_2()
                                .rounded_lg()
                                .child(self.cover(playlist.cover_url.as_deref(), 140.0, cx))
                                .child(
                                    div()
                                        .text_sm()
                                        .truncate()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .child(playlist.name.clone()),
                                )
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
                                .w(px(156.))
                                .flex()
                                .flex_col()
                                .gap_2()
                                .p_2()
                                .rounded_lg()
                                .cursor_pointer()
                                .hover(|style| style.bg(rgb(0xe9edf5)))
                                .child(
                                    self.cover(
                                        group
                                            .tracks
                                            .first()
                                            .and_then(|track| track.cover_url.as_deref()),
                                        140.0,
                                        cx,
                                    ),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap_1()
                                        .child(
                                            div()
                                                .truncate()
                                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                                .child(group.title.clone()),
                                        )
                                        .child(
                                            div()
                                                .truncate()
                                                .text_sm()
                                                .text_color(rgb(0x788196))
                                                .child(preview),
                                        ),
                                )
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
                    .mb_6()
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_3()
                            .pb_2()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child(title),
                            )
                            .when(
                                (item.error || matches!(item.module.kind, 3 | 10)) && !view.loading,
                                |row| {
                                    row.child(
                                        ui::icon_button_labeled(
                                            ("home-refresh-module", item_index),
                                            "refresh",
                                            if item.error {
                                                "重新加载"
                                            } else {
                                                "换一批"
                                            },
                                            cx,
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, _, _| {
                                                let _ = this.service.commands.send(
                                                    Command::HomeRefresh(item_index, generation),
                                                );
                                            }),
                                        ),
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
                                        ui::icon_button_labeled(
                                            ("home-queue-module", item_index),
                                            "list",
                                            "全部加入队列",
                                            cx,
                                        )
                                        .on_click(
                                            cx.listener(move |this, _, _, _| {
                                                let _ = this.service.commands.send(
                                                    Command::HomeQueueModule(
                                                        item_index, generation, account, queue,
                                                    ),
                                                );
                                            }),
                                        ),
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
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .when(
                                matches!(
                                    item.feed.as_ref().map(|feed| &feed.content),
                                    Some(HomeContent::Tracks(_))
                                ),
                                |body| body.grid().grid_cols(if compact { 1 } else { 2 }).gap_3(),
                            )
                            .when(
                                matches!(
                                    item.feed.as_ref().map(|feed| &feed.content),
                                    Some(
                                        HomeContent::Playlists(_) | HomeContent::AiPreviews { .. }
                                    )
                                ),
                                |body| body.flex().flex_wrap().gap_3(),
                            )
                            .children(contents),
                    )
                    .into_any_element()
            })
            .collect::<Vec<_>>();
        div()
            .id("home-results")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(ui::eyebrow("YOUR DAILY INSPIRATION", cx))
            .child(ui::heading("发现好音乐", cx).mt_1().mb_5())
            .child(self.render_home_hero(compact, cx))
            .children(items)
            .when(view.loading, |body| body.child("正在加载…"))
            .when(view.layout_error, |body| {
                body.child("发现页加载失败，请重新加载")
            })
            .when(
                !view.loading && !view.layout_error && view.items.is_empty() && !view.has_more,
                |body| {
                    body.child(ui::empty_state(
                        "暂无发现内容",
                        "稍后刷新，探索更多音乐",
                        cx,
                    ))
                },
            )
            .when(view.has_more && !view.loading, |body| {
                body.child(
                    ui::icon_button_labeled("home-next", "refresh", "加载更多", cx).on_click(
                        cx.listener(|this, _, _, _| {
                            let _ = this.service.commands.send(Command::HomeNext);
                        }),
                    ),
                )
            })
            .into_any_element()
    }

    fn render_home_hero(&self, compact: bool, cx: &mut Context<Self>) -> gpui::AnyElement {
        let colors = appearance::palette(cx);
        let first = self.home.as_ref().and_then(|home| {
            home.items.iter().enumerate().find_map(|(module, item)| {
                match item.feed.as_ref().map(|feed| &feed.content) {
                    Some(HomeContent::Tracks(tracks)) => tracks
                        .iter()
                        .enumerate()
                        .find(|(_, track)| track.id > 0)
                        .map(|(index, track)| (module, index, track.id, home.generation)),
                    _ => None,
                }
            })
        });
        div()
            .h(px(if compact { 228. } else { 248. }))
            .flex_shrink_0()
            .rounded_lg()
            .overflow_hidden()
            .text_color(colors.hero_text)
            .mb_6()
            .flex()
            .bg(gpui::linear_gradient(
                115.,
                gpui::linear_color_stop(colors.hero_start, 0.),
                gpui::linear_color_stop(colors.hero_end, 1.),
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .p_6()
                    .flex()
                    .flex_col()
                    .justify_center()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(colors.hero_muted)
                            .child("为今天，留一点音乐"),
                    )
                    .child(
                        div()
                            .text_size(px(if compact { 23. } else { 28. }))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("让喜欢的旋律"),
                    )
                    .child(
                        div()
                            .text_size(px(if compact { 23. } else { 28. }))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("陪你度过每个日常。"),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(colors.hero_muted)
                            .child("每日精选 · 从熟悉的声音，遇见新的喜欢"),
                    )
                    .child(
                        div().mt_3().flex().items_center().gap_3().child(
                            ui::primary_icon_button(
                                "home-hero-play",
                                if first.is_some() { "play" } else { "refresh" },
                                if first.is_some() {
                                    "播放今日精选"
                                } else if self.home.as_ref().is_none_or(|view| view.loading) {
                                    "精选音乐加载中"
                                } else {
                                    "重新加载精选"
                                },
                                first.is_some()
                                    || self.home.as_ref().is_some_and(|view| !view.loading),
                                cx,
                            )
                            .when(
                                first.is_none()
                                    && self.home.as_ref().is_some_and(|view| !view.loading),
                                |button| {
                                    button.cursor_pointer().on_click(cx.listener(
                                        |this, _, _, _| {
                                            let _ = this.service.commands.send(Command::LoadHome);
                                        },
                                    ))
                                },
                            )
                            .when_some(
                                first,
                                |button, (module, index, id, generation)| {
                                    button.cursor_pointer().on_click(cx.listener(
                                        move |this, _, _, _| {
                                            let _ = this.service.commands.send(Command::HomePlay(
                                                Some(module),
                                                index,
                                                id,
                                                generation,
                                            ));
                                        },
                                    ))
                                },
                            ),
                        ),
                    ),
            )
            .child(
                div()
                    .w(gpui::relative(0.42))
                    .flex_shrink_0()
                    .child(ui::hero_illustration()),
            )
            .into_any_element()
    }

    pub(super) fn render_library(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        let Some(view) = &self.library else {
            return ui::empty_state("正在加载乐库", "正在整理音乐内容", cx).into_any_element();
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
                            .w(px(146.))
                            .flex()
                            .flex_col()
                            .gap_2()
                            .p_2()
                            .rounded_lg()
                            .child(self.cover(album.cover_url.as_deref(), 130.0, cx))
                            .child(
                                div()
                                    .truncate()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(album.name.clone()),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_sm()
                                    .text_color(rgb(0x788196))
                                    .child(album.artist_text.clone()),
                            )
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
                            ui::icon_button_labeled("library-back", "back", "返回音乐馆", cx)
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
                        .child(div().flex().flex_wrap().gap_2().children(albums))
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
                                ui::icon_button_labeled(
                                    "library-next",
                                    "refresh",
                                    if view.error.is_some() {
                                        "重试加载"
                                    } else {
                                        "加载更多"
                                    },
                                    cx,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, _| {
                                        let _ = this
                                            .service
                                            .commands
                                            .send(Command::LibraryNext(generation));
                                    },
                                )),
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
                    .w(px(170.))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_2()
                    .rounded_lg()
                    .cursor_pointer()
                    .hover(|style| style.bg(rgb(0xe9edf5)))
                    .child(self.cover(group.cover_url.as_deref(), 154.0, cx))
                    .child(
                        div()
                            .truncate()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child(group.name.clone()),
                    )
                    .child(
                        div()
                            .truncate()
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
                    .items_center()
                    .gap_3()
                    .pb_3()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(gpui::FontWeight::BOLD)
                            .child("探索乐库"),
                    )
                    .child(
                        ui::icon_button_labeled("library-reload", "refresh", "重新加载", cx)
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
            .child(
                div()
                    .pb_3()
                    .text_sm()
                    .text_color(rgb(0x788196))
                    .child("找到属于你的声音"),
            )
            .child(div().flex().flex_wrap().gap_3().children(groups))
            .into_any_element()
    }
}
