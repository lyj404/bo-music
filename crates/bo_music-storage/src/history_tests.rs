use super::*;
use std::fs;

fn root() -> PathBuf {
    let id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("bo_music-history-{}-{id}", std::process::id()))
}
fn track(id: i64) -> Track {
    parse_track(&json!({"Id":id,"Title":format!("歌曲{id}"),"DurationSeconds":120})).unwrap()
}

#[test]
fn legacy_dates_are_sorted_by_instant_before_deduplication() {
    let entries = json!([
        {"MusicId":1,"Title":"旧快照","PlayedAt":"2026-01-01T10:00:00+08:00"},
        {"MusicId":2,"Title":"中间","PlayedAt":"2026-01-01T02:30:00Z"},
        {"MusicId":1,"Title":"新快照","PlayedAt":"2026-01-01T03:00:00.123+00:00"}
    ]);
    let loaded = parse(&serde_json::to_vec(&entries).unwrap()).unwrap();
    assert_eq!(
        loaded
            .iter()
            .map(|entry| entry.track.title.as_str())
            .collect::<Vec<_>>(),
        vec!["新快照", "中间"]
    );
    assert_eq!(legacy_time_ms("1970-01-01T08:00:00+08:00"), Some(0));
    assert_eq!(legacy_time_ms("2025-02-29T00:00:00Z"), None);
}

#[test]
fn legacy_import_deduplicates_limits_and_isolates_accounts() {
    let root = root();
    let paths = DataPaths::new(&root);
    let directory = paths.account_dir("42").unwrap();
    fs::create_dir_all(&directory).unwrap();
    let mut entries: Vec<_> = (1..=501)
        .map(|id| {
            json!({
                "MusicId":id,"Title":format!("歌曲{id}"),"PlayedAt":"2026-01-01T00:00:00+00:00",
                "AvailableQualities":["High"],"AlbumId":9,"DurationSeconds":120
            })
        })
        .collect();
    entries.insert(1, entries[0].clone());
    fs::write(
        directory.join("history.json"),
        serde_json::to_vec(&entries).unwrap(),
    )
    .unwrap();
    let store = HistoryStore::new(paths);
    let loaded = store.load("42").unwrap();
    assert_eq!(loaded.len(), 500);
    assert_eq!(
        (
            loaded[0].id,
            loaded[0].album_id,
            loaded[0].available_qualities.len()
        ),
        (1, 9, 1)
    );
    assert!(store.load("anonymous").unwrap().is_empty());
    store.record("42", &track(3)).unwrap();
    let reloaded = store.load("42").unwrap();
    assert_eq!(reloaded.len(), 500);
    assert_eq!(reloaded[0].id, 3);
    assert_eq!(reloaded.iter().filter(|item| item.id == 3).count(), 1);
    store.record("anonymous", &track(99)).unwrap();
    assert_eq!(store.load("anonymous").unwrap()[0].id, 99);
    assert_eq!(store.load("42").unwrap()[0].id, 3);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn corrupt_primary_can_be_read_from_backup_but_is_never_overwritten() {
    let root = root();
    let paths = DataPaths::new(&root);
    let store = HistoryStore::new(paths.clone());
    store.record("42", &track(1)).unwrap();
    store.record("42", &track(2)).unwrap();
    let file = paths.account_dir("42").unwrap().join("history.json");
    fs::write(&file, b"corrupt").unwrap();
    assert_eq!(store.load("42").unwrap()[0].id, 1);
    assert!(matches!(
        store.record("42", &track(3)),
        Err(HistoryError::Invalid)
    ));
    assert_eq!(fs::read(&file).unwrap(), b"corrupt");
    let malformed = r#"[{"MusicId":0,"Title":"无法恢复的记录"}]"#.as_bytes();
    fs::write(&file, malformed).unwrap();
    assert_eq!(store.load("42").unwrap()[0].id, 1);
    assert!(matches!(
        store.record("42", &track(3)),
        Err(HistoryError::Invalid)
    ));
    assert_eq!(fs::read(&file).unwrap(), malformed);
    assert!(store.load("../42").is_err());
    fs::remove_dir_all(root).unwrap();
}
