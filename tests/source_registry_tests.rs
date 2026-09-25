use doris::config::Config;
use doris::search::source::{self, Group, KNOWN_SOURCES, Source, SourceEnv};
use doris::search::rutor::RutorSearcher;
use doris::ui::app::{SettingsAction, source_settings_items};

#[test]
fn test_rutracker_and_rutor_are_registered_and_implemented() {
    for id in ["rutracker", "rutor"] {
        let source = KNOWN_SOURCES.iter().find(|s| s.id == id);
        assert!(source.is_some(), "{} must be in KNOWN_SOURCES", id);
        assert!(source.unwrap().implemented, "{} should be marked implemented", id);
    }
}

#[test]
fn test_future_sources_are_listed_but_not_implemented() {
    for id in ["1337x", "torentino"] {
        let source = KNOWN_SOURCES.iter().find(|s| s.id == id);
        assert!(source.is_some(), "{} should be listed as a planned source", id);
        assert!(!source.unwrap().implemented, "{} should not be marked implemented yet", id);
    }
}

#[test]
fn test_all_source_ids_are_unique() {
    let mut ids: Vec<&str> = KNOWN_SOURCES.iter().map(|s| s.id).collect();
    let original_len = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), original_len, "duplicate source id found in KNOWN_SOURCES");
}

#[test]
fn test_source_ids_are_lowercase_no_spaces() {
    // Source ids double as credential-store keys and Options checklist
    // keys -- they need to be simple, stable identifiers.
    for source in KNOWN_SOURCES {
        assert_eq!(source.id, source.id.to_lowercase(), "{} should be lowercase", source.id);
        assert!(!source.id.contains(' '), "{} should not contain spaces", source.id);
        assert!(!source.id.is_empty());
        assert!(!source.label.is_empty());
    }
}

#[test]
fn test_at_least_one_source_is_usable_today() {
    assert!(KNOWN_SOURCES.iter().any(|s| s.implemented), "at least one source must actually work");
}

// --- B2: registry metadata --------------------------------------------------

#[test]
fn test_implemented_sources_declare_at_least_one_group() {
    // B6 filters by group and the Options UI groups rows by it, so an
    // implemented source with no groups would be unreachable in both.
    for source in KNOWN_SOURCES.iter().filter(|s| s.implemented) {
        assert!(
            !source.groups.is_empty(),
            "implemented source '{}' declares no groups",
            source.id
        );
    }
}

#[test]
fn test_declared_groups_are_the_four_known_ones() {
    // Guards against a typo'd variant making it into the registry (and
    // then into JSON via `TorrentItem::group`).
    let valid = [Group::Games, Group::Movies, Group::TV, Group::Anime];
    for source in KNOWN_SOURCES {
        for group in source.groups {
            assert!(valid.contains(group), "{:?} is not a known group", group);
        }
    }
}

#[test]
fn test_only_browser_backed_sources_ask_for_a_browser() {
    // `requires_browser` is what makes the orchestrator skip
    // `Browser::launch` entirely for plain-HTTP sources (and what B0.1's
    // `source_needs_browser` ends up backed by). Rutracker is the only
    // source that constructs a browser today.
    let browser_backed: Vec<&str> = KNOWN_SOURCES
        .iter()
        .filter(|s| s.requires_browser)
        .map(|s| s.id)
        .collect();
    assert_eq!(browser_backed, vec!["rutracker", "1337x"]);
    // A planned source still declares how its host behaves, because
    // that is what the flag is about: probed 25.09.2026 with a browser
    // UA, 1337x answered 403 to a plain client and torentino answered
    // 200 with its front page. What holds either claim back is that
    // nothing builds them, so neither can route a request today.
    assert!(source::get_source("1337x").unwrap().requires_browser);
    assert!(!source::get_source("torentino").unwrap().requires_browser);
}

#[test]
fn test_registry_metadata_matches_the_buildable_implementation() {
    // Rutor is the one source we can instantiate offline, so its
    // registry row can be compared against the live trait impl; the
    // browser-backed one can't be (constructing it needs a browser).
    let rutor = RutorSearcher::new();
    let info = source::get_source("rutor").expect("rutor must be registered");
    assert_eq!(rutor.id(), info.id);
    assert_eq!(rutor.label(), info.label);
    assert_eq!(rutor.groups(), info.groups);
    assert_eq!(rutor.home_url(), info.home_url);
    assert_eq!(rutor.requires_browser(), info.requires_browser);
    assert!(!rutor.requires_browser());
    assert!(!rutor.supports_browse(), "browse mode is B9, not built yet");
}

#[test]
fn test_requires_browser_falls_back_to_true_for_unknown_ids() {
    // Conservative default: guessing "no browser" for an unknown source
    // would route its login somewhere that can't launch one.
    assert!(source::requires_browser("rutracker"));
    assert!(!source::requires_browser("rutor"));
    assert!(source::requires_browser(""));
    assert!(source::requires_browser("never-heard-of-it"));
}

