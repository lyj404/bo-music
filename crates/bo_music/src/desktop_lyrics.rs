use super::MusicApp;
use bo_music_app::desktop_service::{Command, LyricsView};
use bo_music_domain::lyrics::{LyricDocument, LyricLine};
use gpui::{
    AnyElement, App, Bounds, Context, Div, FontWeight, TitlebarOptions, WeakEntity, Window,
    WindowBackgroundAppearance, WindowBounds, WindowDecorations, WindowKind, WindowOptions, div,
    point, prelude::*, px, rgb, rgba, size,
};

const HIGHLIGHTS: [(u32, &str); 8] = [
    (0x00e5bf, "青绿"),
    (0x000000, "黑色"),
    (0xff4d4f, "红色"),
    (0xff922b, "橙色"),
    (0xfadb14, "黄色"),
    (0x37d67a, "绿色"),
    (0x4d9cff, "蓝色"),
    (0xff6fb5, "粉色"),
];

pub(super) struct DesktopLyrics {
    source: WeakEntity<MusicApp>,
    font_size: f32,
    second_line: bool,
    highlight: u32,
    staggered: bool,
    palette_open: bool,
    #[cfg(windows)]
    unlocked_style: Option<isize>,
}

impl MusicApp {
    pub(super) fn desktop_lyrics_buttons(&self, cx: &mut Context<Self>) -> Div {
        let open = self.desktop_lyrics.is_some();
        let buttons = div().flex().gap_1().child(
            super::ui::icon_button_labeled(
                "desktop-lyrics-toggle",
                "desktop-lyrics",
                if open {
                    "关闭桌面歌词"
                } else {
                    "桌面歌词"
                },
                cx,
            )
            .when(open, |button| {
                button.bg(super::appearance::palette(cx).accent_hover)
            })
            .on_click(cx.listener(|this, _, window, cx| this.toggle_desktop_lyrics(window, cx))),
        );
        #[cfg(windows)]
        let buttons = buttons.when(open, |buttons| {
            buttons.child(
                super::ui::icon_button_labeled(
                    "desktop-lyrics-input-toggle",
                    "lock",
                    if self.desktop_lyrics_locked {
                        "解锁歌词"
                    } else {
                        "锁定歌词"
                    },
                    cx,
                )
                .on_click(cx.listener(|this, _, _, cx| this.toggle_desktop_lyrics_input(cx))),
            )
        });
        buttons
    }

    fn toggle_desktop_lyrics(&mut self, window: &Window, cx: &mut Context<Self>) {
        if let Some(handle) = self.desktop_lyrics
            && handle
                .update(cx, |_, window, _| window.remove_window())
                .is_ok()
        {
            return;
        }
        let source = cx.weak_entity();
        let display = window.display(cx);
        let preferred = size(px(740.), px(158.));
        let bounds = display.as_ref().map_or_else(
            || Bounds::centered(None, preferred, cx),
            |display| {
                let screen = display.bounds();
                let width = preferred.width.min(screen.size.width);
                Bounds::new(
                    point(
                        screen.origin.x + (screen.size.width - width) / 2.,
                        screen.origin.y
                            + (screen.size.height - preferred.height - px(68.)).max(px(0.)),
                    ),
                    size(width, preferred.height),
                )
            },
        );
        let display_id = display.map(|display| display.id());
        cx.defer(move |cx| {
            if let Some(source) = source.upgrade() {
                open_overlay(&source, bounds, display_id, cx);
            }
        });
    }

    #[cfg(windows)]
    fn toggle_desktop_lyrics_input(&mut self, cx: &mut Context<Self>) {
        if let Some(handle) = self.desktop_lyrics {
            let lock = !self.desktop_lyrics_locked;
            match handle.update(cx, |view, window, cx| {
                let changed = view.set_mouse_passthrough(window, lock);
                cx.notify();
                changed
            }) {
                Ok(true) => self.desktop_lyrics_locked = lock,
                Ok(false) => self.status = "无法更改桌面歌词的鼠标穿透状态".into(),
                Err(_) => self.desktop_lyrics = None,
            }
            cx.notify();
        }
    }
}

