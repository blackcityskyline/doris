use doris::config::Config;
use doris::sources::orchestrator;
use doris::sources::source::{self, Group, KNOWN_SOURCES, Source, SourceEnv, SourceInfo, GROUP_ORDER};
use doris::sources::rutor::RutorSearcher;
use doris::sources::x1337x::X1337xSearcher;
use doris::ui::modals::settings::{SettingsAction, source_settings_items, streaming_settings_items};

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
    // Every source that is not implemented yet makes no promises: no
    // groups, no browser, no category filter, no browse. Torentino used
    // to be the example here and B8 wave 3 moved it out; with the list
    // empty this is the invariant the next planned source has to satisfy.
    let planned: Vec<&SourceInfo> = KNOWN_SOURCES
        .iter()
        .filter(|s| !s.implemented)
        .collect();
    for source in planned {
        assert!(
            source.groups.is_empty(),
            "{} is planned and declares groups",
            source.id
        );
        assert!(
            !source.requires_browser,
            "{} is planned and promises a browser",
            source.id
        );
        assert!(
            !source.category_filter,
            "{} is planned and claims a category filter",
            source.id
        );
        assert!(
            !source.supports_browse,
            "{} is planned and claims browse",
            source.id
        );
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

/// B6's acceptance line: a source does not get a category it cannot
/// serve without a documented reason. `SourceInfo::category_filter` is
/// the capability; this list is the reason column -- an implemented
/// source sits here exactly while it declares groups it cannot filter
/// by, and the comment beside its entry in `KNOWN_SOURCES` says why.
/// Empty today: every implemented source serves the categories it
/// declares, rutracker included (its `f[]` slot was verified live
/// 26.09.2026). A new source that cannot filter belongs here.
const CATEGORY_FILTER_UNVERIFIED: &[&str] = &[];

#[test]
fn test_a_category_a_source_cannot_serve_is_documented_as_unverified() {
    for info in KNOWN_SOURCES.iter().filter(|s| s.implemented) {
        let documented = CATEGORY_FILTER_UNVERIFIED.contains(&info.id);
        assert_eq!(
            info.category_filter, !documented,
            "{}: `category_filter: false` must appear in \
             CATEGORY_FILTER_UNVERIFIED (with its reason next to the \
             entry), and every other source is taken to serve the \
             categories it declares",
            info.id
        );
    }

    // Nothing stale on that list either: an entry that no longer
    // exists, is still planned, or does claim the capability it
    // excuses would be paperwork hiding a real behavior.
    for id in CATEGORY_FILTER_UNVERIFIED {
        let info = KNOWN_SOURCES
            .iter()
            .find(|s| s.id == *id)
            .unwrap_or_else(|| panic!("{} is not in KNOWN_SOURCES", id));
        assert!(info.implemented, "{}: a planned source needs no excuse", id);
        assert!(!info.category_filter, "{}: verified at last -- drop it", id);
        assert!(
            !info.groups.is_empty(),
            "{}: a source with no groups has nothing to excuse",
            id
        );
    }

    // The shape the roadmap named, now resolved: the browser-backed
    // source that declares all four groups does filter them, on the
    // strength of its own live test.
    let rutracker = KNOWN_SOURCES
        .iter()
        .find(|s| s.id == "rutracker")
        .expect("rutracker is registered");
    assert!(rutracker.requires_browser);
    assert_eq!(rutracker.groups.len(), 4);
    assert!(
        rutracker.category_filter,
        "rutracker's f[] slot was verified live: two forum ids answer \
         disjoint topic sets"
    );
    for id in ["rutor", "1337x", "nnmclub", "tpb", "yts", "eztv", "nyaa"] {
        let info = KNOWN_SOURCES
            .iter()
            .find(|s| s.id == id)
            .unwrap_or_else(|| panic!("{} is not registered", id));
        assert!(info.category_filter, "{} must serve the category", id);
    }
}

/// B6: the category row is built from `GROUP_ORDER` and `Group::label`,
/// so both must stay in step with the enum -- a group missing from the
/// order would be unreachable from the UI (nothing to click), and a
/// repeated label would make two tabs the same category.
#[test]
fn test_group_order_offers_every_group_once_and_labels_them_distinctly() {
    for group in [Group::Games, Group::Movies, Group::TV, Group::Anime] {
        assert_eq!(
            GROUP_ORDER.iter().filter(|&&g| g == group).count(),
            1,
            "{:?} must appear in the category row exactly once",
            group
        );
    }

    let labels: Vec<&str> = GROUP_ORDER.iter().map(|g| g.label()).collect();
    let mut unique = labels.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        labels.len(),
        "two tabs would read as one category: {:?}",
        labels
    );
    assert_eq!(labels, vec!["Movies", "TV", "Games", "Anime"]);
}

