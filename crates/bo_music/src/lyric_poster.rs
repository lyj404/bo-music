use super::MusicApp;
use crate::color_wheel::{self, Wheel};
use crate::input::TextInput;
use bo_music_app::{
    desktop_service::{Command, LyricPosterStyle, LyricsView, PosterTheme},
    lyric_poster::{self, Lyrics, MAX_LINES},
};
use gpui::{
    AnyElement, App, ClipboardItem, Context, Div, Entity, FocusHandle, Image, ImageFormat,
    KeyBinding, ObjectFit, Stateful, Window, actions, div, img, prelude::*, px, rgb,
};
use std::{path::PathBuf, sync::Arc};

actions!(poster, [Close]);

pub(super) fn register_bindings(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("escape", Close, Some("LyricPoster"))]);
}

struct Draft {
    song: (u64, i64),
    track_title: String,
    artist: String,
    lines: Vec<String>,
    start: usize,
    count: usize,
}

impl Draft {
    fn selection(&self) -> &[String] {
        &self.lines[self.start..self.start + self.count]
    }
}

pub(super) struct PosterPage {
    focus: FocusHandle,
    draft: Option<Draft>,
    theme: PosterTheme,
    font: Entity<TextInput>,
    background: Entity<TextInput>,
    text_color: Entity<TextInput>,
    show_style: bool,
    show_fonts: bool,
    wheel: Option<(bool, Wheel)>,
    request: u64,
    busy: bool,
    message: Option<&'static str>,
    preview: Option<Arc<Image>>,
}

impl PosterPage {
    pub(super) fn new(cx: &mut Context<MusicApp>) -> Self {
        let font = cx.new(|cx| TextInput::with_placeholder(cx, "系统默认字体"));
        let background = cx.new(|cx| TextInput::with_placeholder(cx, "跟随背景选项"));
        let text_color = cx.new(|cx| TextInput::with_placeholder(cx, "自动对比色"));
        for input in [&font, &background, &text_color] {
            cx.observe(input, |app, _, cx| {
                if app.lyric_poster.draft.is_some() {
                    app.lyric_poster.invalidate();
                    cx.notify();
                }
            })
            .detach();
        }
        Self {
            focus: cx.focus_handle(),
            draft: None,
            theme: PosterTheme::default(),
            font,
            background,
            text_color,
            show_style: false,
            show_fonts: false,
            wheel: None,
            request: 0,
            busy: false,
            message: None,
            preview: None,
        }
    }

    pub(super) fn apply_style(&mut self, style: LyricPosterStyle, cx: &mut Context<MusicApp>) {
        self.theme = style.theme;
        self.font
            .update(cx, |input, cx| input.set_text(style.font_family, cx));
        let hex =
            |color: Option<u32>| color.map_or_else(String::new, |color| format!("#{color:06X}"));
        self.background.update(cx, |input, cx| {
            input.set_text(hex(style.background_rgb), cx)
        });
        self.text_color
            .update(cx, |input, cx| input.set_text(hex(style.text_rgb), cx));
    }

    fn invalidate(&mut self) {
        self.request = self.request.wrapping_add(1);
        self.busy = false;
        self.message = None;
        self.preview = None;
    }

    pub(super) fn clear(&mut self) {
        self.draft = None;
        self.wheel = None;
        self.show_fonts = false;
        self.invalidate();
    }
}

