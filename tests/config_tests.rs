use doris::config::*;
use doris::sources::source::KNOWN_SOURCES;
use serial_test::serial;
use std::path::PathBuf;

#[test]
fn test_config_default() {
    // The raw struct: what every "just build me a Config" caller gets.
    let config = Config::default();
    assert_eq!(config.browser_visibility, "hidden");
    assert_eq!(config.torrserver_url, "http://127.0.0.1:8090");
    assert!(config.enable_torrserver);
    assert_eq!(config.bridge_port, 14141);
    assert!(
        std::path::Path::new(&config.cookie_file).is_absolute(),
        "the default cookie file must be absolute, not a path relative to \
         whatever directory doris happened to be started in: `cookies.txt` \
         is how a live rutracker session ended up inside target/release/"
    );
    assert!(config.browser.is_none());

    // Phase 5: Options "general" category fields must have real, sane
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
    // The source list is not here: it is the registry's to say, and a raw
    assert!(config.enabled_sources.is_empty());
    assert!(config.known_sources.is_empty());
    assert!(config.download_enabled);
    assert_eq!(config.download_dir_mode, "default");
    assert!(config.download_dir_custom_1.is_empty());
    assert!(config.close_torrent_core_on_exit);
}

#[test]
fn test_config_new_fields_have_defaults_when_omitted_from_toml() {
    // A config.toml written before Phase 5 won't mention any of these
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

/// A config written before the default became absolute says `cookie_file = "cookies.txt"`,
/// which resolved against the CWD -- so a session landed wherever doris was started from (that
/// is how one ended up inside `target/release/`).
#[test]
#[serial]
fn test_relative_cookie_file_migrates_next_to_the_config() {
    let name = format!("doris-migrate-rel-{}.txt", std::process::id());
    let dir = std::env::temp_dir().join(format!("doris-cfg-mig-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, format!("cookie_file = \"{name}\"\n")).unwrap();

    let config = load(Some(&path)).unwrap();

    assert!(
        std::path::Path::new(&config.cookie_file).is_absolute(),
        "a relative cookie_file must not survive the load"
    );
    assert_eq!(
        std::path::Path::new(&config.cookie_file),
        dir.join(&name),
        "it belongs beside the config, not in the CWD"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// The migration is a `load` fix, so it has to reach the file on disk
/// too -- otherwise the next run resolves the same relative path again
/// and the fix only ever exists in memory.
#[test]
#[serial]
fn test_migration_is_written_back_to_the_config() {
    let name = format!("doris-migrate-wb-{}.txt", std::process::id());
    let dir = std::env::temp_dir().join(format!("doris-cfg-wb-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, format!("cookie_file = \"{name}\"\n")).unwrap();

    load(Some(&path)).unwrap();
    // Reload straight from the file, not from the value load returned.
    let reloaded = load(Some(&path)).unwrap();

    assert_eq!(
        reloaded.cookie_file,
        dir.join(&name).to_str().unwrap(),
        "the migrated path must be persisted, not recomputed each run"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A user who already put the file somewhere absolute meant that, and a
/// migration that moved it would be a change nobody asked for.
#[test]
fn test_absolute_cookie_file_is_left_alone() {
    let dir = std::env::temp_dir().join(format!("doris-cfg-abs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    let mine = dir.join("my-session.txt");
    std::fs::write(&path, "cookie_file = \"my-session.txt\"\n").unwrap();
    std::fs::write(&mine, "# Netscape HTTP Cookie File\n").unwrap();

    let config = load(Some(&path)).unwrap();

    assert_eq!(config.cookie_file, mine.to_str().unwrap());
    assert!(mine.exists(), "the user's own file must survive the load");

    let _ = std::fs::remove_dir_all(&dir);
}

/// The session the old relative path pointed at is a real file with a live login in it.
#[test]
#[serial]
fn test_migration_moves_the_existing_session() {
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    let name = format!("doris-migrate-test-{}.txt", std::process::id());
    let old = std::path::PathBuf::from(&name);
    let _cleanup = Cleanup(old.clone());

    let dir = std::env::temp_dir().join(format!("doris-cfg-move-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, format!("cookie_file = \"{name}\"\n")).unwrap();
    std::fs::write(
        &old,
        "# Netscape HTTP Cookie File\n.bb_session\tTRUE\t/\tTRUE\t0\tbb_session\tlive\n",
    )
    .unwrap();

    let config = load(Some(&path)).unwrap();

    let moved = dir.join(&name);
    assert_eq!(config.cookie_file, moved.to_str().unwrap());
    assert!(moved.exists(), "the session must not be left behind");
    assert!(
        std::fs::read_to_string(&moved)
            .unwrap()
            .contains("bb_session"),
        "the session's contents must survive the move"
    );
    assert!(
        !old.exists(),
        "nothing may be left at the old relative path"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// `rename` cannot cross a filesystem boundary, and these two paths routinely are on different
/// ones: the config in `~/.config` on the root filesystem, the app started from a mounted data
/// disk or a tmpfs.
#[test]
#[serial]
fn test_migration_moves_across_a_filesystem_boundary() {
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    let name = format!("doris-migrate-xdev-{}.txt", std::process::id());
    let old = std::path::PathBuf::from(&name);
    let _cleanup = Cleanup(old.clone());

    let dir = std::env::temp_dir().join(format!("doris-cfg-xdev-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("config.toml");
    std::fs::write(&path, format!("cookie_file = \"{name}\"\n")).unwrap();
    std::fs::write(&old, "session-body\n").unwrap();

    let config = load(Some(&path)).unwrap();

    let moved = dir.join(&name);
    assert_eq!(std::fs::read_to_string(&moved).unwrap(), "session-body\n");
    assert_eq!(config.cookie_file, moved.to_str().unwrap());
    assert!(!old.exists());

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
    let toml_str = r#"
        browser_mode = "gui"
    "#;
    let config: Config = toml::from_str(toml_str).unwrap();
    assert_eq!(config.browser_visibility, "gui");
}

// --- enabled_sources migration (B8 wave 1 fallout) --------------------------

/// What a config written before wave 1 looks like: it lists the two sources that existed, and
/// knows nothing about ids added later.
#[test]
fn test_a_config_from_before_wave1_gains_the_new_sources() {
    let config = from_toml(
        "enabled_sources = [\n    \"rutracker\",\n    \"rutor\",\n]\nsave_config_on_exit = true\n",
    )
    .expect("legacy config parses");

    for id in ["yts", "tpb", "subsplease"] {
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

/// A config that already knows every id -- i.e.
#[test]
fn test_known_sources_are_never_re_enabled() {
    let mut config = Config {
        known_sources: KNOWN_SOURCES.iter().map(|s| s.id.to_string()).collect(),
        enabled_sources: vec!["rutracker".to_string()],
        ..Default::default()
    };

    doris::sources::source::migrate_config(&mut config);

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

    doris::sources::source::migrate_config(&mut config);

    assert_eq!(
        config.enabled_sources, after_once,
        "second run changed the list"
    );
    assert_eq!(
        config.known_sources, known_once,
        "second run changed what is known"
    );
}

/// A first run's config already knows everything, so the migration must leave it untouched --
/// otherwise every startup would be rewriting a user's choices.
#[test]
fn test_a_fresh_config_is_not_migrated() {
    let mut config = Config::default();
    doris::sources::source::first_run_config(&mut config);
    let enabled = config.enabled_sources.clone();
    let known = config.known_sources.clone();
    assert!(!enabled.is_empty(), "setup: a first run has sources on");

    doris::sources::source::migrate_config(&mut config);

    assert_eq!(config.enabled_sources, enabled);
    assert_eq!(config.known_sources, known);
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

    doris::sources::source::migrate_config(&mut config);

    for info in KNOWN_SOURCES.iter().filter(|info| !info.implemented) {
        assert!(
            !config.known_sources.iter().any(|k| k == info.id),
            "{} was a caption in Options, not a choice: {:?}",
            info.id,
            config.known_sources
        );
    }
    // While the planned id is written off, the implemented ones still
    assert_eq!(config.enabled_sources, vec!["rutracker".to_string()]);
}

/// A first run must come up with every implemented source switched on.
#[test]
fn test_a_first_run_enables_every_implemented_source() {
    // The same two calls `load` makes when there is no file -- not a copy
    let mut config = Config::default();
    doris::sources::source::first_run_config(&mut config);
    doris::sources::source::migrate_config(&mut config);

    let implemented: Vec<String> = KNOWN_SOURCES
        .iter()
        .filter(|s| s.implemented)
        .map(|s| s.id.to_string())
        .collect();
    assert_eq!(
        config.enabled_sources, implemented,
        "a machine that has never seen a config gets every source"
    );
    assert_eq!(
        config.known_sources, implemented,
        "and is told about exactly those"
    );
}

/// The other side of the same pair: an empty *file* is a config from
/// before the field existed, and the three sources that existed then are
/// treated as already decided rather than newly arrived.
#[test]
fn test_an_empty_config_file_keeps_the_legacy_sources_off() {
    let config = from_toml("").expect("an empty config is valid");

    let legacy = ["rutracker", "rutor", "nnmclub"];
    for id in legacy {
        assert!(
            !config.enabled_sources.iter().any(|e| e == id),
            "{id} existed before the field, so it is not 'new' and stays off"
        );
    }
    assert!(
        config.enabled_sources.iter().any(|e| e == "yts"),
        "an id that did not exist then is new and arrives enabled"
    );
}

/// `config.rs` is a settings file; the list of sources is a fact about the build.
#[test]
fn the_config_layer_does_not_know_the_source_list() {
    let source = include_str!("../src/config.rs");

    for line in source.lines() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        for needle in ["KNOWN_SOURCES", "LEGACY_SOURCES", "implemented_source_ids"] {
            assert!(
                !line.contains(needle),
                "config.rs names the registry again: {line}"
            );
        }
    }
}

/// The no-file branch of `load` must go through the first-run config.
#[test]
fn load_without_a_file_uses_the_first_run_config() {
    let source = include_str!("../src/config.rs");

    // The *last* `None` arm in `load`: the first one picks the path
    let start = source.find("pub fn load(").expect("load is defined");
    let body = &source[start..];
    let arm = body.rfind("None => {").expect("the no-file arm");
    let arm_text = &body[arm..arm + 400];

    assert!(
        arm_text.contains("first_run()"),
        "a machine with no config must get the registry's defaults: {arm_text}"
    );
    assert!(
        !arm_text.contains("Config::default()"),
        "and not the raw struct, which carries no source list: {arm_text}"
    );
}

/// A source this build no longer has is dropped from the list it was checked
/// in, rather than sitting there invisible.
///
/// EZTV is the case: its API has no search, so it was removed, and the filter
/// that builds the checkbox list works off the registry -- so a stale id in
/// the config produced no row, no error and no way to tell.
#[test]
fn test_a_source_this_build_dropped_leaves_the_enabled_list() {
    let config = from_toml("enabled_sources = [\n    \"rutracker\",\n    \"eztv\",\n]\n")
        .expect("a config naming a removed source still parses");

    assert!(
        !config.enabled_sources.iter().any(|s| s == "eztv"),
        "a removed source must not stay checked in: {:?}",
        config.enabled_sources
    );
    assert!(
        config.enabled_sources.iter().any(|s| s == "rutracker"),
        "and the sources that are still here stay: {:?}",
        config.enabled_sources
    );
    assert!(
        !config.known_sources.iter().any(|s| s == "eztv"),
        "nor in the list of what this build knows: {:?}",
        config.known_sources
    );
}
