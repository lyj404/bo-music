use super::{MusicApp, clock};
use bo_music_app::{
    desktop_service::{Command, PlayerView},
    playback::QueueEdit,
};
use bo_music_playback::mpv::PlaybackState;
use gpui::{
    AnyElement, App, Bounds, Context, DisplayId, Div, Entity, FontWeight, Stateful,
    TitlebarOptions, WeakEntity, Window, WindowBounds, WindowHandle, WindowKind, WindowOptions,
    div, prelude::*, px, rgb, size,
};

#[cfg(windows)]
#[path = "mini_player_dock.rs"]
mod dock;

pub(super) struct MiniPlayer {
    source: WeakEntity<MusicApp>,
    main: WindowHandle<MusicApp>,
    show_queue: bool,
    #[cfg(windows)]
    dock: dock::Dock,
}

impl MusicApp {
    pub(super) fn open_mini_player(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(main) = window.window_handle().downcast::<Self>() else {
            return;
        };
        let display = window.display(cx).map(|display| display.id());
        let source = cx.weak_entity();
        // GPUI draws a new window synchronously. Open it after releasing the main view.
        cx.defer(move |cx| {
            if let Some(source) = source.upgrade() {
                MiniPlayer::open(&source, main, display, cx);
            }
        });
    }
}

