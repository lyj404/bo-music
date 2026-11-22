use super::*;

impl MusicApp {
    pub(super) fn render_playlist_categories(
        &self,
        view: &BrowseView,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        let mut panel = div()
            .id("playlist-square-categories")
            .max_h(px(150.))
            .overflow_y_scroll()
            .pb_2();
        if let Some(square) = &view.square {
            let generation = view.generation;
            for (index, group) in square.groups.iter().enumerate() {
                let categories = group
                    .categories
                    .iter()
                    .enumerate()
                    .map(|(tag_index, tag)| {
                        let id = tag.id;
                        let selected = square.category == Some(id);
                        div()
                            .id(("playlist-square-tag", tag_index))
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .text_color(rgb(if selected { 0x415de5 } else { 0x687083 }))
                            .bg(rgb(if selected { 0xe9edf5 } else { 0xffffff }))
                            .child(tag.name.clone())
                            .on_click(cx.listener(move |this, _, _, _| {
                                if this.page == Page::PlaylistSquare {
                                    let _ = this
                                        .service
                                        .commands
                                        .send(Command::SelectPlaylistCategory(id, generation));
                                }
                            }))
                    })
                    .collect::<Vec<_>>();
                panel = panel.child(
                    div()
                        .id(("playlist-square-group", index))
                        .pb_2()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::BOLD)
                                .child(group.name.clone()),
                        )
                        .child(div().flex().flex_wrap().gap_1().children(categories)),
                );
            }
        }
        panel.into_any_element()
    }

    pub(super) fn close_comments(&mut self, cx: &mut Context<Self>) {
        if self.show_comments {
            self.show_comments = false;
            self.comment_image.clear(cx);
            let _ = self.service.commands.send(Command::CloseComments);
        }
    }

    pub(super) fn clear_playlist_cover(&mut self, cx: &mut Context<Self>) {
        self.playlist_editor_generation = self.playlist_editor_generation.wrapping_add(1);
        if let Some(preview) = self.playlist_cover_preview.take() {
            preview.remove_asset(cx);
        }
        self.playlist_cover_path = None;
        self.playlist_cover_image = None;
        self.playlist_cover_crop = CoverCrop::default();
        self.playlist_cover_loading = false;
        self.playlist_cover_request = self.playlist_cover_request.wrapping_add(1);
        self.playlist_cover_drag = None;
    }

    pub(super) fn cover(
        &self,
        address: Option<&str>,
        size: f32,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        let address = address
            .filter(|address| !address.is_empty())
            .map(str::to_owned);
        let image = address
            .as_deref()
            .and_then(|address| self.artwork.borrow().cached(address));
        let entity = cx.entity().downgrade();
        div()
            .relative()
            .w(px(size))
            .h(px(size))
            .flex_shrink_0()
            .rounded_md()
            .overflow_hidden()
            .bg(rgb(0xe9edf5))
            .when(image.is_none(), |element| {
                element
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(rgb(0x8a91a2))
                    .child(ui::icon("music", rgb(0x8a91a2)))
            })
            .when_some(image, |element, image| {
                element.child(
                    gpui::img(image)
                        .w(px(size))
                        .h(px(size))
                        .object_fit(gpui::ObjectFit::Cover)
                        .rounded_lg(),
                )
            })
            .when_some(address, |element, address| {
                element.child(
                    canvas(
                        move |bounds, window, cx| {
                            if bounds.intersects(&window.content_mask().bounds) {
                                window.defer(cx, move |_, cx| {
                                    let _ = entity.update(cx, |this, cx| {
                                        this.artwork.borrow_mut().get(&address, cx, |address| {
                                            this.service
                                                .commands
                                                .send(Command::LoadArtwork(address))
                                                .is_ok()
                                        });
                                    });
                                });
                            }
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .inset_0(),
                )
            })
            .into_any_element()
    }

    pub(super) fn playlist_cover_cropper(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        let (Some(cover), Some(preview)) =
            (&self.playlist_cover_image, &self.playlist_cover_preview)
        else {
            return div().into_any_element();
        };
        const VIEWPORT: f32 = 200.0;
        let (left, top, side) = self
            .playlist_cover_crop
            .source_rect(cover.width, cover.height);
        if side == 0 {
            return div().into_any_element();
        }
        let scale = VIEWPORT / side as f32;
        div()
            .flex()
            .items_center()
            .gap_4()
            .child(
                div()
                    .id("playlist-cover-cropper")
                    .relative()
                    .w(px(VIEWPORT))
                    .h(px(VIEWPORT))
                    .overflow_hidden()
                    .rounded_md()
                    .bg(rgb(0xd8dce6))
                    .cursor_pointer()
                    .child(
                        gpui::img(Arc::clone(preview))
                            .absolute()
                            .left(px(-(left as f32) * scale))
                            .top(px(-(top as f32) * scale))
                            .w(px(cover.width as f32 * scale))
                            .h(px(cover.height as f32 * scale)),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &gpui::MouseDownEvent, _, _| {
                            if !this.playlist_busy {
                                this.playlist_cover_drag = Some(event.position);
                            }
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, event: &gpui::MouseMoveEvent, _, cx| {
                        if let Some(last) = this.playlist_cover_drag
                            && let Some(image) = &this.playlist_cover_image
                        {
                            let dx = f32::from(event.position.x - last.x);
                            let dy = f32::from(event.position.y - last.y);
                            this.playlist_cover_crop.pan(
                                image.width,
                                image.height,
                                dx,
                                dy,
                                VIEWPORT,
                            );
                            this.playlist_cover_drag = Some(event.position);
                            cx.notify();
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, _| {
                            this.playlist_cover_drag = None;
                        }),
                    )
                    .on_mouse_up_out(
                        MouseButton::Left,
                        cx.listener(|this, _, _, _| {
                            this.playlist_cover_drag = None;
                        }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child("拖动图片调整裁剪区域")
                    .child(format!(
                        "缩放：{:.0}%",
                        self.playlist_cover_crop.zoom * 100.0
                    ))
                    .child(
                        div().flex().gap_3().children(
                            [(-0.25_f32, "缩小"), (0.25_f32, "放大")]
                                .into_iter()
                                .enumerate()
                                .map(|(index, (delta, label))| {
                                    div()
                                        .id(("playlist-cover-zoom", index))
                                        .cursor_pointer()
                                        .text_color(rgb(0x415de5))
                                        .child(label)
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            if !this.playlist_busy {
                                                this.playlist_cover_crop.change_zoom(delta);
                                                cx.notify();
                                            }
                                        }))
                                })
                                .collect::<Vec<_>>(),
                        ),
                    )
                    .child(
                        div()
                            .id("playlist-cover-reset")
                            .cursor_pointer()
                            .text_color(rgb(0x415de5))
                            .child("重置裁剪")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if !this.playlist_busy {
                                    this.playlist_cover_crop = CoverCrop::default();
                                    cx.notify();
                                }
                            })),
                    ),
            )
            .into_any_element()
    }
}