impl MusicApp {
    fn render_style_pickers(&self, cx: &Context<Self>) -> Div {
        let page = &self.lyric_poster;
        let query = page.font.read(cx).text().trim().to_lowercase();
        let weak = cx.entity().downgrade();
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                button(
                    "poster-font-list",
                    if page.show_fonts {
                        "收起字体列表"
                    } else {
                        "已安装字体"
                    },
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.lyric_poster.show_fonts = !this.lyric_poster.show_fonts;
                    cx.notify();
                })),
            )
            .when(page.show_fonts, |view| {
                view.child(
                    div()
                        .id("poster-font-options")
                        .max_h(px(100.))
                        .overflow_y_scroll()
                        .children(
                            crate::appearance::fonts(cx)
                                .iter()
                                .filter(|name| name.to_lowercase().contains(&query))
                                .enumerate()
                                .map(|(index, name)| {
                                    div()
                                        .id(("poster-font-option", index))
                                        .cursor_pointer()
                                        .font_family(name.clone())
                                        .child(name.clone())
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.lyric_poster.font.update(cx, |input, cx| {
                                                input.set_text(name.clone(), cx)
                                            });
                                            this.lyric_poster.show_fonts = false;
                                            cx.notify();
                                        }))
                                }),
                        ),
                )
            })
            .child(
                div().flex().gap_2().children(
                    [
                        (false, "背景色轮", page.background.clone(), 0x24324a),
                        (true, "文字色轮", page.text_color.clone(), 0xffffff),
                    ]
                    .into_iter()
                    .map(|(text, label, input, fallback)| {
                        button(label, label).on_click(cx.listener(move |this, _, _, cx| {
                            this.lyric_poster.wheel = if this
                                .lyric_poster
                                .wheel
                                .is_some_and(|(target, _)| target == text)
                            {
                                None
                            } else {
                                Some((
                                    text,
                                    Wheel::from_rgb(
                                        parse_color(input.read(cx).text())
                                            .ok()
                                            .flatten()
                                            .unwrap_or(fallback),
                                    ),
                                ))
                            };
                            cx.notify();
                        }))
                    }),
                ),
            )
            .when_some(page.wheel, |view, (text, wheel)| {
                view.child(color_wheel::render(wheel, move |value, cx| {
                    let _ = weak.update(cx, |this, cx| {
                        let page = &mut this.lyric_poster;
                        if page.wheel.is_none_or(|(current, _)| current != text) {
                            return;
                        }
                        page.wheel = Some((text, value));
                        let input = if text {
                            &page.text_color
                        } else {
                            &page.background
                        };
                        input.update(cx, |input, cx| {
                            input.set_text(format!("#{:06X}", value.rgb()), cx)
                        });
                        cx.notify();
                    });
                }))
            })
    }

    pub(super) fn open_lyric_poster(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(song), Some(player), LyricsView::Ready(document)) =
            (self.lyric_seek_stamp(), &self.player, &self.lyrics)
        else {
            return;
        };
        let mut start = 0;
        let mut lines = Vec::new();
        for (index, line) in document.lines.iter().enumerate() {
            if line.is_translation || line.text.trim().is_empty() {
                continue;
            }
            if self.lyric_line.is_some_and(|active| active >= index) {
                start = lines.len();
            }
            lines.push(line.text.clone());
        }
        if lines.is_empty() {
            return;
        }
        let count = 4.min(lines.len());
        self.lyric_poster.invalidate();
        self.lyric_poster.draft = Some(Draft {
            song,
            track_title: player.title.clone(),
            artist: player.artist.clone(),
            start: start.min(lines.len() - count),
            count,
            lines,
        });
        window.focus(&self.lyric_poster.focus);
        cx.notify();
    }

    fn close_lyric_poster(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.lyric_poster.clear();
        self.focus_immersive_lyrics(window);
        cx.notify();
    }

    pub(super) fn render_lyric_poster(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let draft = self.lyric_poster.draft.as_ref()?;
        if self.lyric_seek_stamp() != Some(draft.song) || self.auth.busy {
            self.close_lyric_poster(window, cx);
            return None;
        }
        let state = &self.lyric_poster;
        let label = format!(
            "第 {}–{} 句，共 {} 句",
            draft.start + 1,
            draft.start + draft.count,
            draft.lines.len()
        );
        let active_theme = state.theme;
        Some(
            div()
                .id("lyric-poster-page")
                .key_context("LyricPoster")
                .track_focus(&state.focus)
                .size_full()
                .flex()
                .flex_col()
                .gap_4()
                .p_5()
                .bg(rgb(0x202020))
                .text_color(rgb(0xf8f9ff))
                .on_action(
                    cx.listener(|this, _: &Close, window, cx| this.close_lyric_poster(window, cx)),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_4()
                        .child(
                            crate::ui::icon_button_on_dark("poster-back", "back", "返回歌词")
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.close_lyric_poster(window, cx)
                                })),
                        )
                        .child(
                            div()
                                .text_xl()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(draft.track_title.clone()),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .gap_4()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(div().text_sm().child(label))
                                .child(
                                    div().flex().flex_wrap().gap_2().children(
                                        [
                                            (-1, 0, "向上选句"),
                                            (1, 0, "向下选句"),
                                            (0, -1, "少选一句"),
                                            (0, 1, "多选一句"),
                                        ]
                                        .into_iter()
                                        .map(
                                            |(start, count, label)| {
                                                button(label, label).on_click(cx.listener(
                                                    move |this, _, _, cx| {
                                                        let page = &mut this.lyric_poster;
                                                        if let Some(draft) = &mut page.draft {
                                                            draft.count = draft
                                                                .count
                                                                .saturating_add_signed(count)
                                                                .clamp(
                                                                    1,
                                                                    MAX_LINES
                                                                        .min(draft.lines.len()),
                                                                );
                                                            draft.start = draft
                                                                .start
                                                                .saturating_add_signed(start)
                                                                .min(
                                                                    draft.lines.len() - draft.count,
                                                                );
                                                            page.invalidate();
                                                            cx.notify();
                                                        }
                                                    },
                                                ))
                                            },
                                        ),
                                    ),
                                )
                                .child(
                                    div()
                                        .id("poster-selected-lines")
                                        .overflow_y_scroll()
                                        .min_h_0()
                                        .flex_1()
                                        .children(
                                            draft
                                                .selection()
                                                .iter()
                                                .map(|line| div().py_1().child(line.clone())),
                                        ),
                                )
                                .child(
                                    div().flex().flex_wrap().gap_2().children(
                                        [
                                            (PosterTheme::Cover, "封面取色"),
                                            (PosterTheme::Dark, "深色"),
                                            (PosterTheme::Light, "浅色"),
                                            (PosterTheme::Warm, "暖白"),
                                        ]
                                        .into_iter()
                                        .map(
                                            |(theme, label)| {
                                                button(label, label)
                                                    .when(theme == active_theme, |item| {
                                                        item.bg(rgb(0x465fd9))
                                                    })
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.lyric_poster.theme = theme;
                                                        this.lyric_poster
                                                            .background
                                                            .update(cx, |input, cx| {
                                                                input.clear(cx)
                                                            });
                                                        this.lyric_poster.invalidate();
                                                        cx.notify();
                                                    }))
                                            },
                                        ),
                                    ),
                                )
                                .child(
                                    button(
                                        "poster-custom-style",
                                        if state.show_style {
                                            "隐藏自定义样式"
                                        } else {
                                            "自定义样式"
                                        },
                                    )
                                    .on_click(cx.listener(
                                        |this, _, _, cx| {
                                            this.lyric_poster.show_style =
                                                !this.lyric_poster.show_style;
                                            cx.notify();
                                        },
                                    )),
                                )
                                .when(state.show_style, |panel| {
                                    panel
                                        .children(
                                            [
                                                ("字体族", state.font.clone()),
                                                ("背景色（#RRGGBB）", state.background.clone()),
                                                ("文字色（#RRGGBB）", state.text_color.clone()),
                                            ]
                                            .into_iter()
                                            .map(
                                                |(label, input)| {
                                                    div().text_sm().child(label).child(input)
                                                },
                                            ),
                                        )
                                        .child(
                                            div()
                                                .id("poster-style-pickers")
                                                .max_h(px(200.))
                                                .overflow_y_scroll()
                                                .child(self.render_style_pickers(cx)),
                                        )
                                }),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .overflow_hidden()
                                .when_some(state.preview.clone(), |body, image| {
                                    body.child(
                                        img(image).w_full().h_full().object_fit(ObjectFit::Contain),
                                    )
                                })
                                .when(state.preview.is_none(), |body| {
                                    body.child(if state.busy {
                                        "正在生成预览…"
                                    } else {
                                        "生成后可预览海报"
                                    })
                                }),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_3()
                        .items_center()
                        .child(
                            button("poster-render", "生成预览")
                                .on_click(cx.listener(|this, _, _, cx| this.make_lyric_poster(cx))),
                        )
                        .when(state.preview.is_some(), |row| {
                            row.child(button("poster-copy", "复制 PNG").on_click(cx.listener(
                                |this, _, _, cx| {
                                    if let Some(image) = &this.lyric_poster.preview {
                                        cx.write_to_clipboard(ClipboardItem::new_image(image));
                                        this.lyric_poster.message = Some("已复制");
                                        cx.notify();
                                    }
                                },
                            )))
                            .child(
                                button("poster-save", "保存 PNG").on_click(
                                    cx.listener(|this, _, _, cx| this.save_lyric_poster(cx)),
                                ),
                            )
                        })
                        .when_some(state.message, |row, message| row.child(message)),
                )
                .into_any_element(),
        )
    }

    fn make_lyric_poster(&mut self, cx: &mut Context<Self>) {
        let cover_rgb = self
            .player
            .as_ref()
            .and_then(|player| player.cover_url.as_deref())
            .and_then(|cover| self.artwork.borrow().ambient(cover))
            .map(|palette| {
                let [r, g, b] = palette[1];
                u32::from_be_bytes([0, r, g, b])
            });
        let state = &mut self.lyric_poster;
        let Some(draft) = state.draft.as_ref().filter(|_| !state.busy) else {
            return;
        };
        let (Ok(background_rgb), Ok(text_rgb)) = (
            parse_color(state.background.read(cx).text()),
            parse_color(state.text_color.read(cx).text()),
        ) else {
            state.message = Some("颜色请输入六位十六进制值，例如 #24324A");
            cx.notify();
            return;
        };
        let style = LyricPosterStyle {
            theme: state.theme,
            font_family: state
                .font
                .read(cx)
                .text()
                .trim()
                .chars()
                .take(128)
                .collect(),
            background_rgb,
            text_rgb,
        };
        let lyrics = Lyrics {
            title: draft.track_title.clone(),
            artist: draft.artist.clone(),
            lines: draft.selection().to_vec(),
            style: style.clone(),
            cover_rgb,
        };
        let song = draft.song;
        state.invalidate();
        state.busy = true;
        let _ = self
            .service
            .commands
            .send(Command::SaveLyricPosterStyle(style));
        let request = state.request;
        let work = cx
            .background_executor()
            .spawn(async move { lyric_poster::render(&lyrics) });
        cx.spawn(async move |entity, cx| {
            let rendered = work.await;
            let _ = entity.update(cx, |this, cx| {
                if this.lyric_seek_stamp() != Some(song)
                    || this.lyric_poster.request != request
                    || this.auth.busy
                {
                    return;
                }
                let state = &mut this.lyric_poster;
                state.busy = false;
                match rendered {
                    Ok(bytes) => {
                        state.preview = Some(Arc::new(Image::from_bytes(ImageFormat::Png, bytes)));
                    }
                    Err(_) => state.message = Some("生成海报失败，请重试"),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn save_lyric_poster(&mut self, cx: &mut Context<Self>) {
        let page = &mut self.lyric_poster;
        let (Some(draft), Some(image)) = (&page.draft, &page.preview) else {
            return;
        };
        if page.busy {
            return;
        }
        let song = draft.song;
        let request = page.request;
        let image = Arc::clone(image);
        let folder = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .filter(|path| path.is_dir())
            .unwrap_or_else(|| PathBuf::from("."));
        page.busy = true;
        page.message = None;
        let picker = cx.prompt_for_new_path(&folder, Some(&format!("歌词海报-{}.png", song.1)));
        cx.spawn(async move |entity, cx| {
            let path = picker.await;
            let accepted = entity
                .update(cx, |this, _| {
                    this.lyric_seek_stamp() == Some(song)
                        && this.lyric_poster.request == request
                        && !this.auth.busy
                })
                .unwrap_or(false);
            if !accepted {
                return;
            }
            let message = match path {
                Ok(Ok(Some(path)))
                    if path
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("png")) =>
                {
                    let saved = cx
                        .background_executor()
                        .spawn(async move { lyric_poster::save(&path, &image.bytes) })
                        .await;
                    Some(if saved.is_ok() {
                        "海报已保存"
                    } else {
                        "海报保存失败"
                    })
                }
                Ok(Ok(Some(_))) => Some("请选择以 .png 结尾的文件名"),
                Ok(Ok(None)) => None,
                _ => Some("无法打开保存窗口"),
            };
            let _ = entity.update(cx, |this, cx| {
                if this.lyric_poster.request == request {
                    this.lyric_poster.busy = false;
                    this.lyric_poster.message = message;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }
}

fn parse_color(value: &str) -> Result<Option<u32>, ()> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let value = value.strip_prefix('#').unwrap_or(value);
    if value.len() != 6 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(());
    }
    u32::from_str_radix(value, 16).map(Some).map_err(|_| ())
}

fn button(id: &'static str, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .text_sm()
        .bg(rgb(0x303030))
        .hover(|item| item.bg(rgb(0x454545)))
        .child(label)
}
