//! The Results filter's syntax: a grep-shaped mini-language rather than one opaque substring.
//! Every token is ANDed with the rest, so narrowing is the default and widening needs a `-`.

use doris::filter::Filter;
use doris::sources::models::TorrentItem;
use doris::sources::source::Group;

fn row(title: &str, source: &str, group: Option<Group>) -> TorrentItem {
    TorrentItem {
        title: title.to_string(),
        source: source.to_string(),
        group,
        ..Default::default()
    }
}

fn sized(title: &str, size_bytes: u64, seeds_n: u32) -> TorrentItem {
    TorrentItem {
        title: title.to_string(),
        size_bytes,
        seeds_n,
        ..Default::default()
    }
}

// --- plain words ---------------------------------------------------------

#[test]
fn a_plain_word_is_a_substring_of_any_field() {
    let f = Filter::parse("dune");
    assert!(f.matches(&row("Dune 2024", "rutor", None)));
    assert!(f.matches(&row("Something Else", "dune", None)));
    assert!(!f.matches(&row("Blade Runner", "tpb", None)));

    // The category counts as a field too, so its label is findable.
    let by_group = Filter::parse("movies");
    assert!(by_group.matches(&row("Any Title", "tpb", Some(Group::Movies))));
    assert!(!by_group.matches(&row("Any Title", "tpb", Some(Group::TV))));
}

#[test]
fn every_token_must_match() {
    let f = Filter::parse("dune 1080");
    assert!(f.matches(&row("Dune 1080p", "rutor", None)));
    assert!(!f.matches(&row("Dune 720p", "rutor", None)));
}

#[test]
fn a_dash_negates_the_token_after_it() {
    let f = Filter::parse("dune -cam");
    assert!(f.matches(&row("Dune 1080p", "rutor", None)));
    assert!(!f.matches(&row("Dune CAM rip", "rutor", None)));
}

// --- fields --------------------------------------------------------------

#[test]
fn src_selects_the_tracker_id() {
    let f = Filter::parse("src:rutor");
    assert!(f.matches(&row("Any Title", "rutor", None)));
    assert!(!f.matches(&row("Any Title", "rutracker", None)));
}

#[test]
fn tracker_is_an_alias_for_src() {
    let f = Filter::parse("tracker:rutor");
    assert!(f.matches(&row("Any Title", "rutor", None)));
    assert!(!f.matches(&row("Any Title", "tpb", None)));
}

#[test]
fn group_and_cat_select_the_category() {
    for spec in ["group:movies", "cat:movies"] {
        let f = Filter::parse(spec);
        assert!(
            f.matches(&row("Any Title", "rutor", Some(Group::Movies))),
            "{spec} matches Movies"
        );
        assert!(
            !f.matches(&row("Any Title", "rutor", Some(Group::TV))),
            "{spec} rejects TV"
        );
        assert!(
            !f.matches(&row("Any Title", "rutor", None)),
            "{spec} rejects a row with no group"
        );
    }
}

#[test]
fn title_looks_only_at_the_title() {
    let f = Filter::parse("title:dune");
    assert!(f.matches(&row("Dune 2024", "rutor", None)));
    // "dune" only in the source: `title:` must not see it.
    assert!(!f.matches(&row("Blade Runner", "dune", None)));
}

// --- numbers -------------------------------------------------------------

#[test]
fn size_compares_bytes_with_units() {
    let big = sized("Big", 2_000_000_000, 0);
    let small = sized("Small", 500_000, 0);

    assert!(Filter::parse("size:>1gb").matches(&big));
    assert!(!Filter::parse("size:>1gb").matches(&small));
    assert!(Filter::parse("size:<1gb").matches(&small));
    assert!(!Filter::parse("size:<1gb").matches(&big));
    assert!(Filter::parse("size:>=2gb").matches(&big));
    assert!(Filter::parse("size:<=500kb").matches(&small));
    assert!(Filter::parse("size:=2000000000").matches(&big));
}

#[test]
fn seeds_compares_the_peer_count() {
    let busy = sized("Busy", 0, 120);
    let dead = sized("Dead", 0, 0);

    assert!(Filter::parse("seeds:>50").matches(&busy));
    assert!(!Filter::parse("seeds:>50").matches(&dead));
    assert!(Filter::parse("seeds:0").matches(&dead));
    assert!(Filter::parse("seeds:>=100").matches(&busy));
    assert!(!Filter::parse("seeds:<100").matches(&busy));
}

// --- falls back instead of dying silently --------------------------------

#[test]
fn an_unknown_field_is_just_a_word() {
    // `magnet:` is not a field; the token must still match rows that
    let f = Filter::parse("magnet:x");
    assert!(f.matches(&row("has magnet:x in it", "rutor", None)));
    assert!(!f.matches(&row("nothing here", "rutor", None)));
}

#[test]
fn an_unparsable_number_is_just_a_word() {
    let f = Filter::parse("seeds:abc");
    assert!(f.matches(&row("seeds:abc in the title", "rutor", None)));
    assert!(!f.matches(&row("nothing here", "rutor", None)));
}

#[test]
fn an_empty_filter_matches_everything() {
    let f = Filter::parse("   ");
    assert!(f.matches(&row("Anything", "rutor", None)));
}

// --- the box, not just the parser ----------------------------------------

/// Requirement the parser alone cannot prove: with several trackers in
/// play, typing `rutor` narrows to rutor's rows and leaves
/// rutracker's alone -- and the field syntax composes with it.
#[test]
fn the_filter_box_uses_the_syntax_on_real_rows() {
    let mut app = doris::ui::view::App::new("http://127.0.0.1:8090".into(), None);
    app.results = vec![
        TorrentItem {
            title: "Dune 2024 1080p".into(),
            source: "rutor".into(),
            size_bytes: 2_000_000_000,
            seeds_n: 40,
            ..Default::default()
        },
        TorrentItem {
            title: "Dune 2024 cam".into(),
            source: "rutracker".into(),
            size_bytes: 700_000_000,
            seeds_n: 3,
            ..Default::default()
        },
    ];

    app.zones.filter_input = "rutor".into();
    app.update_filter();
    assert_eq!(app.filtered_indices, vec![0], "`rutor` is only rutor");

    app.zones.filter_input = "src:rutor size:>1gb seeds:>10 -cam".into();
    app.update_filter();
    assert_eq!(app.filtered_indices, vec![0], "the tokens compose");

    app.zones.filter_input = "src:rutracker".into();
    app.update_filter();
    assert_eq!(app.filtered_indices, vec![1], "and can be flipped");
}
