//! Shared visual primitives for the Fluent desktop interface.
use gpui::{App, Div, Rgba, Stateful, Svg, div, prelude::*, px};
use std::borrow::Cow;

pub struct Assets;
impl gpui::AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let bytes: &'static [u8] = match path {
            "icons/home.svg" => include_bytes!("../assets/icons/home.svg"),
            "icons/grid.svg" => include_bytes!("../assets/icons/grid.svg"),
            "icons/chart.svg" => include_bytes!("../assets/icons/chart.svg"),
            "icons/heart.svg" => include_bytes!("../assets/icons/heart.svg"),
            "icons/clock.svg" => include_bytes!("../assets/icons/clock.svg"),
            "icons/list.svg" => include_bytes!("../assets/icons/list.svg"),
            "icons/music.svg" => include_bytes!("../assets/icons/music.svg"),
            "icons/settings.svg" => include_bytes!("../assets/icons/settings.svg"),
            "icons/back.svg" => include_bytes!("../assets/icons/back.svg"),
            "icons/search.svg" => include_bytes!("../assets/icons/search.svg"),
            "icons/sun.svg" => include_bytes!("../assets/icons/sun.svg"),
            "icons/play.svg" => include_bytes!("../assets/icons/play.svg"),
            "icons/pause.svg" => include_bytes!("../assets/icons/pause.svg"),
            "icons/next.svg" => include_bytes!("../assets/icons/next.svg"),
            "icons/previous.svg" => include_bytes!("../assets/icons/previous.svg"),
            "icons/shuffle.svg" => include_bytes!("../assets/icons/shuffle.svg"),
            "icons/more.svg" => include_bytes!("../assets/icons/more.svg"),
            "icons/volume.svg" => include_bytes!("../assets/icons/volume.svg"),
            "icons/close.svg" => include_bytes!("../assets/icons/close.svg"),
            "icons/share.svg" => include_bytes!("../assets/icons/share.svg"),
            "icons/edit.svg" => include_bytes!("../assets/icons/edit.svg"),
            "icons/delete.svg" => include_bytes!("../assets/icons/delete.svg"),
            "icons/collapse.svg" => include_bytes!("../assets/icons/collapse.svg"),
            "icons/translate.svg" => include_bytes!("../assets/icons/translate.svg"),
            "icons/add.svg" => include_bytes!("../assets/icons/add.svg"),
            "icons/refresh.svg" => include_bytes!("../assets/icons/refresh.svg"),
            "icons/arrow-right.svg" => include_bytes!("../assets/icons/arrow-right.svg"),
            _ => return Ok(None),
        };
        Ok(Some(Cow::Borrowed(bytes)))
    }
    fn list(&self, _: &str) -> gpui::Result<Vec<gpui::SharedString>> {
        Ok(Vec::new())
    }
}
pub fn icon(name: &str, color: Rgba) -> Svg {
    gpui::svg()
        .path(format!("icons/{name}.svg"))
        .size(px(19.))
        .text_color(color)
}
pub fn icon_button(id: impl Into<gpui::ElementId>, name: &str, cx: &App) -> Stateful<Div> {
    let label = match name {
        "previous" => "上一首",
        "next" => "下一首",
        "shuffle" => "切换播放模式",
        "heart" => "喜欢 / 取消喜欢",
        "more" => "更多播放选项",
        "list" => "播放队列",
        "sun" => "切换深浅主题",
        "chart" => "热门搜索",
        "clock" => "搜索历史",
        "search" => "搜索",
        "back" => "返回",
        "close" => "关闭",
        "play" => "播放",
        "pause" => "暂停",
        "share" => "分享",
        "edit" => "编辑",
        "delete" => "删除",
        "add" => "新建",
        "collapse" => "收起",
        "translate" => "译文",
        _ => "操作",
    };
    icon_button_labeled(id, name, label, cx)
}

pub fn icon_button_labeled(
    id: impl Into<gpui::ElementId>,
    name: &str,
    label: &'static str,
    cx: &App,
) -> Stateful<Div> {
    let rgb = crate::appearance::paint(cx);
    icon_control(id, name, label, rgb(0x20293a), rgb(0xe9edf5))
}

pub fn icon_button_on_dark(
    id: impl Into<gpui::ElementId>,
    name: &str,
    label: &'static str,
) -> Stateful<Div> {
    icon_control(id, name, label, gpui::rgb(0xf8f9ff), gpui::rgb(0x454545))
}

fn icon_control(
    id: impl Into<gpui::ElementId>,
    name: &str,
    label: &'static str,
    ink: Rgba,
    hover: Rgba,
) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(34.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .cursor_pointer()
        .hover(move |style| style.bg(hover))
        .child(icon(name, ink))
        .tooltip(move |_, cx| cx.new(|_| Hint(label)).into())
}

