use doris::app::resolve_download_dir;
use doris::config::Config;

#[test]
fn test_default_mode_uses_os_download_dir_not_empty() {
    let config = Config::default();
    assert_eq!(config.download_dir_mode, "default");
    let resolved = resolve_download_dir(&config);
    // Whatever the OS default happens to be (or the "/tmp" fallback if it
    // can't be determined), it must never be empty.
    assert!(!resolved.is_empty());
}

#[test]
fn test_custom1_mode_uses_custom1_when_set() {
    let mut config = Config::default();
    config.download_dir_mode = "custom1".to_string();
    config.download_dir_custom_1 = "/mnt/media/downloads".to_string();
    assert_eq!(resolve_download_dir(&config), "/mnt/media/downloads");
}

#[test]
fn test_custom2_mode_uses_custom2_when_set() {
    let mut config = Config::default();
    config.download_dir_mode = "custom2".to_string();
    config.download_dir_custom_2 = "/home/user/torrents".to_string();
    assert_eq!(resolve_download_dir(&config), "/home/user/torrents");
}

#[test]
fn test_custom3_mode_uses_custom3_when_set() {
    let mut config = Config::default();
    config.download_dir_mode = "custom3".to_string();
    config.download_dir_custom_3 = "/data/dl".to_string();
    assert_eq!(resolve_download_dir(&config), "/data/dl");
}

#[test]
fn test_custom_mode_with_empty_slot_falls_back_to_os_default() {
    let mut config = Config::default();
    config.download_dir_mode = "custom1".to_string();
    // download_dir_custom_1 left empty (default).
    let resolved = resolve_download_dir(&config);
    assert!(!resolved.is_empty());
    assert_ne!(resolved, "");
}

#[test]
fn test_unknown_mode_falls_back_to_os_default() {
    let mut config = Config::default();
    config.download_dir_mode = "not-a-real-mode".to_string();
    let resolved = resolve_download_dir(&config);
    assert!(!resolved.is_empty());
}

#[test]
fn test_custom_slots_do_not_bleed_into_each_other() {
    let mut config = Config::default();
    config.download_dir_custom_1 = "/one".to_string();
    config.download_dir_custom_2 = "/two".to_string();
    config.download_dir_custom_3 = "/three".to_string();

    config.download_dir_mode = "custom1".to_string();
    assert_eq!(resolve_download_dir(&config), "/one");

    config.download_dir_mode = "custom2".to_string();
    assert_eq!(resolve_download_dir(&config), "/two");

    config.download_dir_mode = "custom3".to_string();
    assert_eq!(resolve_download_dir(&config), "/three");
}