fn open_overlay(
    source: &gpui::Entity<MusicApp>,
    bounds: Bounds<gpui::Pixels>,
    display_id: Option<gpui::DisplayId>,
    cx: &mut App,
) {
    if source
        .read(cx)
        .desktop_lyrics
        .is_some_and(|handle| handle.update(cx, |_, _, _| ()).is_ok())
    {
        return;
    }
    let preferences = source.read(cx).floating_windows.get().unwrap_or_default();
    let minimum = size(px(360.), px(158.));
    let (bounds, display_id) = super::floating_windows::restore_placement(
        preferences.desktop_lyrics,
        bounds,
        display_id,
        minimum,
        cx,
    );
    #[cfg(windows)]
    let (bounds, display_id) = if preferences.desktop_lyrics.is_none() {
        super::floating_windows::restore_legacy_placement(
            source.read(cx).legacy_floating_windows.desktop_lyrics,
            minimum,
            cx,
        )
        .unwrap_or((bounds, display_id))
    } else {
        (bounds, display_id)
    };
    let options = WindowOptions {
        titlebar: Some(TitlebarOptions {
            title: Some("bo-music · 桌面歌词".into()),
            appears_transparent: true,
            ..Default::default()
        }),
        window_bounds: Some(WindowBounds::Windowed(bounds)),
        kind: if cfg!(windows) {
            WindowKind::PopUp
        } else {
            WindowKind::Floating
        },
        window_background: WindowBackgroundAppearance::Transparent,
        window_decorations: Some(WindowDecorations::Client),
        focus: false,
        is_resizable: false,
        is_minimizable: false,
        display_id,
        ..Default::default()
    };
    let result = cx.open_window(options, |window, cx| {
        #[cfg(windows)]
        super::mini_player::pin_above_other_windows(window);
        let source = source.downgrade();
        let handle = window.window_handle();
        cx.new(|cx: &mut Context<DesktopLyrics>| {
            if let Some(main) = source.upgrade() {
                cx.observe(&main, |_, _, cx| cx.notify()).detach();
                cx.observe_release_in(&main, window, |_, _, window, _| window.remove_window())
                    .detach();
                super::floating_windows::observe_placement(
                    main.read(cx),
                    window,
                    cx,
                    px(1.),
                    |settings| &mut settings.desktop_lyrics,
                );
            }
            cx.on_release(move |view, cx| {
                if let Some(main) = view.source.upgrade() {
                    main.update(cx, |main, cx| {
                        if main
                            .desktop_lyrics
                            .is_some_and(|stored| handle == stored.into())
                        {
                            main.desktop_lyrics = None;
                            #[cfg(windows)]
                            {
                                main.desktop_lyrics_locked = false;
                            }
                            cx.notify();
                        }
                    });
                }
            })
            .detach();
            DesktopLyrics {
                source,
                font_size: preferences.lyric_font_size,
                second_line: preferences.lyric_second_line,
                highlight: preferences.lyric_highlight,
                staggered: preferences.lyric_staggered,
                palette_open: false,
                #[cfg(windows)]
                unlocked_style: None,
            }
        })
    });
    source.update(cx, |main, cx| {
        match result {
            Ok(handle) => {
                main.desktop_lyrics = Some(handle);
                #[cfg(windows)]
                {
                    main.desktop_lyrics_locked = false;
                }
            }
            Err(_) => main.status = "无法打开桌面歌词".into(),
        }
        cx.notify();
    });
}

