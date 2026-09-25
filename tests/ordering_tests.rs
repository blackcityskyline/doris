//! Ordering tests mirroring torio's `sort.test.ts` cases plus the dedup
//! cases ROADMAP.md B4 specifies (including the deliberate deviation:
//! rows with an empty info hash are never collapsed).

use doris::search::models::TorrentItem;
use doris::search::ordering::{
    SORT_CYCLE, Sort, SortDir, SortField, default_order, dedupe_by_hash, next_sort, sort_results,
};

/// `torio: r(...)` -- a row with just the fields a sort actually reads.
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

// --- next_sort / SORT_CYCLE --------------------------------------------------

#[test]
fn test_cycle_visits_seven_states_and_wraps_to_none() {
    let mut seq = Vec::new();
    let mut sort = Sort::None;
    for _ in 0..7 {
        sort = next_sort(sort);
        seq.push(sort);
    }
    assert_eq!(
        seq,
        vec![
            Sort::Field(SortField::Size, SortDir::Asc),
            Sort::Field(SortField::Size, SortDir::Desc),
            Sort::Field(SortField::Seeds, SortDir::Asc),
            Sort::Field(SortField::Seeds, SortDir::Desc),
            Sort::Field(SortField::Source, SortDir::Asc),
            Sort::Field(SortField::Source, SortDir::Desc),
            Sort::None,
        ],
        "the cycle must be none -> size -> seeds -> source -> none"
    );
}

#[test]
fn test_cycle_has_exactly_seven_states_starting_with_none() {
    assert_eq!(SORT_CYCLE.len(), 7);
    assert_eq!(SORT_CYCLE[0], Sort::None);
    // Wrapping: after the last state comes the first again.
    assert_eq!(next_sort(*SORT_CYCLE.last().unwrap()), Sort::None);
}

// --- sort_results ------------------------------------------------------------

#[test]
fn test_none_preserves_the_arrival_order() {
    let list = vec![
        with(row("a"), "rutor", 1, 1, 0),
        with(row("b"), "rutor", 9, 9, 0),
        with(row("c"), "rutor", 5, 5, 0),
    ];

    assert_eq!(hashes(&sort_results(&list, Sort::None)), ["a", "b", "c"]);
    // The input is untouched: these functions hand back a new list.
    assert_eq!(hashes(&list), ["a", "b", "c"]);
}

#[test]
fn test_size_directions() {
    let list = vec![
        with(row("a"), "rutor", 500, 0, 0),
        with(row("b"), "rutor", 100, 0, 0),
        with(row("c"), "rutor", 900, 0, 0),
    ];
    let asc = sort_results(&list, Sort::Field(SortField::Size, SortDir::Asc));
    let desc = sort_results(&list, Sort::Field(SortField::Size, SortDir::Desc));

    assert_eq!(hashes(&asc), ["b", "a", "c"], "size asc: smallest first");
    assert_eq!(hashes(&desc), ["c", "a", "b"], "size desc: largest first");
}

#[test]
fn test_seeds_directions() {
    let list = vec![
        with(row("a"), "rutor", 0, 50, 0),
        with(row("b"), "rutor", 0, 5, 0),
        with(row("c"), "rutor", 0, 90, 0),
    ];
    let asc = sort_results(&list, Sort::Field(SortField::Seeds, SortDir::Asc));
    let desc = sort_results(&list, Sort::Field(SortField::Seeds, SortDir::Desc));

    assert_eq!(hashes(&asc), ["b", "a", "c"], "seeds asc: fewest first");
    assert_eq!(hashes(&desc), ["c", "a", "b"], "seeds desc: most first");
}

#[test]
fn test_source_directions() {
    // Ascending: torio's inputs are a=yts, b=eztv, c=nyaa.
    let asc_input = vec![
        with(row("a"), "yts", 0, 0, 0),
        with(row("b"), "eztv", 0, 0, 0),
        with(row("c"), "nyaa", 0, 0, 0),
    ];
    let asc = sort_results(&asc_input, Sort::Field(SortField::Source, SortDir::Asc));
    assert_eq!(hashes(&asc), ["b", "c", "a"], "source asc: A->Z");

    // Descending: torio's inputs are a=eztv, b=yts, c=nyaa.
    let desc_input = vec![
        with(row("a"), "eztv", 0, 0, 0),
        with(row("b"), "yts", 0, 0, 0),
        with(row("c"), "nyaa", 0, 0, 0),
    ];
    let desc = sort_results(&desc_input, Sort::Field(SortField::Source, SortDir::Desc));
    assert_eq!(hashes(&desc), ["b", "c", "a"], "source desc: Z->A");
}

#[test]
fn test_tie_breakers_are_the_ones_torio_uses() {
    // size ties fall back to *more seeds*, in both directions.
    let same_size = vec![
        with(row("fewer"), "rutor", 100, 1, 0),
        with(row("more"), "rutor", 100, 9, 0),
    ];
    for dir in [SortDir::Asc, SortDir::Desc] {
        let out = sort_results(&same_size, Sort::Field(SortField::Size, dir));
        assert_eq!(hashes(&out), ["more", "fewer"], "size tie, {:?}", dir);
    }

    // seed ties fall back to *newer added*, in both directions.
    let same_seeds = vec![
        with(row("old"), "rutor", 0, 5, 1000),
        with(row("new"), "rutor", 0, 5, 2000),
    ];
    for dir in [SortDir::Asc, SortDir::Desc] {
        let out = sort_results(&same_seeds, Sort::Field(SortField::Seeds, dir));
        assert_eq!(hashes(&out), ["new", "old"], "seed tie, {:?}", dir);
    }

    // source ties fall back to *more seeds*, in both directions.
    let same_source = vec![
        with(row("quiet"), "rutor", 0, 1, 0),
        with(row("busy"), "rutor", 0, 7, 0),
    ];
    for dir in [SortDir::Asc, SortDir::Desc] {
        let out = sort_results(&same_source, Sort::Field(SortField::Source, dir));
        assert_eq!(hashes(&out), ["busy", "quiet"], "source tie, {:?}", dir);
    }
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
    // rutracker rows have no hash at all -- collapsing them would leave
    // one row per search.
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
    // torio's BROWSE_SOURCE_PRIORITY against our source ids.
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
