use super::*;
use bowen_domain::audio_effects::CustomPreset;

#[test]
fn equalizer_clamps_gain_and_leaves_headroom() {
    assert_eq!(custom_eq_chain(&[0.0; 10]), "");
    let mut bands = [0.0; 10];
    bands[0] = 20.0;
    bands[5] = -3.5;
    bands[9] = f64::NAN;
    assert_eq!(
        custom_eq_chain(&bands),
        "volume=-12dB,equalizer=f=31:t=q:w=1.2:g=12,equalizer=f=1000:t=q:w=1.2:g=-3.5,alimiter=limit=0.95:level=0"
    );
}

#[test]
fn graph_combines_preset_and_ambient_without_preventing_eof() {
    let settings = AudioEffects {
        enabled: true,
        preset_id: "custom.one".into(),
        ambient_id: "ambient.rain".into(),
        ambient_volume: 25.0,
        custom_presets: vec![CustomPreset {
            id: "custom.one".into(),
            name: "a".into(),
            description: "b".into(),
            bands: [1.0; 10],
        }],
    };
    let graph = build_graph(&settings, Some("C:\\音频\\rain:a,b.wav"), None).unwrap();
    assert!(graph.contains("C\\\\:/音频/rain\\\\:a\\,b.wav"));
    assert!(graph.contains("weights=1 0.25:normalize=0:duration=shortest"));
    assert!(graph.contains("[main]volume=-1dB"));
    assert!(build_graph(&AudioEffects::default(), None, None).is_none());
}
