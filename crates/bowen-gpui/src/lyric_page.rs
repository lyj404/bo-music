use super::{MusicApp, clock};
use bowen_app::artwork::{AmbientPalette, render_ambient};
use bowen_app::desktop_service::{Command, LyricsView};
use bowen_domain::lyrics::LyricKind;
use bowen_playback::mpv::PlaybackState;
use bowen_playback::spectrum::BAND_COUNT;
use bowen_playback::spectrum_output::{OutputSpectrum, OutputSpectrumFrame};
use gpui::{
    AnyElement, App, Context, Div, Entity, FocusHandle, Focusable, FontWeight, Image, ImageFormat,
    KeyBinding, ObjectFit, RenderImage, ScrollHandle, Stateful, Task, Window, actions, div, img,
    prelude::*, px, rgb,
};
use std::sync::Arc;
use std::time::{Duration, Instant};

actions!(lyrics, [CloseImmersive, ToggleFullscreen]);

const BACKDROP_FADE: Duration = Duration::from_millis(600);
const BACKDROP_BASE: u32 = 0x171c32;
const BASE_PALETTE: AmbientPalette = [[23, 28, 50]; 3];

#[derive(Default)]
struct SpectrumView {
    output: OutputSpectrum,
    playing: bool,
    retry: Option<Task<()>>,
}

impl SpectrumView {
    fn set_playing(&mut self, playing: bool, cx: &mut Context<Self>) {
        if self.playing == playing {
            return;
        }
        self.playing = playing;
        if !playing {
            self.output.stop();
            self.retry = None;
        }
        cx.notify();
    }
}

impl Render for SpectrumView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let levels = if self.playing {
            match self.output.frame() {
                OutputSpectrumFrame::Levels(levels) => {
                    window.request_animation_frame();
                    levels
                }
                OutputSpectrumFrame::Waiting => {
                    window.request_animation_frame();
                    [0.0; BAND_COUNT]
                }
                OutputSpectrumFrame::Unavailable => {
                    if self.retry.is_none() {
                        self.retry = Some(cx.spawn(async move |view, cx| {
                            cx.background_executor().timer(Duration::from_secs(1)).await;
                            let _ = view.update(cx, |view, cx| {
                                view.retry = None;
                                cx.notify();
                            });
                        }));
                    }
                    return div().h(px(42.)).px_6().text_sm().child("频谱采集不可用");
                }
                OutputSpectrumFrame::Unsupported => return div(),
            }
        } else {
            [0.0; BAND_COUNT]
        };
        div()
            .flex()
            .items_end()
            .gap(px(2.))
            .mx_6()
            .h(px(42.))
            .children(levels.map(|level| {
                div()
                    .flex_1()
                    .h(px(2. + 40. * level))
                    .rounded_sm()
                    .bg(rgb(0xaab7ff))
            }))
    }
}

#[derive(Default)]
struct LyricBackdrop {
    palette: Option<AmbientPalette>,
    image: Option<Arc<RenderImage>>,
    fade: Option<BackdropFade>,
}

struct BackdropFade {
    from: Option<AmbientPalette>,
    image: Option<Arc<RenderImage>>,
    start: Instant,
}

impl BackdropFade {
    fn opacity(&self, now: Instant) -> f32 {
        let t =
            (now.duration_since(self.start).as_secs_f32() / BACKDROP_FADE.as_secs_f32()).min(1.);
        t * t * (3. - 2. * t)
    }

    fn visible_palette(&self, target: Option<AmbientPalette>, now: Instant) -> AmbientPalette {
        let opacity = self.opacity(now);
        let from = self.from.unwrap_or(BASE_PALETTE);
        let to = target.unwrap_or(BASE_PALETTE);
        std::array::from_fn(|color| {
            std::array::from_fn(|channel| {
                let a = f32::from(from[color][channel]);
                let b = f32::from(to[color][channel]);
                (a + (b - a) * opacity).round() as u8
            })
        })
    }
}

