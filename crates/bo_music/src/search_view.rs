use super::*;

impl MusicApp {
    pub(super) fn search(&mut self, _: &Search, _: &mut Window, cx: &mut Context<Self>) {
        if !self.show_account {
            self.run_search(cx);
        }
    }

    pub(super) fn run_search(&mut self, cx: &mut Context<Self>) {
        if self.auth.busy
            || self.playlist_editor.dialog != PlaylistDialog::Closed
            || self.search.input.read(cx).is_composing()
        {
            return;
        }
        self.appearance_page.open = false;
        self.shortcuts.open = false;
        self.navigation.browse = None;
        self.navigation.overview_detail_open = false;
        let query = self.search.input.read(cx).text().trim().to_owned();
        self.search.dismiss_assist();
        self.search.invalidate_suggestions();
        let _ = self.service.commands.send(Command::SearchSuggestions(
            self.search.suggestion_generation,
            String::new(),
        ));
        let command = match self.navigation.page {
            Page::Tracks => Command::Search(query),
            Page::AlbumSearch => Command::SearchAlbums(query),
            Page::ArtistSearch => Command::SearchArtists(query),
            Page::PlaylistSearch => Command::SearchPlaylists(query),
            _ => {
                self.navigation.page = Page::Overview;
                self.search.overview_generation = self.search.overview_generation.wrapping_add(1);
                self.search.overview_query.clone_from(&query);
                self.search.overview_result = None;
                Command::SearchOverview(self.search.overview_generation, query)
            }
        };
        let _ = self.service.commands.send(command);
        cx.notify();
    }

    pub(super) fn search_input_changed(&mut self, cx: &mut Context<Self>) {
        let input = self.search.input.read(cx);
        let query = (!input.is_composing()).then(|| input.text().trim().to_owned());
        if !self.search.update_suggestion_query(query.clone()) {
            return;
        }
        let _ = self.service.commands.send(Command::SearchSuggestions(
            self.search.suggestion_generation,
            query.unwrap_or_default(),
        ));
        cx.notify();
    }

    pub(super) fn load_hot_words(&mut self, cx: &mut Context<Self>) {
        if !self.search.hot_words_loading {
            self.search.hot_words_loading = self
                .service
                .commands
                .send(Command::LoadSearchHotWords)
                .is_ok();
            cx.notify();
        }
    }

    pub(super) fn dismiss_search_assist(&mut self, cx: &mut Context<Self>) {
        self.search.dismiss_assist();
        cx.notify();
    }

