use super::*;

impl MusicApp {
    pub(super) fn render_effects_panel(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
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
            let preset_rows = bowen_domain::audio_effects::PRESETS
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
                        .p_2()
                        .rounded_md()
                        .cursor_pointer()
                        .when(is_selected, |row| {
                            row.bg(rgb(0xe9edff)).text_color(rgb(0x415de5))
                        })
                        .hover(|row| row.bg(rgb(0xe9edf5)))
                        .child(name)
                        .on_click(cx.listener(move |this, _, _, _| {
                            let _ = this
                                .service
                                .commands
                                .send(Command::AudioEffect(AudioEffectAction::Select(id.clone())));
                        }))
                })
                .collect::<Vec<_>>();
            let eq_rows = bowen_domain::audio_effects::EQ_FREQUENCIES
                .iter()
                .enumerate()
                .map(|(index, frequency)| {
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .py_1()
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
            let ambient_rows = bowen_domain::audio_effects::AMBIENT_SOUNDS
                .iter()
                .enumerate()
                .map(|(index, sound)| {
                    let id = sound.id;
                    div()
                        .id(("ambient-preset", index))
                        .p_2()
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
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(
                    div()
                        .id("effects-toggle")
                        .p_2()
                        .cursor_pointer()
                        .text_color(rgb(0x415de5))
                        .child(if effects.enabled {
                            "关闭音效"
                        } else {
                            "启用音效"
                        })
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
                        .id("effect-none")
                        .p_2()
                        .cursor_pointer()
                        .child("无预设")
                        .on_click(cx.listener(|this, _, _, _| {
                            let _ = this.service.commands.send(Command::AudioEffect(
                                AudioEffectAction::Select(String::new()),
                            ));
                        })),
                )
                .children(preset_rows)
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
                            div()
                                .id("preset-rename")
                                .cursor_pointer()
                                .text_color(rgb(0x415de5))
                                .child("修改")
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
                            div()
                                .id("preset-delete")
                                .cursor_pointer()
                                .text_color(rgb(0x415de5))
                                .child("删除")
                                .on_click(cx.listener(|this, _, _, _| {
                                    let _ = this.service.commands.send(Command::AudioEffect(
                                        AudioEffectAction::DeleteCustom,
                                    ));
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
                .children(ambient_rows)
                .into_any_element()
        } else {
            div().child("播放器不可用").into_any_element()
        }
    }
}