impl LyricBackdrop {
    fn clear(&mut self, window: &mut Window) {
        if let Some(fade) = self.fade.take() {
            release_backdrop(fade.image, window);
        }
        release_backdrop(self.image.take(), window);
        self.palette = None;
    }

    fn render(&mut self, palette: Option<AmbientPalette>, window: &mut Window, cx: &App) -> Div {
        let now = Instant::now();
        if self
            .fade
            .as_ref()
            .is_some_and(|fade| now - fade.start >= BACKDROP_FADE)
            && let Some(fade) = self.fade.take()
        {
            release_backdrop(fade.image, window);
        }

        if palette != self.palette {
            let mut from = self.palette;
            let mut previous = self.image.take();
            if let Some(fade) = self.fade.take() {
                from = Some(fade.visible_palette(self.palette, now));
                release_backdrop(fade.image, window);
                release_backdrop(previous, window);
                previous = backdrop_image(from, cx);
            }
            self.fade = Some(BackdropFade {
                from,
                image: previous,
                start: now,
            });
            self.palette = palette;
            self.image = backdrop_image(palette, cx);
        }

        let mut background = div().absolute().inset_0();
        if let Some(fade) = &self.fade {
            window.request_animation_frame();
            background = background.child(backdrop_layer(fade.image.clone()));
            background.child(backdrop_layer(self.image.clone()).opacity(fade.opacity(now)))
        } else {
            background.child(backdrop_layer(self.image.clone()))
        }
    }
}

fn backdrop_image(palette: Option<AmbientPalette>, cx: &App) -> Option<Arc<RenderImage>> {
    let png = render_ambient(palette?)?;
    Image::from_bytes(ImageFormat::Png, png)
        .to_image_data(cx.svg_renderer())
        .ok()
}

fn release_backdrop(image: Option<Arc<RenderImage>>, window: &mut Window) {
    if let Some(image) = image {
        let _ = window.drop_image(image);
    }
}

fn backdrop_layer(image: Option<Arc<RenderImage>>) -> Div {
    let layer = div().absolute().inset_0();
    match image {
        Some(image) => layer.child(img(image).size_full().object_fit(ObjectFit::Fill)),
        None => layer.bg(rgb(BACKDROP_BASE)),
    }
}

pub(super) fn register_bindings(cx: &mut App) {
    let scope = Some("ImmersiveLyrics");
    cx.bind_keys([
        KeyBinding::new("escape", CloseImmersive, scope),
        KeyBinding::new("f11", ToggleFullscreen, scope),
    ]);
}

pub(super) struct LyricPage {
    pub open: bool,
    focus: FocusHandle,
    scroll: ScrollHandle,
    followed: Option<usize>,
    manual_until: Option<Instant>,
    translations: bool,
    font_size: f32,
    restore_fullscreen: bool,
    backdrop: LyricBackdrop,
    spectrum: Entity<SpectrumView>,
}

impl LyricPage {
    pub fn new(cx: &mut Context<MusicApp>) -> Self {
        Self {
            open: false,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            followed: None,
            manual_until: None,
            translations: true,
            font_size: 30.,
            restore_fullscreen: false,
            backdrop: LyricBackdrop::default(),
            spectrum: cx.new(|_| SpectrumView::default()),
        }
    }

    pub fn reset_follow(&mut self) {
        self.followed = None;
        self.manual_until = None;
        self.scroll.set_offset(gpui::point(px(0.), px(0.)));
    }

    pub fn follow(&mut self, current: Option<usize>) -> bool {
        if !self.open
            || self
                .manual_until
                .is_some_and(|until| Instant::now() < until)
        {
            return false;
        }
        let resumed = self.manual_until.take().is_some();
        if self.followed == current && !resumed {
            return false;
        }
        self.followed = current;
        if let Some(index) = current {
            self.scroll.scroll_to_item(index);
        }
        true
    }
}

impl MusicApp {
    pub(super) fn focus_immersive_lyrics(&self, window: &mut Window) {
        window.focus(&self.lyric_page.focus);
    }

