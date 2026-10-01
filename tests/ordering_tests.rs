//! Ordering tests: the dedup rules and the two default orders.

use doris::sources::models::TorrentItem;
use doris::sources::ordering::{dedupe_by_hash, default_order};

/// A row with just the fields an order actually reads.
fn row(hash: &str) -> TorrentItem {
    TorrentItem {
        title: hash.to_string(),
        info_hash: hash.to_string(),
        ..Default::default()
    }
}

fn with(mut item: TorrentItem, source: &str, size: u64, seeds: u32, added: i64) -> TorrentItem {
    item.source = source.to_string();
    item.size_bytes = size;
    item.seeds_n = seeds;
    item.added = added;
    item
}

fn hashes(items: &[TorrentItem]) -> Vec<&str> {
    items.iter().map(|i| i.info_hash.as_str()).collect()
}

// --- dedupe_by_hash ----------------------------------------------------------

#[test]
fn test_same_hash_keeps_the_healthier_copy_in_first_position() {
    let list = vec![
        with(row("dup"), "rutor", 100, 3, 0),
        with(row("other"), "rutor", 100, 1, 0),
        with(row("dup"), "rutracker", 100, 12, 0),
    ];

    let out = dedupe_by_hash(&list);

    assert_eq!(hashes(&out), ["dup", "other"], "one row per hash");
    assert_eq!(out[0].seeds_n, 12, "the healthier copy wins");
    assert_eq!(out[0].source, "rutracker");
    assert_eq!(out[0].title, "dup");
}

#[test]
fn test_equal_seeds_keep_the_first_occurrence() {
    let list = vec![
        with(row("dup"), "rutor", 100, 5, 0),
        with(row("dup"), "rutracker", 100, 5, 0),
    ];

    let out = dedupe_by_hash(&list);

    assert_eq!(out.len(), 1);
    assert_eq!(out[0].source, "rutor", "first in wins when health is equal");
}

#[test]
fn test_rows_without_a_hash_are_never_collapsed() {
    // The deviation from torio: an empty hash proves nothing, and
    let list = vec![
        with(row(""), "rutracker", 100, 3, 0),
        with(row(""), "rutracker", 100, 3, 0),
        with(row(""), "rutor", 100, 3, 0),
    ];

    let out = dedupe_by_hash(&list);

    assert_eq!(out.len(), 3, "hashless rows must all survive");
    assert_eq!(hashes(&out), ["", "", ""]);
}

// --- default_order -----------------------------------------------------------

#[test]
fn test_search_order_is_most_seeds_then_newest() {
    let list = vec![
        with(row("cold"), "rutor", 0, 2, 9000),
        with(row("healthy"), "rutor", 0, 40, 1000),
        with(row("warm"), "rutor", 0, 40, 2000),
    ];

    let out = default_order(&list, false);

    assert_eq!(
        hashes(&out),
        ["warm", "healthy", "cold"],
        "seeds desc, then added desc"
    );
}

#[test]
fn test_browse_order_ranks_the_registered_providers_first() {
    // B9 will pass `browsing: true`; for now this pins the port of
    let list = vec![
        with(row("other"), "rutracker", 0, 0, 9000),
        with(row("rutor-new"), "rutor", 0, 0, 1000),
        with(row("rutor-newer"), "rutor", 0, 0, 2000),
        with(row("nnm"), "nnmclub", 0, 0, 500),
    ];

    let out = default_order(&list, true);

    assert_eq!(
        hashes(&out),
        ["nnm", "rutor-newer", "rutor-new", "other"],
        "nnmclub, then rutor newest-first, then everything else"
    );
}
