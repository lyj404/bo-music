use crate::{MusicApp, input::TextInput};
use bowen_app::desktop_service::{AppearanceSettings, Command, ThemeMode};
use gpui::{
    AnyElement, App, Context, Div, Entity, Global, Rgba, Stateful, Window, WindowAppearance,
    WindowBackgroundAppearance, div, prelude::*, px,
};
use std::sync::OnceLock;

#[derive(Default)]
pub(super) struct Appearance {
    settings: AppearanceSettings,
    system_dark: bool,
}
impl Global for Appearance {}

impl Appearance {
    fn dark(&self) -> bool {
        match self.settings.theme {
            ThemeMode::System => self.system_dark,
            ThemeMode::Light => false,
            ThemeMode::Dark => true,
        }
    }
}

fn is_dark(value: WindowAppearance) -> bool {
    matches!(
        value,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}

pub(super) fn init(cx: &mut App) {
    cx.set_global(Appearance {
        system_dark: is_dark(cx.window_appearance()),
        ..Default::default()
    });
}

pub(super) fn observe(window: &mut Window, cx: &Context<MusicApp>) {
    cx.observe_window_appearance(window, |_, window, cx| {
        cx.update_global::<Appearance, _>(|appearance, _| {
            appearance.system_dark = is_dark(window.appearance())
        });
        cx.notify();
    })
    .detach();
}

pub(super) fn paint(cx: &App) -> impl Fn(u32) -> Rgba + Copy + use<> {
    let dark = cx.try_global::<Appearance>().is_some_and(Appearance::dark);
    move |light| gpui::rgb(if dark { dark_color(light) } else { light })
}

fn dark_color(light: u32) -> u32 {
    match light {
        0xffffff => 0x232a3b,
        0xf5f7fb => 0x171d2a,
        0xf1f3f9 | 0xeeeeee | 0xe9edf5 => 0x30394d,
        0xe9edff | 0xe8edff => 0x334168,
        0xdbe4ff => 0x43517b,
        0xd8dce6 => 0x4a5469,
        0x20293a => 0xeff3ff,
        0x687083 | 0x788196 => 0xc1cada,
        0x8a91a2 | 0x9ca4b4 | 0xaab0bd => 0xaeb9cc,
        0x354db6 | 0x415de5 => 0xaebaff,
        0xc23b46 => 0xff9099,
        _ => light,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FontTarget {
    Interface,
    Lyrics,
    Mini,
    Desktop,
}

impl FontTarget {
    fn label(self) -> &'static str {
        match self {
            Self::Interface => "界面",
            Self::Lyrics => "沉浸歌词",
            Self::Mini => "迷你播放器歌词",
            Self::Desktop => "桌面歌词",
        }
    }
    fn font(self, settings: &AppearanceSettings) -> &str {
        match self {
            Self::Interface => &settings.ui_font,
            Self::Lyrics => &settings.lyric_font,
            Self::Mini => &settings.mini_lyric_font,
            Self::Desktop => &settings.desktop_lyric_font,
        }
    }
    fn set(self, settings: &mut AppearanceSettings, font: String) {
        match self {
            Self::Interface => settings.ui_font = font,
            Self::Lyrics => settings.lyric_font = font,
            Self::Mini => settings.mini_lyric_font = font,
            Self::Desktop => settings.desktop_lyric_font = font,
        }
    }
}

pub(super) fn fonts(cx: &App) -> &'static [String] {
    static FONTS: OnceLock<Vec<String>> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut names = cx.text_system().all_font_names();
        names.sort_unstable_by_key(|name| name.to_lowercase());
        names.dedup_by(|right, left| right.eq_ignore_ascii_case(left));
        names
    })
}

pub(super) fn font(target: FontTarget, cx: &App) -> Option<&'static str> {
    let prefs = &cx.try_global::<Appearance>()?.settings;
    let name = target.font(prefs);
    if name.is_empty() {
        return None;
    }
    fonts(cx)
        .iter()
        .find(|installed| installed.eq_ignore_ascii_case(name))
        .map(String::as_str)
}

pub(super) struct AppearancePage {
    pub open: bool,
    picker: Option<FontTarget>,
    filter: Entity<TextInput>,
    pub error: bool,
    applied_acrylic: bool,
}

impl AppearancePage {
    pub(super) fn new(cx: &mut Context<MusicApp>) -> Self {
        let filter = cx.new(|cx| TextInput::with_placeholder(cx, "搜索已安装字体"));
        cx.observe(&filter, |_, _, cx| cx.notify()).detach();
        Self {
            open: false,
            picker: None,
            filter,
            error: false,
            applied_acrylic: false,
        }
    }
}

impl MusicApp {
    pub(super) fn apply_appearance(
        &mut self,
        settings: AppearanceSettings,
        cx: &mut Context<Self>,
    ) {
        cx.update_global::<Appearance, _>(|appearance, _| appearance.settings = settings);
        cx.notify();
    }

