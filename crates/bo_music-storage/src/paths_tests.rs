use super::*;
#[test]
fn account_paths_reject_ambiguous_and_traversal_scopes() {
    let paths = DataPaths::new("root");
    assert_eq!(
        paths.account_dir("anonymous").unwrap(),
        Path::new("root/accounts/anonymous")
    );
    assert!(paths.account_dir("123").is_ok());
    for bad in ["", "../42", "42/1", "42\\1", ".", "-1", "abc", " 42"] {
        assert!(paths.account_dir(bad).is_err(), "{bad}");
    }
}

#[test]
fn renamed_profile_preserves_existing_data_and_does_not_overwrite_new_profile() {
    let root = std::env::temp_dir().join(format!("bo_music-paths-{}", std::process::id()));
    let legacy = root.join("BowenRust");
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(legacy.join("config.json"), b"old settings").unwrap();
    let paths = DataPaths::from_data_home(root.clone()).unwrap();
    assert_eq!(paths.root(), root.join("bo_music"));
    assert_eq!(std::fs::read(paths.config()).unwrap(), b"old settings");
    assert!(!legacy.exists());
    std::fs::create_dir_all(&legacy).unwrap();
    std::fs::write(legacy.join("config.json"), b"different settings").unwrap();
    let paths = DataPaths::from_data_home(root.clone()).unwrap();
    assert_eq!(std::fs::read(paths.config()).unwrap(), b"old settings");
    assert!(legacy.join("config.json").exists());
    std::fs::remove_dir_all(root).unwrap();
}
