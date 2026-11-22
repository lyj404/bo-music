use super::*;
use std::path::PathBuf;
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn silent_wav(seconds: u32) -> Fixture {
    wav(seconds, |_| 0)
}

fn wav(seconds: u32, sample: impl Fn(u32) -> i16) -> Fixture {
    let data_size = 44_100_u32 * 2 * seconds;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_size).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    bytes.extend_from_slice(&44_100_u32.to_le_bytes());
    bytes.extend_from_slice(&88_200_u32.to_le_bytes());
    bytes.extend_from_slice(&2_u16.to_le_bytes());
    bytes.extend_from_slice(&16_u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_size.to_le_bytes());
    for frame in 0..data_size / 2 {
        bytes.extend_from_slice(&sample(frame).to_le_bytes());
    }
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "bo_music-playback-{}-{unique}.wav",
        std::process::id()
    ));
    fs::write(&path, bytes).unwrap();
    Fixture(path)
}

#[test]
#[ignore = "requires an active Windows playback endpoint and plays a short tone"]
fn wasapi_loopback_reads_the_real_mpv_output() {
    use crate::spectrum_output::{OutputSpectrum, OutputSpectrumFrame};

    let mut spectrum = OutputSpectrum::default();
    let ready = Instant::now() + Duration::from_secs(5);
    while !matches!(spectrum.frame(), OutputSpectrumFrame::Levels(_)) {
        assert!(Instant::now() < ready, "WASAPI loopback did not start");
        std::thread::sleep(Duration::from_millis(20));
    }

    let library = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../libmpv/libmpv-2.dll");
    let player = MpvPlayer::open(
        library,
        MpvOptions {
            audio_output: Some("wasapi".into()),
        },
    )
    .unwrap();
    player.volume(40.0).unwrap();
    let file = wav(2, |frame| {
        let phase = std::f64::consts::TAU * 1_000.0 * f64::from(frame) / 44_100.0;
        (phase.sin() * 6_000.0) as i16
    });
    player
        .load(LoadSource {
            address: file.0.to_str().unwrap().into(),
            start_seconds: None,
            end_seconds: None,
            initially_paused: false,
        })
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut strongest = (0, 0.0);
    loop {
        if let OutputSpectrumFrame::Levels(levels) = spectrum.frame() {
            let (dominant, peak) = levels
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.total_cmp(b))
                .unwrap();
            if *peak > strongest.1 {
                strongest = (dominant, *peak);
            }
            if (24..=26).contains(&dominant) && *peak > 0.4 {
                break;
            }
        }
        assert!(
            Instant::now() < deadline,
            "loopback did not detect the 1 kHz tone, strongest {strongest:?}, playback {:?}",
            player.snapshot().state,
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    spectrum.stop();
}
fn wait_for(player: &MpvPlayer, predicate: impl Fn(PlaybackSnapshot) -> bool) -> PlaybackSnapshot {
    let until = Instant::now() + Duration::from_secs(8);
    loop {
        let snapshot = player.snapshot();
        if predicate(snapshot) {
            return snapshot;
        }
        assert_ne!(
            snapshot.state,
            PlaybackState::Failed,
            "libmpv playback failed: {:?}",
            snapshot.last_error
        );
        assert!(
            Instant::now() < until,
            "timed out with snapshot {snapshot:?}"
        );
        thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn real_mpv_loads_all_builtin_presets_and_custom_equalizer() {
    use bo_music_domain::audio_effects::{AudioEffects, CustomPreset, PRESETS};
    let library = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../libmpv/libmpv-2.dll");
    let player = MpvPlayer::open(
        library,
        MpvOptions {
            audio_output: Some("null".into()),
        },
    )
    .unwrap();
    let file = silent_wav(1);
    for (index, preset) in PRESETS.iter().enumerate() {
        let effects = AudioEffects {
            enabled: true,
            preset_id: preset.id.into(),
            ..Default::default()
        };
        let graph = crate::effects::build_graph(&effects, None, None).unwrap();
        player
            .audio_filter(Some(&graph))
            .unwrap_or_else(|error| panic!("{}: {error}", preset.id));
        player
            .load(LoadSource {
                address: file.0.to_str().unwrap().into(),
                start_seconds: None,
                end_seconds: None,
                initially_paused: true,
            })
            .unwrap();
        wait_for(&player, |s| {
            s.loaded_count > index as u64 && s.state == PlaybackState::Paused
        });
    }
    let effects = AudioEffects {
        enabled: true,
        preset_id: "custom.one".into(),
        custom_presets: vec![CustomPreset {
            id: "custom.one".into(),
            name: "one".into(),
            description: String::new(),
            bands: [3.0; 10],
        }],
        ..Default::default()
    };
    player
        .audio_filter(crate::effects::build_graph(&effects, None, None).as_deref())
        .unwrap();
    player.pause(false).unwrap();
    wait_for(&player, |s| s.completed_count == 1);
    player.audio_filter(None).unwrap();
}

#[test]
fn real_mpv_mixes_ambient_and_vinyl_but_still_finishes_track() {
    use bo_music_domain::audio_effects::AudioEffects;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let player = MpvPlayer::open(
        root.join("../../libmpv/libmpv-2.dll"),
        MpvOptions {
            audio_output: Some("null".into()),
        },
    )
    .unwrap();
    let file = silent_wav(1);
    let ambient = root.join("assets/ambient/ambient.rain.ogg");
    let texture = root.join("assets/ambient/texture.vinyl.ogg");
    let settings = AudioEffects {
        enabled: true,
        preset_id: "galaxy.vinyl".into(),
        ambient_id: "ambient.rain".into(),
        ambient_volume: 30.0,
        ..Default::default()
    };
    let graph = crate::effects::build_graph(&settings, ambient.to_str(), texture.to_str()).unwrap();
    player.audio_filter(Some(&graph)).unwrap();
    player
        .load(LoadSource {
            address: file.0.to_str().unwrap().into(),
            start_seconds: None,
            end_seconds: None,
            initially_paused: true,
        })
        .unwrap();
    wait_for(&player, |snapshot| snapshot.state == PlaybackState::Paused);
    let mut quieter = settings.clone();
    quieter.ambient_volume = 10.0;
    let next_graph =
        crate::effects::build_graph(&quieter, ambient.to_str(), texture.to_str()).unwrap();
    player
        .audio_filter_weights("1 0.5 0.1", &next_graph)
        .unwrap();
    player.pause(false).unwrap();
    wait_for(&player, |snapshot| snapshot.completed_count == 1);
    assert_eq!(player.snapshot().state, PlaybackState::Stopped);
}
#[test]
fn owns_native_handle_and_controls_audio_to_completion() {
    let library = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../libmpv/libmpv-2.dll");
    let player = MpvPlayer::open(
        library,
        MpvOptions {
            audio_output: Some("null".into()),
        },
    )
    .unwrap();
    let file = silent_wav(3);
    assert!(matches!(
        player.volume(101.0),
        Err(MpvError::InvalidArgument)
    ));
    assert!(matches!(
        player.seek(f64::NAN),
        Err(MpvError::InvalidArgument)
    ));
    player.volume(0.0).unwrap();
    player
        .load(LoadSource {
            address: file.0.to_str().unwrap().into(),
            start_seconds: None,
            end_seconds: None,
            initially_paused: true,
        })
        .unwrap();
    wait_for(&player, |state| {
        state.state == PlaybackState::Paused && state.duration_seconds.is_some()
    });
    player.seek(1.5).unwrap();
    wait_for(&player, |state| state.position_seconds >= 1.45);
    player.pause(false).unwrap();
    wait_for(&player, |state| state.state == PlaybackState::Playing);
    let completed = wait_for(&player, |state| state.completed_count == 1);
    assert_eq!(completed.state, PlaybackState::Stopped);
    player.stop().unwrap();
    assert_eq!(player.snapshot().completed_count, 1);
}

#[test]
fn replacing_a_paused_file_does_not_finish_the_new_file() {
    let library = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../libmpv/libmpv-2.dll");
    let player = MpvPlayer::open(
        library,
        MpvOptions {
            audio_output: Some("null".into()),
        },
    )
    .unwrap();
    let first = silent_wav(3);
    let second = silent_wav(1);
    player
        .load(LoadSource {
            address: first.0.to_str().unwrap().into(),
            start_seconds: None,
            end_seconds: None,
            initially_paused: true,
        })
        .unwrap();
    wait_for(&player, |s| s.duration_seconds.is_some_and(|d| d > 2.5));
    assert!(matches!(
        player.load(LoadSource {
            address: "bad\0address".into(),
            start_seconds: None,
            end_seconds: None,
            initially_paused: false,
        }),
        Err(MpvError::InvalidArgument)
    ));
    player
        .load(LoadSource {
            address: second.0.to_str().unwrap().into(),
            start_seconds: None,
            end_seconds: None,
            initially_paused: true,
        })
        .unwrap();
    let loaded = wait_for(&player, |s| {
        s.state == PlaybackState::Paused && s.duration_seconds.is_some_and(|d| d < 1.5)
    });
    assert_eq!(loaded.completed_count, 0);
    player.pause(false).unwrap();
    let finished = wait_for(&player, |s| s.completed_count == 1);
    assert_eq!(finished.state, PlaybackState::Stopped);
}

#[test]
fn rapid_replacements_keep_the_last_entry_and_stop_does_not_finish_it() {
    let library = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../libmpv/libmpv-2.dll");
    let player = MpvPlayer::open(
        library,
        MpvOptions {
            audio_output: Some("null".into()),
        },
    )
    .unwrap();
    let file = silent_wav(1);
    for _ in 0..20 {
        player
            .load(LoadSource {
                address: file.0.to_str().unwrap().into(),
                start_seconds: None,
                end_seconds: None,
                initially_paused: true,
            })
            .unwrap();
    }
    wait_for(&player, |s| {
        s.state == PlaybackState::Paused && s.duration_seconds.is_some()
    });
    player.stop().unwrap();
    thread::sleep(Duration::from_millis(400));
    let stopped = player.snapshot();
    assert_eq!(stopped.state, PlaybackState::Stopped);
    assert_eq!(stopped.completed_count, 0);
}

#[test]
fn missing_library_returns_an_error() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("this-library-does-not-exist.dll");
    assert!(matches!(
        MpvPlayer::open(path, MpvOptions::default()),
        Err(MpvError::Library)
    ));
}
