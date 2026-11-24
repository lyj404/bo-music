use super::MusicApp;
use bo_music_app::artwork::{AmbientPalette, render_ambient};
use bo_music_app::desktop_service::LyricsView;
use bo_music_domain::lyrics::LyricKind;
use bo_music_playback::mpv::PlaybackState;
use bo_music_playback::spectrum::BAND_COUNT;
use bo_music_playback::spectrum_output::{OutputSpectrum, OutputSpectrumFrame};
use gpui::{
    AnyElement, App, Context, Div, Entity, FocusHandle, Focusable, FontWeight, Image, ImageFormat,
    KeyBinding, ObjectFit, RenderImage, ScrollHandle, Stateful, Task, Window, actions, div, img,
    prelude::*, px, rgb,
};
use std::sync::Arc;
use std::time::{Duration, Instant};

actions!(lyrics, [CloseImmersive, ToggleFullscreen]);

const BACKDROP_FADE: Duration = Duration::from_millis(600);
const BACKDROP_BASE: u32 = 0x202020;
const BASE_PALETTE: AmbientPalette = [[32, 32, 32]; 3];

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
                    .bg(crate::appearance::paint(cx)(0x415de5))
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
    spectrum_open: bool,
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
            font_size: 22.,
            restore_fullscreen: false,
            backdrop: LyricBackdrop::default(),
            spectrum: cx.new(|_| SpectrumView::default()),
            spectrum_open: false,
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
        self.panels.player_tools_open = false;
        let page = &mut self.lyric_page;
        page.open = true;
        page.restore_fullscreen = window.is_fullscreen();
        page.reset_follow();
        page.follow(self.lyric_line);
        window.focus(&page.focus);
        cx.notify();
    }

    pub(super) fn close_immersive_lyrics(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.lyric_poster.clear();
        let page = &mut self.lyric_page;
        page.open = false;
        page.backdrop.clear(window);
        page.spectrum
            .update(cx, |view, cx| view.set_playing(false, cx));
        if window.is_fullscreen() != page.restore_fullscreen {
            window.toggle_fullscreen();
        }
        window.focus(&self.search.input.read(cx).focus_handle(cx));
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
        let rgb = crate::appearance::paint(cx);
        let compact = window.viewport_size().width < px(1050.);
        let player = self.player.as_ref();
        let cover = player.and_then(|p| p.cover_url.as_deref());
        let title = player.map_or_else(|| "未在播放".to_owned(), |p| p.title.clone());
        let artist = player.map_or_else(String::new, |p| p.artist.clone());
        let position = player.map_or(0, |p| p.position_ms);
        let playing = player.is_some_and(|p| p.state == PlaybackState::Playing);
        let palette = cover.and_then(|address| self.artwork.borrow().ambient(address));
        let backdrop = self.lyric_page.backdrop.render(palette, window, cx);
        let page = &mut self.lyric_page;
        let spectrum_open = page.spectrum_open;
        page.spectrum.update(cx, |view, cx| {
            view.set_playing(playing && spectrum_open, cx)
        });
        let spectrum = page.spectrum.clone();
        let header = div()
            .h(px(74.))
            .flex_shrink_0()
            .px_6()
            .flex()
            .items_center()
            .gap_3()
            .child(
                crate::ui::icon_button_labeled("lyric-back", "collapse", "收起歌词 · Esc", cx)
                    .on_click(
                        cx.listener(|this, _, window, cx| this.close_immersive_lyrics(window, cx)),
                    ),
            )
            .child(div().flex_1())
            .when(matches!(&self.lyrics, LyricsView::Ready(_)), |row| {
                row.child(
                    control("lyric-poster", "歌词海报", cx)
                        .text_color(rgb(0x687083))
                        .px_2()
                        .py_1()
                        .on_click(
                            cx.listener(|this, _, window, cx| this.open_lyric_poster(window, cx)),
                        ),
                )
            })
            .child(
                crate::ui::icon_button_labeled(
                    "lyric-translations",
                    "translate",
                    if page.translations {
                        "隐藏译文"
                    } else {
                        "显示译文"
                    },
                    cx,
                )
                .when(page.translations, |button| button.bg(rgb(0xe9edff)))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.lyric_page.translations = !this.lyric_page.translations;
                    cx.notify();
                })),
            )
            .child(
                control("lyric-spectrum", "频谱", cx)
                    .text_color(rgb(0x687083))
                    .px_2()
                    .py_1()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.lyric_page.spectrum_open = !this.lyric_page.spectrum_open;
                        cx.notify();
                    })),
            )
            .children([(-2., "A−"), (2., "A+")].into_iter().enumerate().map(
                |(index, (delta, label))| {
                    control(("lyric-font", index), label, cx)
                        .text_color(rgb(0x687083))
                        .px_2()
                        .py_1()
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.lyric_page.font_size =
                                (this.lyric_page.font_size + delta).clamp(18., 40.);
                            cx.notify();
                        }))
                },
            ))
            .child(ui_close(cx).on_click(
                cx.listener(|this, _, window, cx| this.close_immersive_lyrics(window, cx)),
            ));
        let album = div()
            .w(gpui::relative(0.44))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .child(self.cover(cover, if compact { 220. } else { 300. }, cx))
            .child(
                div()
                    .mt_4()
                    .text_size(px(25.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(div().text_color(rgb(0x788196)).child(artist))
            .when(spectrum_open, |column| {
                column.child(div().w(px(260.)).child(spectrum))
            });
        let lines = self.render_immersive_lines(position, cx);
        div()
            .id("immersive-lyrics")
            .key_context("ImmersiveLyrics")
            .track_focus(&self.lyric_page.focus)
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0xf5f7fb))
            .text_color(rgb(0x20293a))
            .on_action(cx.listener(|this, _: &CloseImmersive, window, cx| {
                this.close_immersive_lyrics(window, cx)
            }))
            .on_action(cx.listener(|_, _: &ToggleFullscreen, window, cx| {
                window.toggle_fullscreen();
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.close_immersive_lyrics(window, cx);
                    cx.stop_propagation();
                } else {
                    this.playback_shortcut(event, window, cx);
                }
            }))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .child(backdrop.opacity(0.04))
                    .child(header)
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .gap_6()
                            .px_6()
                            .pb_6()
                            .child(album)
                            .child(lines),
                    ),
            )
            .child(self.render_player_bar(compact, cx))
            .into_any_element()
    }

    fn render_immersive_lines(&self, position: u64, cx: &mut Context<Self>) -> AnyElement {
        let page = &self.lyric_page;
        let rgb = crate::appearance::paint(cx);
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
                    if line.text.trim().is_empty() || (line.is_translation && !page.translations) {
                        return div().into_any_element();
                    }
                    let current = active_unit.is_some() && active_unit == units.get(index);
                    let row = div()
                        .id(("immersive-lyric", index))
                        .px_6()
                        .py_4()
                        .rounded_lg()
                        .flex()
                        .flex_wrap()
                        .text_size(px(if line.is_translation {
                            page.font_size * 0.67
                        } else {
                            page.font_size
                        }))
                        .text_color(rgb(if current { 0x415de5 } else { 0x687083 }))
                        .font_weight(if current {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::MEDIUM
                        })
                        .when(current, |row| row.bg(rgb(0xe9edff)))
                        .cursor_pointer()
                        .hover(|row| row.bg(rgb(0xe9edf5)));
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
                                    0x415de5
                                } else {
                                    0x788196
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

fn ui_close(cx: &App) -> Stateful<Div> {
    crate::ui::icon_button("lyric-close", "close", cx)
}

fn control(id: impl Into<gpui::ElementId>, label: &'static str, cx: &App) -> Stateful<Div> {
    div()
        .id(id)
        .cursor_pointer()
        .text_color(crate::appearance::paint(cx)(0x415de5))
        .child(label)
}

#[cfg(test)]
#[path = "lyric_page_tests.rs"]
mod tests;
