//! Compile the effect settings into a libmpv lavfi graph.
use bowen_domain::audio_effects::{AudioEffects, EQ_FREQUENCIES, PRESETS};

fn number(value: f64) -> String {
    let mut result = format!("{value:.3}");
    while result.contains('.') && result.ends_with('0') {
        result.pop();
    }
    if result.ends_with('.') {
        result.pop();
    }
    if result == "-0" {
        result = "0".into();
    }
    result
}

/// Input ordering is main signal, optional vinyl texture, then ambient.
pub fn mix_weights(settings: &AudioEffects, has_texture: bool) -> String {
    let mut weights = vec!["1".to_owned()];
    if has_texture {
        weights.push("0.5".into());
    }
    let volume = if settings.ambient_volume.is_finite() {
        settings.ambient_volume.clamp(0.0, 100.0)
    } else {
        30.0
    };
    weights.push(number(volume / 100.0));
    weights.join(" ")
}

pub fn custom_eq_chain(bands: &[f64; 10]) -> String {
    let max_gain = bands
        .iter()
        .copied()
        .filter(|gain| gain.is_finite())
        .map(|gain| gain.clamp(-12.0, 12.0))
        .fold(0.0_f64, f64::max);
    let mut stages = Vec::new();
    if max_gain > 0.001 {
        stages.push(format!("volume={}dB", number(-max_gain)));
    }
    for (frequency, gain) in EQ_FREQUENCIES.iter().zip(bands) {
        if gain.is_finite() && gain.abs() >= 0.05 {
            stages.push(format!(
                "equalizer=f={frequency}:t=q:w=1.2:g={}",
                number(gain.clamp(-12.0, 12.0))
            ));
        }
    }
    if !stages.is_empty() {
        stages.push("alimiter=limit=0.95:level=0".into());
    }
    stages.join(",")
}

/// Inputs must be existing files. Missing optional sources are omitted by the caller.
pub fn build_graph(
    settings: &AudioEffects,
    ambient_path: Option<&str>,
    texture_path: Option<&str>,
) -> Option<String> {
    if !settings.enabled {
        return None;
    }
    let preset = PRESETS
        .iter()
        .find(|p| p.id.eq_ignore_ascii_case(&settings.preset_id));
    let chain = settings
        .custom_presets
        .iter()
        .find(|p| p.id.eq_ignore_ascii_case(&settings.preset_id))
        .map(|p| custom_eq_chain(&p.bands))
        .or_else(|| preset.map(|p| p.filter.to_owned()))
        .unwrap_or_default();
    let ambient = (!settings.ambient_id.is_empty())
        .then_some(ambient_path)
        .flatten()
        .filter(|s| !s.is_empty());
    let texture = preset
        .and_then(|p| p.texture)
        .and(texture_path)
        .filter(|s| !s.is_empty());
    if chain.is_empty() && ambient.is_none() {
        return None;
    }
    if ambient.is_none() && texture.is_none() {
        return Some(format!("@bowenFx:lavfi=[{chain}]"));
    }
    let mut parts = Vec::new();
    let mut inputs = Vec::new();
    if chain.is_empty() {
        parts.push("anull[main]".to_string());
        inputs.push("[main]");
    } else {
        parts.push(format!("anull[main];[main]{chain}[fx]"));
        inputs.push("[fx]");
    }
    let mut weights = vec!["1".to_string()];
    if let Some(path) = texture {
        parts.push(format!(
            "amovie={}:loop=0,asetpts=N/SR/TB[tx]",
            escape_path(path)
        ));
        inputs.push("[tx]");
        weights.push("0.5".into());
    }
    if let Some(path) = ambient {
        parts.push(format!(
            "amovie={}:loop=0,asetpts=N/SR/TB[amb]",
            escape_path(path)
        ));
        inputs.push("[amb]");
        let volume = if settings.ambient_volume.is_finite() {
            settings.ambient_volume.clamp(0.0, 100.0)
        } else {
            30.0
        };
        weights.push(number(volume / 100.0));
    }
    parts.push(format!(
        "{}amix=inputs={}:weights={}:normalize=0:duration=shortest",
        inputs.concat(),
        inputs.len(),
        weights.join(" ")
    ));
    Some(format!("@bowenFx:lavfi=[{}]", parts.join(";")))
}

fn escape_path(path: &str) -> String {
    let mut result = String::with_capacity(path.len());
    for ch in path.chars() {
        match ch {
            '\\' => {
                result.push('/');
                continue;
            }
            '\'' => result.push_str("\\\\\\"),
            ':' | '=' => result.push_str("\\\\"),
            '[' | ']' | ',' | ';' => result.push('\\'),
            _ => {}
        }
        result.push(ch);
    }
    result
}

#[cfg(test)]
#[path = "effects_tests.rs"]
mod tests;
