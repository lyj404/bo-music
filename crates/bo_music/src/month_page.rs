use super::{MusicApp, Page};
use bo_music_app::desktop_service::{Command, MonthPosterKind, MonthPosterStyle};
use bo_music_domain::account::{MonthAvailability, MonthPeriod, MonthReport};
use gpui::{
    AnyElement, Context, Div, FontWeight, Image, ImageFormat, Stateful, div, prelude::*, px,
};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
pub(super) struct MonthPage {
    generation: u64,
    period: Option<MonthPeriod>,
    report: Option<Result<Option<Box<MonthReport>>, ()>>,
    availability: HashMap<MonthPeriod, MonthAvailability>,
    probe_generation: Option<u64>,
    confirm_create: bool,
    creating: HashSet<MonthPeriod>,
    playlist_results: HashMap<MonthPeriod, bool>,
    poster: PosterPreview,
}

#[derive(Default)]
struct PosterPreview {
    request: u64,
    image: Option<Arc<Image>>,
    pending: bool,
    saving: bool,
    message: Option<&'static str>,
}

impl MonthPage {
    pub(super) fn reset(&mut self, cx: &mut Context<MusicApp>) {
        self.clear_poster(cx);
        *self = Self {
            generation: self.generation.wrapping_add(1),
            ..Default::default()
        };
    }

    fn clear_poster(&mut self, cx: &mut Context<MusicApp>) {
        if let Some(image) = self.poster.image.take() {
            image.remove_asset(cx);
        }
        self.poster = PosterPreview {
            request: self.poster.request.wrapping_add(1),
            ..Default::default()
        };
    }

    pub(super) fn accept_poster(
        &mut self,
        generation: u64,
        request: u64,
        image: Result<Arc<Vec<u8>>, ()>,
    ) {
        if self.generation != generation || self.poster.request != request || !self.is_open() {
            return;
        }
        self.poster.pending = false;
        match image {
            Ok(bytes) => {
                self.poster.image = Some(Arc::new(Image::from_bytes(
                    ImageFormat::Png,
                    bytes.as_ref().clone(),
                )))
            }
            Err(()) => self.poster.message = Some("无法生成海报，请重试"),
        }
    }

    pub(super) fn accept_poster_saved(&mut self, generation: u64, request: u64, saved: bool) {
        if self.generation == generation && self.poster.request == request && self.is_open() {
            self.poster.saving = false;
            self.poster.message = Some(if saved {
                "海报已保存"
            } else {
                "无法保存海报，请选择其他位置重试"
            });
        }
    }

    pub(super) fn is_open(&self) -> bool {
        self.period.is_some()
    }

    pub(super) fn accept_report(
        &mut self,
        generation: u64,
        result: Result<Option<Box<MonthReport>>, ()>,
    ) {
        if generation == self.generation
            && let Some(period) = self.period
        {
            let availability = match &result {
                Ok(Some(_)) => MonthAvailability::Available,
                Ok(None) => MonthAvailability::Empty,
                Err(()) => MonthAvailability::Failed,
            };
            self.availability.insert(period, availability);
            self.report = Some(result);
        }
    }

    pub(super) fn accept_availability(
        &mut self,
        generation: u64,
        period: MonthPeriod,
        status: MonthAvailability,
    ) {
        if self.probe_generation == Some(generation)
            && self
                .period
                .is_some_and(|selected| selected.year() == period.year())
            && (self.period != Some(period) || self.report.is_none())
        {
            self.availability.insert(period, status);
        }
    }

    pub(super) fn finish_probe(&mut self, generation: u64) {
        if self.probe_generation == Some(generation) {
            self.probe_generation = None;
        }
    }

    pub(super) fn accept_playlist(&mut self, period: MonthPeriod, created: bool) {
        if self.creating.remove(&period) {
            self.playlist_results.insert(period, created);
        }
    }
}

impl MusicApp {
    pub(super) fn open_month_report(&mut self, cx: &mut Context<Self>) {
        let current = current_month();
        let latest = self
            .account_overview
            .as_deref()
            .and_then(|result| result.as_ref().ok())
            .and_then(|view| view.month_summary.as_ref())
            .and_then(|summary| summary.month_desc.as_deref())
            .and_then(|description| MonthPeriod::from_summary(description, current))
            .filter(|period| *period <= current)
            .unwrap_or(current);
        self.load_month_report(latest, cx);
    }