    fn update_appearance(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut AppearanceSettings),
    ) {
        let mut settings = cx.global::<Appearance>().settings.clone();
        change(&mut settings);
        self.appearance_page.error = self
            .service
            .commands
            .send(Command::SaveAppearance(settings.clone()))
            .is_err();
        self.apply_appearance(settings, cx);
    }

    pub(super) fn appearance_background(&mut self, window: &mut Window, cx: &App) -> Rgba {
        let acrylic = cx.global::<Appearance>().settings.acrylic;
        if acrylic != self.appearance_page.applied_acrylic {
            window.set_background_appearance(if acrylic {
                WindowBackgroundAppearance::Blurred
            } else {
                WindowBackgroundAppearance::Opaque
            });
            self.appearance_page.applied_acrylic = acrylic;
        }
        let mut color = paint(cx)(0xf5f7fb);
        if acrylic {
            color.a = 0.92;
        }
        color
    }

    pub(super) fn render_appearance(&self, cx: &mut Context<Self>) -> AnyElement {
        let rgb = paint(cx);
        let settings = &cx.global::<Appearance>().settings;
        div()
            .id("appearance-settings")
            .size_full()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_4()
            .p_6()
            .bg(rgb(0xf5f7fb))
            .text_color(rgb(0x20293a))
            .when_some(font(FontTarget::Interface, cx), |view, family| {
                view.font_family(family)
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(
                        button("appearance-back", "← 返回", rgb).on_click(cx.listener(
                            |this, _, window, cx| {
                                this.appearance_page.open = false;
                                window.focus(&this.playback_focus);
                                cx.notify();
                            },
                        )),
                    )
                    .child(div().text_xl().child("外观和字体")),
            )
            .child(
                div().flex().gap_2().children(
                    [
                        (ThemeMode::System, "跟随系统"),
                        (ThemeMode::Light, "浅色"),
                        (ThemeMode::Dark, "深色"),
                    ]
                    .into_iter()
                    .map(|(theme, label)| {
                        button(label, label, rgb)
                            .when(settings.theme == theme, |item| {
                                item.border_1().border_color(rgb(0x415de5))
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.update_appearance(cx, |settings| settings.theme = theme)
                            }))
                    }),
                ),
            )
            .child(
                div().flex().child(
                    button(
                        "appearance-blur",
                        if settings.acrylic {
                            "关闭窗口磨砂"
                        } else {
                            "开启窗口磨砂"
                        },
                        rgb,
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.update_appearance(cx, |settings| settings.acrylic = !settings.acrylic)
                    })),
                ),
            )
            .children(
                [
                    FontTarget::Interface,
                    FontTarget::Lyrics,
                    FontTarget::Mini,
                    FontTarget::Desktop,
                ]
                .into_iter()
                .enumerate()
                .map(|(index, target)| {
                    let family = target.font(settings).to_owned();
                    div()
                        .max_w(px(600.))
                        .flex()
                        .items_center()
                        .justify_between()
                        .rounded_md()
                        .p_2()
                        .bg(rgb(0xffffff))
                        .child(target.label())
                        .child(
                            button(
                                ("appearance-font", index),
                                if family.is_empty() {
                                    "系统默认".into()
                                } else {
                                    family
                                },
                                rgb,
                            )
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.appearance_page
                                        .filter
                                        .update(cx, |input, cx| input.clear(cx));
                                    this.appearance_page.picker = Some(target);
                                    cx.notify();
                                },
                            )),
                        )
                }),
            )
            .when_some(self.appearance_page.picker, |page, target| {
                let filter = self
                    .appearance_page
                    .filter
                    .read(cx)
                    .text()
                    .trim()
                    .to_lowercase();
                page.child(
                    div()
                        .flex()
                        .gap_2()
                        .items_center()
                        .child(div().w(px(300.)).child(self.appearance_page.filter.clone()))
                        .child(
                            button("default-font", "恢复系统默认", rgb).on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.appearance_page.picker = None;
                                    this.update_appearance(cx, |settings| {
                                        target.set(settings, String::new())
                                    });
                                },
                            )),
                        ),
                )
                .child(
                    div()
                        .id("installed-fonts")
                        .max_h(px(190.))
                        .overflow_y_scroll()
                        .children(
                            fonts(cx)
                                .iter()
                                .filter(|name| name.to_lowercase().contains(&filter))
                                .enumerate()
                                .map(|(index, name)| {
                                    button(("installed-font", index), name.clone(), rgb)
                                        .font_family(name.clone())
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.appearance_page.picker = None;
                                            this.update_appearance(cx, |settings| {
                                                target.set(settings, name.clone())
                                            });
                                        }))
                                }),
                        ),
                )
            })
            .when(self.appearance_page.error, |page| {
                page.child("外观设置未能保存")
            })
            .into_any_element()
    }
}

fn button(
    id: impl Into<gpui::ElementId>,
    label: impl Into<gpui::SharedString>,
    rgb: impl Fn(u32) -> Rgba + Copy,
) -> Stateful<Div> {
    div()
        .id(id)
        .cursor_pointer()
        .px_3()
        .py_1()
        .rounded_md()
        .bg(rgb(0xe8edff))
        .text_color(rgb(0x354db6))
        .hover(|item| item.bg(rgb(0xdbe4ff)))
        .child(label.into())
}
