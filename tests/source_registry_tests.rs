use doris::search::source::KNOWN_SOURCES;

#[test]
fn test_rutracker_is_registered_and_implemented() {
    let rutracker = KNOWN_SOURCES.iter().find(|s| s.id == "rutracker");
    assert!(rutracker.is_some(), "rutracker must be in KNOWN_SOURCES");
    assert!(rutracker.unwrap().implemented, "rutracker should be marked implemented");
}

#[test]
fn test_future_sources_are_listed_but_not_implemented() {
    for id in ["rutor", "nnmclub"] {
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
        assert!(!source.display_name.is_empty());
    }
}

#[test]
fn test_at_least_one_source_is_usable_today() {
    assert!(KNOWN_SOURCES.iter().any(|s| s.implemented), "at least one source must actually work");
}
