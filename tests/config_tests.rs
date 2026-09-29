use doris::config::*;
use doris::sources::source::KNOWN_SOURCES;

#[test]
fn test_config_default() {
    let config = Config::default();
    assert_eq!(config.browser_visibility, "hidden");
    assert_eq!(config.torrserver_url, "http://127.0.0.1:8090");
    assert!(config.enable_torrserver);
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
    // The tiling grammar is rows via `,` and columns via `|`, so the
    // very first preset is the default UI the options list promises:
    // Results on top, Trackers and Log in the row under it, no Torrent.
    assert_eq!(config.presets[0], "1,3|4");
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
    // The default is "every implemented source", asserted *against the
    // registry* so adding a source to KNOWN_SOURCES forces the decision
    // of whether it ships enabled rather than forgetting it silently.
    let implemented: Vec<String> = KNOWN_SOURCES
        .iter()
        .filter(|s| s.implemented)
        .map(|s| s.id.to_string())
        .collect();
    assert_eq!(config.enabled_sources, implemented);
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

    let config = Config {
        theme_name: Some("dracula".to_string()),
        vim_keys: false,
        update_ms: 2500,
        rounded_corners: false,
        enable_torrserver: false,
        ..Default::default()
    };

    save(&config, Some(&path)).unwrap();
    let loaded = load(Some(&path)).unwrap();

    assert_eq!(loaded.theme_name.as_deref(), Some("dracula"));
    assert!(!loaded.vim_keys);
    assert_eq!(loaded.update_ms, 2500);
    assert!(!loaded.rounded_corners);
    assert!(!loaded.enable_torrserver);
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

// --- enabled_sources migration (B8 wave 1 fallout) --------------------------

/// What a config written before wave 1 looks like: it lists the two
/// sources that existed, and knows nothing about ids added later. This
/// is the live case -- the finished wave ran into it on an installed
/// config, where every new tab answered "Selected source is disabled".
#[test]
fn test_a_config_from_before_wave1_gains_the_new_sources() {
    let config = from_toml(
        "enabled_sources = [\n    \"rutracker\",\n    \"rutor\",\n]\nsave_config_on_exit = true\n",
    )
    .expect("legacy config parses");

    for id in ["yts", "tpb", "subsplease", "eztv"] {
        assert!(
            config.enabled_sources.iter().any(|s| s == id),
            "{} must arrive enabled, got {:?}",
            id,
            config.enabled_sources
        );
    }
    assert!(config.enabled_sources.iter().any(|s| s == "rutracker"));
    assert!(
        config.known_sources.iter().any(|s| s == "tpb"),
        "and the config now knows it: {:?}",
        config.known_sources
    );
}

/// The distinction `known_sources` exists for: `rutor` was in the
/// legacy baseline, so a user who switched it off before wave 1 keeps
/// it off -- the migration adds only ids the config has never seen.
#[test]
fn test_a_source_the_user_switched_off_before_wave1_stays_off() {
    let config = from_toml("enabled_sources = [ \"rutracker\" ]").expect("legacy config parses");

    assert!(
        !config.enabled_sources.iter().any(|s| s == "rutor"),
        "rutor existed then and was turned off deliberately"
    );
    assert!(
        config.enabled_sources.iter().any(|s| s == "tpb"),
        "tpb did not exist then and is new"
    );
}

/// A config that already knows every id -- i.e. one this build has
/// saved -- is left exactly as the user configured it.
#[test]
fn test_known_sources_are_never_re_enabled() {
    let mut config = Config {
        known_sources: KNOWN_SOURCES.iter().map(|s| s.id.to_string()).collect(),
        enabled_sources: vec!["rutracker".to_string()],
        ..Default::default()
    };

    config.migrate_sources();

    assert_eq!(
        config.enabled_sources,
        vec!["rutracker".to_string()],
        "five deliberately disabled sources came back on"
    );
}

#[test]
fn test_migration_is_idempotent() {
    let mut config =
        from_toml("enabled_sources = [ \"rutracker\", \"rutor\" ]").expect("legacy config parses");
    let after_once = config.enabled_sources.clone();
    let known_once = config.known_sources.clone();

    config.migrate_sources();

    assert_eq!(
        config.enabled_sources, after_once,
        "second run changed the list"
    );
    assert_eq!(
        config.known_sources, known_once,
        "second run changed what is known"
    );
}

/// A fresh config already knows everything, so `Config::default()` must
/// come through the migration untouched -- otherwise every startup
/// would be rewriting a user's choices.
#[test]
fn test_a_fresh_default_config_is_not_migrated() {
    let mut config = Config::default();
    let enabled = config.enabled_sources.clone();

    config.migrate_sources();

    assert_eq!(config.enabled_sources, enabled);
    assert_eq!(
        config.known_sources.len(),
        KNOWN_SOURCES.iter().filter(|info| info.implemented).count(),
        "and only the ids that could be switched on or off are in it"
    );
}

/// What wave 3 ran into live: nnmclub sat in `known_sources` as a
/// placeholder row, then went implemented -- and because the id looked
/// already seen, the migration kept it switched off, so the source
/// shipped and the user's tab bar never mentioned it.
#[test]
fn test_a_planned_source_is_never_recorded_as_seen() {
    let mut config = Config {
        known_sources: KNOWN_SOURCES.iter().map(|s| s.id.to_string()).collect(),
        enabled_sources: vec!["rutracker".to_string()],
        ..Default::default()
    };

    config.migrate_sources();

    for info in KNOWN_SOURCES.iter().filter(|info| !info.implemented) {
        assert!(
            !config.known_sources.iter().any(|k| k == info.id),
            "{} was a caption in Options, not a choice: {:?}",
            info.id,
            config.known_sources
        );
    }
    // While the planned id is written off, the implemented ones still
    // count -- which is the half that protects a deliberate "off".
    assert_eq!(config.enabled_sources, vec!["rutracker".to_string()]);
}