    pub(super) fn open_immersive_lyrics(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let page = &mut self.lyric_page;
        page.open = true;
        page.restore_fullscreen = window.is_fullscreen();
        page.reset_follow();
        page.follow(self.lyric_line);
        window.focus(&page.focus);
        cx.notify();
    }

    fn close_immersive_lyrics(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.lyric_poster.clear();
        let page = &mut self.lyric_page;
        page.open = false;
        page.backdrop.clear(window);
        page.spectrum
            .update(cx, |view, cx| view.set_playing(false, cx));
        if window.is_fullscreen() != page.restore_fullscreen {
            window.toggle_fullscreen();
        }
        window.focus(&self.input.read(cx).focus_handle(cx));
        cx.notify();
    }

    pub(super) fn render_immersive_lyrics(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(poster) = self.render_lyric_poster(window, cx) {
            self.lyric_page
                .spectrum
                .update(cx, |view, cx| view.set_playing(false, cx));
            return poster;
        }
        let page = &mut self.lyric_page;
        let player = self.player.as_ref();
        let cover = player.and_then(|p| p.cover_url.as_deref());
        let palette = cover.and_then(|address| self.artwork.borrow().ambient(address));
        let backdrop = page.backdrop.render(palette, window, cx);
        let position = player.map_or(0, |p| p.position_ms);
        let fullscreen = window.is_fullscreen();
        let playing = player.is_some_and(|p| p.state == PlaybackState::Playing);
        page.spectrum
            .update(cx, |view, cx| view.set_playing(playing, cx));
        let spectrum = page.spectrum.clone();
        let title = player
            .map(|p| p.title.clone())
            .unwrap_or_else(|| "未在播放".into());
        let artist = player.map(|p| p.artist.clone()).unwrap_or_default();
        let time = player
            .map(|p| format!("{} / {}", clock(p.position), clock(p.duration)))
            .unwrap_or_default();
        div()
            .id("immersive-lyrics")
            .key_context("ImmersiveLyrics")
            .track_focus(&page.focus)
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(rgb(BACKDROP_BASE))
            .text_color(rgb(0xf8f9ff))
            .on_action(cx.listener(|this, _: &CloseImmersive, window, cx| {
                this.close_immersive_lyrics(window, cx)
            }))
            .on_action(cx.listener(|_, _: &ToggleFullscreen, window, cx| {
                window.toggle_fullscreen();
                cx.notify();
            }))
            .on_key_down(cx.listener(Self::playback_shortcut))
            .child(backdrop)
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_4()
                    .p_4()
                    .child(control("lyric-back", "← 返回").on_click(
                        cx.listener(|this, _, window, cx| this.close_immersive_lyrics(window, cx)),
                    ))
                    .when(matches!(&self.lyrics, LyricsView::Ready(_)), |row| {
                        row.child(control("lyric-poster", "歌词海报").on_click(cx.listener(
                            |this, _, window, cx| {
                                this.open_lyric_poster(window, cx);
                            },
                        )))
                    })
                    .child(
                        control(
                            "lyric-fullscreen",
                            if fullscreen { "退出全屏" } else { "全屏" },
                        )
                        .on_click(cx.listener(|_, _, window, cx| {
                            window.toggle_fullscreen();
                            cx.notify();
                        })),
                    )
                    .child(
                        control(
                            "lyric-translations",
                            if page.translations {
                                "隐藏译文"
                            } else {
                                "显示译文"
                            },
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.lyric_page.translations = !this.lyric_page.translations;
                            if let Some(index) = this.lyric_line {
                                this.lyric_page.scroll.scroll_to_item(index);
                            }
                            cx.notify();
                        })),
                    )
                    .children([(-2., "A−"), (2., "A+")].into_iter().enumerate().map(
                        |(index, (delta, label))| {
                            control(("lyric-font", index), label).on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.lyric_page.font_size =
                                        (this.lyric_page.font_size + delta).clamp(18., 52.);
                                    if let Some(index) = this.lyric_line {
                                        this.lyric_page.scroll.scroll_to_item(index);
                                    }
                                    cx.notify();
                                },
                            ))
                        },
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .px_6()
                    .pb_3()
                    .child(self.cover(cover, 72., cx))
                    .child(
                        div()
                            .child(div().text_xl().font_weight(FontWeight::BOLD).child(title))
                            .child(div().text_color(rgb(0xb5bed9)).child(artist)),
                    ),
            )
            .child(self.render_immersive_lines(position, cx))
            .child(spectrum)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_6()
                    .p_4()
                    .bg(rgb(0x222943))
                    .child(control("lyric-prev", "上一首").on_click(cx.listener(
                        |this, _, _, _| {
                            let _ = this.service.commands.send(Command::Previous);
                        },
                    )))
                    .child(
                        control("lyric-play", if playing { "暂停" } else { "播放" }).on_click(
                            cx.listener(|this, _, _, _| {
                                let _ = this.service.commands.send(Command::Toggle);
                            }),
                        ),
                    )
                    .child(control("lyric-next", "下一首").on_click(cx.listener(
                        |this, _, _, _| {
                            let _ = this.service.commands.send(Command::Next);
                        },
                    )))
                    .child(time),
            )
            .into_any_element()
    }

    fn render_immersive_lines(&self, position: u64, cx: &mut Context<Self>) -> AnyElement {
        let page = &self.lyric_page;
        let body = div()
            .id("immersive-lyric-scroll")
            .when_some(
                crate::appearance::font(crate::appearance::FontTarget::Lyrics, cx),
                |view, font| view.font_family(font),
            )
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&page.scroll)
            .on_scroll_wheel(cx.listener(|this, _, _, _| {
                this.lyric_page.manual_until = Some(Instant::now() + Duration::from_secs(5));
            }));
        let document = match &self.lyrics {
            LyricsView::Ready(doc) => doc,
            state => {
                let message = match state {
                    LyricsView::Empty => "暂无歌词",
                    LyricsView::Loading => "正在加载歌词…",
                    _ => "歌词加载失败",
                };
                return body.child(div().p_6().child(message)).into_any_element();
            }
        };
        let units = document.scroll_units();
        let active_unit = self.lyric_line.and_then(|index| units.get(index));
        let stamp = self.lyric_seek_stamp();
        body.children(
            document
                .lines
                .iter()
                .enumerate()
                .map(|(index, line)| {
                    if line.is_translation && !page.translations {
                        return div().into_any_element();
                    }
                    let current = active_unit.is_some() && active_unit == units.get(index);
                    let row = div()
                        .id(("immersive-lyric", index))
                        .px_6()
                        .py_2()
                        .flex()
                        .flex_wrap()
                        .text_size(px(if line.is_translation {
                            page.font_size * 0.67
                        } else {
                            page.font_size
                        }))
                        .text_color(rgb(if current { 0xf8f9ff } else { 0xb9c4df }))
                        .when(current, |row| row.bg(rgb(0x222943)))
                        .cursor_pointer()
                        .hover(|row| row.bg(rgb(0x293253)));
                    let target = line.start_ms as f64 / 1000.;
                    let row = row.on_click(cx.listener(move |this, _, _, _| {
                        this.seek_lyric(stamp, target);
                    }));
                    if current
                        && !line.is_translation
                        && document.kind == LyricKind::WordByWord
                        && !line.syllables.is_empty()
                    {
                        row.children(line.syllables.iter().map(|syllable| {
                            div()
                                .text_color(rgb(if position >= syllable.start_ms {
                                    0xaab7ff
                                } else {
                                    0xf8f9ff
                                }))
                                .child(syllable.text.clone())
                        }))
                        .into_any_element()
                    } else {
                        row.child(line.text.clone()).into_any_element()
                    }
                })
                .collect::<Vec<_>>(),
        )
        .into_any_element()
    }
}

fn control(id: impl Into<gpui::ElementId>, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .cursor_pointer()
        .text_color(rgb(0xaab7ff))
        .child(label)
}

#[cfg(test)]
#[path = "lyric_page_tests.rs"]
mod tests;