impl MiniPlayer {
    fn open(
        source: &Entity<MusicApp>,
        main: WindowHandle<MusicApp>,
        display: Option<DisplayId>,
        cx: &mut App,
    ) {
        if let Some(mini) = source.read(cx).mini_player
            && mini
                .update(cx, |player, window, cx| {
                    #[cfg(windows)]
                    if player.dock.expand(window) {
                        cx.notify();
                    }
                    #[cfg(not(windows))]
                    let _ = (player, cx);
                    window.activate_window();
                })
                .is_ok()
        {
            return;
        }
        let minimum = size(px(400.), px(200.));
        let fallback = Bounds::centered(display, size(px(478.), px(218.)), cx);
        let saved = source
            .read(cx)
            .floating_windows
            .get()
            .and_then(|settings| settings.mini_player);
        let (bounds, display) =
            super::floating_windows::restore_placement(saved, fallback, display, minimum, cx);
        #[cfg(windows)]
        let (bounds, display) = if saved.is_none() {
            super::floating_windows::restore_legacy_placement(
                source.read(cx).legacy_floating_windows.mini_player,
                minimum,
                cx,
            )
            .unwrap_or((bounds, display))
        } else {
            (bounds, display)
        };
        let result = cx.open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some("bo_music · 迷你播放器".into()),
                    appears_transparent: cfg!(windows),
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(if cfg!(windows) {
                    size(px(32.), minimum.height)
                } else {
                    minimum
                }),
                display_id: display,
                kind: if cfg!(windows) {
                    WindowKind::PopUp
                } else {
                    WindowKind::Floating
                },
                window_decorations: cfg!(windows).then_some(gpui::WindowDecorations::Client),
                is_resizable: !cfg!(windows),
                is_minimizable: false,
                ..Default::default()
            },
            |window, cx| {
                #[cfg(windows)]
                pin_above_other_windows(window);
                cx.new(|cx| MiniPlayer::new(source, main, window, cx))
            },
        );
        source.update(cx, |source, cx| match result {
            Ok(mini) => source.mini_player = Some(mini),
            Err(_) => {
                source.status = "无法打开迷你播放器".into();
                cx.notify();
            }
        });
    }
    fn new(
        source: &Entity<MusicApp>,
        main: WindowHandle<MusicApp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.observe(source, |_, _, cx| cx.notify()).detach();
        cx.observe_release_in(source, window, |_, _, window, _| window.remove_window())
            .detach();
        super::floating_windows::observe_placement(
            source.read(cx),
            window,
            cx,
            px(320.),
            |settings| &mut settings.mini_player,
        );
        #[cfg(windows)]
        dock::watch(window, cx);
        Self {
            source: source.downgrade(),
            main,
            show_queue: false,
            #[cfg(windows)]
            dock: dock::Dock::default(),
        }
    }

    fn send(&self, command: Command, cx: &Context<Self>) {
        if let Some(source) = self.source.upgrade() {
            let _ = source.read(cx).service.commands.send(command);
        }
    }

    fn queue(&self, player: &PlayerView, cx: &mut Context<Self>) -> AnyElement {
        let account = player.queue_account_revision;
        let revision = player.queue_revision;
        div()
            .id("mini-queue")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .when(player.queue.is_empty(), |queue| queue.child("队列为空"))
            .children(player.queue.iter().enumerate().map(|(index, title)| {
                div()
                    .id(("mini-track", index))
                    .cursor_pointer()
                    .truncate()
                    .px_2()
                    .py_1()
                    .text_color(rgb(if player.selected == Some(index) {
                        0xaab7ff
                    } else {
                        0xf8f9ff
                    }))
                    .hover(|row| row.bg(rgb(0x373737)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.send(
                            Command::EditQueue(account, revision, QueueEdit::Select(index)),
                            cx,
                        );
                    }))
                    .child(title.clone())
            }))
            .into_any_element()
    }

    fn transport(&self, player: Option<&PlayerView>, cx: &mut Context<Self>) -> Div {
        let playing = player.is_some_and(|p| p.state == PlaybackState::Playing);
        let volume = player.map_or(0, |p| p.volume);
        div()
            .flex()
            .items_center()
            .gap_1()
            .justify_between()
            .child(
                crate::ui::icon_button_on_dark("mini-previous", "previous", "上一首")
                    .on_click(cx.listener(|this, _, _, cx| this.send(Command::Previous, cx))),
            )
            .child(
                button("mini-back-ten", "−10 秒").on_click(
                    cx.listener(|this, _, _, cx| this.send(Command::SeekRelative(-10), cx)),
                ),
            )
            .child(
                crate::ui::icon_button_on_dark(
                    "mini-play",
                    if playing { "pause" } else { "play" },
                    if playing { "暂停" } else { "播放" },
                )
                .bg(rgb(0x415de5))
                .on_click(cx.listener(|this, _, _, cx| this.send(Command::Toggle, cx))),
            )
            .child(
                button("mini-forward-ten", "+10 秒").on_click(
                    cx.listener(|this, _, _, cx| this.send(Command::SeekRelative(10), cx)),
                ),
            )
            .child(
                crate::ui::icon_button_on_dark("mini-next", "next", "下一首")
                    .on_click(cx.listener(|this, _, _, cx| this.send(Command::Next, cx))),
            )
            .child(
                button("mini-volume-down", "−").on_click(
                    cx.listener(|this, _, _, cx| this.send(Command::VolumeDelta(-10), cx)),
                ),
            )
            .child(div().text_color(rgb(0xb9c4df)).child(format!("{volume}%")))
            .child(
                button("mini-volume-up", "+").on_click(
                    cx.listener(|this, _, _, cx| this.send(Command::VolumeDelta(10), cx)),
                ),
            )
    }
}

