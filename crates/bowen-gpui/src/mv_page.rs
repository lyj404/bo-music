use crate::{MusicApp, clock};
use bowen_app::desktop_service::{Command, MvAction, MvQuality, MvView};
use bowen_playback::{
    mpv::PlaybackState,
    video::{VideoFrame, VideoSnapshot},
};
use gpui::{
    AnyElement, App, Context, DispatchPhase, Div, FocusHandle, Focusable, KeyBinding, MouseButton,
    MouseDownEvent, ObjectFit, RenderImage, Stateful, Task, Window, actions, canvas, div, img,
    prelude::*, px, relative, rgb,
};
use std::{sync::Arc, time::Duration};

actions!(mv_player, [Close, Toggle, Fullscreen]);

pub(super) fn register_bindings(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("escape", Close, Some("MvPlayer")),
        KeyBinding::new("space", Toggle, Some("MvPlayer")),
        KeyBinding::new("f11", Fullscreen, Some("MvPlayer")),
    ]);
}

pub(super) struct MvPage {
    view: Option<MvView>,
    focus: FocusHandle,
    original_fullscreen: Option<bool>,
    ticker: Option<Task<()>>,
    revision: Option<u64>,
    frame: Option<Arc<VideoFrame>>,
    image: Option<Arc<RenderImage>>,
    volume: f64,
    audible_volume: f64,
}

impl MvPage {
    pub(super) fn new(cx: &mut Context<MusicApp>) -> Self {
        Self {
            view: None,
            focus: cx.focus_handle(),
            original_fullscreen: None,
            ticker: None,
            revision: None,
            frame: None,
            image: None,
            volume: 100.,
            audible_volume: 100.,
        }
    }

    fn snapshot(&self) -> VideoSnapshot {
        self.view
            .as_ref()
            .and_then(|view| view.source.as_ref())
            .map_or_else(VideoSnapshot::default, |source| source.snapshot())
    }

    fn sync_image(&mut self, frame: Option<Arc<VideoFrame>>, window: &mut Window) {
        if self
            .frame
            .as_ref()
            .zip(frame.as_ref())
            .is_some_and(|(shown, latest)| Arc::ptr_eq(shown, latest))
        {
            return;
        }
        if let Some(image) = self.image.take() {
            let _ = window.drop_image(image);
        }
        self.image = frame.as_ref().and_then(|frame| {
            image::RgbaImage::from_raw(frame.width, frame.height, frame.bgra.clone())
                .map(|pixels| Arc::new(RenderImage::new([image::Frame::new(pixels)])))
        });
        self.frame = frame;
    }
}

