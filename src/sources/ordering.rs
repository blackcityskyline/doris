//! Pure ordering functions for the merged multi-source result list. Everything here is a plain
//! function over `&[TorrentItem]` returning a new `Vec` -- no UI, no network, no state.

use super::models::TorrentItem;

/// Rows carrying the same info hash are the same torrent, so keep only the healthiest copy
/// (highest `seeds_n`) at the first occurrence's position.
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

/// The default order: healthiest first (`seeds_n` descending), then newest `added` first.
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
