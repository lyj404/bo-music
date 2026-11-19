use super::{MusicApp, Page};
use bowen_app::desktop_service::Command;
use bowen_domain::account::VipBadge;
use gpui::{AnyElement, Context, FontWeight, div, prelude::*};
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

    pub(super) fn render_account_overview(&self, cx: &mut Context<Self>) -> AnyElement {
        let rgb = crate::appearance::paint(cx);
        let body = div()
            .id("account-overview")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p_3();
        if self.auth.busy {
            return body.child("正在读取账号…").into_any_element();
        }
        let Some(account) = &self.auth.account else {
            return body
                .child("登录后可查看个人资料和听歌统计")
                .child(
                    div()
                        .id("stats-login")
                        .cursor_pointer()
                        .text_color(rgb(0x415de5))
                        .child("登录")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_account = true;
                            let _ = this.service.commands.send(Command::RefreshAccounts);
                            cx.notify();
                        })),
                )
                .into_any_element();
        };
        if self.month_page.is_open() {
            return self.render_month_report(cx);
        }
        let body = body.child(
            div()
                .flex()
                .justify_between()
                .items_center()
                .pb_3()
                .child(
                    div().text_xl().font_weight(FontWeight::BOLD).child(
                        account
                            .nickname
                            .clone()
                            .filter(|name| !name.trim().is_empty())
                            .unwrap_or_else(|| format!("账号 {}", account.uid)),
                    ),
                )
                .child(
                    div()
                        .id("stats-refresh")
                        .cursor_pointer()
                        .text_color(rgb(0x415de5))
                        .child("刷新")
                        .on_click(cx.listener(|this, _, _, cx| this.load_account_overview(cx))),
                ),
        );
        let overview = match self.account_overview.as_deref() {
            Some(Ok(overview)) => overview,
            Some(Err(())) => {
                return body
                    .child("无法读取账号统计，请刷新重试")
                    .into_any_element();
            }
            None => return body.child("正在加载统计…").into_any_element(),
        };
        let body = body
            .child(title("个人资料"))
            .child(
                div().flex().flex_wrap().gap_4().pb_2().children([
                    metric(
                        "关注",
                        overview
                            .metadata
                            .as_ref()
                            .and_then(|data| data.follow_count),
                    ),
                    metric(
                        "粉丝",
                        overview.metadata.as_ref().and_then(|data| data.fans_count),
                    ),
                    metric(
                        "关注歌手",
                        overview
                            .metadata
                            .as_ref()
                            .and_then(|data| data.follow_artist_count),
                    ),
                    metric(
                        "获赞",
                        overview.metadata.as_ref().and_then(|data| data.praised),
                    ),
                ]),
            )
            .when_some(overview.vip.as_ref(), |body, vip| {
                let label = match vip.badge {
                    VipBadge::None => "普通账号",
                    VipBadge::Standard => "畅听会员",
                    VipBadge::Big => "大会员",
                    VipBadge::Welfare => "福利会员",
                };
                body.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_4()
                        .pb_2()
                        .child(label)
                        .when_some(vip.created_at.as_ref(), |row, created| {
                            row.child(format!("注册于 {created}"))
                        })
                        .when(vip.badge != VipBadge::None, |row| {
                            row.when_some(vip.expires_unix_ms, |row, expires| {
                                row.child(expiry_label(expires))
                            })
                        }),
                )
            })
            .child(title("累计听歌"))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_4()
                    .pb_2()
                    .child(metric(
                        "播放次数",
                        overview.playback.as_ref().and_then(|data| data.play_count),
                    ))
                    .child(format!(
                        "听歌时长 {}",
                        overview
                            .playback
                            .as_ref()
                            .and_then(|data| data.play_seconds)
                            .filter(|seconds| *seconds >= 0)
                            .map(|seconds| format!(
                                "{} 小时 {} 分钟",
                                seconds / 3600,
                                seconds % 3600 / 60
                            ))
                            .unwrap_or_else(|| "暂无数据".into())
                    )),
            )
            .child(
                div()
                    .id("open-month-report")
                    .cursor_pointer()
                    .text_color(rgb(0x415de5))
                    .child("查看听歌月报")
                    .on_click(cx.listener(|this, _, _, cx| this.open_month_report(cx))),
            )
            .when_some(overview.month_summary.as_ref(), |body, summary| {
                body.child(title("最近月报")).child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .pb_2()
                        .child(self.cover(summary.cover_url.as_deref(), 52.0, cx))
                        .child(summary.month_desc.clone().unwrap_or_default())
                        .when_some(summary.listen_total_minutes.as_ref(), |row, minutes| {
                            row.child(format!("听歌 {minutes} 分钟"))
                        })
                        .when_some(summary.listen_chain.as_ref(), |row, chain| {
                            row.child(format!("连续听歌 {chain}"))
                        }),
                )
            })
            .child(title("本周最爱"));

        let Some(weekly) = &overview.weekly else {
            return body
                .child("暂时无法读取周排行，请刷新重试")
                .into_any_element();
        };
        let generation = self.account_overview_generation;
        body.when(weekly.is_empty(), |body| body.child("本周暂无听歌排行"))
            .children(
                weekly
                    .iter()
                    .enumerate()
                    .map(|(index, entry)| {
                        let track = &entry.track;
                        let id = track.id;
                        div()
                            .id(("weekly-track", index))
                            .flex()
                            .items_center()
                            .gap_3()
                            .p_2()
                            .rounded_md()
                            .cursor_pointer()
                            .hover(|style| style.bg(rgb(0xe9edf5)))
                            .child((index + 1).to_string())
                            .child(self.cover(track.cover_url.as_deref(), 42.0, cx))
                            .child(
                                div().flex_1().min_w_0().child(track.title.clone()).child(
                                    div()
                                        .text_sm()
                                        .text_color(rgb(0x687083))
                                        .child(track.artist_text.clone()),
                                ),
                            )
                            .child(format!("{} 次", entry.play_count))
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
                    .collect::<Vec<_>>(),
            )
            .into_any_element()
    }
}

fn title(text: &'static str) -> impl IntoElement {
    div()
        .pt_3()
        .pb_2()
        .font_weight(FontWeight::BOLD)
        .child(text)
}

fn metric(label: &str, value: Option<i64>) -> String {
    format!(
        "{label} {}",
        value.map_or_else(|| "暂无数据".to_owned(), |number| number.to_string())
    )
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