    fn load_month_report(&mut self, period: MonthPeriod, cx: &mut Context<Self>) {
        let state = &mut self.month_page;
        state.clear_poster(cx);
        let changed_year = state
            .period
            .is_none_or(|selected| selected.year() != period.year());
        state.generation = state.generation.wrapping_add(1);
        state.period = Some(period);
        state.report = None;
        state.confirm_create = false;
        if self
            .service
            .commands
            .send(Command::LoadMonthReport(state.generation, period))
            .is_err()
        {
            state.report = Some(Err(()));
        } else if changed_year {
            state.availability.clear();
            self.probe_month_reports(cx);
        }
        cx.notify();
    }

    fn month_bounds(&self, year: u16) -> Option<(MonthPeriod, MonthPeriod)> {
        let current = current_month();
        let earliest = self.month_registration();
        let first = MonthPeriod::new(year, 1)?
            .max(earliest.unwrap_or_else(|| MonthPeriod::new(1, 1).expect("valid year")));
        let last = MonthPeriod::new(year, 12)?.min(current);
        (first <= last).then_some((first, last))
    }

    fn month_registration(&self) -> Option<MonthPeriod> {
        self.account_overview
            .as_deref()
            .and_then(|result| result.as_ref().ok())
            .and_then(|view| view.vip.as_ref())
            .and_then(|vip| vip.created_at.as_deref())
            .and_then(MonthPeriod::from_date)
            .filter(|start| *start <= current_month())
    }

    fn probe_month_reports(&mut self, cx: &mut Context<Self>) {
        let Some((first, last)) = self
            .month_page
            .period
            .and_then(|period| self.month_bounds(period.year()))
        else {
            return;
        };
        let generation = self.month_page.generation;
        self.month_page.probe_generation = self
            .service
            .commands
            .send(Command::ProbeMonthReports(generation, first, last))
            .is_ok()
            .then_some(generation);
        cx.notify();
    }

    fn render_month_choices(&self, period: MonthPeriod, cx: &mut Context<Self>) -> AnyElement {
        let rgb = crate::appearance::paint(cx);
        let Some((first, last)) = self.month_bounds(period.year()) else {
            return div().into_any_element();
        };
        let state = &self.month_page;
        let scanning = state.probe_generation.is_some();
        let months = (first.month()..=last.month())
            .filter_map(|month| MonthPeriod::new(period.year(), month));
        let mut has_unconfirmed = false;
        let choices = months
            .map(|choice| {
                let status = state.availability.get(&choice).copied();
                let label = match status {
                    Some(MonthAvailability::Available) => "有报告",
                    Some(MonthAvailability::Empty) => "暂无报告",
                    Some(MonthAvailability::Failed) => "读取失败",
                    None if scanning => "检查中",
                    None => "待检查",
                };
                let current = choice == period;
                has_unconfirmed |=
                    !current && matches!(status, None | Some(MonthAvailability::Failed));
                let selectable = !current && status != Some(MonthAvailability::Empty);
                div()
                    .id(("month-choice", usize::from(choice.month())))
                    .px_2()
                    .py_1()
                    .rounded_md()
                    .bg(rgb(if current { 0xe9edf5 } else { 0xffffff }))
                    .text_color(rgb(if current || selectable {
                        0x415de5
                    } else {
                        0x687083
                    }))
                    .child(format!("{}月 · {label}", choice.month()))
                    .when(selectable, |button| {
                        button.cursor_pointer().on_click(
                            cx.listener(move |this, _, _, cx| this.load_month_report(choice, cx)),
                        )
                    })
            })
            .collect::<Vec<_>>();
        div()
            .pb_3()
            .child(div().flex().flex_wrap().gap_2().children(choices))
            .when(!scanning && has_unconfirmed, |panel| {
                panel.child(
                    control("month-probe-retry", "重新检查月份", rgb)
                        .on_click(cx.listener(|this, _, _, cx| this.probe_month_reports(cx))),
                )
            })
            .into_any_element()
    }

