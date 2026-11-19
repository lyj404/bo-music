use super::*;

impl MusicApp {
    pub(super) fn render_comments_panel(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        if let Some(view) = &self.comments {
            let generation = view.generation;
            let same_track = view.track_id.is_some()
                && view.track_id == self.player.as_ref().and_then(|player| player.track_id);
            let in_thread = view.root.is_some();
            let mut rows: Vec<(bowen_domain::comments::SongComment, bool)> = Vec::new();
            if let Some(root) = &view.root {
                rows.push((root.clone(), true));
            }
            rows.extend(view.rows.iter().cloned().map(|row| (row, false)));
            let comment_rows = rows
                .into_iter()
                .enumerate()
                .map(|(index, (row, root))| {
                    let id = row.id;
                    let pending = view.pending_likes.contains(&id);
                    let uncertain = view.uncertain_likes.contains(&id);
                    let like_label = if pending {
                        "处理中".to_owned()
                    } else if uncertain {
                        "状态未知 · 刷新".to_owned()
                    } else {
                        format!(
                            "{} {}",
                            if row.is_liked { "♥" } else { "♡" },
                            row.like_count
                        )
                    };
                    let reply_label = if in_thread {
                        if root {
                            "回复楼主".to_owned()
                        } else {
                            "回复".to_owned()
                        }
                    } else {
                        format!("回复 {}", row.reply_count)
                    };
                    div()
                        .id(("comment-row", index))
                        .p_2()
                        .mb_2()
                        .rounded_md()
                        .bg(rgb(0xf5f7fb))
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .text_sm()
                                .text_color(rgb(0x687083))
                                .child(self.cover(row.avatar_url.as_deref(), 26.0, cx))
                                .child(format!("{}  {}", row.nickname, row.publish_time)),
                        )
                        .when(!row.reply_nickname.is_empty(), |body| {
                            body.child(
                                div()
                                    .text_sm()
                                    .text_color(rgb(0x8a91a2))
                                    .child(format!("回复 {}", row.reply_nickname)),
                            )
                        })
                        .child(div().child(row.content))
                        .when_some(row.image_url.as_deref(), |body, url| {
                            body.child(
                                div()
                                    .id(("comment-image", index))
                                    .cursor_pointer()
                                    .child(self.cover(Some(url), 96.0, cx))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        if same_track && this.show_comments {
                                            this.open_comment_image(generation, id, false, cx);
                                        }
                                    })),
                            )
                        })
                        .child(
                            div()
                                .flex()
                                .gap_3()
                                .text_sm()
                                .child(
                                    div()
                                        .id(("comment-like", index))
                                        .text_color(rgb(0x415de5))
                                        .child(like_label)
                                        .when(!pending && same_track, |button| {
                                            button.cursor_pointer().on_click(cx.listener(
                                                move |this, _, _, _| {
                                                    let command = if uncertain {
                                                        Command::CommentsRefresh(generation)
                                                    } else {
                                                        Command::ToggleCommentLike(id, generation)
                                                    };
                                                    let _ = this.service.commands.send(command);
                                                },
                                            ))
                                        }),
                                )
                                .child(
                                    div()
                                        .id(("comment-reply", index))
                                        .text_color(rgb(0x415de5))
                                        .child(reply_label)
                                        .when(same_track && !view.posting, |button| {
                                            button.cursor_pointer().on_click(cx.listener(
                                                move |this, _, _, _| {
                                                    let command = if !in_thread {
                                                        Command::OpenCommentThread(id, generation)
                                                    } else if root {
                                                        Command::ClearCommentReply(generation)
                                                    } else {
                                                        Command::ReplyToComment(id, generation)
                                                    };
                                                    let _ = this.service.commands.send(command);
                                                },
                                            ))
                                        }),
                                )
                                .when(same_track && id > 0, |row| {
                                    row.child(
                                        div()
                                            .id(("comment-share-poster", index))
                                            .cursor_pointer()
                                            .text_color(rgb(0x415de5))
                                            .child("分享海报")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                if this.show_comments {
                                                    this.open_comment_image(
                                                        generation, id, true, cx,
                                                    );
                                                }
                                            })),
                                    )
                                }),
                        )
                })
                .collect::<Vec<_>>();
            let key = comment_draft_key(view);
            let can_post = same_track
                && view.track_id.is_some_and(|id| id <= i64::from(i32::MAX))
                && key.is_some()
                && !view.posting
                && self.submitted_comment.is_none();
            let message = if !same_track {
                "选择一首歌曲后查看评论".to_owned()
            } else if view.loading && view.rows.is_empty() {
                "正在加载评论…".to_owned()
            } else if let Some(error) = &view.error {
                error.clone()
            } else if view.rows.is_empty() {
                if in_thread {
                    "暂无回复".into()
                } else {
                    "暂无评论".into()
                }
            } else {
                String::new()
            };
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .text_sm()
                        .when(in_thread, |row| {
                            row.child(
                                div()
                                    .id("comments-back")
                                    .cursor_pointer()
                                    .text_color(rgb(0x415de5))
                                    .child("← 评论列表")
                                    .on_click(cx.listener(move |this, _, _, _| {
                                        let _ = this
                                            .service
                                            .commands
                                            .send(Command::CloseCommentThread(generation));
                                    })),
                            )
                        })
                        .when(!in_thread, |row| {
                            row.children(
                                [
                                    (CommentSort::Recommended, "推荐"),
                                    (CommentSort::Latest, "最新"),
                                ]
                                .into_iter()
                                .enumerate()
                                .map(|(index, (sort, label))| {
                                    div()
                                        .id(("comments-sort", index))
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
                                                .send(Command::CommentSort(sort, generation));
                                        }))
                                })
                                .collect::<Vec<_>>(),
                            )
                        })
                        .when(same_track && !view.loading, |row| {
                            row.child(div().text_color(rgb(0x8a91a2)).child(format!(
                                "{} {}",
                                if in_thread { "回复" } else { "评论" },
                                view.total
                            )))
                        })
                        .child(
                            div()
                                .id("comments-refresh")
                                .cursor_pointer()
                                .text_color(rgb(0x415de5))
                                .child("刷新")
                                .on_click(cx.listener(move |this, _, _, _| {
                                    let _ = this
                                        .service
                                        .commands
                                        .send(Command::CommentsRefresh(generation));
                                })),
                        ),
                )
                .child(
                    div()
                        .id("comments-list")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .when(!message.is_empty(), |body| {
                            body.child(
                                div()
                                    .p_2()
                                    .text_sm()
                                    .text_color(rgb(0x8a91a2))
                                    .child(message),
                            )
                        })
                        .children(comment_rows)
                        .when(same_track && (view.has_more || view.loading), |body| {
                            body.child(
                                div()
                                    .id("comments-more")
                                    .p_2()
                                    .text_sm()
                                    .text_color(rgb(0x415de5))
                                    .child(if view.loading {
                                        "正在加载…"
                                    } else if view.error.is_some() {
                                        "重试"
                                    } else {
                                        "加载更多"
                                    })
                                    .when(!view.loading, |button| {
                                        button.cursor_pointer().on_click(cx.listener(
                                            move |this, _, _, _| {
                                                let _ = this
                                                    .service
                                                    .commands
                                                    .send(Command::CommentsNext(generation));
                                            },
                                        ))
                                    }),
                            )
                        }),
                )
                .when(same_track, |body| {
                    body.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .when(view.reply_target.is_some(), |composer| {
                                composer.child(
                                    div()
                                        .id("comment-cancel-reply")
                                        .text_sm()
                                        .cursor_pointer()
                                        .text_color(rgb(0x687083))
                                        .child(format!(
                                            "回复 {} · 取消",
                                            view.reply_target
                                                .as_ref()
                                                .map(|target| target.1.as_str())
                                                .unwrap_or("")
                                        ))
                                        .on_click(cx.listener(move |this, _, _, _| {
                                            let _ = this
                                                .service
                                                .commands
                                                .send(Command::ClearCommentReply(generation));
                                        })),
                                )
                            })
                            .child(self.comment_input.clone())
                            .child(
                                div()
                                    .flex()
                                    .gap_3()
                                    .text_sm()
                                    .child(
                                        div()
                                            .id("comment-anonymous")
                                            .cursor_pointer()
                                            .text_color(rgb(0x687083))
                                            .child(if self.comment_anonymous {
                                                "☑ 匿名"
                                            } else {
                                                "☐ 匿名"
                                            })
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                if this.submitted_comment.is_none() {
                                                    this.comment_anonymous =
                                                        !this.comment_anonymous;
                                                    cx.notify();
                                                }
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("comment-publish")
                                            .text_color(rgb(0x415de5))
                                            .child(if view.posting {
                                                "发送中…"
                                            } else {
                                                "发送"
                                            })
                                            .when(can_post, |button| {
                                                button.cursor_pointer().on_click(cx.listener(
                                                    move |this, _, _, cx| {
                                                        let text = this
                                                            .comment_input
                                                            .read(cx)
                                                            .text()
                                                            .to_owned();
                                                        if text.trim().is_empty() {
                                                            this.status = "请输入评论内容".into();
                                                            cx.notify();
                                                            return;
                                                        }
                                                        if text.encode_utf16().count() > 1000 {
                                                            this.status =
                                                                "评论不能超过 1000 个字符".into();
                                                            cx.notify();
                                                            return;
                                                        }
                                                        let Some(key) = key else {
                                                            return;
                                                        };
                                                        this.submitted_comment =
                                                            Some((generation, key, text.clone()));
                                                        if this
                                                            .service
                                                            .commands
                                                            .send(Command::PublishComment(
                                                                generation,
                                                                text,
                                                                this.comment_anonymous,
                                                            ))
                                                            .is_err()
                                                        {
                                                            this.submitted_comment = None;
                                                            this.status =
                                                                "评论服务不可用，草稿已保留".into();
                                                        }
                                                        cx.notify();
                                                    },
                                                ))
                                            }),
                                    ),
                            ),
                    )
                })
                .into_any_element()
        } else {
            div()
                .p_3()
                .text_color(rgb(0x8a91a2))
                .child("正在打开评论…")
                .into_any_element()
        }
    }
}
