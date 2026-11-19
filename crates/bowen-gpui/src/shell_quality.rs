use super::*;

impl MusicApp {
    pub(super) fn render_quality_panel(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        if let Some(player) = &self.player {
            let actual = player.current_quality.as_ref().map_or_else(
                || "尚未播放".to_owned(),
                |quality| {
                    let label = quality.requested.map(quality_name).unwrap_or("试听");
                    let bitrate = if quality.bitrate_kbps > 0 {
                        format!(" · {} kbps", quality.bitrate_kbps)
                    } else {
                        String::new()
                    };
                    format!(
                        "{label} · {}{bitrate}{}",
                        quality.format.to_uppercase(),
                        if quality.downgraded {
                            " · 已降档"
                        } else {
                            ""
                        }
                    )
                },
            );
            let choices = [
                bowen_domain::track::AudioQuality::Standard,
                bowen_domain::track::AudioQuality::High,
                bowen_domain::track::AudioQuality::Lossless,
            ];
            let current_buttons = choices
                .into_iter()
                .enumerate()
                .map(|(index, quality)| {
                    let selected = player.current_quality.as_ref().is_some_and(|current| {
                        current.requested == Some(quality) && !current.downgraded
                    });
                    let available =
                        player.can_change_quality && player.available_qualities.contains(&quality);
                    div()
                        .id(("current-quality", index))
                        .p_2()
                        .rounded_md()
                        .child(quality_name(quality))
                        .text_color(if available {
                            rgb(0x415de5)
                        } else {
                            rgb(0xaab0bd)
                        })
                        .when(selected, |row| row.bg(rgb(0xe9edff)))
                        .when(available, |row| {
                            row.cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, _| {
                                    let _ =
                                        this.service.commands.send(Command::SwitchQuality(quality));
                                }))
                        })
                })
                .collect::<Vec<_>>();
            let default_buttons = choices
                .into_iter()
                .enumerate()
                .map(|(index, quality)| {
                    div()
                        .id(("default-quality", index))
                        .p_2()
                        .rounded_md()
                        .cursor_pointer()
                        .child(quality_name(quality))
                        .text_color(rgb(0x415de5))
                        .when(player.preferred_quality == quality, |row| {
                            row.bg(rgb(0xe9edff))
                        })
                        .on_click(cx.listener(move |this, _, _, _| {
                            let _ = this
                                .service
                                .commands
                                .send(Command::SetPreferredQuality(quality));
                        }))
                })
                .collect::<Vec<_>>();
            div()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .p_2()
                        .font_weight(gpui::FontWeight::BOLD)
                        .child("当前歌曲"),
                )
                .child(div().p_2().text_sm().child(actual))
                .when(player.changing_quality, |view| {
                    view.child(div().p_2().text_sm().child("正在切换…"))
                })
                .child(div().flex().flex_wrap().children(current_buttons))
                .child(
                    div()
                        .p_2()
                        .font_weight(gpui::FontWeight::BOLD)
                        .child("默认音质"),
                )
                .child(
                    div()
                        .px_2()
                        .text_sm()
                        .text_color(rgb(0x8a91a2))
                        .child("用于之后播放的歌曲"),
                )
                .child(div().flex().flex_wrap().children(default_buttons))
                .into_any_element()
        } else {
            div().child("播放器不可用").into_any_element()
        }
    }
}
