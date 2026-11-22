use super::{MusicApp, Page, ui};
use bo_music_app::desktop_service::Command;
use bo_music_domain::account::VipBadge;
use gpui::{AnyElement, Context, Div, FontWeight, Rgba, div, prelude::*, px};
use std::time::{SystemTime, UNIX_EPOCH};

impl MusicApp {
    pub(super) fn load_account_overview(&mut self, cx: &mut Context<Self>) {
        self.account_overview_generation = self.account_overview_generation.wrapping_add(1);
        self.account_overview = None;
        if !self.auth.busy
            && self.auth.account.is_some()
            && self
                .service
                .commands
                .send(Command::LoadAccountOverview(
                    self.account_overview_generation,
                ))
                .is_err()
        {
            self.account_overview = Some(Box::new(Err(())));
        }
        cx.notify();
    }

    pub(super) fn render_account_overview(
        &self,
        compact: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rgb = crate::appearance::paint(cx);
        if self.month_page.is_open() && self.auth.account.is_some() && !self.auth.busy {
            return self.render_month_report(cx);
        }
        let body = div()
            .id("account-overview")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px_1()
            .pb_5()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .mb_5()
                    .child(
                        div()
                            .child(ui::eyebrow("LISTENING INSIGHTS", cx))
                            .child(ui::heading("听歌统计", cx).mt_1()),
                    )
                    .child(
                        ui::icon_button_labeled("stats-refresh", "refresh", "刷新统计", cx)
                            .on_click(cx.listener(|this, _, _, cx| this.load_account_overview(cx))),
                    ),
            );
        if self.auth.busy {
            return body
                .child(empty_card("正在读取账号…", rgb))
                .into_any_element();
        }
        let Some(account) = &self.auth.account else {
            return body
                .child(
                    div()
                        .p_6()
                        .rounded_lg()
                        .bg(rgb(0xf5f7fb))
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_3()
                        .child(ui::icon("chart", rgb(0x415de5)).size(px(32.)))
                        .child(
                            div()
                                .text_lg()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("登录后，查看你的音乐足迹"),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(rgb(0x788196))
                                .child("听歌时长、每周最爱和月报都在这里"),
                        )
                        .child(
                            div()
                                .id("stats-login")
                                .px_4()
                                .py_2()
                                .rounded_md()
                                .bg(rgb(0x4d66e5))
                                .text_color(gpui::rgb(0xffffff))
                                .cursor_pointer()
                                .child("登录账号")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.show_account = true;
                                    let _ = this.service.commands.send(Command::RefreshAccounts);
                                    cx.notify();
                                })),
                        ),
                )
                .into_any_element();
        };
        let overview = match self.account_overview.as_deref() {
            Some(Ok(overview)) => overview,
            Some(Err(())) => {
                return body
                    .child(empty_card("统计暂时无法读取，请点击右上角刷新重试", rgb))
                    .into_any_element();
            }
            None => {
                return body
                    .child(empty_card("正在整理你的听歌足迹…", rgb))
                    .into_any_element();
            }
        };
        let name = account
            .nickname
            .clone()
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| format!("账号 {}", account.uid));
        let initial = name.chars().next().unwrap_or('♪').to_string();
        let profile = div()
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(rgb(0xd8dce6))
            .mb_5()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(
                        div()
                            .size(px(56.))
                            .flex_shrink_0()
                            .rounded_full()
                            .bg(rgb(0xe9edff))
                            .text_color(rgb(0x415de5))
                            .text_xl()
                            .flex()
                            .items_center()
                            .justify_center()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(initial),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(20.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(name),
                            )
                            .when_some(overview.vip.as_ref(), |column, vip| {
                                column.when_some(vip.created_at.as_ref(), |column, date| {
                                    column.child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(rgb(0x788196))
                                            .child(format!("注册于 {date}")),
                                    )
                                })
                            }),
                    )
                    .when_some(overview.vip.as_ref(), |row, vip| {
                        let badge = match vip.badge {
                            VipBadge::None => "普通账号",
                            VipBadge::Standard => "畅听会员",
                            VipBadge::Big => "大会员",
                            VipBadge::Welfare => "福利会员",
                        };
                        row.child(
                            div()
                                .flex_shrink_0()
                                .flex()
                                .flex_col()
                                .items_end()
                                .gap_1()
                                .child(
                                    div()
                                        .px_3()
                                        .py_1()
                                        .rounded_full()
                                        .bg(rgb(0xe9edff))
                                        .text_color(rgb(0x415de5))
                                        .text_size(px(11.))
                                        .child(badge),
                                )
                                .when(vip.badge != VipBadge::None, |column| {
                                    column.when_some(vip.expires_unix_ms, |column, expires| {
                                        column.child(
                                            div()
                                                .text_size(px(10.))
                                                .text_color(rgb(0x788196))
                                                .child(expiry_label(expires)),
                                        )
                                    })
                                }),
                        )
                    }),
            )
            .child(
                div()
                    .mt_4()
                    .grid()
                    .grid_cols(if compact { 2 } else { 4 })
                    .gap_3()
                    .children(
                        [
                            (
                                "关注",
                                overview
                                    .metadata
                                    .as_ref()
                                    .and_then(|data| data.follow_count),
                            ),
                            (
                                "粉丝",
                                overview.metadata.as_ref().and_then(|data| data.fans_count),
                            ),
                            (
                                "关注歌手",
                                overview
                                    .metadata
                                    .as_ref()
                                    .and_then(|data| data.follow_artist_count),
                            ),
                            (
                                "获赞",
                                overview.metadata.as_ref().and_then(|data| data.praised),
                            ),
                        ]
                        .into_iter()
                        .map(|(label, value)| {
                            div()
                                .min_w_0()
                                .px_3()
                                .py_2()
                                .rounded_md()
                                .bg(rgb(0xf5f7fb))
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap_2()
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(rgb(0x788196))
                                        .child(label),
                                )
                                .child(
                                    div()
                                        .text_size(px(18.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child(
                                            value
                                                .filter(|v| *v >= 0)
                                                .map_or_else(|| "—".into(), |v| v.to_string()),
                                        ),
                                )
                        }),
                    ),
            );
        let seconds = overview
            .playback
            .as_ref()
            .and_then(|data| data.play_seconds)
            .filter(|v| *v >= 0);
        let hours = seconds.map_or_else(|| "—".into(), |s| format!("{} 小时", s / 3600));
        let minutes = seconds.map_or_else(
            || "听歌时长暂不可用".into(),
            |s| format!("{} 分钟 · 累计听歌时长", s % 3600 / 60),
        );
        let plays = overview
            .playback
            .as_ref()
            .and_then(|data| data.play_count)
            .filter(|v| *v >= 0)
            .map_or_else(|| "—".into(), |v| v.to_string());
        let body = body
            .child(profile)
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_4()
                    .mb_6()
                    .child(stat_card("clock", "听歌时长", hours, minutes, true, rgb))
                    .child(stat_card(
                        "play",
                        "播放次数",
                        plays,
                        "累计播放的歌曲次数".into(),
                        false,
                        rgb,
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .mb_3()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("听歌月报"),
                    )
                    .child(
                        ui::icon_button_labeled(
                            "open-month-report",
                            "arrow-right",
                            "查看完整听歌月报",
                            cx,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.open_month_report(cx))),
                    ),
            )
            .child(
                div()
                    .id("stats-month-card")
                    .p_4()
                    .mb_6()
                    .rounded_lg()
                    .bg(rgb(0xf5f7fb))
                    .border_1()
                    .border_color(rgb(0xd8dce6))
                    .cursor_pointer()
                    .hover(|card| card.bg(rgb(0xe9edf5)))
                    .flex()
                    .items_center()
                    .gap_4()
                    .child(
                        self.cover(
                            overview
                                .month_summary
                                .as_ref()
                                .and_then(|summary| summary.cover_url.as_deref()),
                            76.,
                            cx,
                        ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(
                                div()
                                    .text_size(px(17.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(
                                        overview
                                            .month_summary
                                            .as_ref()
                                            .and_then(|summary| summary.month_desc.clone())
                                            .unwrap_or_else(|| "你的专属听歌月报".into()),
                                    ),
                            )
                            .when_some(overview.month_summary.as_ref(), |column, summary| {
                                column
                                    .when_some(
                                        summary.listen_total_minutes.as_ref(),
                                        |column, minutes| {
                                            column.child(
                                                div()
                                                    .text_sm()
                                                    .text_color(rgb(0x687083))
                                                    .child(format!("听歌 {minutes} 分钟")),
                                            )
                                        },
                                    )
                                    .when_some(summary.listen_chain.as_ref(), |column, chain| {
                                        column.child(
                                            div()
                                                .text_size(px(12.))
                                                .text_color(rgb(0x788196))
                                                .child(chain.clone()),
                                        )
                                    })
                            })
                            .when(overview.month_summary.is_none(), |column| {
                                column.child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(0x788196))
                                        .child("查看按月记录的听歌时光"),
                                )
                            }),
                    )
                    .child(ui::icon("arrow-right", rgb(0x415de5)))
                    .on_click(cx.listener(|this, _, _, cx| this.open_month_report(cx))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .mb_3()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("本周最爱"),
                    )
                    .when_some(overview.weekly.as_ref(), |row, weekly| {
                        row.child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgb(0x788196))
                                .child(format!("{} 首歌曲", weekly.len())),
                        )
                    }),
            );
        let Some(weekly) = &overview.weekly else {
            return body
                .child(empty_card("周排行暂时无法读取，请刷新重试", rgb))
                .into_any_element();
        };
        let generation = self.account_overview_generation;
        body.when(weekly.is_empty(), |body| {
            body.child(empty_card("本周暂无听歌排行", rgb))
        })
        .child(
            div()
                .grid()
                .grid_cols(if compact { 1 } else { 2 })
                .gap_3()
                .children(weekly.iter().enumerate().map(|(index, entry)| {
                    let track = &entry.track;
                    let id = track.id;
                    div()
                        .id(("weekly-track", index))
                        .min_w_0()
                        .h(px(74.))
                        .px_3()
                        .rounded_lg()
                        .bg(rgb(0xf5f7fb))
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .w(px(22.))
                                .flex_shrink_0()
                                .text_size(px(13.))
                                .text_color(rgb(if index < 3 { 0x415de5 } else { 0x788196 }))
                                .child(format!("{:02}", index + 1)),
                        )
                        .child(self.cover(track.cover_url.as_deref(), 44., cx))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap_1()
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(13.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .child(track.title.clone()),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(12.))
                                        .text_color(rgb(0x788196))
                                        .child(track.artist_text.clone()),
                                ),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_size(px(11.))
                                .text_color(rgb(0x788196))
                                .child(format!("{} 次", entry.play_count)),
                        )
                        .when(id > 0, |row| {
                            row.cursor_pointer()
                                .hover(|row| row.bg(rgb(0xe9edf5)))
                                .on_click(cx.listener(move |this, _, _, _| {
                                    if this.page == Page::AccountOverview
                                        && this.account_overview_generation == generation
                                    {
                                        let _ = this
                                            .service
                                            .commands
                                            .send(Command::PlayWeeklyTrack(generation, index, id));
                                    }
                                }))
                        })
                })),
        )
        .into_any_element()
    }
}