/// Primary action with an explicit tooltip and a disabled state.
pub fn primary_icon_button(
    id: impl Into<gpui::ElementId>,
    name: &str,
    label: &'static str,
    enabled: bool,
    cx: &App,
) -> Stateful<Div> {
    let rgb = crate::appearance::paint(cx);
    div()
        .id(id)
        .size(px(36.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .bg(rgb(0x4d66e5))
        .child(icon(name, gpui::rgb(0xffffff)))
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(|style| style.bg(rgb(0x3854d5)))
        })
        .when(!enabled, |button| button.opacity(0.45))
        .tooltip(move |_, cx| cx.new(|_| Hint(label)).into())
}

pub fn eyebrow(label: &'static str, cx: &App) -> Div {
    div()
        .text_size(px(10.))
        .text_color(crate::appearance::paint(cx)(0x8a91a2))
        .child(label)
}
pub fn heading(label: impl Into<gpui::SharedString>, cx: &App) -> Div {
    div()
        .text_size(px(29.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(crate::appearance::paint(cx)(0x20293a))
        .child(label.into())
}
pub fn section_label(label: &'static str, cx: &App) -> Div {
    div()
        .px_3()
        .pt_5()
        .pb_2()
        .text_size(px(11.))
        .text_color(crate::appearance::paint(cx)(0x8a91a2))
        .child(label)
}

/// Pointer hit area stays larger than the visible track.
pub fn slider(
    fraction: f64,
    rgb: impl Fn(u32) -> Rgba + Copy,
    adjust: impl Fn(f64, &mut App) + 'static,
) -> Div {
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
                .h(px(3.))
                .rounded_full()
                .bg(rgb(0xd8dce6))
                .child(
                    div()
                        .h_full()
                        .w(gpui::relative(fraction.clamp(0., 1.) as f32))
                        .rounded_full()
                        .bg(rgb(0x415de5)),
                ),
        )
        .child(
            gpui::canvas(
                |bounds, _, _| bounds,
                move |_, bounds, window, _| {
                    let adjust = std::rc::Rc::new(adjust);
                    let drag = adjust.clone();
                    window.on_mouse_event(move |event: &gpui::MouseMoveEvent, phase, _, cx| {
                        if phase == gpui::DispatchPhase::Bubble
                            && event.dragging()
                            && bounds.contains(&event.position)
                            && bounds.size.width > px(0.)
                        {
                            drag(
                                f64::from(
                                    ((event.position.x - bounds.origin.x) / bounds.size.width)
                                        .clamp(0., 1.),
                                ),
                                cx,
                            );
                        }
                    });
                    window.on_mouse_event(move |event: &gpui::MouseDownEvent, phase, _, cx| {
                        if phase == gpui::DispatchPhase::Bubble
                            && event.button == gpui::MouseButton::Left
                            && bounds.contains(&event.position)
                            && bounds.size.width > px(0.)
                        {
                            adjust(
                                f64::from(
                                    ((event.position.x - bounds.origin.x) / bounds.size.width)
                                        .clamp(0., 1.),
                                ),
                                cx,
                            );
                        }
                    });
                },
            )
            .absolute()
            .inset_0(),
        )
}

pub fn logo() -> gpui::Img {
    static IMAGE: std::sync::OnceLock<std::sync::Arc<gpui::Image>> = std::sync::OnceLock::new();
    gpui::img(
        IMAGE
            .get_or_init(|| {
                std::sync::Arc::new(gpui::Image::from_bytes(
                    gpui::ImageFormat::Png,
                    include_bytes!("../../../assets/bo_music.png").to_vec(),
                ))
            })
            .clone(),
    )
    .size(px(30.))
    .flex_shrink_0()
    .rounded_lg()
    .object_fit(gpui::ObjectFit::Contain)
}

pub fn hero_illustration() -> gpui::Img {
    static IMAGE: std::sync::OnceLock<std::sync::Arc<gpui::Image>> = std::sync::OnceLock::new();
    gpui::img(
        IMAGE
            .get_or_init(|| {
                std::sync::Arc::new(gpui::Image::from_bytes(
                    gpui::ImageFormat::Png,
                    include_bytes!("../assets/hero-record.png").to_vec(),
                ))
            })
            .clone(),
    )
    .w_full()
    .h_full()
    .object_fit(gpui::ObjectFit::Contain)
}

struct Hint(&'static str);
impl gpui::Render for Hint {
    fn render(&mut self, _: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let rgb = crate::appearance::paint(cx);
        div()
            .px_3()
            .py_2()
            .rounded_md()
            .border_1()
            .border_color(rgb(0xd8dce6))
            .shadow_md()
            .bg(rgb(0xffffff))
            .text_color(rgb(0x20293a))
            .text_size(px(12.))
            .child(self.0)
    }
}
