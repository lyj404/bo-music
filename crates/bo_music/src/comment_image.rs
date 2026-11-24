use super::MusicApp;
use bo_music_app::desktop_service::Command;
use gpui::{
    AnyElement, ClipboardItem, Context, Div, Image, ImageFormat, Stateful, div, prelude::*, px,
};
use std::{path::PathBuf, sync::Arc};

#[derive(Default)]
pub(super) struct CommentImage {
    request: u64,
    target: Option<(u64, i64, bool)>,
    image: Option<Arc<Image>>,
    message: Option<&'static str>,
    saving: bool,
}

impl CommentImage {
    pub(super) fn is_open(&self) -> bool {
        self.target.is_some()
    }

    pub(super) fn clear(&mut self, cx: &mut Context<MusicApp>) {
        if !self.is_open() {
            return;
        }
        if let Some(image) = self.image.take() {
            image.remove_asset(cx);
        }
        *self = Self {
            request: self.request.wrapping_add(1),
            ..Self::default()
        };
    }

    pub(super) fn accept(&mut self, request: u64, result: Result<Arc<Vec<u8>>, ()>) {
        if self.is_open() && self.request == request {
            match result {
                Ok(bytes) => {
                    self.image = Some(Arc::new(Image::from_bytes(
                        ImageFormat::Png,
                        bytes.as_ref().clone(),
                    )))
                }
                Err(()) => self.message = Some("图片加载失败，请重试"),
            }
        }
    }

    pub(super) fn saved(&mut self, request: u64, saved: bool) {
        if self.is_open() && self.request == request {
            self.saving = false;
            self.message = Some(if saved {
                "图片已保存"
            } else {
                "图片保存失败，请重试"
            });
        }
    }
}

impl MusicApp {
    pub(super) fn open_comment_image(
        &mut self,
        generation: u64,
        id: i64,
        poster: bool,
        cx: &mut Context<Self>,
    ) {
        let state = &mut self.comment_panel.image;
        state.clear(cx);
        state.request = state.request.wrapping_add(1);
        state.target = Some((generation, id, poster));
        if self
            .service
            .commands
            .send(Command::LoadCommentImage(
                state.request,
                generation,
                id,
                poster,
            ))
            .is_err()
        {
            state.message = Some("图片加载失败，请重试");
        }
        cx.notify();
    }

    pub(super) fn render_comment_image(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = &self.comment_panel.image;
        let rgb = crate::appearance::paint(cx);
        let body = div()
            .id("comment-image-view")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_3()
            .child(
                crate::ui::icon_button_labeled("comment-image-back", "back", "返回评论", cx)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.comment_panel.image.clear(cx);
                        cx.notify();
                    })),
            );
        let Some(image) = state.image.clone() else {
            return body
                .child(div().py_2().child(state.message.unwrap_or(
                    if state.target.is_some_and(|(_, _, poster)| poster) {
                        "正在生成分享海报…"
                    } else {
                        "正在加载图片…"
                    },
                )))
                .when_some(
                    state.target.filter(|_| state.message.is_some()),
                    |body, (generation, id, poster)| {
                        body.child(button("comment-image-retry", "重试", rgb).on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.open_comment_image(generation, id, poster, cx);
                            }),
                        ))
                    },
                )
                .into_any_element();
        };
        let preview_height = if state.target.is_some_and(|(_, _, poster)| poster) {
            540.
        } else {
            340.
        };
        body.child(
            div()
                .my_3()
                .rounded_lg()
                .overflow_hidden()
                .child(gpui::img(image).w_full().h(px(preview_height))),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_3()
                .child(
                    button("comment-image-copy", "复制 PNG", rgb).on_click(cx.listener(
                        |this, _, _, cx| {
                            if let Some(image) = &this.comment_panel.image.image {
                                cx.write_to_clipboard(ClipboardItem::new_image(image));
                                this.comment_panel.image.message = Some("图片已复制");
                                cx.notify();
                            }
                        },
                    )),
                )
                .child(
                    button(
                        "comment-image-save",
                        if state.saving {
                            "正在保存…"
                        } else {
                            "保存 PNG"
                        },
                        rgb,
                    )
                    .on_click(cx.listener(|this, _, _, cx| this.save_comment_image(cx))),
                ),
        )
        .when_some(state.message, |body, message| {
            body.child(div().py_2().child(message))
        })
        .into_any_element()
    }

    fn save_comment_image(&mut self, cx: &mut Context<Self>) {
        let state = &mut self.comment_panel.image;
        let Some((_, id, poster)) = state.target else {
            return;
        };
        if state.saving || state.image.is_none() {
            return;
        }
        state.saving = true;
        state.message = None;
        let request = state.request;
        let folder = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .filter(|path| path.is_dir())
            .unwrap_or_else(|| PathBuf::from("."));
        let label = if poster {
            "评论海报"
        } else {
            "评论图片"
        };
        let picker = cx.prompt_for_new_path(&folder, Some(&format!("{label}-{id}.png")));
        cx.spawn(async move |entity, cx| {
            let selection = picker.await;
            let _ = entity.update(cx, |this, cx| {
                let state = &mut this.comment_panel.image;
                if state.request != request || !state.is_open() || !this.panels.show_comments {
                    return;
                }
                if let Ok(Ok(Some(path))) = &selection {
                    if path
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
                    {
                        if this
                            .service
                            .commands
                            .send(Command::SaveCommentImage(request, path.clone()))
                            .is_ok()
                        {
                            return;
                        }
                        state.message = Some("图片保存失败，请重试");
                    } else {
                        state.message = Some("请选择以 .png 结尾的文件名");
                    }
                } else if !matches!(selection, Ok(Ok(None))) {
                    state.message = Some("无法打开保存窗口，请重试");
                }
                state.saving = false;
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

fn button(
    id: &'static str,
    text: &'static str,
    rgb: impl Fn(u32) -> gpui::Rgba + Copy,
) -> Stateful<Div> {
    div()
        .id(id)
        .cursor_pointer()
        .px_3()
        .py_2()
        .rounded_md()
        .bg(rgb(0xe9edf5))
        .text_color(rgb(0x415de5))
        .hover(|item| item.bg(rgb(0xe9edff)))
        .child(text)
}
