use super::*;

impl MusicApp {
    pub(super) fn render_shell_overlays(
        &self,
        root: gpui::Div,
        show_search_assist: bool,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let rgb = appearance::paint(cx);
        let account_panel = self.render_account_panel(cx);
        root.when(show_search_assist, |root| {
            root.child(self.render_search_assist(cx))
        })
        .when(self.show_account, |root| {
            root.child(
                div()
                    .px_6()
                    .py_3()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap_2()
                    .bg(rgb(0xffffff))
                    .child(account_panel)
                    .when(self.account_cleanup_pending, |panel| {
                        panel.child(
                            div()
                                .id("account-cleanup")
                                .text_sm()
                                .text_color(rgb(0x415de5))
                                .child("有凭据待清理，点击重试")
                                .when(!self.auth.busy, |button| {
                                    button.cursor_pointer().on_click(cx.listener(
                                        |this, _, _, _| {
                                            let _ = this
                                                .service
                                                .commands
                                                .send(Command::RetryAccountCleanup);
                                        },
                                    ))
                                }),
                        )
                    })
                    .when(
                        self.adding_account && self.auth.account.is_some(),
                        |panel| {
                            panel.child(
                                div()
                                    .id("account-add-cancel")
                                    .text_sm()
                                    .cursor_pointer()
                                    .text_color(rgb(0x415de5))
                                    .child("取消添加")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        if !this.auth.busy {
                                            this.adding_account = false;
                                            let _ = this.service.commands.send(Command::CancelQr);
                                            cx.notify();
                                        }
                                    })),
                            )
                        },
                    )
                    .children(
                        self.remembered_accounts
                            .iter()
                            .enumerate()
                            .map(|(index, entry)| {
                                let uid = entry.uid.clone();
                                let switch_uid = uid.clone();
                                let current = self
                                    .auth
                                    .account
                                    .as_ref()
                                    .is_some_and(|account| account.uid == uid);
                                let name = entry
                                    .nickname
                                    .clone()
                                    .filter(|value| !value.is_empty())
                                    .unwrap_or_else(|| format!("账号 {uid}"));
                                let can_switch = !self.auth.busy && !entry.stale && !current;
                                let confirm_forget =
                                    self.forget_pending.as_deref() == Some(uid.as_str());
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .text_sm()
                                    .child(div().child(format!(
                                        "{}{}{}",
                                        name,
                                        if current { "（当前）" } else { "" },
                                        if entry.stale { "（已失效）" } else { "" }
                                    )))
                                    .child(
                                        div()
                                            .id(("switch-account", index))
                                            .text_color(if can_switch {
                                                rgb(0x415de5)
                                            } else {
                                                rgb(0xaab0bd)
                                            })
                                            .child("切换")
                                            .when(can_switch, |button| {
                                                button.cursor_pointer().on_click(cx.listener(
                                                    move |this, _, _, _| {
                                                        let _ = this.service.commands.send(
                                                            Command::SwitchAccount(
                                                                switch_uid.clone(),
                                                            ),
                                                        );
                                                    },
                                                ))
                                            }),
                                    )
                                    .child(
                                        div()
                                            .id(("forget-account", index))
                                            .text_color(rgb(0x415de5))
                                            .child(if confirm_forget {
                                                "确认移除"
                                            } else {
                                                "移除"
                                            })
                                            .when(!self.auth.busy, |button| {
                                                button.cursor_pointer().on_click(cx.listener(
                                                    move |this, _, _, cx| {
                                                        if this.forget_pending.as_deref()
                                                            == Some(uid.as_str())
                                                        {
                                                            let _ = this.service.commands.send(
                                                                Command::ForgetAccount(uid.clone()),
                                                            );
                                                            this.forget_pending = None;
                                                        } else {
                                                            this.forget_pending = Some(uid.clone());
                                                        }
                                                        cx.notify();
                                                    },
                                                ))
                                            }),
                                    )
                            }),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x788196))
                            .child(self.auth.message.clone()),
                    ),
            )
        })
        .when(self.batch_progress.is_some(), |root| {
            let progress = self.batch_progress.expect("progress checked");
            root.child(
                div()
                    .mx_5()
                    .mt_3()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(0xffffff))
                    .flex()
                    .gap_3()
                    .items_center()
                    .child(format!(
                        "批量处理：{}/{}，{} 首成功，{} 首已喜欢，{} 首结果未确认",
                        progress.processed,
                        progress.total,
                        progress.succeeded,
                        progress.skipped,
                        progress.unconfirmed
                    ))
                    .child(
                        div()
                            .id("cancel-batch")
                            .cursor_pointer()
                            .text_color(rgb(0x415de5))
                            .child("停止")
                            .on_click(cx.listener(|this, _, _, _| {
                                let _ = this.service.commands.send(Command::CancelBatch);
                            })),
                    ),
            )
        })
        .when(self.track_picker.is_some(), |root| {
            let picker = self.track_picker.as_ref().expect("picker checked");
            let track_id = picker.track_id;
            let picker_generation = picker.generation;
            let track_count = picker.track_count;
            let can_select =
                !picker.busy && !picker.loading && !self.playlist_busy && picker.error.is_none();
            let entries = picker
                .playlists
                .iter()
                .enumerate()
                .map(|(index, playlist)| {
                    let id = playlist.id;
                    div()
                        .id(("picker-playlist", index))
                        .p_2()
                        .rounded_md()
                        .child(playlist.name.clone())
                        .when(can_select, |row| {
                            row.cursor_pointer()
                                .hover(|style| style.bg(rgb(0xe9edf5)))
                                .on_click(cx.listener(move |this, _, _, _| {
                                    let command = if track_count > 1 {
                                        Command::AddPickedTracks(picker_generation, id)
                                    } else {
                                        Command::AddTrackToPlaylist(track_id, id)
                                    };
                                    let _ = this.service.commands.send(command);
                                }))
                        })
                })
                .collect::<Vec<_>>();
            let message = if picker.loading {
                "正在加载歌单…".to_owned()
            } else if let Some(error) = &picker.error {
                error.clone()
            } else if picker.playlists.is_empty() {
                "暂无自建歌单".to_owned()
            } else if picker.busy || self.playlist_busy {
                "正在保存…".to_owned()
            } else {
                String::new()
            };
            root.child(
                div()
                    .mx_5()
                    .mt_3()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(0xffffff))
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .child(if track_count > 1 {
                                format!("将 {} 首歌曲加入歌单", track_count)
                            } else {
                                format!("将「{}」加入歌单", picker.track_title)
                            })
                            .child(
                                div()
                                    .id("picker-close")
                                    .cursor_pointer()
                                    .text_color(rgb(0x415de5))
                                    .child("关闭")
                                    .on_click(cx.listener(|this, _, _, _| {
                                        let _ =
                                            this.service.commands.send(Command::CloseTrackPicker);
                                    })),
                            ),
                    )
                    .when(!message.is_empty(), |panel| {
                        panel.child(div().text_sm().text_color(rgb(0x788196)).child(message))
                    })
                    .child(
                        div()
                            .id("picker-choices")
                            .max_h(px(180.))
                            .overflow_y_scroll()
                            .children(entries),
                    ),
            )
        })
    }
}
