use super::*;

#[test]
fn second_process_requests_activation_without_acquiring_the_lock() {
    const TEST_ROOT: &str = "BO_MUSIC_INSTANCE_TEST_ROOT";
    if let Some(root) = std::env::var_os(TEST_ROOT) {
        assert!(
            PrimaryInstance::start_in(Path::new(&root))
                .unwrap()
                .is_none()
        );
        return;
    }
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("bo_music-instance-{}-{unique}", std::process::id()));
    let primary = PrimaryInstance::start_in(&root).unwrap().unwrap();
    let inbox = primary.activation_inbox();
    assert!(!inbox.take_request().unwrap());
    for _ in 0..2 {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "instance::tests::second_process_requests_activation_without_acquiring_the_lock",
            ])
            .env(TEST_ROOT, &root)
            .output()
            .unwrap();
        assert!(
            child.status.success(),
            "{} {}",
            String::from_utf8_lossy(&child.stdout),
            String::from_utf8_lossy(&child.stderr)
        );
    }
    assert!(inbox.take_request().unwrap());
    assert!(!inbox.take_request().unwrap());
    drop(primary);
    assert!(PrimaryInstance::start_in(&root).unwrap().is_some());
    fs::remove_dir_all(root).unwrap();
}