    pub(super) fn render_search_assist(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        let history = self.search.show_history;
        let words = if history {
            self.search
                .history
                .as_ref()
                .and_then(|result| result.as_ref().ok())
                .map(Vec::as_slice)
                .unwrap_or_default()
        } else if self.search.show_hot_words {
            self.search.hot_words.as_deref().unwrap_or_default()
        } else {
            &self.search.suggestions
        };
        let heading = if history {
            "搜索历史"
        } else if self.search.show_hot_words {
            "热门搜索"
        } else {
            "搜索建议"
        };
        let loading = if history {
            self.search.history.is_none()
        } else {
            self.search.show_hot_words && self.search.hot_words_loading
        };
        let error = if history {
            self.search
                .history
                .as_ref()
                .and_then(|result| result.as_ref().err())
                .cloned()
        } else if self.search.show_hot_words
            && !self.search.hot_words_loading
            && self.search.hot_words.is_none()
        {
            Some("热门搜索加载失败".into())
        } else {
            None
        };
        div()
            .id("search-assist")
            .absolute()
            .top(px(46.))
            .left_0()
            .w_full()
            .occlude()
            .p_2()
            .rounded_lg()
            .bg(appearance::palette(cx).elevated)
            .border_1()
            .border_color(rgb(0xd8dce6))
            .shadow_md()
            .on_mouse_down_out(cx.listener(|this, _, _, cx| this.dismiss_search_assist(cx)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .px_2()
                    .pb_1()
                    .child(
                        div()
                            .text_size(px(12.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(0x687083))
                            .child(heading),
                    )
                    .when(history && !words.is_empty(), |header| {
                        header.child(
                            ui::icon_button_labeled(
                                "search-history-clear",
                                "delete",
                                if self.search.confirm_clear_history {
                                    "确认清空搜索记录"
                                } else {
                                    "清空搜索记录"
                                },
                                cx,
                            )
                            .when(self.search.confirm_clear_history, |button| {
                                button.bg(rgb(0xe9edff))
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.search.confirm_clear_history {
                                    this.request_search_history(Command::ClearSearchHistory, cx);
                                } else {
                                    this.search.confirm_clear_history = true;
                                    cx.notify();
                                }
                            })),
                        )
                    }),
            )
            .when(self.search.confirm_clear_history && history, |panel| {
                panel.child(
                    div()
                        .px_3()
                        .py_2()
                        .mb_1()
                        .rounded_md()
                        .bg(rgb(0xe9edff))
                        .text_size(px(12.))
                        .text_color(rgb(0x415de5))
                        .child("清空全部搜索记录？再次点击垃圾桶确认。"),
                )
            })
            .when(loading, |panel| {
                panel.child(
                    div()
                        .p_3()
                        .text_sm()
                        .text_color(rgb(0x788196))
                        .child("正在加载…"),
                )
            })
            .when_some(error, |panel, message| {
                panel.child(
                    div()
                        .p_2()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_sm()
                                .text_color(rgb(0xc23b46))
                                .child(message),
                        )
                        .child(
                            ui::icon_button_labeled(
                                "search-assist-retry",
                                "refresh",
                                "重新加载",
                                cx,
                            )
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.search.show_history {
                                    this.request_search_history(Command::LoadSearchHistory, cx);
                                } else {
                                    this.load_hot_words(cx);
                                }
                            })),
                        ),
                )
            })
            .when(
                !loading
                    && words.is_empty()
                    && (if history {
                        self.search.history.as_ref().is_some_and(Result::is_ok)
                    } else {
                        !self.search.show_hot_words || self.search.hot_words.is_some()
                    }),
                |panel| {
                    panel.child(
                        div()
                            .p_3()
                            .text_sm()
                            .text_color(rgb(0x788196))
                            .child(if history {
                                "暂无搜索记录"
                            } else {
                                "暂无搜索内容"
                            }),
                    )
                },
            )
            .child(
                div()
                    .id("search-assist-words")
                    .max_h(px(280.))
                    .overflow_y_scroll()
                    .children(words.iter().enumerate().map(|(index, word)| {
                        let word = word.clone();
                        div()
                            .id(("search-assist-word", index))
                            .h(px(40.))
                            .px_3()
                            .flex()
                            .items_center()
                            .gap_3()
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|row| row.bg(rgb(0xe9edf5)))
                            .child(ui::icon(
                                if history { "clock" } else { "search" },
                                rgb(0x8a91a2),
                            ))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(13.))
                                    .child(word.clone()),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.search.suggestion_query = Some(word.trim().to_owned());
                                this.search
                                    .input
                                    .update(cx, |input, cx| input.set_text(word.clone(), cx));
                                this.run_search(cx);
                            }))
                    })),
            )
            .into_any_element()
    }

    pub(super) fn request_search_history(&mut self, command: Command, cx: &mut Context<Self>) {
        self.search.confirm_clear_history = false;
        self.search.history = None;
        if self.service.commands.send(command).is_err() {
            self.search.history = Some(Err("搜索历史服务不可用".into()));
        }
        cx.notify();
    }

    pub(super) fn render_search_overview(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        let body = div()
            .id(("search-overview-results", self.search.overview_generation))
            .flex_1()
            .min_h_0()
            .overflow_y_scroll();
        let sections = match &self.search.overview_result {
            Some(Ok(sections)) if !sections.is_empty() => sections,
            result => {
                if matches!(result, Some(Err(()))) {
                    return body
                        .child(
                            div().p_4().child("综合搜索失败").child(
                                div()
                                    .id("overview-retry")
                                    .cursor_pointer()
                                    .text_color(rgb(0x415de5))
                                    .child("重试")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.search_overview_category(Page::Overview, cx);
                                    })),
                            ),
                        )
                        .into_any_element();
                }
                let message = match result {
                    Some(Ok(_)) => "暂无搜索结果",
                    _ if self.search.overview_query.is_empty() => "输入关键词开始搜索",
                    _ => "正在搜索…",
                };
                return body
                    .child(div().p_4().text_color(rgb(0x687083)).child(message))
                    .into_any_element();
            }
        };
        let generation = self.search.overview_generation;
        body.children(
            sections
                .iter()
                .enumerate()
                .map(|(section_index, section)| {
                    let (page, heading, entries): (_, _, Vec<_>) = match section {
                        SearchSection::Tracks(items) => (
                            Page::Tracks,
                            "歌曲",
                            items
                                .iter()
                                .map(|item| {
                                    (
                                        item.id,
                                        item.title.as_str(),
                                        item.artist_text.as_str(),
                                        item.cover_url.as_deref(),
                                    )
                                })
                                .collect(),
                        ),
                        SearchSection::Playlists(items) => (
                            Page::PlaylistSearch,
                            "歌单",
                            items
                                .iter()
                                .map(|item| {
                                    (
                                        item.id,
                                        item.name.as_str(),
                                        item.creator_name.as_str(),
                                        item.cover_url.as_deref(),
                                    )
                                })
                                .collect(),
                        ),
                        SearchSection::Albums(items) => (
                            Page::AlbumSearch,
                            "专辑",
                            items
                                .iter()
                                .map(|item| {
                                    (
                                        item.id,
                                        item.name.as_str(),
                                        item.artist_text.as_str(),
                                        item.cover_url.as_deref(),
                                    )
                                })
                                .collect(),
                        ),
                        SearchSection::Artists(items) => (
                            Page::ArtistSearch,
                            "歌手",
                            items
                                .iter()
                                .map(|item| {
                                    (item.id, item.name.as_str(), "", item.cover_url.as_deref())
                                })
                                .collect(),
                        ),
                    };
                    div()
                        .id(("overview-section", section_index))
                        .mb_3()
                        .p_3()
                        .rounded_lg()
                        .bg(rgb(0xf5f7fb))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .p_2()
                                .child(
                                    div()
                                        .text_lg()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .child(heading),
                                )
                                .child(
                                    div()
                                        .id("overview-more")
                                        .cursor_pointer()
                                        .text_color(rgb(0x415de5))
                                        .child("查看更多")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.search_overview_category(page, cx)
                                        })),
                                ),
                        )
                        .children(
                            entries
                                .into_iter()
                                .enumerate()
                                .filter(|(_, (id, _, _, _))| *id > 0)
                                .map(|(index, (_, title, subtitle, cover))| {
                                    div()
                                        .id(("overview-row", index))
                                        .flex()
                                        .items_center()
                                        .gap_3()
                                        .p_2()
                                        .rounded_md()
                                        .cursor_pointer()
                                        .hover(|style| style.bg(rgb(0xe9edf5)))
                                        .child(self.cover(cover, 44.0, cx))
                                        .child(div().min_w_0().child(title.to_owned()).when(
                                            !subtitle.is_empty(),
                                            |column| {
                                                column.child(
                                                    div()
                                                        .text_sm()
                                                        .text_color(rgb(0x687083))
                                                        .child(subtitle.to_owned()),
                                                )
                                            },
                                        ))
                                        .on_click(cx.listener(move |this, _, _, _| {
                                            if this.navigation.page == Page::Overview
                                                && !this.navigation.overview_detail_open
                                                && this.search.overview_generation == generation
                                            {
                                                let _ = this.service.commands.send(
                                                    Command::SelectSearchOverview(
                                                        generation,
                                                        section_index,
                                                        index,
                                                    ),
                                                );
                                            }
                                        }))
                                })
                                .collect::<Vec<_>>(),
                        )
                })
                .collect::<Vec<_>>(),
        )
        .into_any_element()
    }

    pub(super) fn search_overview_category(&mut self, page: Page, cx: &mut Context<Self>) {
        let query = self.search.overview_query.clone();
        self.search.suggestion_query = Some(query.clone());
        self.search
            .input
            .update(cx, |input, cx| input.set_text(query, cx));
        self.navigation.page = page;
        self.run_search(cx);
    }
}
