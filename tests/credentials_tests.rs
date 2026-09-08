use doris::credentials::*;
use serial_test::serial;

#[test]
#[serial]
fn test_save_and_load_credentials() {
    let _ = std::fs::remove_file(credentials_path());
    save_credentials("testuser", "testpass123").unwrap();
    let loaded = load_credentials().unwrap();
    assert_eq!(loaded.0, "testuser");
    assert_eq!(loaded.1, "testpass123");
    let _ = std::fs::remove_file(credentials_path());
}

#[test]
#[serial]
fn test_load_nonexistent() {
    let _ = std::fs::remove_file(credentials_path());
    assert!(load_credentials().is_none());
}

#[test]
#[serial]
fn test_special_chars() {
    let _ = std::fs::remove_file(credentials_path());
    save_credentials("user@domain.com", "p@$$w0rd!#%").unwrap();
    let loaded = load_credentials().unwrap();
    assert_eq!(loaded.0, "user@domain.com");
    assert_eq!(loaded.1, "p@$$w0rd!#%");
    let _ = std::fs::remove_file(credentials_path());
}

#[test]
#[serial]
fn test_password_containing_colon_roundtrips() {
    // Regression test for the pre-Phase-4 bug: storage used to join as
    // "username:password" and split on the first ':', silently truncating
    // any password containing one. Storage is JSON now, so this must
    // round-trip exactly.
    let _ = std::fs::remove_file(credentials_path());
    save_credentials("user", "pass:with:colons").unwrap();
    let loaded = load_credentials().unwrap();
    assert_eq!(loaded.0, "user");
    assert_eq!(loaded.1, "pass:with:colons");
    let _ = std::fs::remove_file(credentials_path());
}

#[test]
#[serial]
fn test_multiple_resources_do_not_clobber_each_other() {
    let _ = std::fs::remove_file(credentials_path());
    save_credential("rutracker", "alice", "alice-pass").unwrap();
    save_credential("rutor", "bob", "bob-pass").unwrap();

    assert_eq!(load_credential("rutracker"), Some(("alice".into(), "alice-pass".into())));
    assert_eq!(load_credential("rutor"), Some(("bob".into(), "bob-pass".into())));
    assert_eq!(load_credential("nnmclub"), None);

    // The old single-resource API only ever sees "rutracker".
    assert_eq!(load_credentials(), Some(("alice".into(), "alice-pass".into())));

    let _ = std::fs::remove_file(credentials_path());
}

#[test]
#[serial]
fn test_delete_credential() {
    let _ = std::fs::remove_file(credentials_path());
    save_credential("rutracker", "alice", "alice-pass").unwrap();
    delete_credential("rutracker").unwrap();
    assert_eq!(load_credential("rutracker"), None);
    let _ = std::fs::remove_file(credentials_path());
}