impl Render for DesktopLyrics {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let source = self.source.upgrade();
        let main = source.as_ref().map(|source| source.read(cx));
        let position = main
            .and_then(|main| main.player.as_ref())
            .map_or(0, |p| p.position_ms);
        let lyrics = main.map(|main| &main.lyrics);
        let (active, secondary) = match lyrics {
            Some(LyricsView::Ready(document)) => lyric_lines(document, position),
            _ => (None, None),
        };
        let fallback = match lyrics {
            Some(LyricsView::Loading) => "正在加载歌词…",
            Some(LyricsView::Failed) => "歌词加载失败",
            Some(LyricsView::Ready(_)) => "等待歌词…",
            _ => "暂无歌词",
        };
        let dark_text = self.highlight == 0;
        let upcoming_color = if dark_text { 0x424b61 } else { 0xffffff };
        let detail_color = if dark_text { 0x363f54 } else { 0xb9c4df };
        let staggered = self.second_line && self.staggered;
        let row = div()
            .when_some(
                crate::appearance::font(crate::appearance::FontTarget::Desktop, cx),
                |view, font| view.font_family(font),
            )
            .flex()
            .items_center()
            .when(staggered, |row| row.justify_start())
            .when(!staggered, |row| row.justify_center())
            .overflow_hidden()
            .font_weight(FontWeight::BOLD)
            .text_size(px(self.font_size));
        let primary = match active {
            Some(line) if !line.syllables.is_empty() => {
                row.children(line.syllables.iter().map(|syllable| {
                    div()
                        .text_color(rgb(if position >= syllable.start_ms {
                            self.highlight
                        } else {
                            upcoming_color
                        }))
                        .child(syllable.text.clone())
                }))
            }
            Some(line) => row.child(
                div()
                    .truncate()
                    .text_color(rgb(self.highlight))
                    .child(line.text.clone()),
            ),
            None => row.child(fallback),
        };
        let mut toolbar = div()
            .flex()
            .items_center()
            .gap_3()
            .text_sm()
            .text_color(rgb(detail_color))
            .child(drag_handle(cx));
        toolbar = toolbar
            .children([(-2., "A−"), (2., "A+")].into_iter().enumerate().map(
                |(index, (delta, label))| {
                    div()
                        .id(("desktop-lyrics-size", index))
                        .cursor_pointer()
                        .child(label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.font_size = (this.font_size + delta).clamp(18., 48.);
                            this.save_appearance(cx);
                            cx.notify();
                        }))
                },
            ))
            .child(
                div()
                    .id("desktop-lyrics-lines")
                    .cursor_pointer()
                    .child(if self.second_line { "单行" } else { "双行" })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.second_line = !this.second_line;
                        this.save_appearance(cx);
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("desktop-lyrics-colors")
                    .cursor_pointer()
                    .child("颜色")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.palette_open = !this.palette_open;
                        cx.notify();
                    })),
            )
            .when(self.second_line, |toolbar| {
                toolbar.child(
                    div()
                        .id("desktop-lyrics-alignment")
                        .cursor_pointer()
                        .child(if staggered {
                            "切换居中"
                        } else {
                            "切换错行"
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.staggered = !this.staggered;
                            this.save_appearance(cx);
                            cx.notify();
                        })),
                )
            })
            .child(
                div()
                    .id("desktop-lyrics-close")
                    .cursor_pointer()
                    .child("关闭")
                    .on_click(|_, window, _| window.remove_window()),
            );
        #[cfg(windows)]
        let toolbar = toolbar.when(self.unlocked_style.is_some(), |toolbar| toolbar.invisible());
        let show_palette = self.palette_open;
        #[cfg(windows)]
        let show_palette = show_palette && self.unlocked_style.is_none();

        div().size_full().p_2().child(
            div()
                .size_full()
                .rounded_lg()
                .px_4()
                .py_2()
                .flex()
                .flex_col()
                .gap_1()
                .bg(rgba(if dark_text { 0xf5f5f5ed } else { 0x202020e8 }))
                .text_color(rgb(upcoming_color))
                .child(toolbar)
                .when(show_palette, |body| {
                    body.child(div().flex().gap_1().flex_wrap().children(
                        HIGHLIGHTS.into_iter().map(|(color, name)| {
                            div()
                                .id(("desktop-lyric-highlight", color as usize))
                                .cursor_pointer()
                                .px_1()
                                .rounded_sm()
                                .text_sm()
                                .bg(rgb(color))
                                .text_color(rgb(if color == 0 { 0xffffff } else { 0x141922 }))
                                .child(if self.highlight == color {
                                    format!("✓ {name}")
                                } else {
                                    name.into()
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.highlight = color;
                                    this.palette_open = false;
                                    this.save_appearance(cx);
                                    cx.notify();
                                }))
                        }),
                    ))
                })
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .justify_center()
                        .child(primary)
                        .when_some(secondary.filter(|_| self.second_line), |body, line| {
                            body.child(
                                div()
                                    .truncate()
                                    .when(staggered, |line| line.text_right())
                                    .when(!staggered, |line| line.text_center())
                                    .text_color(rgb(detail_color))
                                    .text_size(px(self.font_size * 0.62))
                                    .child(line.text.clone()),
                            )
                        }),
                ),
        )
    }
}

