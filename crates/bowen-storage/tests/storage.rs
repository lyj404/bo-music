use bowen_domain::queue::PlayMode;
use bowen_domain::track::AudioQuality;
use bowen_storage::{
    device, migration,
    paths::DataPaths,
    queue::{QueueError, QueueStore},
    search_history::SearchHistoryStore,
    settings::{
        FloatingBounds, FloatingWindows, LoadedFrom, PhysicalWindowBounds, PlaybackSettings,
        SettingsError, SettingsStore,
    },
    shortcuts::{Action, Gesture},
};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[test]
fn standalone_backups_supply_missing_preferences_without_replacing_current_values() {
    let temp = TestDir::new();
    let source = temp.0.join("Bowen");
    let target = DataPaths::new(temp.0.join("BowenRust"));
    fs::create_dir(&source).unwrap();
    fs::create_dir(target.root()).unwrap();
    fs::write(
        source.join("settings.json"),
        br#"{"Theme":"Dark","AcrylicEffects":false}"#,
    )
    .unwrap();
    fs::write(
        source.join("font-settings.json"),
        br#"{"FontFamily":" Segoe UI "}"#,
    )
    .unwrap();
    fs::write(source.join("playback.json"), br#""Shuffle""#).unwrap();
    fs::write(source.join("volume.json"), br#"{"Level":10}"#).unwrap();
    fs::write(
        source.join("window.json"),
        br#"{"X":-900,"Y":30,"Width":1100,"Height":700}"#,
    )
    .unwrap();
    fs::write(target.config(), br#"{"Volume":{"Level":64}}"#).unwrap();
    migration::import_standalone_settings(&source, &target).unwrap();
    fs::write(source.join("settings.json"), br#"{"Theme":"Light"}"#).unwrap();
    migration::import_standalone_settings(&source, &target).unwrap();
    let store = SettingsStore::open(target.config()).unwrap();
    assert_eq!(
        store.appearance().unwrap().theme,
        bowen_storage::settings::ThemeMode::Dark
    );
    assert_eq!(store.appearance().unwrap().ui_font, "Segoe UI");
    assert_eq!(store.playback().unwrap().mode, PlayMode::Shuffle);
    assert_eq!(store.volume().unwrap(), 64.);
    assert_eq!(
        store.legacy_floating_windows().unwrap().main.unwrap().x,
        -900
    );
    let bounds = FloatingBounds {
        x: 24.,
        y: 36.,
        width: 1000.,
        height: 640.,
    };
    store.set_main_window_bounds(bounds).unwrap();
    assert_eq!(
        SettingsStore::open(target.config())
            .unwrap()
            .main_window_bounds()
            .unwrap(),
        Some(bounds)
    );
}
struct TestDir(PathBuf);
impl TestDir {
    fn new() -> Self {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "bowen-storage-test-{}-{unique}-{}",
            std::process::id(),
            TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn imports_legacy_shortcuts_and_rejects_conflicting_changes() {
    let temp = TestDir::new();
    let path = temp.0.join("config.json");
    fs::write(&path, br#"{"Shortcuts":{"Bindings":[{"Action":"NextTrack","Key":"D4","Modifiers":"Control, Shift"},{"Action":6,"Key":120,"Modifiers":0},{"Action":"VolumeDown","Key":"Escape","Modifiers":0}]}}"#).unwrap();
    let store = SettingsStore::open(&path).unwrap();
    let mut shortcuts = store.shortcuts().unwrap();
    let next = Gesture::new("4", true, false, true).unwrap();
    assert_eq!(shortcuts.action_for(&next), Some(Action::Next));
    assert_eq!(
        shortcuts.action_for(&Gesture::new("F9", false, false, false).unwrap()),
        Some(Action::Mute)
    );
    assert_eq!(
        shortcuts.action_for(&Gesture::new("down", true, false, false).unwrap()),
        Some(Action::VolumeDown)
    );
    assert_eq!(shortcuts.rebind(Action::VolumeUp, next), Err(Action::Next));
    assert!(Gesture::new("escape", true, false, false).is_none());
    assert!(Gesture::new("l", false, false, false).is_none());
    shortcuts
        .rebind(
            Action::Previous,
            Gesture::new("f4", false, false, false).unwrap(),
        )
        .unwrap();
    store.set_shortcuts(shortcuts.clone()).unwrap();
    assert_eq!(
        SettingsStore::open(path).unwrap().shortcuts().unwrap(),
        shortcuts
    );
}

#[test]
fn reads_old_physical_positions_without_overwriting_saved_logical_positions() {
    let temp = TestDir::new();
    let path = temp.0.join("config.json");
    fs::write(&path, br#"{"Windows":{"MiniPlayer":{"X":-999,"Y":24,"Width":590,"Height":260},"DesktopLyrics":{"X":2147483647,"Y":0,"Width":20,"Height":100}},"floatingWindows":{"miniPlayer":{"x":12,"y":36,"width":478,"height":218}}}"#).unwrap();
    let store = SettingsStore::open(path).unwrap();
    let legacy = store.legacy_floating_windows().unwrap();
    assert_eq!(
        legacy.mini_player,
        Some(PhysicalWindowBounds {
            x: -999,
            y: 24,
            width: 590,
            height: 260
        })
    );
    assert_eq!(legacy.desktop_lyrics, None);
    assert_eq!(
        store.floating_windows().unwrap().mini_player.unwrap().x,
        12.
    );
}

#[test]
fn imports_and_retains_lyric_poster_font_and_colors() {
    let temp = TestDir::new();
    let path = temp.0.join("config.json");
    fs::write(&path, br#"{"LyricPoster":{"FontFamily":"  Segoe UI  ","BackgroundSource":"Custom","BackgroundColorArgb":4280562250,"TextColorMode":2}}"#).unwrap();
    let store = SettingsStore::open(&path).unwrap();
    let mut style = store.lyric_poster_style().unwrap();
    assert_eq!(style.font_family, "Segoe UI");
    assert_eq!(style.background_rgb, Some(0x24324a));
    assert_eq!(style.text_rgb, Some(0x171717));
    style.text_rgb = Some(0xeeeeee);
    store.set_lyric_poster_style(style.clone()).unwrap();
    assert_eq!(
        SettingsStore::open(path)
            .unwrap()
            .lyric_poster_style()
            .unwrap(),
        style
    );
}

#[test]
fn floating_window_settings_round_trip_and_keep_other_configuration() {
    let temp = TestDir::new();
    let path = temp.0.join("config.json");
    fs::write(
        &path,
        br#"{"volume":{"level":25},"floatingWindows":{"futureOption":true}}"#,
    )
    .unwrap();
    let store = SettingsStore::open(&path).unwrap();
    assert_eq!(
        store.floating_windows().unwrap(),
        FloatingWindows::default()
    );
    let mini = FloatingBounds {
        x: -540.,
        y: 30.,
        width: 480.,
        height: 220.,
    };
    store
        .set_floating_windows(FloatingWindows {
            mini_player: Some(mini),
            desktop_lyrics: Some(FloatingBounds {
                width: f32::NAN,
                ..mini
            }),
            lyric_font_size: 90.,
            lyric_second_line: false,
            lyric_highlight: 0x4d9cff,
            lyric_staggered: true,
        })
        .unwrap();
    let reopened = SettingsStore::open(&path).unwrap();
    assert_eq!(
        reopened.floating_windows().unwrap(),
        FloatingWindows {
            mini_player: Some(mini),
            desktop_lyrics: None,
            lyric_font_size: 48.,
            lyric_second_line: false,
            lyric_highlight: 0x4d9cff,
            lyric_staggered: true,
        }
    );
    assert_eq!(reopened.volume().unwrap(), 25.);
    assert_eq!(
        reopened.snapshot().unwrap()["floatingWindows"]["futureOption"],
        true
    );
}

#[test]
fn migrated_search_history_can_be_updated_and_cleared_per_account() {
    let temp = TestDir::new();
    let old = temp.0.join("old");
    let paths = DataPaths::new(temp.0.join("new"));
    fs::create_dir(paths.root()).unwrap();
    let old_history = old.join("accounts/42/search-history.json");
    fs::create_dir_all(old_history.parent().unwrap()).unwrap();
    let original = br#"[" Song ", "song", "", "Other"]"#;
    fs::write(&old_history, original).unwrap();
    migration::import_legacy(&old, &paths).unwrap();

    let store = SearchHistoryStore::new(paths.clone());
    assert_eq!(store.load("42").unwrap(), ["Song", "Other"]);
    assert!(store.load("anonymous").unwrap().is_empty());
    assert_eq!(
        store.record("42", " other ".into()).unwrap(),
        ["other", "Song"]
    );
    for number in 0..25 {
        store.record("anonymous", number.to_string()).unwrap();
    }
    store.clear("42").unwrap();
    let restored = SearchHistoryStore::new(paths.clone());
    assert!(restored.load("42").unwrap().is_empty());
    let words = restored.load("anonymous").unwrap();
    assert_eq!(words.len(), 20);
    assert_eq!((words[0].as_str(), words[19].as_str()), ("24", "5"));
    assert_eq!(fs::read(old_history).unwrap(), original);

    let live_history = paths.account_dir("42").unwrap().join("search-history.json");
    fs::write(&live_history, b"invalid json").unwrap();
    assert!(restored.record("42", "new".into()).is_err());
    assert_eq!(fs::read(live_history).unwrap(), b"invalid json");
}

#[test]
fn legacy_lyric_appearance_defaults_to_the_imported_unified_settings() {
    let temp = TestDir::new();
    let path = temp.0.join("config.json");
    fs::write(&path, br#"{"DesktopLyrics":{"FontSize":42,"TextColorArgb":4283276543,"DualLine":true,"Alignment":"Staggered"}}"#).unwrap();
    let store = SettingsStore::open(&path).unwrap();
    let legacy = store.floating_windows().unwrap();
    assert_eq!(legacy.lyric_font_size, 42.);
    assert_eq!(legacy.lyric_highlight, 0x4d9cff);
    assert!(legacy.lyric_second_line && legacy.lyric_staggered);
    store
        .set_floating_windows(FloatingWindows {
            lyric_highlight: 0xfadb14,
            ..legacy
        })
        .unwrap();
    assert_eq!(
        SettingsStore::open(&path)
            .unwrap()
            .floating_windows()
            .unwrap()
            .lyric_highlight,
        0xfadb14
    );
}

#[test]
fn settings_recover_backup_preserve_unknown_and_reject_future_version() {
    let temp = TestDir::new();
    let path = temp.0.join("config.json");
    let settings = SettingsStore::open(&path).unwrap();
    assert_eq!(settings.loaded_from(), LoadedFrom::Default);
    settings
        .update(|map| {
            map.insert("unknown".into(), serde_json::json!({"x": 42}));
        })
        .unwrap();
    settings
        .set_playback(PlaybackSettings {
            mode: PlayMode::Shuffle,
            restore_queue: false,
        })
        .unwrap();
    assert_eq!(settings.playback().unwrap().mode, PlayMode::Shuffle);
    fs::write(&path, b"broken").unwrap();
    let restored = SettingsStore::open(&path).unwrap();
    assert_eq!(restored.loaded_from(), LoadedFrom::Backup);
    assert_eq!(restored.snapshot().unwrap()["unknown"]["x"], 42);
    restored
        .set_playback(PlaybackSettings {
            mode: PlayMode::SingleLoop,
            restore_queue: true,
        })
        .unwrap();
    assert_eq!(
        SettingsStore::open(&path).unwrap().playback().unwrap().mode,
        PlayMode::SingleLoop
    );
    fs::write(&path, br#"{"schemaVersion":999,"unknown":true}"#).unwrap();
    assert!(matches!(
        SettingsStore::open(&path),
        Err(SettingsError::FutureVersion(999))
    ));
    assert!(matches!(
        restored.set_playback(PlaybackSettings::default()),
        Err(SettingsError::FutureVersion(999))
    ));
    assert_eq!(
        fs::read(&path).unwrap(),
        br#"{"schemaVersion":999,"unknown":true}"#
    );
}

#[test]
fn audio_effects_read_legacy_case_normalize_and_preserve_unknown_fields() {
    let temp = TestDir::new();
    let path = temp.0.join("config.json");
    fs::write(&path, br#"{"AudioEffects":{"Enabled":true,"PresetId":"CUSTOM.One","AmbientId":"ambient.rain","AmbientVolume":120,"future":42,"CustomPresets":[{"Id":"custom.one","Name":"  ","Bands":[20,-20,2]},{"Id":"CUSTOM.ONE","Bands":[1]}]}}"#).unwrap();
    let settings = SettingsStore::open(&path).unwrap();
    let effects = settings.audio_effects().unwrap();
    assert!(effects.enabled);
    assert_eq!(effects.preset_id, "custom.one");
    assert_eq!(effects.ambient_volume, 100.0);
    assert_eq!(effects.custom_presets.len(), 1);
    assert_eq!(
        effects.custom_presets[0].bands[..4],
        [12.0, -12.0, 2.0, 0.0]
    );
    assert_eq!(effects.custom_presets[0].name, "未命名");
    settings.set_audio_effects(&effects).unwrap();
    let restored = SettingsStore::open(&path).unwrap();
    assert_eq!(restored.audio_effects().unwrap(), effects);
    assert_eq!(restored.snapshot().unwrap()["audioEffects"]["future"], 42);
}

#[test]
fn preferred_quality_reads_legacy_values_and_preserves_other_fields() {
    let temp = TestDir::new();
    let path = temp.0.join("config.json");
    fs::write(&path, br#"{"AudioQuality":{"Preferred":1,"future":42}}"#).unwrap();
    let store = SettingsStore::open(&path).unwrap();
    assert_eq!(store.preferred_quality().unwrap(), AudioQuality::High);
    store.set_preferred_quality(AudioQuality::Standard).unwrap();
    let loaded = SettingsStore::open(&path).unwrap();
    assert_eq!(loaded.preferred_quality().unwrap(), AudioQuality::Standard);
    assert_eq!(loaded.snapshot().unwrap()["audioQuality"]["future"], 42);
    fs::write(&path, br#"{"audioQuality":{"preferred":"unsupported"}}"#).unwrap();
    assert_eq!(
        SettingsStore::open(&path)
            .unwrap()
            .preferred_quality()
            .unwrap(),
        AudioQuality::Lossless
    );
}

#[test]
fn legacy_settings_accept_comments_and_trailing_commas_without_altering_strings() {
    let temp = TestDir::new();
    let path = temp.0.join("config.json");
    fs::write(
        &path,
        br#"{
        // User comment
        "schemaVersion": 1,
        "url": "https://example.test/a/*literal*/",
        "message": "quote: \" // still text",
        "playback": {"mode": "Shuffle", "restoreQueue": false, },
        "list": [1, 2, /* another comment */ ],
    }"#,
    )
    .unwrap();
    let store = SettingsStore::open(&path).unwrap();
    assert_eq!(store.playback().unwrap().mode, PlayMode::Shuffle);
    assert_eq!(
        store.snapshot().unwrap()["url"],
        "https://example.test/a/*literal*/"
    );
    assert_eq!(
        store.snapshot().unwrap()["message"],
        "quote: \" // still text"
    );
    store.set_playback(PlaybackSettings::default()).unwrap();
    assert_eq!(
        SettingsStore::open(&path).unwrap().snapshot().unwrap()["list"][1],
        2
    );
    fs::write(&path, br#"{"schemaVersion":1,/* unfinished"#).unwrap();
    let backup = path.with_extension("json.bak");
    fs::remove_file(backup).unwrap();
    assert!(matches!(
        SettingsStore::open(&path),
        Err(SettingsError::Invalid)
    ));
}

#[test]
fn migration_preserves_sources_existing_targets_and_excludes_credentials() {
    let temp = TestDir::new();
    let old = temp.0.join("old");
    let new = temp.0.join("new");
    fs::create_dir_all(old.join("accounts/42")).unwrap();
    fs::create_dir_all(old.join("accounts/anonymous")).unwrap();
    fs::create_dir_all(&new).unwrap();
    let config = br#"{"schemaVersion":1,"playback":{"mode":"ListLoop"}}"#;
    fs::write(old.join("config.json"), config).unwrap();
    fs::write(old.join("devid.txt"), b"0123456789abcdef0123456789abcdef").unwrap();
    fs::write(old.join("session.dat"), b"private").unwrap();
    fs::write(old.join("accounts/42/queue.json"), br#"{"Items":[{"Id":123,"Title":"Song"}],"CurrentTrackId":123,"CurrentIndex":0,"PositionSeconds":9}"#).unwrap();
    fs::write(old.join("accounts/anonymous/search-history.json"), b"[]").unwrap();
    let paths = DataPaths::new(&new);
    let first = migration::import_legacy(&old, &paths).unwrap();
    assert_eq!(first.imported, 4);
    assert_eq!(fs::read(old.join("config.json")).unwrap(), config);
    assert_eq!(
        fs::read(new.join("legacy-backup/config.json")).unwrap(),
        config
    );
    assert!(!new.join("session.dat").exists());
    assert!(!new.join("legacy-backup/session.dat").exists());
    fs::write(
        new.join("config.json"),
        br#"{"schemaVersion":1,"custom":true}"#,
    )
    .unwrap();
    fs::write(
        old.join("config.json"),
        br#"{"schemaVersion":1,"changed":true}"#,
    )
    .unwrap();
    let second = migration::import_legacy(&old, &paths).unwrap();
    assert_eq!(second.imported, 0);
    assert_eq!(second.preserved, 4);
    assert_eq!(
        fs::read(new.join("legacy-backup/config.json")).unwrap(),
        config
    );
    assert_eq!(
        SettingsStore::open(paths.config())
            .unwrap()
            .snapshot()
            .unwrap()["custom"],
        true
    );
    assert_eq!(QueueStore::new(paths).load("42").unwrap().tracks[0].id, 123);
}

#[test]
fn queue_filters_invalid_entries_restores_by_id_and_isolates_accounts() {
    let temp = TestDir::new();
    let paths = DataPaths::new(&temp.0);
    let account = paths.account_dir("42").unwrap();
    fs::create_dir_all(&account).unwrap();
    fs::write(account.join("queue.json"), br#"{
        "Items":[{"Id":0,"Title":"bad"},{"Id":42,"Title":"ok",
        "AvailableQualities":["Lossless",99,"High"],
        "AudioVariants":[{"Quality":"Lossless","Level":"ff","Format":"flac","BitrateKbps":1411,"SizeBytes":99},
                         {"Quality":"Lossless","Level":"zply","Format":"flac","BitrateKbps":2000}]}],
        "CurrentTrackId":42,"CurrentIndex":1,"PositionSeconds":12.5
    }"#).unwrap();
    let store = QueueStore::new(paths);
    let snapshot = store.load("42").unwrap();
    assert_eq!(snapshot.tracks.len(), 1);
    assert_eq!(snapshot.current_index, Some(0));
    assert_eq!(snapshot.position_seconds, 12.5);
    assert_eq!(snapshot.tracks[0].audio_variants.len(), 1);
    assert_eq!(snapshot.tracks[0].available_qualities.len(), 2);
    assert!(store.load("anonymous").unwrap().tracks.is_empty());
    store.save("42", &snapshot).unwrap();
    assert_eq!(
        store.load("42").unwrap().tracks[0].audio_variants[0].bitrate_kbps,
        1411
    );
    fs::write(account.join("queue.json"), b"corrupt").unwrap();
    assert_eq!(store.load("42").unwrap().tracks[0].id, 42);
    fs::write(
        account.join("queue.json"),
        br#"{"SchemaVersion":9,"Items":[]}"#,
    )
    .unwrap();
    assert!(matches!(
        store.load("42"),
        Err(QueueError::FutureVersion(9))
    ));
    assert!(matches!(
        store.save("42", &snapshot),
        Err(QueueError::FutureVersion(9))
    ));
    assert!(store.load("../42").is_err());
}

#[test]
fn device_id_is_stable_and_invalid_existing_data_is_preserved() {
    let temp = TestDir::new();
    let path = temp.0.join("devid.txt");
    let id = device::get_or_create(&path).unwrap();
    assert_eq!(id.len(), 32);
    assert_eq!(device::get_or_create(&path).unwrap(), id);
    fs::write(&path, b"invalid").unwrap();
    assert!(device::get_or_create(&path).is_err());
    assert_eq!(fs::read(path).unwrap(), b"invalid");
}

#[test]
fn overlapping_migration_does_not_modify_legacy_tree() {
    let temp = TestDir::new();
    let old = temp.0.join("old");
    fs::create_dir(&old).unwrap();
    let inside = old.join("new");
    fs::create_dir(&inside).unwrap();
    assert!(matches!(
        migration::import_legacy(&old, &DataPaths::new(&inside)),
        Err(migration::MigrationError::OverlappingRoots)
    ));
    assert!(!inside.join("legacy-backup").exists());
}

#[test]
fn interrupted_import_has_no_partial_live_data_and_can_retry() {
    let temp = TestDir::new();
    let old = temp.0.join("old");
    let new = temp.0.join("new");
    fs::create_dir_all(old.join("accounts/42")).unwrap();
    fs::create_dir(&new).unwrap();
    fs::write(old.join("config.json"), br#"{"schemaVersion":1}"#).unwrap();
    let huge = fs::File::create(old.join("accounts/42/queue.json")).unwrap();
    huge.set_len((bowen_storage::atomic::MAX_DOCUMENT_BYTES + 1) as u64)
        .unwrap();
    drop(huge);
    let paths = DataPaths::new(&new);
    assert!(migration::import_legacy(&old, &paths).is_err());
    assert!(!new.join("config.json").exists());
    assert!(!new.join("accounts/42/queue.json").exists());
    fs::write(old.join("accounts/42/queue.json"), br#"{"Items":[]}"#).unwrap();
    let result = migration::import_legacy(&old, &paths).unwrap();
    assert_eq!(result.imported, 2);
    assert!(new.join("legacy-backup/accounts/42/queue.json").exists());
}

#[test]
fn concurrent_settings_transactions_do_not_lose_updates() {
    let temp = TestDir::new();
    let store = std::sync::Arc::new(SettingsStore::open(temp.0.join("config.json")).unwrap());
    let workers: Vec<_> = (0..4)
        .map(|_| {
            let store = store.clone();
            std::thread::spawn(move || {
                for _ in 0..5 {
                    store
                        .update(|map| {
                            let current = map.get("counter").and_then(|v| v.as_u64()).unwrap_or(0);
                            map.insert("counter".into(), serde_json::json!(current + 1));
                        })
                        .unwrap();
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(store.snapshot().unwrap()["counter"], 20);
    assert_eq!(
        SettingsStore::open(temp.0.join("config.json"))
            .unwrap()
            .snapshot()
            .unwrap()["counter"],
        20
    );
}
