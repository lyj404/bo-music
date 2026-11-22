use super::*;

impl MusicApp {
    pub(super) fn render_effects_panel(
        &self,
        embedded: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let rgb = appearance::paint(cx);
        if let Some(player) = &self.player {
            let effects = &player.audio_effects;
            let selected = effects.preset_id.clone();
            let bands = effects
                .custom_presets
                .iter()
                .find(|preset| preset.id.eq_ignore_ascii_case(&selected))
                .or_else(|| {
                    effects
                        .custom_presets
                        .iter()
                        .find(|preset| preset.id.eq_ignore_ascii_case("custom.gpui"))
                })
                .map_or([0.0; 10], |preset| preset.bands);
            let preset_rows = bo_music_domain::audio_effects::PRESETS
                .iter()
                .map(|preset| (preset.id.to_owned(), preset.name.to_owned()))
                .chain(
                    effects
                        .custom_presets
                        .iter()
                        .map(|preset| (preset.id.clone(), preset.name.clone())),
                )
                .enumerate()
                .map(|(index, (id, name))| {
                    let is_selected = id == selected;
                    div()
                        .id(("effect-preset", index))
                        .w(px(126.))
                        .h(px(54.))
                        .flex_shrink_0()
                        .px_3()
                        .flex()
                        .items_center()
                        .justify_between()
                        .rounded_md()
                        .border_1()
                        .border_color(rgb(0xd8dce6))
                        .cursor_pointer()
                        .when(is_selected, |row| {
                            row.bg(rgb(0xe9edff))
                                .border_color(rgb(0x415de5))
                                .text_color(rgb(0x415de5))
                        })
                        .hover(|row| row.bg(rgb(0xe9edf5)))
                        .child(div().truncate().child(name))
                        .when(is_selected, |card| card.child("✓"))
                        .on_click(cx.listener(move |this, _, _, _| {
                            let _ = this
                                .service
                                .commands
                                .send(Command::AudioEffect(AudioEffectAction::Select(id.clone())));
                        }))
                })
                .collect::<Vec<_>>();
            let eq_rows = bo_music_domain::audio_effects::EQ_FREQUENCIES
                .iter()
                .enumerate()
                .map(|(index, frequency)| {
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px_2()
                        .py_1()
                        .rounded_md()
                        .text_sm()
                        .child(format!("{frequency} Hz"))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(
                                    div()
                                        .id(("eq-down", index))
                                        .cursor_pointer()
                                        .child("−")
                                        .on_click(cx.listener(move |this, _, _, _| {
                                            let _ =
                                                this.service.commands.send(Command::AudioEffect(
                                                    AudioEffectAction::AdjustBand(index, -1.0),
                                                ));
                                        })),
                                )
                                .child(format!("{:+.1} dB", bands[index]))
                                .child(
                                    div()
                                        .id(("eq-up", index))
                                        .cursor_pointer()
                                        .child("+")
                                        .on_click(cx.listener(move |this, _, _, _| {
                                            let _ =
                                                this.service.commands.send(Command::AudioEffect(
                                                    AudioEffectAction::AdjustBand(index, 1.0),
                                                ));
                                        })),
                                ),
                        )
                })
                .collect::<Vec<_>>();
            let ambient_rows = bo_music_domain::audio_effects::AMBIENT_SOUNDS
                .iter()
                .enumerate()
                .map(|(index, sound)| {
                    let id = sound.id;
                    div()
                        .id(("ambient-preset", index))
                        .w(px(126.))
                        .flex_shrink_0()
                        .px_3()
                        .py_2()
                        .border_1()
                        .border_color(rgb(0xd8dce6))
                        .rounded_md()
                        .cursor_pointer()
                        .when(effects.ambient_id == id, |row| {
                            row.bg(rgb(0xe9edff)).text_color(rgb(0x415de5))
                        })
                        .hover(|row| row.bg(rgb(0xe9edf5)))
                        .child(sound.name)
                        .on_click(cx.listener(move |this, _, _, _| {
                            let _ = this.service.commands.send(Command::AudioEffect(
                                AudioEffectAction::SelectAmbient(id.into()),
                            ));
                        }))
                })
                .collect::<Vec<_>>();
            div()
                .id("effects-panel")
                .flex()
                .flex_col()
                .gap_2()
                .when(!embedded, |panel| {
                    panel.flex_1().min_h_0().overflow_y_scroll()
                })
                .child(
                    div()
                        .id("effects-toggle")
                        .p_3()
                        .mb_2()
                        .rounded_md()
                        .bg(rgb(0xf5f7fb))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_3()
                        .cursor_pointer()
                        .child(div().child("启用音效").child(
                            div().text_size(px(11.)).text_color(rgb(0x788196)).child(
                                if effects.enabled {
                                    "已启用，选择预设以调整听感"
                                } else {
                                    "已关闭，开启后应用所选预设"
                                },
                            ),
                        ))
                        .child(
                            div()
                                .w(px(36.))
                                .h(px(20.))
                                .px(px(3.))
                                .rounded_full()
                                .flex()
                                .items_center()
                                .bg(rgb(if effects.enabled { 0x4d66e5 } else { 0xd8dce6 }))
                                .when(effects.enabled, |track| track.justify_end())
                                .child(div().size(px(14.)).rounded_full().bg(gpui::rgb(0xffffff))),
                        )
                        .on_click(cx.listener(|this, _, _, _| {
                            let _ = this
                                .service
                                .commands
                                .send(Command::AudioEffect(AudioEffectAction::Toggle));
                        })),
                )
                .child(
                    div()
                        .p_2()
                        .font_weight(gpui::FontWeight::BOLD)
                        .child("音效预设"),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            div()
                                .id("effect-none")
                                .w(px(126.))
                                .h(px(54.))
                                .flex_shrink_0()
                                .px_3()
                                .rounded_md()
                                .border_1()
                                .border_color(rgb(if selected.is_empty() {
                                    0x415de5
                                } else {
                                    0xd8dce6
                                }))
                                .when(selected.is_empty(), |card| {
                                    card.bg(rgb(0xe9edff)).text_color(rgb(0x415de5))
                                })
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child("原声")
                                .when(selected.is_empty(), |card| card.child("✓"))
                                .hover(|card| card.bg(rgb(0xe9edf5)))
                                .on_click(cx.listener(|this, _, _, _| {
                                    let _ = this.service.commands.send(Command::AudioEffect(
                                        AudioEffectAction::Select(String::new()),
                                    ));
                                })),
                        )
                        .children(preset_rows),
                )
                .child(
                    div()
                        .pt_4()
                        .px_2()
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child("自定义预设"),
                )
                .child(div().p_2().child(self.preset_name_input.clone()))
                .child(div().p_2().child(self.preset_description_input.clone()))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_3()
                        .p_2()
                        .text_sm()
                        .child(
                            div()
                                .id("preset-create")
                                .cursor_pointer()
                                .text_color(rgb(0x415de5))
                                .child("新建")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let name = this.preset_name_input.read(cx).text().to_owned();
                                    let description =
                                        this.preset_description_input.read(cx).text().to_owned();
                                    let _ = this.service.commands.send(Command::AudioEffect(
                                        AudioEffectAction::CreateCustom { name, description },
                                    ));
                                })),
                        )
                        .child(
                            ui::icon_button_labeled("preset-rename", "edit", "修改自定义预设", cx)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let name = this.preset_name_input.read(cx).text().to_owned();
                                    let description =
                                        this.preset_description_input.read(cx).text().to_owned();
                                    let _ = this.service.commands.send(Command::AudioEffect(
                                        AudioEffectAction::RenameCustom { name, description },
                                    ));
                                })),
                        )
                        .child(
                            ui::icon_button_labeled(
                                "preset-delete",
                                "delete",
                                "删除自定义预设",
                                cx,
                            )
                            .on_click(cx.listener(|this, _, _, _| {
                                let _ = this
                                    .service
                                    .commands
                                    .send(Command::AudioEffect(AudioEffectAction::DeleteCustom));
                            })),
                        ),
                )
                .child(
                    div()
                        .p_2()
                        .font_weight(gpui::FontWeight::BOLD)
                        .child("十段均衡器"),
                )
                .child(
                    div()
                        .id("eq-reset")
                        .p_2()
                        .text_sm()
                        .cursor_pointer()
                        .text_color(rgb(0x415de5))
                        .child("重置均衡器")
                        .on_click(cx.listener(|this, _, _, _| {
                            let _ = this
                                .service
                                .commands
                                .send(Command::AudioEffect(AudioEffectAction::ResetBands));
                        })),
                )
                .children(eq_rows)
                .child(
                    div()
                        .p_2()
                        .font_weight(gpui::FontWeight::BOLD)
                        .child("环境音"),
                )
                .child(
                    div()
                        .id("ambient-none")
                        .p_2()
                        .cursor_pointer()
                        .child("关闭环境音")
                        .on_click(cx.listener(|this, _, _, _| {
                            let _ = this.service.commands.send(Command::AudioEffect(
                                AudioEffectAction::SelectAmbient(String::new()),
                            ));
                        })),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_3()
                        .p_2()
                        .text_sm()
                        .child(
                            div()
                                .id("ambient-down")
                                .cursor_pointer()
                                .child("−")
                                .on_click(cx.listener(|this, _, _, _| {
                                    let _ = this.service.commands.send(Command::AudioEffect(
                                        AudioEffectAction::AdjustAmbientVolume(-10.0),
                                    ));
                                })),
                        )
                        .child(format!("音量 {:.0}%", effects.ambient_volume))
                        .child(div().id("ambient-up").cursor_pointer().child("+").on_click(
                            cx.listener(|this, _, _, _| {
                                let _ = this.service.commands.send(Command::AudioEffect(
                                    AudioEffectAction::AdjustAmbientVolume(10.0),
                                ));
                            }),
                        )),
                )
                .child(div().flex().flex_wrap().gap_2().children(ambient_rows))
                .into_any_element()
        } else {
            div().child("播放器不可用").into_any_element()
        }
    }
}
