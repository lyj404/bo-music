use super::*;

#[test]
fn audio_effect_commands_preserve_custom_curves_and_clamp_bands() {
    let initial = AudioEffects::default();
    let selected =
        changed_audio_effects(&initial, AudioEffectAction::Select("galaxy.bass".into())).unwrap();
    assert!(selected.enabled);
    assert_eq!(selected.preset_id, "galaxy.bass");
    let custom = changed_audio_effects(&selected, AudioEffectAction::AdjustBand(0, 30.0)).unwrap();
    assert_eq!(custom.preset_id, "custom.gpui");
    assert_eq!(custom.custom_presets[0].bands[0], 12.0);
    let adjusted = changed_audio_effects(&custom, AudioEffectAction::AdjustBand(9, -2.0)).unwrap();
    assert_eq!(adjusted.custom_presets[0].bands[9], -2.0);
    assert!(changed_audio_effects(&adjusted, AudioEffectAction::AdjustBand(10, 1.0)).is_err());
    let off = changed_audio_effects(&adjusted, AudioEffectAction::Toggle).unwrap();
    assert!(!off.enabled);
    assert_eq!(off.custom_presets, adjusted.custom_presets);
    let ambient = changed_audio_effects(
        &off,
        AudioEffectAction::SelectAmbient("ambient.rain".into()),
    )
    .unwrap();
    let ambient =
        changed_audio_effects(&ambient, AudioEffectAction::AdjustAmbientVolume(200.0)).unwrap();
    assert!(ambient.enabled);
    assert_eq!(ambient.ambient_id, "ambient.rain");
    assert_eq!(ambient.ambient_volume, 100.0);
}

#[test]
fn custom_presets_can_be_created_renamed_reset_and_deleted() {
    let current = AudioEffects::default();
    let created = changed_audio_effects(
        &current,
        AudioEffectAction::CreateCustom {
            name: " 自己的音效 ".into(),
            description: "试听".into(),
        },
    )
    .unwrap();
    assert_eq!(created.custom_presets[0].name, "自己的音效");
    let changed = changed_audio_effects(&created, AudioEffectAction::AdjustBand(3, 4.0)).unwrap();
    let renamed = changed_audio_effects(
        &changed,
        AudioEffectAction::RenameCustom {
            name: "新名称".into(),
            description: String::new(),
        },
    )
    .unwrap();
    assert_eq!(renamed.custom_presets[0].name, "新名称");
    assert_eq!(renamed.custom_presets[0].description, "试听");
    let reset = changed_audio_effects(&renamed, AudioEffectAction::ResetBands).unwrap();
    assert_eq!(reset.custom_presets[0].bands, [0.0; 10]);
    let removed = changed_audio_effects(&reset, AudioEffectAction::DeleteCustom).unwrap();
    assert!(removed.custom_presets.is_empty());
    assert!(removed.preset_id.is_empty());
    assert!(changed_audio_effects(&removed, AudioEffectAction::DeleteCustom).is_err());
}
