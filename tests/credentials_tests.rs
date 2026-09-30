//! The credential store's own tests, against a scratch directory.
//!
//! Every test here used to save and delete `~/.config/doris/credentials.enc`
//! -- the user's real one. `cargo test` therefore overwrote whatever login
//! was saved (verified: the file came back as `{}`), and the tests needed
//! `#[serial]` to at least keep from racing each other. They now name the
//! file they mean, so nothing outside the temp directory is touched.

use doris::credentials::{
    delete_credential_at, load_credential_at, load_store_at, save_credential_at, STORE_FILE,
};
use std::path::PathBuf;

/// A directory of its own per test, cleared first so a leftover from an
/// earlier run cannot answer for the current one.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("doris-cred-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir.join(STORE_FILE)
}

#[test]
fn test_save_and_load_credentials() {
    let path = scratch("save-load");
    save_credential_at(&path, "rutracker", "testuser", "testpass123").unwrap();
    assert_eq!(
        load_credential_at(&path, "rutracker"),
        Some(("testuser".into(), "testpass123".into()))
    );
}

#[test]
fn test_load_nonexistent() {
    let path = scratch("nonexistent");
    assert_eq!(load_credential_at(&path, "rutracker"), None);
}

#[test]
fn test_special_chars() {
    let path = scratch("special");
    save_credential_at(&path, "rutracker", "user@domain.com", "p@$$w0rd!#%").unwrap();
    assert_eq!(
        load_credential_at(&path, "rutracker"),
        Some(("user@domain.com".into(), "p@$$w0rd!#%".into()))
    );
}

#[test]
fn test_password_containing_colon_roundtrips() {
    // Regression test for the pre-Phase-4 bug: storage used to join as
    // "username:password" and split on the first ':', silently truncating
    // any password containing one. Storage is JSON now, so this must
    // round-trip exactly.
    let path = scratch("colon");
    save_credential_at(&path, "rutracker", "user", "pass:with:colons").unwrap();
    assert_eq!(
        load_credential_at(&path, "rutracker"),
        Some(("user".into(), "pass:with:colons".into()))
    );
}

#[test]
fn test_multiple_resources_do_not_clobber_each_other() {
    let path = scratch("multi");
    save_credential_at(&path, "rutracker", "alice", "alice-pass").unwrap();
    save_credential_at(&path, "rutor", "bob", "bob-pass").unwrap();

    assert_eq!(
        load_credential_at(&path, "rutracker"),
        Some(("alice".into(), "alice-pass".into()))
    );
    assert_eq!(
        load_credential_at(&path, "rutor"),
        Some(("bob".into(), "bob-pass".into()))
    );
    assert_eq!(load_credential_at(&path, "nnmclub"), None);
    // Still exactly one resource in the store: saving a second login
    // merges rather than replacing.
    assert_eq!(load_store_at(&path).len(), 2);
}

#[test]
fn test_delete_credential() {
    let path = scratch("delete");
    save_credential_at(&path, "rutracker", "alice", "alice-pass").unwrap();
    delete_credential_at(&path, "rutracker").unwrap();
    assert_eq!(load_credential_at(&path, "rutracker"), None);
}

#[test]
fn test_saving_one_resource_keeps_the_other() {
    // `save_credential` reads the whole store, inserts, and writes it
    // back -- so the second save is where a clobber would show up.
    let path = scratch("merge");
    save_credential_at(&path, "rutracker", "alice", "alice-pass").unwrap();
    save_credential_at(&path, "rutor", "bob", "bob-pass").unwrap();
    save_credential_at(&path, "rutracker", "alice2", "alice-pass2").unwrap();
    assert_eq!(
        load_credential_at(&path, "rutor"),
        Some(("bob".into(), "bob-pass".into())),
        "rewriting one login must not drop the other"
    );
}
