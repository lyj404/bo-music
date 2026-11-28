//! Shared visual primitives for the Fluent desktop interface.
use gpui::{App, Div, Rgba, Stateful, Svg, div, prelude::*, px};
use std::borrow::Cow;

pub struct Assets;
impl gpui::AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let bytes: &'static [u8] = match path {
            "icons/lyrics.svg" => include_bytes!("../assets/icons/lyrics.svg"),
            "icons/mini-player.svg" => include_bytes!("../assets/icons/mini-player.svg"),
            "icons/desktop-lyrics.svg" => include_bytes!("../assets/icons/desktop-lyrics.svg"),
            "icons/lock.svg" => include_bytes!("../assets/icons/lock.svg"),
            "icons/equalizer.svg" => include_bytes!("../assets/icons/equalizer.svg"),
            "icons/quality.svg" => include_bytes!("../assets/icons/quality.svg"),
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
            "icons/play-sequential.svg" => include_bytes!("../assets/icons/play-sequential.svg"),
            "icons/repeat.svg" => include_bytes!("../assets/icons/repeat.svg"),
            "icons/repeat-one.svg" => include_bytes!("../assets/icons/repeat-one.svg"),
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
        "refresh" => "刷新",
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
    let palette = crate::appearance::palette(cx);
    let (ink, hover) = match name {
        "delete" => (palette.danger, palette.danger_hover),
        "heart" => (palette.accent, palette.accent_hover),
        "add" | "edit" | "share" | "refresh" => (palette.action, palette.hover),
        _ => (palette.icon, palette.hover),
    };
    icon_control(id, name, label, ink, hover)
}

pub fn icon_button_on_dark(
    id: impl Into<gpui::ElementId>,
    name: &str,
    label: &'static str,
) -> Stateful<Div> {
    icon_control(id, name, label, gpui::rgb(0xb5c3d6), gpui::rgb(0x38383d))
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
        .text_color(ink)
        .hover(move |style| style.bg(hover))
        .active(|style| style.opacity(0.65))
        // GPUI Svg::paint requires its own color; parent text color is not inherited.
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
        .size(px(40.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(rgb(0x4d66e5))
        .text_color(gpui::rgb(0xffffff))
        .child(icon(name, gpui::rgb(0xffffff)))
        .when(enabled, |button| {
            button
                .cursor_pointer()
                .hover(|style| style.bg(rgb(0x3854d5)))
                .active(|style| style.opacity(0.85))
        })
        .when(!enabled, |button| button.cursor_default().opacity(0.45))
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
        .text_size(px(27.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(crate::appearance::paint(cx)(0x20293a))
        .child(label.into())
}
/// Consistent loading and empty-content treatment across browsing pages.
pub fn empty_state(title: &'static str, detail: &'static str, cx: &App) -> Div {
    let colors = crate::appearance::palette(cx);
    div()
        .w_full()
        .py_6()
        .px_4()
        .flex()
        .flex_col()
        .items_center()
        .gap_2()
        .text_color(colors.muted)
        .child(icon("music", colors.muted).size(px(28.)))
        .child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::MEDIUM)
                .child(title),
        )
        .child(div().text_size(px(12.)).child(detail))
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
                .h(px(4.))
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
