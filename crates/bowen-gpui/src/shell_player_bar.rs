use super::*;

impl MusicApp {
    pub(super) fn render_player_bar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        let playing = self
            .player
            .as_ref()
            .is_some_and(|p| p.state == PlaybackState::Playing);
        let title = self
            .player
            .as_ref()
            .map_or("未播放", |p| p.title.as_str())
            .to_owned();
        let artist = self
            .player
            .as_ref()
            .map_or("", |p| p.artist.as_str())
            .to_owned();
        let like_track = self
            .player
            .as_ref()
            .and_then(|player| player.track_id)
            .filter(|id| *id > 0);
        let like_state = if self.like.track_id == like_track {
            self.like.state
        } else {
            LikeState::Loading
        };
        let time = self.player.as_ref().map_or_else(
            || "0:00 / 0:00".into(),
            |p| format!("{} / {}", clock(p.position), clock(p.duration)),
        );
        let queue_stamp = self
            .player
            .as_ref()
            .map(|p| (p.queue_account_revision, p.queue_revision));
        let volume = self.player.as_ref().map_or(100, |p| p.volume);
        let mode = self.player.as_ref().map(|p| p.mode).unwrap_or_default();
        let mode_label = match mode {
            bowen_domain::queue::PlayMode::Sequential => "顺序",
            bowen_domain::queue::PlayMode::ListLoop => "列表循环",
            bowen_domain::queue::PlayMode::SingleLoop => "单曲循环",
            bowen_domain::queue::PlayMode::Shuffle => "随机",
        };
        div()
            .h(px(84.))
            .px_6()
            .flex()
            .items_center()
            .justify_between()
            .bg(rgb(0xffffff))
            .child(
                self.cover(
                    self.player
                        .as_ref()
                        .and_then(|player| player.cover_url.as_deref()),
                    56.0,
                    cx,
                ),
            )
            .child(
                div()
                    .w(px(270.))
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().font_weight(gpui::FontWeight::BOLD).child(title))
                    .child(div().text_sm().text_color(rgb(0x788196)).child(artist)),
            )
            .when(like_track.is_some(), |bar| {
                let id = like_track.expect("track is present");
                let label = match like_state {
                    LikeState::Empty | LikeState::Loading => "查询喜欢…",
                    LikeState::LoginRequired => "登录后喜欢",
                    LikeState::NoPlaylist => "暂无喜欢歌单 · 重试",
                    LikeState::Ready(true) => "♥ 已喜欢",
                    LikeState::Ready(false) => "♡ 喜欢",
                    LikeState::Writing => "保存中…",
                    LikeState::Unknown => "喜欢状态未知 · 重试",
                };
                bar.child(
                    div()
                        .id("like-current-track")
                        .px_2()
                        .text_sm()
                        .text_color(rgb(0x415de5))
                        .child(label)
                        .when(
                            matches!(
                                like_state,
                                LikeState::Ready(_) | LikeState::Unknown | LikeState::NoPlaylist
                            ),
                            |button| {
                                button.cursor_pointer().on_click(cx.listener(
                                    move |this, _, _, _| {
                                        let command = if matches!(like_state, LikeState::Ready(_)) {
                                            Command::ToggleLike(id)
                                        } else {
                                            Command::RetryLike(id)
                                        };
                                        let _ = this.service.commands.send(command);
                                    },
                                ))
                            },
                        ),
                )
            })
            .when(
                like_track.is_some() && queue_stamp.is_some() && !self.auth.busy,
                |bar| {
                    let id = like_track.expect("track is present");
                    let (account_revision, queue_revision) = queue_stamp.expect("queue is present");
                    bar.child(
                        div()
                            .id("open-current-mv")
                            .cursor_pointer()
                            .px_2()
                            .text_sm()
                            .text_color(rgb(0x415de5))
                            .child("MV")
                            .on_click(cx.listener(move |this, _, _, _| {
                                let _ = this
                                    .service
                                    .commands
                                    .send(Command::OpenMv(account_revision, id));
                            })),
                    )
                    .child(
                        div()
                            .id("share-current-track")
                            .cursor_pointer()
                            .px_2()
                            .text_sm()
                            .text_color(rgb(0x415de5))
                            .child("分享")
                            .on_click(cx.listener(move |this, _, _, _| {
                                let _ = this.service.commands.send(Command::ShareCurrent(
                                    account_revision,
                                    queue_revision,
                                    id,
                                ));
                            })),
                    )
                },
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .id("previous")
                            .cursor_pointer()
                            .child("上一首")
                            .on_click(cx.listener(|this, _, _, _| {
                                let _ = this.service.commands.send(Command::Previous);
                            })),
                    )
                    .child(
                        div()
                            .id("rewind")
                            .cursor_pointer()
                            .text_sm()
                            .child("−10秒")
                            .on_click(cx.listener(|this, _, _, _| {
                                let _ = this.service.commands.send(Command::SeekRelative(-10));
                            })),
                    )
                    .child(
                        div()
                            .id("toggle")
                            .px_4()
                            .py_2()
                            .rounded_md()
                            .bg(rgb(0x4d66e5))
                            .text_color(gpui::rgb(0xffffff))
                            .cursor_pointer()
                            .child(if playing { "暂停" } else { "播放" })
                            .on_click(cx.listener(|this, _, _, _| {
                                let _ = this.service.commands.send(Command::Toggle);
                            })),
                    )
                    .child(
                        div()
                            .id("next")
                            .cursor_pointer()
                            .child("下一首")
                            .on_click(cx.listener(|this, _, _, _| {
                                let _ = this.service.commands.send(Command::Next);
                            })),
                    )
                    .child(
                        div()
                            .id("forward")
                            .cursor_pointer()
                            .text_sm()
                            .child("+10秒")
                            .on_click(cx.listener(|this, _, _, _| {
                                let _ = this.service.commands.send(Command::SeekRelative(10));
                            })),
                    ),
            )
            .child(
                div()
                    .w(px(230.))
                    .flex()
                    .flex_col()
                    .items_end()
                    .gap_1()
                    .child(div().text_sm().text_color(rgb(0x788196)).child(time))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .text_sm()
                            .child(
                                div()
                                    .id("mode")
                                    .cursor_pointer()
                                    .child(mode_label)
                                    .on_click(cx.listener(|this, _, _, _| {
                                        let _ = this.service.commands.send(Command::CycleMode);
                                    })),
                            )
                            .child(
                                div()
                                    .id("volume-down")
                                    .cursor_pointer()
                                    .child("−")
                                    .on_click(cx.listener(|this, _, _, _| {
                                        let _ =
                                            this.service.commands.send(Command::VolumeDelta(-10));
                                    })),
                            )
                            .child(format!("音量 {volume}%"))
                            .child(div().id("volume-up").cursor_pointer().child("+").on_click(
                                cx.listener(|this, _, _, _| {
                                    let _ = this.service.commands.send(Command::VolumeDelta(10));
                                }),
                            )),
                    ),
            )
            .into_any_element()
    }
}
