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
