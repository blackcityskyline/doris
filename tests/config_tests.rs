use doris::config::*;

#[test]
fn test_config_default() {
    let config = Config::default();
    assert_eq!(config.browser_visibility, "hidden");
    assert_eq!(config.torrserver_url, "http://127.0.0.1:8090");
    assert_eq!(config.bridge_port, 14141);
    assert_eq!(config.cookie_file, "cookies.txt");
    assert!(config.browser.is_none());

    // Phase 5: Options "general" category fields must have real, sane
    // defaults -- these used to be hardcoded display strings with no
    // backing field at all.
    assert!(config.theme_name.is_none());
    assert!(config.theme_background);
    assert!(config.truecolor);
    assert!(!config.false_tty);
    assert!(config.vim_keys);
    assert!(!config.disable_mouse);
    assert!(!config.disable_presets);
    assert!(!config.presets.is_empty());
    assert_eq!(config.preset_index, 0);
    assert!(config.show_boxes);
    assert_eq!(config.update_ms, 1000);
    assert!(config.rounded_corners);
    assert!(config.terminal_sync);
    assert_eq!(config.graph_symbol, "braille");
    assert!(!config.save_config_on_exit);

    // Phase 6: Options "streaming"/"download" category fields.
    assert!(config.close_browser_on_exit);
    assert!(config.save_cookies);
    assert!(config.save_credentials);
    assert_eq!(config.enabled_sources, vec!["rutracker".to_string()]);
    assert!(config.download_enabled);
    assert_eq!(config.download_dir_mode, "default");
    assert!(config.download_dir_custom_1.is_empty());
    assert!(!config.download_sequential);
    assert_eq!(config.download_speed_limit_kbps, 0);
    assert!(config.close_torrent_core_on_exit);
}

#[test]
fn test_config_new_fields_have_defaults_when_omitted_from_toml() {
    // A config.toml written before Phase 5 won't mention any of these
    // keys at all; loading it must not fail, and must fall back to the
    // same defaults as Config::default().
    let toml_str = r#"
        browser = "brave"
    "#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert!(config.theme_background);
    assert!(config.vim_keys);
    assert_eq!(config.update_ms, 1000);
    assert_eq!(config.graph_symbol, "braille");
}

#[test]
fn test_config_save_and_load_round_trip() {
    let dir = std::env::temp_dir().join(format!("doris-config-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");

    let mut config = Config::default();
    config.theme_name = Some("dracula".to_string());
    config.vim_keys = false;
    config.update_ms = 2500;
    config.rounded_corners = false;

    save(&config, Some(&path)).unwrap();
    let loaded = load(Some(&path)).unwrap();

    assert_eq!(loaded.theme_name.as_deref(), Some("dracula"));
    assert!(!loaded.vim_keys);
    assert_eq!(loaded.update_ms, 2500);
    assert!(!loaded.rounded_corners);
    // Untouched fields should still round-trip with their defaults.
    assert!(loaded.truecolor);
    assert_eq!(loaded.browser_visibility, "hidden");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_config_parse_toml() {
    let toml_str = r#"
        browser = "helium"
        browser_visibility = "hidden"
        torrserver_url = "http://192.168.1.100:8090"
        bridge_port = 14142
        cookie_file = "/tmp/cookies.txt"
    "#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.browser.as_deref(), Some("helium"));
    assert_eq!(config.browser_visibility, "hidden");
    assert_eq!(config.torrserver_url, "http://192.168.1.100:8090");
    assert_eq!(config.bridge_port, 14142);
    assert_eq!(config.cookie_file, "/tmp/cookies.txt");
}

#[test]
fn test_config_parse_partial_toml() {
    let toml_str = r#"
        browser = "brave"
    "#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.browser.as_deref(), Some("brave"));
    assert_eq!(config.browser_visibility, "hidden");
    assert_eq!(config.torrserver_url, "http://127.0.0.1:8090");
}

#[test]
fn test_config_parse_empty() {
    let config: Config = toml::from_str("").unwrap();
    assert!(config.browser.is_none());
    assert_eq!(config.browser_visibility, "hidden");
}

#[test]
fn test_config_legacy_browser_mode_alias_still_works() {
    // Old configs written before the headless/gui -> hidden/visible rename
    // must keep loading without an error.
    let toml_str = r#"
        browser_mode = "gui"
    "#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.browser_visibility, "gui");
}

#[test]
fn test_config_parse_with_keybindings() {
    let toml_str = r#"
        [keybindings]
        quit = "ctrl+x"
        focus_search = "/"
    "#;
    let config: Config = toml::from_str(toml_str).unwrap();
    let kb = config.keybindings.unwrap();
    assert_eq!(kb.quit.as_deref(), Some("ctrl+x"));
    assert_eq!(kb.focus_search.as_deref(), Some("/"));
}
