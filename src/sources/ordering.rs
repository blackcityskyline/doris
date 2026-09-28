//! Pure ordering functions for the merged multi-source result list
//! (ROADMAP.md B4), ported from torio's `dedupe` + `defaultOrder`
//! (`ui/hooks/useConcurrentSearch.ts`) and `ui/sort.ts`.
//!
//! Everything here is a plain function over `&[TorrentItem]` returning a
//! new `Vec` -- no UI, no network, no state -- which is what lets
//! `tests/ordering_tests.rs` mirror torio's `sort.test.ts` case by case.
//!
//! Applying any of them to `App::ui.results` happens once, when a search
//! generation finishes (`search.rs::finish_search`): sorting per arriving
//! batch would reshuffle rows under the user's selection while sources
//! are still answering.

use super::models::TorrentItem;

/// Rows carrying the same info hash are the same torrent, so keep only
/// the healthiest copy (highest `seeds_n`) at the first occurrence's
/// position.
///
/// Deliberate deviation from torio: rows with an **empty** `info_hash`
/// are never collapsed against each other. torio's `Map` keyed on the
/// hash would merge every hashless row into one, and a missing hash
/// proves nothing about two rows being the same torrent (rutracker
/// doesn't expose hashes at all today).
pub fn dedupe_by_hash(items: &[TorrentItem]) -> Vec<TorrentItem> {
    let mut out: Vec<TorrentItem> = Vec::with_capacity(items.len());
    // Keyed by the input's hash strings; the value is the position in
    // `out`, so a better copy replaces in place and keeps that position.
    let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();

    for item in items {
        if item.info_hash.is_empty() {
            out.push(item.clone());
            continue;
        }
        match seen.get(item.info_hash.as_str()) {
            Some(&slot) => {
                if item.seeds_n > out[slot].seeds_n {
                    out[slot] = item.clone();
                }
            }
            None => {
                seen.insert(item.info_hash.as_str(), out.len());
                out.push(item.clone());
            }
        }
    }
    out
}

/// torio's `BROWSE_SOURCE_PRIORITY`, re-keyed by *our* source ids: in
/// browse mode the Russian providers come first, everything else (today
/// that includes rutracker) trails.
///
/// torio additionally maps `torentino` to 2; doris has no such source, so
/// the map stops at rutor rather than naming an id that can never appear
/// in a list.
const BROWSE_SOURCE_PRIORITY: &[(&str, u8)] = &[("nnmclub", 0), ("rutor", 1)];

fn browse_priority(source: &str) -> u8 {
    BROWSE_SOURCE_PRIORITY
        .iter()
        .find(|(id, _)| *id == source)
        .map(|(_, rank)| *rank)
        .unwrap_or(u8::MAX)
}

/// torio's `defaultOrder`: healthiest first -- `seeds_n` descending,
/// then newest `added` first. Browse mode ("fresh releases", B9) instead
/// ranks sources by [`BROWSE_SOURCE_PRIORITY`] and then by `added`;
/// no caller passes `true` yet because browse mode does not exist.
pub fn default_order(items: &[TorrentItem], browsing: bool) -> Vec<TorrentItem> {
    let mut out = items.to_vec();
    if browsing {
        out.sort_by(|a, b| {
            browse_priority(&a.source)
                .cmp(&browse_priority(&b.source))
                .then_with(|| b.added.cmp(&a.added))
        });
    } else {
        out.sort_by(|a, b| {
            b.seeds_n
                .cmp(&a.seeds_n)
                .then_with(|| b.added.cmp(&a.added))
        });
    }
    out
}

/// One selectable sort, or [`Sort::None`] for the untouched default
/// order (torio's `"none"`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    None,
    Field(SortField, SortDir),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortField {
    Size,
    Seeds,
    Source,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

/// The order the `s` key cycles through (torio's `SORT_CYCLE`): start
/// untouched, then each field ascending then descending, then back to
/// untouched. Seven states.
pub const SORT_CYCLE: [Sort; 7] = [
    Sort::None,
    Sort::Field(SortField::Size, SortDir::Asc),
    Sort::Field(SortField::Size, SortDir::Desc),
    Sort::Field(SortField::Seeds, SortDir::Asc),
    Sort::Field(SortField::Seeds, SortDir::Desc),
    Sort::Field(SortField::Source, SortDir::Asc),
    Sort::Field(SortField::Source, SortDir::Desc),
];

/// The next state of the sort cycle, wrapping around to [`Sort::None`].
pub fn next_sort(current: Sort) -> Sort {
    let pos = SORT_CYCLE.iter().position(|s| *s == current).unwrap_or(0);
    SORT_CYCLE[(pos + 1) % SORT_CYCLE.len()]
}

/// Apply one sort selection, leaving [`Sort::None`] alone (arrival order
/// preserved). Tie-breakers match torio's exactly, and stay *descending*
/// regardless of the primary direction: size ties fall back to more
/// seeds, seed ties to newer `added`, source ties to more seeds.
pub fn sort_results(items: &[TorrentItem], sort: Sort) -> Vec<TorrentItem> {
    let mut out = items.to_vec();
    match sort {
        Sort::None => return out,
        Sort::Field(SortField::Size, dir) => out.sort_by(|a, b| {
            with_dir(a.size_bytes.cmp(&b.size_bytes), dir).then_with(|| b.seeds_n.cmp(&a.seeds_n))
        }),
        Sort::Field(SortField::Seeds, dir) => out.sort_by(|a, b| {
            with_dir(a.seeds_n.cmp(&b.seeds_n), dir).then_with(|| b.added.cmp(&a.added))
        }),
        Sort::Field(SortField::Source, dir) => out.sort_by(|a, b| {
            with_dir(a.source.cmp(&b.source), dir).then_with(|| b.seeds_n.cmp(&a.seeds_n))
        }),
    }
    out
}

fn with_dir(ord: std::cmp::Ordering, dir: SortDir) -> std::cmp::Ordering {
    match dir {
        SortDir::Asc => ord,
        SortDir::Desc => ord.reverse(),
    }
}
