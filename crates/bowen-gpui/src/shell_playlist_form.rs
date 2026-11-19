use super::*;

impl MusicApp {
    pub(super) fn render_playlist_form(
        &self,
        view: &BrowseView,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        let mode = self.playlist_dialog;
        let busy = self.playlist_busy;
        let tag_groups = self.playlist_category_groups.as_ref().map(|groups| {
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
                            let checked = self.playlist_selected_categories.contains(&id);
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
                                        if this.playlist_busy {
                                            return;
                                        }
                                        if let Some(position) = this
                                            .playlist_selected_categories
                                            .iter()
                                            .position(|value| *value == id)
                                        {
                                            this.playlist_selected_categories.remove(position);
                                        } else if this.playlist_selected_categories.len() < 3 {
                                            this.playlist_selected_categories.push(id);
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
            .playlist_selected_categories
            .iter()
            .map(|id| {
                self.playlist_category_groups
                    .as_ref()
                    .and_then(|groups| {
                        groups
                            .iter()
                            .flat_map(|group| &group.categories)
                            .find(|category| category.id == i64::from(*id))
                    })
                    .or_else(|| {
                        self.playlist_original_categories
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
            .max_h(px(400.))
            .overflow_y_scroll()
            .p_3()
            .mb_3()
            .rounded_md()
            .bg(rgb(0xf1f3f9))
            .flex()
            .flex_col()
            .gap_2();
        let form = match mode {
            PlaylistDialog::Create => form
                .child("新建歌单")
                .child(div().w(px(300.)).child(self.playlist_name_input.clone()))
                .child(
                    div()
                        .id("playlist-private")
                        .cursor_pointer()
                        .text_color(rgb(0x415de5))
                        .child(if self.playlist_private {
                            "☑ 设为私密歌单"
                        } else {
                            "☐ 设为私密歌单"
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            if !this.playlist_busy {
                                this.playlist_private = !this.playlist_private;
                                cx.notify();
                            }
                        })),
                ),
            PlaylistDialog::EditLoading(_) => form.child("正在加载歌单详情…"),
            PlaylistDialog::Edit(_) => form
                .child("编辑歌单")
                .child(div().w(px(300.)).child(self.playlist_name_input.clone()))
                .child(
                    div()
                        .w(px(400.))
                        .child(self.playlist_description_input.clone()),
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
                                    let PlaylistDialog::Edit(id) = this.playlist_dialog else {
                                        return;
                                    };
                                    if this.playlist_busy {
                                        return;
                                    }
                                    let generation = this.playlist_editor_generation;
                                    let result = cx.prompt_for_paths(PathPromptOptions {
                                        files: true,
                                        directories: false,
                                        multiple: false,
                                        prompt: Some("选择歌单封面".into()),
                                    });
                                    cx.spawn(async move |entity, cx| {
                                        let result = result.await;
                                        let _ = entity.update(cx, |this, cx| {
                                            if this.playlist_dialog != PlaylistDialog::Edit(id)
                                                || this.playlist_editor_generation != generation
                                                || this.playlist_busy
                                            {
                                                return;
                                            }
                                            match result {
                                                Ok(Ok(Some(paths))) => {
                                                    if let Some(path) = paths.into_iter().next() {
                                                        this.clear_playlist_cover(cx);
                                                        this.playlist_cover_path =
                                                            Some(path.clone());
                                                        this.playlist_cover_loading = true;
                                                        if this
                                                            .service
                                                            .commands
                                                            .send(Command::LoadPlaylistCover(
                                                                this.playlist_cover_request,
                                                                path,
                                                            ))
                                                            .is_err()
                                                        {
                                                            this.playlist_cover_loading = false;
                                                            this.playlist_cover_path = None;
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
                            self.playlist_cover_path
                                .as_ref()
                                .and_then(|path| path.file_name())
                                .map(|name| name.to_string_lossy().into_owned())
                                .unwrap_or_else(|| "沿用原封面".into()),
                        )
                        .when(self.playlist_cover_path.is_some(), |row| {
                            row.child(
                                div()
                                    .id("playlist-clear-cover")
                                    .cursor_pointer()
                                    .text_color(rgb(0x415de5))
                                    .child("沿用原封面")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if !this.playlist_busy {
                                            this.clear_playlist_cover(cx);
                                            cx.notify();
                                        }
                                    })),
                            )
                        }),
                )
                .when(self.playlist_cover_loading, |form| {
                    form.child("正在加载封面…")
                })
                .when(self.playlist_cover_image.is_some(), |form| {
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
                    tag_groups.is_some() && !self.playlist_selected_categories.is_empty(),
                    |form| {
                        form.child(
                            div()
                                .id("playlist-clear-tags")
                                .cursor_pointer()
                                .text_color(rgb(0x415de5))
                                .text_sm()
                                .child("清空标签")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if !this.playlist_busy {
                                        this.playlist_selected_categories.clear();
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
        let form = form.child(
            div()
                .flex()
                .gap_4()
                .when(!matches!(mode, PlaylistDialog::EditLoading(_)), |row| {
                    row.child(
                        div()
                            .id("playlist-confirm")
                            .text_color(rgb(0x415de5))
                            .child(if busy {
                                "正在保存…"
                            } else if self.playlist_cover_loading
                                && matches!(mode, PlaylistDialog::Edit(_))
                            {
                                "正在加载封面…"
                            } else {
                                "确认"
                            })
                            .when(
                                !(busy
                                    || self.playlist_cover_loading
                                        && matches!(mode, PlaylistDialog::Edit(_))),
                                |button| {
                                    button.cursor_pointer().on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            let name =
                                                this.playlist_name_input.read(cx).text().to_owned();
                                            let description = this
                                                .playlist_description_input
                                                .read(cx)
                                                .text()
                                                .to_owned();
                                            let command = match mode {
                                                PlaylistDialog::Create => {
                                                    Some(Command::CreatePlaylist(
                                                        name,
                                                        this.playlist_private,
                                                    ))
                                                }
                                                PlaylistDialog::Edit(id) => {
                                                    Some(Command::SavePlaylist(
                                                        id,
                                                        name,
                                                        description,
                                                        this.playlist_selected_categories.clone(),
                                                        this.playlist_cover_image.as_ref().map(
                                                            |image| CoverEdit {
                                                                image: Arc::clone(image),
                                                                crop: this.playlist_cover_crop,
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
                })
                .child(
                    div()
                        .id("playlist-cancel")
                        .cursor_pointer()
                        .text_color(rgb(0x687083))
                        .child("取消")
                        .on_click(cx.listener(|this, _, _, cx| {
                            if !this.playlist_busy {
                                this.playlist_dialog = PlaylistDialog::Closed;
                                this.clear_playlist_cover(cx);
                                cx.notify();
                            }
                        })),
                ),
        );
        form.into_any_element()
    }
}