impl MusicApp {
    pub(super) fn accept_mv(&mut self, view: Option<MvView>, cx: &mut Context<Self>) {
        let page = &mut self.mv_page;
        if page.view.is_none() && view.is_some() {
            page.volume = self
                .player
                .as_ref()
                .map_or(100., |player| f64::from(player.volume));
            page.audible_volume = if page.volume > 0. { page.volume } else { 100. };
        }
        if view.is_none() {
            page.ticker = None;
        } else if page.ticker.is_none() {
            page.ticker = Some(cx.spawn(async move |entity, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(33))
                        .await;
                    if entity
                        .update(cx, |app, cx| {
                            if app.mv_page.view.is_none() {
                                return false;
                            }
                            let revision = app.mv_page.snapshot().revision;
                            if app.mv_page.revision != Some(revision) {
                                app.mv_page.revision = Some(revision);
                                cx.notify();
                            }
                            true
                        })
                        .ok()
                        != Some(true)
                    {
                        break;
                    }
                }
            }));
        }
        page.revision = None;
        page.view = view;
    }

    fn mv_volume(&mut self, volume: f64, cx: &mut Context<Self>) {
        self.mv_page.volume = volume;
        if volume > 0. {
            self.mv_page.audible_volume = volume;
        }
        let _ = self
            .service
            .commands
            .send(Command::ControlMv(MvAction::Volume(volume)));
        cx.notify();
    }

    pub(super) fn render_mv(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let page = &mut self.mv_page;
        let Some(view) = page.view.clone() else {
            if let Some(fullscreen) = page.original_fullscreen.take() {
                page.sync_image(None, window);
                if window.is_fullscreen() != fullscreen {
                    window.toggle_fullscreen();
                }
                window.focus(&self.input.read(cx).focus_handle(cx));
            }
            return None;
        };
        if page.original_fullscreen.is_none() {
            page.original_fullscreen = Some(window.is_fullscreen());
            window.focus(&page.focus);
        }
        let status = page.snapshot();
        page.sync_image(status.frame.clone(), window);
        let playing = status.state == PlaybackState::Playing;
        let has_source = view.source.is_some();
        let duration = status.duration_seconds.unwrap_or(0.);
        let limit = view.preview_seconds.map_or(duration, |preview| {
            if duration > 0. {
                duration.min(f64::from(preview))
            } else {
                f64::from(preview)
            }
        });
        let position = status.position_seconds.min(limit);
        let percent = if limit > 0. { position / limit } else { 0. };
        let seek = self.service.commands.clone();
        let entity = cx.entity().downgrade();
        let volume = page.volume;
        let current_quality = view
            .retry_quality
            .or(view.quality)
            .unwrap_or(MvQuality::High);
        let message = view.error.or_else(|| {
            if status.failure.is_some() {
                Some("MV 播放中断，请重新加载")
            } else if view.requested.is_some() && has_source {
                Some("正在切换画质…")
            } else if view.requested.is_some() {
                Some("正在加载 MV…")
            } else {
                None
            }
        });

        Some(
            div()
                .id("mv-page")
                .key_context("MvPlayer")
                .track_focus(&page.focus)
                .size_full()
                .flex()
                .flex_col()
                .bg(rgb(0x121621))
                .text_color(rgb(0xf8f9ff))
                .on_action(cx.listener(|this, _: &Close, _, _| {
                    let _ = this.service.commands.send(Command::CloseMv);
                }))
                .on_action(cx.listener(|this, _: &Toggle, _, _| {
                    let _ = this
                        .service
                        .commands
                        .send(Command::ControlMv(MvAction::Playing(None)));
                }))
                .on_action(cx.listener(|_, _: &Fullscreen, window, cx| {
                    window.toggle_fullscreen();
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_4()
                        .px_5()
                        .py_3()
                        .child(button("mv-back", "← 返回").on_click(cx.listener(
                            |this, _, _, _| {
                                let _ = this.service.commands.send(Command::CloseMv);
                            },
                        )))
                        .child(div().flex_1().min_w_0().truncate().child(view.title))
                        .child(
                            button(
                                "mv-fullscreen",
                                if window.is_fullscreen() {
                                    "退出全屏"
                                } else {
                                    "全屏"
                                },
                            )
                            .on_click(cx.listener(
                                |_, _, window, cx| {
                                    window.toggle_fullscreen();
                                    cx.notify();
                                },
                            )),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .relative()
                        .bg(rgb(0x080a10))
                        .when_some(page.image.clone(), |stage, image| {
                            stage.child(
                                img(image)
                                    .absolute()
                                    .inset_0()
                                    .size_full()
                                    .object_fit(ObjectFit::Contain),
                            )
                        })
                        .when(page.image.is_none(), |stage| {
                            stage
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(message.unwrap_or("等待视频画面"))
                        }),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .px_5()
                        .py_3()
                        .child(bar(percent, move |fraction, _| {
                            if limit > 0. {
                                let _ =
                                    seek.send(Command::ControlMv(MvAction::Seek(fraction * limit)));
                            }
                        }))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .when(has_source && status.failure.is_none(), |row| {
                                    row.child(
                                        button("mv-toggle", if playing { "暂停" } else { "播放" })
                                            .on_click(cx.listener(|this, _, _, _| {
                                                let _ = this.service.commands.send(
                                                    Command::ControlMv(MvAction::Playing(None)),
                                                );
                                            })),
                                    )
                                })
                                .child(div().text_sm().child(format!(
                                    "{} / {}",
                                    clock(position as u64),
                                    clock(limit as u64)
                                )))
                                .when_some(view.preview_seconds, |row, _| {
                                    row.child(div().text_sm().child("试看"))
                                })
                                .child(div().flex_1())
                                .when(message.is_some() && page.image.is_some(), |row| {
                                    row.child(div().text_sm().child(message.unwrap_or_default()))
                                })
                                .when(view.error.is_some() || status.failure.is_some(), |row| {
                                    row.child(button("mv-retry", "重新加载").on_click(cx.listener(
                                        move |this, _, _, _| {
                                            let _ = this.service.commands.send(Command::ControlMv(
                                                MvAction::Quality(current_quality),
                                            ));
                                        },
                                    )))
                                })
                                .children(
                                    [
                                        (MvQuality::Low, "流畅"),
                                        (MvQuality::Standard, "标清"),
                                        (MvQuality::High, "高清"),
                                    ]
                                    .into_iter()
                                    .map(|(quality, label)| {
                                        button(label, label)
                                            .when(view.quality == Some(quality), |item| {
                                                item.bg(rgb(0x465fd9))
                                            })
                                            .on_click(cx.listener(move |this, _, _, _| {
                                                if this.mv_page.view.as_ref().is_some_and(|view| {
                                                    view.requested != Some(quality)
                                                        && view.quality != Some(quality)
                                                }) {
                                                    let _ = this.service.commands.send(
                                                        Command::ControlMv(MvAction::Quality(
                                                            quality,
                                                        )),
                                                    );
                                                }
                                            }))
                                    }),
                                )
                                .child(
                                    button(
                                        "mv-mute",
                                        if volume > 0. {
                                            "静音"
                                        } else {
                                            "取消静音"
                                        },
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            let volume = if this.mv_page.volume > 0. {
                                                0.
                                            } else {
                                                this.mv_page.audible_volume
                                            };
                                            this.mv_volume(volume, cx);
                                        },
                                    )),
                                )
                                .child(div().w(px(88.)).child(bar(
                                    volume / 100.,
                                    move |fraction, cx| {
                                        let _ = entity.update(cx, |this, cx| {
                                            this.mv_volume(fraction * 100., cx)
                                        });
                                    },
                                ))),
                        ),
                )
                .into_any_element(),
        )
    }
}

fn button(id: &'static str, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .text_sm()
        .bg(rgb(0x282f45))
        .hover(|item| item.bg(rgb(0x3b4665)))
        .child(label)
}

fn bar(fraction: f64, adjust: impl Fn(f64, &mut App) + 'static) -> Div {
    div()
        .relative()
        .w_full()
        .h(px(16.))
        .flex()
        .items_center()
        .cursor_pointer()
        .child(
            div()
                .w_full()
                .h(px(4.))
                .rounded_full()
                .bg(rgb(0x454e64))
                .child(
                    div()
                        .h_full()
                        .w(relative(fraction.clamp(0., 1.) as f32))
                        .rounded_full()
                        .bg(rgb(0x819aff)),
                ),
        )
        .child(
            canvas(
                |bounds, _, _| bounds,
                move |_, bounds, window, _| {
                    window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble
                            && event.button == MouseButton::Left
                            && bounds.contains(&event.position)
                            && bounds.size.width > px(0.)
                        {
                            let fraction = (event.position.x - bounds.origin.x) / bounds.size.width;
                            adjust(f64::from(fraction.clamp(0., 1.)), cx);
                        }
                    });
                },
            )
            .absolute()
            .inset_0(),
        )
}
