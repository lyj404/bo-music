use super::*;

impl MusicApp {
    pub(super) fn open_playlist_dialog(
        &mut self,
        mode: PlaylistDialog,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.status.clear();
        self.playlist_editor.dialog = mode;
        self.show_account = false;
        self.panels.player_tools_open = false;
        window.focus(&self.playlist_editor.modal_focus);
        cx.notify();
    }
    pub(super) fn close_playlist_dialog(&mut self, cx: &mut Context<Self>) {
        if !self.playlist_editor.busy {
            self.playlist_editor.dialog = PlaylistDialog::Closed;
            self.clear_playlist_cover(cx);
            self.restore_playlist_focus(cx);
            cx.notify();
        }
    }
    pub(super) fn restore_playlist_focus(&self, cx: &mut Context<Self>) {
        let focus = self.playback_focus.clone();
        cx.defer(move |cx| {
            if let Some(handle) = cx
                .windows()
                .iter()
                .find_map(|window| window.downcast::<MusicApp>())
            {
                let _ = handle.update(cx, |_, window, _| window.focus(&focus));
            }
        });
    }
    pub(super) fn render_playlist_modal(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let Some(view) = self
            .navigation
            .browse
            .as_ref()
            .filter(|_| self.playlist_editor.dialog != PlaylistDialog::Closed)
        else {
            return div().into_any_element();
        };
        let rgb = appearance::paint(cx);
        let title = match self.playlist_editor.dialog {
            PlaylistDialog::Create => "新建歌单",
            PlaylistDialog::Delete(_) => "删除歌单",
            _ => "编辑歌单",
        };
        let form = self.render_playlist_form(view, cx);
        div()
            .id("playlist-modal-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .p_6()
            .bg(gpui::rgba(0x00000055))
            .track_focus(&self.playlist_editor.modal_focus)
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    if !this.playlist_editor.busy {
                        this.close_playlist_dialog(cx);
                        window.focus(&this.playback_focus);
                    }
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .w_full()
                    .max_w(px(620.))
                    .max_h(gpui::relative(0.9))
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .rounded_lg()
                    .overflow_hidden()
                    .bg(appearance::palette(cx).elevated)
                    .border_1()
                    .border_color(appearance::palette(cx).border)
                    .shadow_lg()
                    .child(
                        div()
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_between()
                            .px_6()
                            .py_4()
                            .border_b_1()
                            .border_color(rgb(0xd8dce6))
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(title),
                            )
                            .child(
                                ui::icon_button("playlist-modal-close", "close", cx)
                                    .when(self.playlist_editor.busy, |button| button.opacity(0.4))
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        if !this.playlist_editor.busy {
                                            this.close_playlist_dialog(cx);
                                            window.focus(&this.playback_focus);
                                        }
                                    })),
                            ),
                    )
                    .child(form),
            )
            .into_any_element()
    }

    pub(super) fn render_playlist_form(
        &self,
        view: &BrowseView,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        let mode = self.playlist_editor.dialog;
        let busy = self.playlist_editor.busy;
        let tag_groups = self.playlist_editor.category_groups.as_ref().map(|groups| {
            groups
                .iter()
                .enumerate()
                .map(|(group_index, group)| {
                    let tags = group
                        .categories
                        .iter()
                        .enumerate()
                        .filter_map(|(index, category)| {
                            let id = i32::try_from(category.id).ok().filter(|id| *id > 0)?;
                            let checked = self.playlist_editor.selected_categories.contains(&id);
                            Some(
                                div()
                                    .id(("playlist-tag", group_index * 10000 + index))
                                    .cursor_pointer()
                                    .p_1()
                                    .text_color(if checked {
                                        rgb(0x415de5)
                                    } else {
                                        rgb(0x687083)
                                    })
                                    .child(format!(
                                        "{} {}",
                                        if checked { "☑" } else { "☐" },
                                        category.name
                                    ))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if this.playlist_editor.busy {
                                            return;
                                        }
                                        if let Some(position) = this
                                            .playlist_editor
                                            .selected_categories
                                            .iter()
                                            .position(|value| *value == id)
                                        {
                                            this.playlist_editor
                                                .selected_categories
                                                .remove(position);
                                        } else if this.playlist_editor.selected_categories.len() < 3
                                        {
                                            this.playlist_editor.selected_categories.push(id);
                                        } else {
                                            this.status = "最多选择三个标签".into();
                                        }
                                        cx.notify();
                                    })),
                            )
                        })
                        .collect::<Vec<_>>();
                    div()
                        .child(
                            div()
                                .font_weight(gpui::FontWeight::BOLD)
                                .child(group.name.clone()),
                        )
                        .child(div().flex().flex_wrap().gap_2().children(tags))
                })
                .collect::<Vec<_>>()
        });
        let selected_labels = self
            .playlist_editor
            .selected_categories
            .iter()
            .map(|id| {
                self.playlist_editor
                    .category_groups
                    .as_ref()
                    .and_then(|groups| {
                        groups
                            .iter()
                            .flat_map(|group| &group.categories)
                            .find(|category| category.id == i64::from(*id))
                    })
                    .or_else(|| {
                        self.playlist_editor
                            .original_categories
                            .iter()
                            .find(|category| category.id == i64::from(*id))
                    })
                    .map(|category| category.name.clone())
                    .unwrap_or_else(|| id.to_string())
            })
            .collect::<Vec<_>>()
            .join("、");
        let form = div()
            .id("playlist-dialog-form")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_6()
            .py_4()
            .bg(rgb(0xffffff))
            .flex()
            .flex_col()
            .gap_2();
        let form = match mode {
            PlaylistDialog::Create => form
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(div().text_sm().child("歌单名称"))
                        .child(self.playlist_editor.name_input.clone()),
                )
                .child(
                    div()
                        .id("playlist-private")
                        .cursor_pointer()
                        .text_color(rgb(0x415de5))
                        .child(if self.playlist_editor.private {
                            "☑ 设为私密歌单"
                        } else {
                            "☐ 设为私密歌单"
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            if !this.playlist_editor.busy {
                                this.playlist_editor.private = !this.playlist_editor.private;
                                cx.notify();
                            }
                        })),
                ),
            PlaylistDialog::EditLoading(_) => form.child("正在加载歌单详情…"),
            PlaylistDialog::Edit(_) => form
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(div().text_sm().child("歌单名称"))
                        .child(self.playlist_editor.name_input.clone()),
                )
                .child(
                    div()
                        .w_full()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(div().text_sm().child("歌单简介"))
                        .child(self.playlist_editor.description_input.clone()),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .id("playlist-pick-cover")
                                .cursor_pointer()
                                .text_color(rgb(0x415de5))
                                .child("选择封面")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let PlaylistDialog::Edit(id) = this.playlist_editor.dialog
                                    else {
                                        return;
                                    };
                                    if this.playlist_editor.busy {
                                        return;
                                    }
                                    let generation = this.playlist_editor.generation;
                                    let result = cx.prompt_for_paths(PathPromptOptions {
                                        files: true,
                                        directories: false,
                                        multiple: false,
                                        prompt: Some("选择歌单封面".into()),
                                    });
                                    cx.spawn(async move |entity, cx| {
                                        let result = result.await;
                                        let _ = entity.update(cx, |this, cx| {
                                            if this.playlist_editor.dialog
                                                != PlaylistDialog::Edit(id)
                                                || this.playlist_editor.generation != generation
                                                || this.playlist_editor.busy
                                            {
                                                return;
                                            }
                                            match result {
                                                Ok(Ok(Some(paths))) => {
                                                    if let Some(path) = paths.into_iter().next() {
                                                        this.clear_playlist_cover(cx);
                                                        this.playlist_editor.cover_path =
                                                            Some(path.clone());
                                                        this.playlist_editor.cover_loading = true;
                                                        if this
                                                            .service
                                                            .commands
                                                            .send(Command::LoadPlaylistCover(
                                                                this.playlist_editor.cover_request,
                                                                path,
                                                            ))
                                                            .is_err()
                                                        {
                                                            this.playlist_editor.cover_loading =
                                                                false;
                                                            this.playlist_editor.cover_path = None;
                                                            this.status =
                                                                "无法加载封面，请重试".into();
                                                        }
                                                    }
                                                }
                                                Ok(Ok(None)) => {}
                                                _ => {
                                                    this.status =
                                                        "无法打开文件选择器，请重试".into()
                                                }
                                            }
                                            cx.notify();
                                        });
                                    })
                                    .detach();
                                })),
                        )
                        .child(
                            self.playlist_editor
                                .cover_path
                                .as_ref()
                                .and_then(|path| path.file_name())
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_else(|| "沿用原封面".into()),
                        )
                        .when(self.playlist_editor.cover_path.is_some(), |row| {
                            row.child(
                                div()
                                    .id("playlist-clear-cover")
                                    .cursor_pointer()
                                    .text_color(rgb(0x415de5))
                                    .child("沿用原封面")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if !this.playlist_editor.busy {
                                            this.clear_playlist_cover(cx);
                                            cx.notify();
                                        }
                                    })),
                            )
                        }),
                )
                .when(self.playlist_editor.cover_loading, |form| {
                    form.child("正在加载封面…")
                })
                .when(self.playlist_editor.cover_image.is_some(), |form| {
                    form.child(self.playlist_cover_cropper(cx))
                })
                .child(div().text_sm().child(format!(
                    "已选标签（最多三个）：{}",
                    if selected_labels.is_empty() {
                        "无"
                    } else {
                        &selected_labels
                    }
                )))
                .when(
                    tag_groups.is_some() && !self.playlist_editor.selected_categories.is_empty(),
                    |form| {
                        form.child(
                            div()
                                .id("playlist-clear-tags")
                                .cursor_pointer()
                                .text_color(rgb(0x415de5))
                                .text_sm()
                                .child("清空标签")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if !this.playlist_editor.busy {
                                        this.playlist_editor.selected_categories.clear();
                                        cx.notify();
                                    }
                                })),
                        )
                    },
                )
                .when(tag_groups.is_none(), |form| {
                    form.child(
                        div()
                            .text_sm()
                            .text_color(rgb(0xc23b46))
                            .child("标签列表加载失败，将保留原标签"),
                    )
                })
                .when_some(tag_groups, |form, groups| {
                    form.child(
                        div()
                            .id("playlist-tag-groups")
                            .max_h(px(180.))
                            .overflow_y_scroll()
                            .children(groups),
                    )
                }),
            PlaylistDialog::Delete(_) => {
                form.child(format!("确定删除「{}」？此操作无法撤销。", view.title))
            }
            PlaylistDialog::Closed => form,
        };
        let form = form.when(
            ["失败", "无法", "不能为空", "最多选择"]
                .iter()
                .any(|word| self.status.contains(word)),
            |form| {
                form.child(
                    div()
                        .text_sm()
                        .text_color(rgb(0xc23b46))
                        .child(self.status.clone()),
                )
            },
        );
        let footer = div()
            .flex_shrink_0()
            .px_6()
            .py_4()
            .border_t_1()
            .border_color(rgb(0xd8dce6))
            .flex()
            .justify_end()
            .gap_4()
            .child(
                div()
                    .id("playlist-cancel")
                    .px_4()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(0xe9edf5))
                    .cursor_pointer()
                    .text_color(rgb(0x687083))
                    .child("取消")
                    .on_click(cx.listener(|this, _, window, cx| {
                        if !this.playlist_editor.busy {
                            this.close_playlist_dialog(cx);
                            window.focus(&this.playback_focus);
                        }
                    })),
            )
            .when(!matches!(mode, PlaylistDialog::EditLoading(_)), |row| {
                row.child(
                    div()
                        .id("playlist-confirm")
                        .px_4()
                        .py_2()
                        .rounded_md()
                        .bg(rgb(0x4d66e5))
                        .text_color(gpui::rgb(0xffffff))
                        .child(if busy {
                            "正在保存…"
                        } else if self.playlist_editor.cover_loading
                            && matches!(mode, PlaylistDialog::Edit(_))
                        {
                            "正在加载封面…"
                        } else {
                            "确认"
                        })
                        .when(
                            !(busy
                                || self.playlist_editor.cover_loading
                                    && matches!(mode, PlaylistDialog::Edit(_))),
                            |button| {
                                button.cursor_pointer().on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        let name = this
                                            .playlist_editor
                                            .name_input
                                            .read(cx)
                                            .text()
                                            .to_owned();
                                        let description = this
                                            .playlist_editor
                                            .description_input
                                            .read(cx)
                                            .text()
                                            .to_owned();
                                        let command = match mode {
                                            PlaylistDialog::Create => {
                                                Some(Command::CreatePlaylist(
                                                    name,
                                                    this.playlist_editor.private,
                                                ))
                                            }
                                            PlaylistDialog::Edit(id) => {
                                                Some(Command::SavePlaylist(
                                                    id,
                                                    name,
                                                    description,
                                                    this.playlist_editor
                                                        .selected_categories
                                                        .clone(),
                                                    this.playlist_editor.cover_image.as_ref().map(
                                                        |image| CoverEdit {
                                                            image: Arc::clone(image),
                                                            crop: this.playlist_editor.cover_crop,
                                                        },
                                                    ),
                                                ))
                                            }
                                            PlaylistDialog::Delete(id) => {
                                                Some(Command::DeletePlaylist(id))
                                            }
                                            _ => None,
                                        };
                                        if let Some(command) = command {
                                            let _ = this.service.commands.send(command);
                                        }
                                    },
                                ))
                            },
                        ),
                )
            });
        div()
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(form)
            .child(footer)
            .into_any_element()
    }
}