#[test]
fn test_get_source_and_sources_by_group_view_the_same_registry() {
    assert!(source::get_source("rutracker").is_some());
    assert!(source::get_source("nope").is_none());

    let games = source::sources_by_group(Group::Games);
    assert!(games.iter().any(|s| s.id == "rutracker"));
    assert!(games.iter().any(|s| s.id == "rutor"));
    assert!(games.iter().all(|s| s.groups.contains(&Group::Games)));

    // A planned source has no rows yet, so it belongs to no group
    // view -- and the group list is the claim it may not make.
    for planned in KNOWN_SOURCES.iter().filter(|s| !s.implemented) {
        assert!(planned.groups.is_empty(), "{} is planned: no rows, no groups", planned.id);
        assert!(!planned.home_url.is_empty(), "{} still says where it will live", planned.id);
        for group in [Group::Games, Group::Movies, Group::TV, Group::Anime] {
            assert!(
                source::sources_by_group(group).iter().all(|s| s.id != planned.id),
                "{} must not be in the {:?} view",
                planned.id,
                group
            );
        }
    }
}

#[test]
fn test_implemented_sources_have_a_home_url() {
    // The orchestrator hands this to `Browser::launch` as the cookie
    // injection target before the Source instance itself exists.
    for source in KNOWN_SOURCES.iter().filter(|s| s.implemented) {
        assert!(
            !source.home_url.is_empty(),
            "implemented source '{}' has no home_url",
            source.id
        );
        assert!(
            source.home_url.starts_with("http"),
            "home_url for '{}' should be an absolute URL: {}",
            source.id,
            source.home_url
        );
    }
}

// --- B2: build_source factory ------------------------------------------------

#[test]
fn test_build_source_builds_every_browser_free_implemented_source() {
    // Whatever is marked implemented and needs no browser must actually
    // be constructible offline -- otherwise `implemented` is a lie the
    // Options checklist happily prints.
    for info in KNOWN_SOURCES.iter().filter(|s| s.implemented && !s.requires_browser) {
        assert!(
            build_ok(info.id),
            "implemented browser-free source '{}' does not build",
            info.id
        );
    }
}

fn build_ok(id: &str) -> bool {
    matches!(source::build_source(id, SourceEnv { browser: None }), Ok(_))
}

#[test]
fn test_build_source_refuses_browser_backed_sources_without_a_browser() {
    // The instance is what needs the browser, so handing in `None` must
    // fail loudly instead of producing a source that panics later.
    let err = match source::build_source("rutracker", SourceEnv { browser: None }) {
        Ok(_) => panic!("rutracker must not build without a browser session"),
        Err(e) => e,
    };
    let msg = err.to_string();
    assert!(msg.contains("browser"), "expected a browser complaint, got: {}", msg);
}

#[test]
fn test_build_source_rejects_unknown_ids() {
    assert!(!build_ok("never-heard-of-it"));
    assert!(!build_ok(""));
}

// --- Options rows derive from the same registry -----------------------------

/// B8 wave 1 shipped four sources with no way to enable them: the
/// `streaming -> Sources` rows were written out by hand for
/// rutracker/rutor/nnmclub, so the new tabs only ever answered
/// "Selected source is disabled". These tests pin the derivation itself,
/// not the current roster -- a fifth source must appear on its own.
#[test]
fn test_options_lists_exactly_the_registry_losing_nothing_to_handwriting() {
    let config = Config::default();
    let items = source_settings_items(&config);

    assert_eq!(
        items.len(),
        KNOWN_SOURCES.len(),
        "every registered source must have a row -- handwritten lists rot"
    );
    for (item, info) in items.iter().zip(KNOWN_SOURCES.iter()) {
        assert_eq!(item.label, format!("Sources: {}", info.label));
    }
}

#[test]
fn test_each_toggles_its_own_registry_id_and_says_whether_it_is_on() {
    let mut config = Config::default();
    config.enabled_sources = vec!["rutracker".to_string(), "yts".to_string()];

    for (item, info) in source_settings_items(&config).iter().zip(KNOWN_SOURCES.iter()) {
        if info.implemented {
            assert_eq!(
                item.action,
                SettingsAction::ToggleSource(info.id),
                "{} must toggle itself, not a neighbour",
                info.id
            );
            let expected = if info.id == "rutracker" || info.id == "yts" {
                "True"
            } else {
                "False"
            };
            assert_eq!(item.value, expected, "{}'s state", info.id);
        } else {
            assert_eq!(item.value, "planned", "{} is not implemented", info.id);
            assert_ne!(
                item.action,
                SettingsAction::ToggleSource(info.id),
                "a planned source must not claim to be toggleable"
            );
        }
    }
}

/// The description is assembled from registry facts, so it can lag the
/// code only if the registry itself lies.
#[test]
fn test_a_row_describes_groups_and_the_browser_the_registry_declares() {
    let config = Config::default();

    for (item, info) in source_settings_items(&config).iter().zip(KNOWN_SOURCES.iter()) {
        let text = item.description.join("\n");
        assert!(text.contains("Enable/disable"), "{}", info.id);
        let groups: Vec<String> = info.groups.iter().map(|g| format!("{:?}", g)).collect();
        assert!(text.contains(&groups.join(", ")), "{}: {}", info.id, text);

        let browser_line = if info.requires_browser {
            "Uses an automated browser."
        } else {
            "No browser required."
        };
        assert!(text.contains(browser_line), "{}: {}", info.id, text);
    }
}