    pub(super) fn render_month_report(&self, cx: &mut Context<Self>) -> AnyElement {
        let rgb = crate::appearance::paint(cx);
        let Some(period) = self.month_page.period else {
            return div().into_any_element();
        };
        let current = current_month();
        let earliest = self.month_registration();
        let body = div()
            .id("month-report")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_3()
            .child(
                crate::ui::icon_button_labeled("month-report-back", "back", "返回我的统计", cx)
                    .on_click(cx.listener(|this, _, _, cx| {
                        let _ = this
                            .service
                            .commands
                            .send(Command::CloseMonthReport(this.month_page.generation));
                        this.month_page.period = None;
                        this.month_page.report = None;
                        this.month_page.confirm_create = false;
                        this.month_page.probe_generation = None;
                        this.month_page.clear_poster(cx);
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .items_center()
                    .gap_3()
                    .py_3()
                    .child(div().text_xl().font_weight(FontWeight::BOLD).child(format!(
                        "{}年{}月听歌月报",
                        period.year(),
                        period.month()
                    )))
                    .children(
                        [
                            (-12, "上一年"),
                            (-1, "上个月"),
                            (1, "下个月"),
                            (12, "下一年"),
                        ]
                        .into_iter()
                        .enumerate()
                        .filter_map(|(index, (delta, label))| {
                            let mut target = period.offset(delta)?;
                            if delta.abs() == 12 {
                                let (first, last) = self.month_bounds(target.year())?;
                                target = target.clamp(first, last);
                            } else if target > current
                                || earliest.is_some_and(|earliest| target < earliest)
                            {
                                return None;
                            }
                            Some(
                                control(("month-move", index), label, rgb).on_click(cx.listener(
                                    move |this, _, _, cx| this.load_month_report(target, cx),
                                )),
                            )
                        })
                        .collect::<Vec<_>>(),
                    )
                    .child(
                        control("month-latest", "最近月报", rgb)
                            .on_click(cx.listener(|this, _, _, cx| this.open_month_report(cx))),
                    ),
            )
            .child(self.render_month_choices(period, cx));
        let report = match self.month_page.report.as_ref() {
            None => return body.child("正在加载月报…").into_any_element(),
            Some(Ok(None)) => return body.child("该月份暂无月报").into_any_element(),
            Some(Err(())) => {
                return body
                    .child("无法读取月报")
                    .child(control("month-retry", "重试", rgb).on_click(
                        cx.listener(move |this, _, _, cx| this.load_month_report(period, cx)),
                    ))
                    .into_any_element();
            }
            Some(Ok(Some(report))) => report,
        };
        let state = &self.month_page;
        let generation = state.generation;
        let creation_label = match state.playlist_results.get(&period) {
            Some(true) => "已生成月报歌单",
            _ if state.creating.contains(&period) => "正在生成歌单…",
            _ if state.confirm_create => "确认生成歌单",
            _ => "生成月报歌单",
        };
        body.child(self.render_month_poster(period, cx))
            .child(
                div().flex().flex_wrap().gap_4().pb_3().children(
                    [
                        report
                            .listen_total_minutes
                            .as_ref()
                            .map(|v| format!("听歌 {v} 分钟")),
                        report.play_count.map(|v| format!("播放 {v} 次")),
                        report.song_count.map(|v| format!("听过 {v} 首")),
                        report.time_rank.map(|v| format!("听歌时长排名 {v}")),
                        report
                            .listen_chain
                            .as_ref()
                            .map(|v| format!("连续听歌 {v}")),
                        report.play_chain.as_ref().map(|v| format!("连续播放 {v}")),
                    ]
                    .into_iter()
                    .flatten()
                    .map(|value| {
                        div()
                            .px_3()
                            .py_2()
                            .rounded_lg()
                            .bg(rgb(0xf5f7fb))
                            .child(value)
                    }),
                ),
            )
            .children(
                [
                    &report.listen_content,
                    &report.play_content,
                    &report.listen_top_track_title,
                ]
                .into_iter()
                .flatten()
                .cloned()
                .map(|value| div().pb_1().child(value)),
            )
            .when(report.artist_name.is_some(), |body| {
                body.child(
                    div()
                        .py_3()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(self.cover(report.artist_cover_url.as_deref(), 52.0, cx))
                        .child(
                            div()
                                .font_weight(FontWeight::BOLD)
                                .child(report.artist_name.clone().unwrap_or_default()),
                        )
                        .when_some(report.artist_music_name.as_ref(), |row, name| {
                            row.child(self.cover(
                                report.artist_music_cover_url.as_deref(),
                                38.0,
                                cx,
                            ))
                            .child(name.clone())
                        })
                        .when_some(report.artist_content.as_ref(), |row, content| {
                            row.child(content.clone())
                        }),
                )
            })
            .when(!report.top_tracks.is_empty(), |body| {
                body.child(div().py_2().font_weight(FontWeight::BOLD).child("本月常听"))
                    .children(
                        report
                            .top_tracks
                            .iter()
                            .enumerate()
                            .map(|(index, track)| {
                                let id = track.id;
                                div()
                                    .id(("month-track", index))
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .p_2()
                                    .rounded_md()
                                    .cursor_pointer()
                                    .hover(|style| style.bg(rgb(0xe9edf5)))
                                    .child(self.cover(track.cover_url.as_deref(), 42.0, cx))
                                    .child(
                                        div().flex_1().min_w_0().child(track.title.clone()).child(
                                            div()
                                                .text_sm()
                                                .text_color(rgb(0x687083))
                                                .child(track.artist.clone()),
                                        ),
                                    )
                                    .when_some(track.play_count, |row, count| {
                                        row.child(format!("{count} 次"))
                                    })
                                    .on_click(cx.listener(move |this, _, _, _| {
                                        if this.navigation.page == Page::AccountOverview
                                            && this.month_page.generation == generation
                                            && this.month_page.period == Some(period)
                                        {
                                            let _ = this
                                                .service
                                                .commands
                                                .send(Command::PlayMonthTrack(generation, id));
                                        }
                                    }))
                            })
                            .collect::<Vec<_>>(),
                    )
            })
            .when(!report.album_covers.is_empty(), |body| {
                body.child(div().py_2().font_weight(FontWeight::BOLD).child("常听专辑"))
                    .child(
                        div().flex().flex_wrap().gap_2().children(
                            report
                                .album_covers
                                .iter()
                                .map(|url| self.cover(Some(url), 52.0, cx))
                                .collect::<Vec<_>>(),
                        ),
                    )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .py_3()
                    .child(control("month-create", creation_label, rgb).when(
                        !state.creating.contains(&period)
                            && state.playlist_results.get(&period) != Some(&true),
                        |button| {
                            button.on_click(cx.listener(move |this, _, _, cx| {
                                let state = &mut this.month_page;
                                if state.generation != generation || state.period != Some(period) {
                                    return;
                                }
                                if state.confirm_create {
                                    state.confirm_create = false;
                                    if this
                                        .service
                                        .commands
                                        .send(Command::CreateMonthPlaylist(generation, period))
                                        .is_ok()
                                    {
                                        state.creating.insert(period);
                                        state.playlist_results.remove(&period);
                                    } else {
                                        state.playlist_results.insert(period, false);
                                    }
                                } else {
                                    state.confirm_create = true;
                                }
                                cx.notify();
                            }))
                        },
                    ))
                    .when(state.confirm_create, |row| {
                        row.child(control("month-create-cancel", "取消", rgb).on_click(
                            cx.listener(|this, _, _, cx| {
                                this.month_page.confirm_create = false;
                                cx.notify();
                            }),
                        ))
                    })
                    .when(state.playlist_results.get(&period) == Some(&false), |row| {
                        row.child("生成结果未确认，请先到「我的歌单」查看")
                    }),
            )
            .into_any_element()
    }
}

impl MusicApp {
    fn render_month_poster(&self, period: MonthPeriod, cx: &mut Context<Self>) -> AnyElement {
        let rgb = crate::appearance::paint(cx);
        let poster = &self.month_page.poster;
        let styles = [
            (MonthPosterKind::Overview, true, "深色总览"),
            (MonthPosterKind::Overview, false, "浅色总览"),
            (MonthPosterKind::Tracks, true, "深色排行"),
            (MonthPosterKind::Tracks, false, "浅色排行"),
        ];
        div()
            .pb_4()
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_3()
                    .items_center()
                    .child(div().font_weight(FontWeight::BOLD).child("海报"))
                    .children(styles.into_iter().enumerate().map(
                        |(index, (kind, dark, label))| {
                            control(("poster-style", index), label, rgb).on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.start_month_poster(MonthPosterStyle { kind, dark }, cx);
                                },
                            ))
                        },
                    )),
            )
            .when(poster.pending, |body| body.child("正在生成预览…"))
            .when_some(poster.image.clone(), |body, image| {
                body.child(gpui::img(image).w(px(264.)).h(px(360.))).child(
                    control(
                        "save-month-poster",
                        if poster.saving {
                            "正在保存…"
                        } else {
                            "保存 PNG"
                        },
                        rgb,
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| {
                            this.choose_month_poster_path(period, cx)
                        }),
                    ),
                )
            })
            .when_some(poster.message, |body, message| {
                body.child(div().text_sm().child(message))
            })
            .into_any_element()
    }

    fn start_month_poster(&mut self, style: MonthPosterStyle, cx: &mut Context<Self>) {
        let state = &mut self.month_page;
        if state.poster.saving || !matches!(state.report, Some(Ok(Some(_)))) {
            return;
        }
        state.clear_poster(cx);
        state.poster.pending = self
            .service
            .commands
            .send(Command::RenderMonthPoster(
                state.generation,
                state.poster.request,
                style,
            ))
            .is_ok();
        if !state.poster.pending {
            state.poster.message = Some("无法生成海报，请重试");
        }
        cx.notify();
    }

    fn choose_month_poster_path(&mut self, period: MonthPeriod, cx: &mut Context<Self>) {
        let poster = &mut self.month_page.poster;
        if poster.saving || poster.image.is_none() {
            return;
        }
        poster.saving = true;
        poster.message = None;
        let generation = self.month_page.generation;
        let request = poster.request;
        let base = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .filter(|path| path.is_dir())
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        let documents = base.join("Documents");
        let folder = if documents.is_dir() {
            &documents
        } else {
            &base
        };
        let filename = format!("音波回忆-{}.png", period.request_value());
        let picker = cx.prompt_for_new_path(folder, Some(&filename));
        cx.spawn(async move |entity, cx| {
            let selected = picker.await;
            let _ = entity.update(cx, |this, cx| {
                let state = &mut this.month_page;
                if state.generation != generation || state.poster.request != request {
                    return;
                }
                match selected {
                    Ok(Ok(Some(path))) => {
                        if !path
                            .extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
                        {
                            state.poster.saving = false;
                            state.poster.message = Some("请选择以 .png 结尾的文件名");
                        } else if this
                            .service
                            .commands
                            .send(Command::SaveMonthPoster(generation, request, path))
                            .is_err()
                        {
                            state.poster.saving = false;
                            state.poster.message = Some("无法保存海报，请重试");
                        }
                    }
                    Ok(Ok(None)) => state.poster.saving = false,
                    _ => {
                        state.poster.saving = false;
                        state.poster.message = Some("无法打开保存窗口，请重试");
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
}

fn control(
    id: impl Into<gpui::ElementId>,
    label: &'static str,
    rgb: impl Fn(u32) -> gpui::Rgba + Copy,
) -> Stateful<Div> {
    div()
        .id(id)
        .cursor_pointer()
        .px_3()
        .py_1()
        .rounded_md()
        .bg(rgb(0xe9edf5))
        .text_color(rgb(0x415de5))
        .hover(|item| item.bg(rgb(0xe9edff)))
        .child(label)
}

fn current_month() -> MonthPeriod {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // Convert UTC epoch days to a Gregorian year and month using 400-year cycles.
    let days = seconds / 86_400 + 719_468;
    let era = days / 146_097;
    let day_of_era = days % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    u16::try_from(year)
        .ok()
        .and_then(|year| MonthPeriod::new(year, month as u8))
        .unwrap_or_else(|| MonthPeriod::new(1970, 1).expect("valid epoch"))
}
