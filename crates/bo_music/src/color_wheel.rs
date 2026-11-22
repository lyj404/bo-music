use gpui::{
    App, Bounds, DispatchPhase, Div, MouseButton, MouseDownEvent, Pixels, RenderImage, canvas, div,
    img, prelude::*, px, relative, rgb,
};
use std::sync::{Arc, OnceLock};

const EDGE: u32 = 128;

#[derive(Clone, Copy)]
pub(super) struct Wheel {
    hue: f32,
    saturation: f32,
    value: f32,
}

impl Wheel {
    pub(super) fn from_rgb(rgb: u32) -> Self {
        let [_, red, green, blue] = rgb.to_be_bytes();
        let [r, g, b] = [red, green, blue].map(|channel| f32::from(channel) / 255.);
        let value = r.max(g).max(b);
        let chroma = value - r.min(g).min(b);
        let hue = if chroma == 0. {
            0.
        } else if value == r {
            (g - b) / chroma
        } else if value == g {
            2. + (b - r) / chroma
        } else {
            4. + (r - g) / chroma
        };
        Self {
            hue: (hue / 6.).rem_euclid(1.),
            saturation: if value == 0. { 0. } else { chroma / value },
            value,
        }
    }

    pub(super) fn rgb(self) -> u32 {
        let chroma = self.value * self.saturation;
        let hue = self.hue * 6.;
        let next = chroma * (1. - (hue.rem_euclid(2.) - 1.).abs());
        let [r, g, b] = match hue as u8 {
            0 => [chroma, next, 0.],
            1 => [next, chroma, 0.],
            2 => [0., chroma, next],
            3 => [0., next, chroma],
            4 => [next, 0., chroma],
            _ => [chroma, 0., next],
        }
        .map(|channel| ((channel + self.value - chroma) * 255.).round() as u8);
        u32::from_be_bytes([0, r, g, b])
    }

    fn point(self, x: f32, y: f32) -> Option<Self> {
        let (x, y) = (x * 2. - 1., y * 2. - 1.);
        let saturation = x.hypot(y);
        (saturation <= 1.).then(|| Self {
            hue: (y.atan2(x) / std::f32::consts::TAU).rem_euclid(1.),
            saturation,
            ..self
        })
    }
}

fn texture() -> Arc<RenderImage> {
    static IMAGE: OnceLock<Arc<RenderImage>> = OnceLock::new();
    IMAGE
        .get_or_init(|| {
            let pixels = image::RgbaImage::from_fn(EDGE, EDGE, |x, y| {
                let wheel = Wheel {
                    hue: 0.,
                    saturation: 0.,
                    value: 1.,
                };
                let Some(color) = wheel.point(
                    (x as f32 + 0.5) / EDGE as f32,
                    (y as f32 + 0.5) / EDGE as f32,
                ) else {
                    return image::Rgba([0, 0, 0, 0]);
                };
                let [_, r, g, b] = color.rgb().to_be_bytes();
                image::Rgba([b, g, r, 255])
            });
            Arc::new(RenderImage::new([image::Frame::new(pixels)]))
        })
        .clone()
}

pub(super) fn render(wheel: Wheel, change: impl Fn(Wheel, &mut App) + Clone + 'static) -> Div {
    let angle = wheel.hue * std::f32::consts::TAU;
    let point = |axis| (axis * wheel.saturation + 1.) * EDGE as f32 / 2. - 4.;
    let color = wheel.rgb();
    let select = change.clone();
    div()
        .flex()
        .gap_3()
        .items_center()
        .child(
            div()
                .relative()
                .size(px(EDGE as f32))
                .cursor_pointer()
                .child(img(texture()).size_full())
                .child(
                    div()
                        .absolute()
                        .size(px(8.))
                        .left(px(point(angle.cos())))
                        .top(px(point(angle.sin())))
                        .rounded_full()
                        .border_2()
                        .border_color(rgb(0xffffff))
                        .bg(rgb(color)),
                )
                .child(hit(move |x, y, cx| {
                    if let Some(next) = wheel.point(x, y) {
                        select(next, cx);
                    }
                })),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .text_sm()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().size(px(20.)).rounded_md().bg(rgb(color)))
                        .child(format!("#{color:06X}")),
                )
                .child("亮度")
                .child(
                    div()
                        .relative()
                        .cursor_pointer()
                        .w(px(100.))
                        .h(px(14.))
                        .rounded_full()
                        .overflow_hidden()
                        .bg(rgb(0x495369))
                        .child(div().bg(rgb(0xaebaff)).w(relative(wheel.value)).h_full())
                        .child(hit(move |value, _, cx| {
                            change(Wheel { value, ..wheel }, cx)
                        })),
                ),
        )
}

fn hit(f: impl Fn(f32, f32, &mut App) + 'static) -> impl IntoElement {
    canvas(
        |bounds, _, _| bounds,
        move |_, bounds: Bounds<Pixels>, window, _| {
            window.on_mouse_event(move |event: &MouseDownEvent, phase, _, cx| {
                if phase == DispatchPhase::Bubble
                    && event.button == MouseButton::Left
                    && bounds.contains(&event.position)
                    && bounds.size.width > px(0.)
                    && bounds.size.height > px(0.)
                {
                    let x = (event.position.x - bounds.origin.x) / bounds.size.width;
                    let y = (event.position.y - bounds.origin.y) / bounds.size.height;
                    f(x.clamp(0., 1.), y.clamp(0., 1.), cx);
                }
            });
        },
    )
    .absolute()
    .inset_0()
}

#[cfg(test)]
#[path = "color_wheel_tests.rs"]
mod tests;
