use super::*;

impl MusicApp {
    pub(super) fn search(&mut self, _: &Search, _: &mut Window, cx: &mut Context<Self>) {
        if !self.show_account {
            self.run_search(cx);
        }
    }

    pub(super) fn run_search(&mut self, cx: &mut Context<Self>) {
        if self.auth.busy || self.input.read(cx).is_composing() {
            return;
        }
        let query = self.input.read(cx).text().trim().to_owned();
        self.show_hot_words = false;
        self.show_search_history = false;
        self.confirm_clear_search_history = false;
        self.suggestion_generation = self.suggestion_generation.wrapping_add(1);
        self.search_suggestions.clear();
        let _ = self.service.commands.send(Command::SearchSuggestions(
            self.suggestion_generation,
            String::new(),
        ));
        let command = match self.page {
            Page::Tracks => Command::Search(query),
            Page::AlbumSearch => Command::SearchAlbums(query),
            Page::ArtistSearch => Command::SearchArtists(query),
            Page::PlaylistSearch => Command::SearchPlaylists(query),
            _ => {
                self.page = Page::Overview;
                self.overview_generation = self.overview_generation.wrapping_add(1);
                self.overview_query.clone_from(&query);
                self.overview_result = None;
                self.overview_detail_open = false;
                Command::SearchOverview(self.overview_generation, query)
            }
        };
        let _ = self.service.commands.send(command);
        cx.notify();
    }

    pub(super) fn search_input_changed(&mut self, cx: &mut Context<Self>) {
        let input = self.input.read(cx);
        let query = (!input.is_composing()).then(|| input.text().trim().to_owned());
        if self.suggestion_query == query {
            return;
        }
        self.suggestion_query = query.clone();
        self.suggestion_generation = self.suggestion_generation.wrapping_add(1);
        self.search_suggestions.clear();
        self.show_hot_words = false;
        self.show_search_history = false;
        self.confirm_clear_search_history = false;
        let _ = self.service.commands.send(Command::SearchSuggestions(
            self.suggestion_generation,
            query.unwrap_or_default(),
        ));
        cx.notify();
    }

    pub(super) fn load_hot_words(&mut self, cx: &mut Context<Self>) {
        if !self.hot_words_loading {
            self.hot_words_loading = self
                .service
                .commands
                .send(Command::LoadSearchHotWords)
                .is_ok();
            cx.notify();
        }
    }

    pub(super) fn render_search_assist(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        let words = if self.show_search_history {
            self.search_history
                .as_ref()
                .and_then(|result| result.as_ref().ok())
                .map(Vec::as_slice)
                .unwrap_or_default()
        } else if self.show_hot_words {
            self.hot_words.as_deref().unwrap_or_default()
        } else {
            &self.search_suggestions
        };
        let heading = if self.show_search_history {
            "搜索历史"
        } else if self.show_hot_words {
            "热门搜索"
        } else {
            "搜索建议"
        };
        div()
            .id("search-assist")
            .flex()
            .items_center()
            .gap_3()
            .px_6()
            .py_2()
            .bg(rgb(0xffffff))
            .overflow_x_scroll()
            .child(div().text_sm().text_color(rgb(0x687083)).child(heading))
            .when(self.show_search_history, |row| match &self.search_history {
                None => row.child("正在处理…"),
                Some(Ok(words)) if words.is_empty() => row.child("暂无搜索历史"),
                Some(Err(message)) => row.child(message.clone()).child(
                    div()
                        .id("search-history-retry")
                        .cursor_pointer()
                        .text_color(rgb(0x415de5))
                        .child("重新读取")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.request_search_history(Command::LoadSearchHistory, cx);
                        })),
                ),
                _ => row,
            })
            .when(
                self.show_search_history
                    && self.search_history.as_ref().is_some_and(|result| {
                        result.as_ref().map_or(true, |words| !words.is_empty())
                    }),
                |row| {
                    row.child(
                        div()
                            .id("search-history-clear")
                            .flex_shrink_0()
                            .cursor_pointer()
                            .text_color(rgb(0x415de5))
                            .child(if self.confirm_clear_search_history {
                                "确认清空"
                            } else {
                                "清空"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                if this.confirm_clear_search_history {
                                    this.request_search_history(Command::ClearSearchHistory, cx);
                                } else {
                                    this.confirm_clear_search_history = true;
                                    cx.notify();
                                }
                            })),
                    )
                },
            )
            .when(self.show_hot_words && self.hot_words_loading, |row| {
                row.child("加载中…")
            })
            .when(
                self.show_hot_words && self.hot_words.as_ref().is_some_and(Vec::is_empty),
                |row| row.child("暂无热门搜索"),
            )
            .when(
                self.show_hot_words && !self.hot_words_loading && self.hot_words.is_none(),
                |row| {
                    row.child(
                        div()
                            .id("hot-words-retry")
                            .cursor_pointer()
                            .text_color(rgb(0x415de5))
                            .child("加载失败，重试")
                            .on_click(cx.listener(|this, _, _, cx| this.load_hot_words(cx))),
                    )
                },
            )
            .children(
                words
                    .iter()
                    .enumerate()
                    .map(|(index, word)| {
                        let word = word.clone();
                        div()
                            .id(("search-assist-word", index))
                            .flex_shrink_0()
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(rgb(0xf1f3f9))
                            .hover(|style| style.bg(rgb(0xe9edf5)))
                            .child(word.clone())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.suggestion_query = Some(word.trim().to_owned());
                                this.input
                                    .update(cx, |input, cx| input.set_text(word.clone(), cx));
                                this.run_search(cx);
                            }))
                    })
                    .collect::<Vec<_>>(),
            )
            .into_any_element()
    }

    pub(super) fn request_search_history(&mut self, command: Command, cx: &mut Context<Self>) {
        self.confirm_clear_search_history = false;
        self.search_history = None;
        if self.service.commands.send(command).is_err() {
            self.search_history = Some(Err("搜索历史服务不可用".into()));
        }
        cx.notify();
    }

    pub(super) fn render_search_overview(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        let body = div()
            .id(("search-overview-results", self.overview_generation))
            .flex_1()
            .min_h_0()
            .overflow_y_scroll();
        let sections = match &self.overview_result {
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
                    _ if self.overview_query.is_empty() => "输入关键词开始搜索",
                    _ => "正在搜索…",
                };
                return body
                    .child(div().p_4().text_color(rgb(0x687083)).child(message))
                    .into_any_element();
            }
        };
        let generation = self.overview_generation;
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
                        .pb_4()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .p_2()
                                .child(div().font_weight(gpui::FontWeight::BOLD).child(heading))
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
                                            if this.page == Page::Overview
                                                && !this.overview_detail_open
                                                && this.overview_generation == generation
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
        let query = self.overview_query.clone();
        self.suggestion_query = Some(query.clone());
        self.input.update(cx, |input, cx| input.set_text(query, cx));
        self.page = page;
        self.run_search(cx);
    }
}