impl Render for MiniPlayer {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        #[cfg(windows)]
        if let Some(arrow) = self.dock.arrow() {
            return div()
                .id("mini-dock-ribbon")
                .size_full()
                .bg(rgb(0x202020))
                .text_color(rgb(0xf8f9ff))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .on_click(cx.listener(|this, _, window, cx| {
                    if this.dock.expand(window) {
                        cx.notify();
                    }
                }))
                .child(arrow)
                .into_any_element();
        }
        let Some(source) = self.source.upgrade() else {
            return div().into_any_element();
        };
        let (player, cover, lyric) = source.update(cx, |source, cx| {
            let player = source.player.clone();
            let cover = source.cover(
                player.as_ref().and_then(|p| p.cover_url.as_deref()),
                44.,
                cx,
            );
            let lyric = player
                .as_ref()
                .filter(|player| player.state == PlaybackState::Playing)
                .and_then(|player| match &source.lyrics {
                    bo_music_app::desktop_service::LyricsView::Ready(document) => document
                        .line_at(player.position_ms)
                        .and_then(|index| document.source_line(index))
                        .and_then(|index| document.lines.get(index))
                        .map(|line| line.text.clone()),
                    _ => None,
                });
            (player, cover, lyric)
        });
        let player = player.as_ref();
        let title = player.map_or("未播放", |p| p.title.as_str()).to_owned();
        let artist = player.map_or("", |p| p.artist.as_str()).to_owned();
        let (position, duration) = player.map_or((0, 0), |p| (p.position, p.duration));
        let progress = if duration == 0 {
            0.
        } else {
            (position as f32 / duration as f32).clamp(0., 1.)
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .text_sm()
            .bg(rgb(0x202020))
            .text_color(rgb(0xf8f9ff))
            .when_some(
                crate::appearance::font(crate::appearance::FontTarget::Interface, cx),
                |view, font| view.font_family(font),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(cover)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_color(rgb(0xb9c4df))
                                    .when(lyric.is_some(), |line| {
                                        line.when_some(
                                            crate::appearance::font(
                                                crate::appearance::FontTarget::Mini,
                                                cx,
                                            ),
                                            |line, font| line.font_family(font),
                                        )
                                    })
                                    .child(lyric.unwrap_or(artist)),
                            ),
                    )
                    .child(
                        button(
                            "mini-queue-toggle",
                            if self.show_queue {
                                "收起队列"
                            } else {
                                "队列"
                            },
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_queue = !this.show_queue;
                            cx.notify();
                        })),
                    )
                    .child(
                        button("mini-main", "主窗口").on_click(cx.listener(|this, _, _, cx| {
                            let _ = this
                                .main
                                .update(cx, |_, window, _| super::activate_main_window(window));
                        })),
                    )
                    .when(cfg!(windows), |header| {
                        header
                            .child(
                                div()
                                    .cursor_move()
                                    .window_control_area(gpui::WindowControlArea::Drag)
                                    .child("拖动"),
                            )
                            .child(
                                button("mini-close", "关闭")
                                    .on_click(|_, window, _| window.remove_window()),
                            )
                    }),
            )
            .when(self.show_queue && player.is_none(), |body| {
                body.child(div().flex_1().child("队列为空"))
            })
            .when_some(player.filter(|_| self.show_queue), |body, player| {
                body.child(self.queue(player, cx))
            })
            .when(!self.show_queue, |body| body.child(div().flex_1()))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .h(px(4.))
                            .rounded_sm()
                            .bg(rgb(0x454545))
                            .child(
                                div()
                                    .w(gpui::relative(progress))
                                    .h_full()
                                    .rounded_sm()
                                    .bg(rgb(0x8192ff)),
                            ),
                    )
                    .child(div().text_color(rgb(0xb9c4df)).child(format!(
                        "{} / {}",
                        clock(position),
                        clock(duration)
                    ))),
            )
            .child(self.transport(player, cx))
            .into_any_element()
    }
}

fn button(id: &'static str, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .cursor_pointer()
        .px_2()
        .py_1()
        .rounded_md()
        .hover(|button| button.bg(rgb(0x373737)))
        .child(label)
}

#[cfg(windows)]
pub(super) fn pin_above_other_windows(window: &Window) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
    };

    if let Ok(handle) = HasWindowHandle::window_handle(window)
        && let RawWindowHandle::Win32(handle) = handle.as_raw()
    {
        // The GPUI window owns this HWND, and the borrowed handle is live for the call.
        unsafe {
            SetWindowPos(
                handle.hwnd.get() as _,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
            )
        };
    }
}
