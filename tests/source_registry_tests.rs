use doris::search::source::{self, Group, KNOWN_SOURCES, Source};
use doris::search::rutor::RutorSearcher;

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
    for id in ["nnmclub"] {
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
    assert_eq!(browser_backed, vec!["rutracker"]);
    // Planned sources must not promise a browser: nothing builds them,
    // so nothing can hand one over.
    assert!(!source::get_source("nnmclub").unwrap().requires_browser);
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

    // A planned source with no groups belongs to no group view.
    assert!(source::sources_by_group(Group::Anime).iter().all(|s| s.id != "nnmclub"));
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