#[test]
fn test_only_browser_backed_sources_ask_for_a_browser() {
    // `requires_browser` is what makes the orchestrator skip
    // `Browser::launch` entirely for plain-HTTP sources (and what B0.1's
    // `source_needs_browser` ends up backed by). Rutracker is the only
    // source that constructs a browser today -- 1337x used to be listed
    // beside it on the strength of three mirrors answering 403, and wave
    // 3 took it off (B8): the fourth mirror answers every path, so the
    // challenge is those mirrors' business, not a session it lacks.
    let browser_backed: Vec<&str> = KNOWN_SOURCES
        .iter()
        .filter(|s| s.requires_browser)
        .map(|s| s.id)
        .collect();
    assert_eq!(browser_backed, vec!["rutracker"]);
    assert!(!source::get_source("1337x").unwrap().requires_browser);
    // A planned source still declares how its host behaves, because
    // that is what the flag is about: probed 25.09.2026 with a browser
    // UA, torentino answered 200 with its front page. What holds the
    // claim back is that nothing builds it, so it cannot route a
    // request today.
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

    // 1337x is constructible offline too, so its row gets the same
    // treatment -- and this is the flip wave 3 is made of: implemented,
    // no browser, four groups, one home URL that is not the mirror the
    // probes found answering.
    let x = X1337xSearcher::new();
    let x_info = source::get_source("1337x").expect("1337x must be registered");
    assert_eq!(x.id(), x_info.id);
    assert_eq!(x.label(), x_info.label);
    assert_eq!(x.groups(), x_info.groups);
    assert_eq!(x.home_url(), x_info.home_url);
    assert_eq!(x.requires_browser(), x_info.requires_browser);
    assert!(!x.requires_browser());
    assert!(x_info.implemented);
    // B9: the trait's browse answer and the registry's must agree, or a
    // browse would ask a source that answers with a broken page.
    assert!(rutor.supports_browse(), "rutor's homepage answers an empty query");
    assert_eq!(rutor.supports_browse(), info.supports_browse);
    assert!(x.supports_browse(), "1337x's /home/ answers an empty query");
    assert_eq!(x.supports_browse(), x_info.supports_browse);
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

/// B9: the CLI asks what the `all` tab asks -- one list, not two. A
/// second hand-written list in `main.rs` is how the CLI ends up skipping
/// a source the TUI offers, or offering one the user disabled.
/// B9: the registry's `supports_browse` is what `selected_sources` filters
/// on, so it has to agree with what each source's `Source` impl answers.
/// The two that cannot are the browser-backed one (rutracker) and the
/// one whose empty-query shape was never verified (1337x).
#[test]
fn test_the_registry_says_which_sources_can_browse() {
    for (id, can) in [
        ("rutor", true),
        ("yts", true),
        ("tpb", true),
        ("subsplease", true),
        ("eztv", true),
        ("nnmclub", true),
        ("1337x", true),
        ("nyaa", false),
        ("rutracker", false),
    ] {
        let info = source::get_source(id)
            .unwrap_or_else(|| panic!("{id} is registered"));
        assert_eq!(info.supports_browse, can, "{id}");
    }
}

/// The streaming category is derived too: it must offer the TorrServer
/// gate and the reachability check, so a user can switch the server off
/// and still see whether it answers.
#[test]
fn test_the_streaming_category_offers_the_torrserver_gate() {
    let items = streaming_settings_items(&Config::default(), true, "Streaming");
    let labels: Vec<&str> = items.iter().map(|item| item.label.as_str()).collect();

    let gate = items
        .iter()
        .find(|item| item.label == "Enable TorrServer")
        .expect("the category must offer the TorrServer gate");
    assert!(
        matches!(gate.action, SettingsAction::ToggleEnableTorrserver),
        "the gate must toggle the config"
    );
    assert_eq!(gate.value, "True", "enabled by default");

    assert!(
        labels.contains(&"TorrServer"),
        "the reachability check must stay: {:?}",
        labels
    );
    assert!(
        labels.contains(&"Edit credentials"),
        "the login entry point must stay: {:?}",
        labels
    );
}

#[test]
fn test_the_cli_asks_what_the_all_tab_asks() {
    let enabled: Vec<String> = KNOWN_SOURCES
        .iter()
        .filter(|s| s.implemented)
        .map(|s| s.id.to_string())
        .collect();

    let cli = source::cli_sources(None, &enabled).expect("the registry answers");
    let tab = orchestrator::selected_sources("all", &enabled, None, false);
    assert_eq!(
        cli.len(),
        tab.len(),
        "CLI and tab bar must derive from one list"
    );

    // A disabled source is not asked when the CLI defaults to "all".
    let mut mostly_disabled = enabled.clone();
    mostly_disabled.retain(|id| id != "rutor");
    let cli = source::cli_sources(None, &mostly_disabled).expect("the registry answers");
    assert!(
        !cli.iter().any(|info| info.id == "rutor"),
        "a disabled source is skipped"
    );

    // `--source` names exactly one, and refuses what it cannot serve
    // rather than silently falling back to a default.
    let one = source::cli_sources(Some("rutracker"), &enabled).expect("rutracker is registered");
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].id, "rutracker");
    assert!(
        source::cli_sources(Some("never-heard-of-it"), &enabled).is_err(),
        "an unknown id is refused"
    );
    // Torentino is implemented now (B8 wave 3), so naming it works --
    // and names exactly it.
    let torentino =
        source::cli_sources(Some("torentino"), &enabled).expect("torentino is implemented");
    assert_eq!(torentino.len(), 1);
    assert_eq!(torentino[0].id, "torentino");
}
