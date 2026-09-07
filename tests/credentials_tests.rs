use t_hunter::credentials::*;
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