impl DesktopLyrics {
    fn save_appearance(&self, cx: &Context<Self>) {
        if let Some(main) = self.source.upgrade() {
            let main = main.read(cx);
            if let Some(mut preferences) = main.floating_windows.get() {
                if preferences.lyric_font_size == self.font_size
                    && preferences.lyric_second_line == self.second_line
                    && preferences.lyric_highlight == self.highlight
                    && preferences.lyric_staggered == self.staggered
                {
                    return;
                }
                preferences.lyric_font_size = self.font_size;
                preferences.lyric_second_line = self.second_line;
                preferences.lyric_highlight = self.highlight;
                preferences.lyric_staggered = self.staggered;
                main.floating_windows.set(Some(preferences));
                let _ = main
                    .service
                    .commands
                    .send(Command::SaveFloatingWindows(preferences));
            }
        }
    }
}

fn lyric_lines(
    document: &LyricDocument,
    position: u64,
) -> (Option<&LyricLine>, Option<&LyricLine>) {
    let Some(index) = document
        .line_at(position)
        .and_then(|index| document.source_line(index))
    else {
        return (None, None);
    };
    let active = &document.lines[index];
    let next = document.lines.get(index + 1);
    (Some(active), next)
}

fn drag_handle(cx: &Context<DesktopLyrics>) -> AnyElement {
    let handle = div().flex_1().cursor_move().child("拖动");
    #[cfg(windows)]
    {
        let _ = cx;
        handle
            .window_control_area(gpui::WindowControlArea::Drag)
            .into_any_element()
    }
    #[cfg(not(windows))]
    handle
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(|_, _, window, _| window.start_window_move()),
        )
        .into_any_element()
}

#[cfg(windows)]
impl DesktopLyrics {
    fn set_mouse_passthrough(&mut self, window: &Window, lock: bool) -> bool {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GWL_EXSTYLE, GetWindowLongPtrW, LWA_ALPHA, SetLayeredWindowAttributes,
            SetWindowLongPtrW, WS_EX_LAYERED, WS_EX_TRANSPARENT,
        };
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return false;
        };
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return false;
        };
        let hwnd = handle.hwnd.get() as _;
        // The live, borrowed HWND belongs to this overlay and this runs on its UI thread.
        unsafe {
            let before = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let style = if lock {
                before | (WS_EX_LAYERED | WS_EX_TRANSPARENT) as isize
            } else {
                self.unlocked_style.unwrap_or(before)
            };
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style);
            if GetWindowLongPtrW(hwnd, GWL_EXSTYLE) != style
                || (lock && SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA) == 0)
            {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, before);
                return false;
            }
            self.unlocked_style = lock.then_some(before);
        }
        true
    }
}

#[cfg(test)]
#[path = "desktop_lyrics_tests.rs"]
mod tests;
