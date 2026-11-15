//! Pure ordering functions for the merged multi-source result list. Everything here is a plain
//! function over `&[TorrentItem]` returning a new `Vec` -- no UI, no network, no state.

use super::models::TorrentItem;

/// Rows carrying the same info hash are the same torrent, so keep only the healthiest copy
/// (highest `seeds_n`) at the first occurrence's position.
pub fn dedupe_by_hash(items: &[TorrentItem]) -> Vec<TorrentItem> {
    let mut out: Vec<TorrentItem> = Vec::with_capacity(items.len());
    // Keyed by the input's hash strings; the value is the position in
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

/// Which source goes first in browse mode: the Russian providers, then
/// everything else (today that includes rutracker), each group newest
/// `added` first.
const BROWSE_SOURCE_PRIORITY: &[(&str, u8)] = &[("nnmclub", 0), ("rutor", 1)];

fn browse_priority(source: &str) -> u8 {
    BROWSE_SOURCE_PRIORITY
        .iter()
        .find(|(id, _)| *id == source)
        .map(|(_, rank)| *rank)
        .unwrap_or(u8::MAX)
}

/// Which key the Results table sorts on, in the order `o` cycles them.
///
/// Two states, not three: the first is what a search already did before the
/// key existed (healthiest first), so a table nobody has touched reads the
/// same as it always has, and the key only ever adds a second arrangement.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortBy {
    /// Seeds descending, then newest -- the default, and the browse order
    /// when there is no query.
    #[default]
    Seeds,
    /// Size descending, then seeds, then newest.
    Size,
}

impl SortBy {
    /// The next arrangement `o` asks for.
    pub fn next(self) -> Self {
        match self {
            SortBy::Seeds => SortBy::Size,
            SortBy::Size => SortBy::Seeds,
        }
    }

    /// What the frame and the log call it.
    pub fn label(self) -> &'static str {
        match self {
            SortBy::Seeds => "seeds",
            SortBy::Size => "size",
        }
    }
}

/// Put the rows in `sort`'s order.
///
/// `browsing` keeps the source ranking for the default arrangement only:
/// browse mode is "freshest from the trackers I trust", which is a
/// different question from "healthiest", and the `o` key asking for size
/// should still answer it.
pub fn order(items: &[TorrentItem], sort: SortBy, browsing: bool) -> Vec<TorrentItem> {
    let mut out = items.to_vec();
    match sort {
        SortBy::Seeds if !browsing => out.sort_by(|a, b| {
            b.seeds_n
                .cmp(&a.seeds_n)
                .then_with(|| b.added.cmp(&a.added))
        }),
        SortBy::Seeds => {
            out.sort_by(|a, b| {
                browse_priority(&a.source)
                    .cmp(&browse_priority(&b.source))
                    .then_with(|| b.added.cmp(&a.added))
            });
        }
        SortBy::Size => out.sort_by(|a, b| {
            b.size_bytes
                .cmp(&a.size_bytes)
                .then_with(|| b.seeds_n.cmp(&a.seeds_n))
                .then_with(|| b.added.cmp(&a.added))
                .then_with(|| a.title.cmp(&b.title))
        }),
    }
    out
}

/// The default order: healthiest first (`seeds_n` descending), then newest `added` first.
pub fn default_order(items: &[TorrentItem], browsing: bool) -> Vec<TorrentItem> {
    order(items, SortBy::Seeds, browsing)
}
