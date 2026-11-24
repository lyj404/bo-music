//! Audio-effect settings and filter construction.

use super::*;

pub(super) fn changed_audio_effects(
    current: &AudioEffects,
    action: AudioEffectAction,
) -> Result<AudioEffects, &'static str> {
    let mut next = current.clone();
    match action {
        AudioEffectAction::Toggle => next.enabled = !next.enabled,
        AudioEffectAction::Select(id) => {
            if !id.is_empty() {
                next.enabled = true;
            }
            next.preset_id = id;
        }
        AudioEffectAction::Set(value) => next = value,
        AudioEffectAction::SelectAmbient(id) => {
            if !id.is_empty() {
                next.enabled = true;
            }
            next.ambient_id = id;
        }
        AudioEffectAction::AdjustAmbientVolume(delta) => {
            if !delta.is_finite() {
                return Err("无效的环境音音量");
            }
            next.ambient_volume = (next.ambient_volume + delta).clamp(0.0, 100.0);
        }
        AudioEffectAction::CreateCustom { name, description } => {
            if next.custom_presets.len() >= MAX_CUSTOM_PRESETS {
                return Err("自定义预设已达上限");
            }
            let number = (1..=MAX_CUSTOM_PRESETS)
                .find(|number| {
                    let id = format!("custom.gpui.{number}");
                    !next
                        .custom_presets
                        .iter()
                        .any(|preset| preset.id.eq_ignore_ascii_case(&id))
                })
                .ok_or("自定义预设已达上限")?;
            let id = format!("custom.gpui.{number}");
            let name = if name.trim().is_empty() {
                format!("自定义 {number}")
            } else {
                name
            };
            next.custom_presets.push(CustomPreset {
                id: id.clone(),
                name,
                description,
                bands: [0.0; 10],
            });
            next.preset_id = id;
            next.enabled = true;
        }
        AudioEffectAction::RenameCustom { name, description } => {
            let preset = next
                .custom_presets
                .iter_mut()
                .find(|preset| preset.id.eq_ignore_ascii_case(&next.preset_id))
                .ok_or("请先选择自定义预设")?;
            if !name.trim().is_empty() {
                preset.name = name;
            }
            if !description.trim().is_empty() {
                preset.description = description;
            }
        }
        AudioEffectAction::DeleteCustom => {
            let old_len = next.custom_presets.len();
            next.custom_presets
                .retain(|preset| !preset.id.eq_ignore_ascii_case(&next.preset_id));
            if old_len == next.custom_presets.len() {
                return Err("请先选择自定义预设");
            }
            next.preset_id.clear();
        }
        AudioEffectAction::ResetBands => {
            let preset = next
                .custom_presets
                .iter_mut()
                .find(|preset| preset.id.eq_ignore_ascii_case(&next.preset_id))
                .ok_or("请先选择自定义预设")?;
            preset.bands = [0.0; 10];
        }
        AudioEffectAction::AdjustBand(index, delta) => {
            if index >= 10 || !delta.is_finite() {
                return Err("无效的均衡器参数");
            }
            let selected = next
                .custom_presets
                .iter()
                .position(|p| p.id.eq_ignore_ascii_case(&next.preset_id));
            let index_of_preset = if let Some(index) = selected {
                index
            } else {
                let id = "custom.gpui";
                match next
                    .custom_presets
                    .iter()
                    .position(|preset| preset.id.eq_ignore_ascii_case(id))
                {
                    Some(index) => index,
                    None if next.custom_presets.len() < MAX_CUSTOM_PRESETS => {
                        next.custom_presets.push(CustomPreset {
                            id: id.into(),
                            name: "自定义".into(),
                            description: "自定义十段均衡曲线".into(),
                            bands: [0.0; 10],
                        });
                        next.custom_presets.len() - 1
                    }
                    None => return Err("自定义预设已达上限"),
                }
            };
            let preset = &mut next.custom_presets[index_of_preset];
            preset.bands[index] = (preset.bands[index] + delta).clamp(-12.0, 12.0);
            next.preset_id = preset.id.clone();
            next.enabled = true;
        }
    }
    next.normalize();
    Ok(next)
}

fn cached_effect_asset(root: &Path, id: &str) -> std::io::Result<PathBuf> {
    let (name, bytes) = bo_music_playback::ambient::asset(id).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "unknown audio effect asset")
    })?;
    let path = root.join(name);
    if std::fs::metadata(&path).map_or(true, |metadata| metadata.len() != bytes.len() as u64) {
        atomic::replace(&path, bytes)?;
    }
    Ok(path)
}

pub(super) fn audio_filter(
    settings: &AudioEffects,
    root: &Path,
) -> std::io::Result<Option<String>> {
    if !settings.enabled {
        return Ok(None);
    }
    let ambient = if settings.ambient_id.is_empty() {
        None
    } else {
        Some(cached_effect_asset(root, &settings.ambient_id)?)
    };
    let texture = PRESETS
        .iter()
        .find(|preset| preset.id == settings.preset_id)
        .and_then(|preset| preset.texture)
        .map(|id| cached_effect_asset(root, id))
        .transpose()?;
    fn unicode_path(path: &Path) -> std::io::Result<&str> {
        path.to_str().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "non-Unicode audio asset path",
            )
        })
    }
    let ambient = ambient.as_deref().map(unicode_path).transpose()?;
    let texture = texture.as_deref().map(unicode_path).transpose()?;
    Ok(bo_music_playback::effects::build_graph(
        settings, ambient, texture,
    ))
}