fn stat_card(
    icon: &str,
    label: &'static str,
    value: String,
    detail: String,
    featured: bool,
    rgb: impl Fn(u32) -> Rgba + Copy,
) -> Div {
    div()
        .min_w_0()
        .h(px(136.))
        .p_4()
        .rounded_lg()
        .bg(rgb(if featured { 0xe9edff } else { 0xf5f7fb }))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .text_size(px(12.))
                .text_color(rgb(if featured { 0x415de5 } else { 0x687083 }))
                .child(
                    ui::icon(icon, rgb(if featured { 0x415de5 } else { 0x687083 })).size(px(17.)),
                )
                .child(label),
        )
        .child(
            div()
                .mt_2()
                .text_size(px(30.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(value),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(rgb(0x788196))
                .child(detail),
        )
}
fn empty_card(message: &'static str, rgb: impl Fn(u32) -> Rgba + Copy) -> Div {
    div()
        .p_5()
        .rounded_lg()
        .bg(rgb(0xf5f7fb))
        .flex()
        .items_center()
        .gap_3()
        .child(ui::icon("music", rgb(0x8a91a2)))
        .child(div().text_sm().text_color(rgb(0x788196)).child(message))
}

fn expiry_label(expires: i64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let remaining = expires.saturating_sub(i64::try_from(now.as_millis()).unwrap_or(i64::MAX));
    match remaining {
        ..=0 => "会员已到期".into(),
        1..=86_400_000 => "距到期不足一天".into(),
        _ => format!("距到期约 {} 天", remaining / 86_400_000),
    }
}
