//! EZTV's JSON API, ported from torio's `eztv.ts`. Live-checked 25.09.2026, and three findings
//! shape this file: - **`eztvx.to` answers, `eztv.re` only 301s** -- so the host is the one
//! that works, not the one an older list may name.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::Deserialize;

use super::format::{format_bytes, format_date};
use super::magnet::{build_magnet, normalize_info_hash};
use super::models::{FlexNum, TorrentItem};
use super::net::{browser_client, fetch_resilient, FetchOptions};
use super::source::{AuthContext, Group, LogFn, SearchPage, SearchRequest, Source};

/// The endpoint that answers (see the module doc for why not `eztv.re`).
pub const API: &str = "https://eztvx.to/api/get-torrents";

pub const PAGE_SIZE: usize = 100;

#[derive(Debug, Deserialize)]
struct EztvTorrent {
    title: Option<String>,
    filename: Option<String>,
    hash: Option<String>,
    magnet_url: Option<String>,
    /// `string | number` by the API's own typings -- live: a string.
    size_bytes: Option<FlexNum>,
    /// Live: numbers, read through [`FlexNum`] so a quoted one costs
    /// nothing rather than the page.
    seeds: Option<FlexNum>,
    peers: Option<FlexNum>,
    date_released_unix: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct EztvResponse {
    torrents_count: Option<u64>,
    torrents: Option<Vec<EztvTorrent>>,
}

/// The paged URL: `offset` is in rows, and only whole pages of them
/// count -- see [`parse_page`].
pub fn torrents_url(offset: usize) -> String {
    let page = (offset / PAGE_SIZE) + 1;
    format!("{}?limit={}&page={}", API, PAGE_SIZE, page)
}

pub fn parse_page(body: &str, offset: usize) -> Result<SearchPage> {
    let response: EztvResponse =
        serde_json::from_str(body).map_err(|e| anyhow!("EZTV response did not parse: {}", e))?;
    let page_number = (offset / PAGE_SIZE) + 1;

    let items: Vec<TorrentItem> = response
        .torrents
        .iter()
        .flatten()
        .filter_map(to_row)
        .collect();

    let loaded = (page_number * PAGE_SIZE) as u64;
    let has_more = match response.torrents_count {
        // The index says how far it goes, so the verdict is arithmetic
        Some(total) => loaded < total,
        // No count in the answer: a full page may well have a next one
        None => items.len() >= PAGE_SIZE,
    };

    Ok(SearchPage {
        items,
        has_more,
        // The *page* boundary, not `offset + rows`: dropping a row with
        next_offset: Some(page_number * PAGE_SIZE),
    })
}

/// One API row -> one result, or `None` when it cannot be used: no
/// hash, a hash that is not a hash, or no magnet (torio skips the same
/// three cases).
fn to_row(row: &EztvTorrent) -> Option<TorrentItem> {
    let raw = row.hash.as_deref()?.trim();
    if raw.is_empty() {
        return None;
    }
    // The API sends lowercase hex already; `normalize_info_hash` also
    let info_hash = normalize_info_hash(raw);
    if info_hash.len() != 40 || !info_hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }

    let title = row
        .title
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| row.filename.as_deref().filter(|s| !s.trim().is_empty()))
        .unwrap_or(&info_hash)
        .to_string();
    // `magnet_url` is shipped, but a row without it is still usable --
    let magnet = match row.magnet_url.as_deref().map(str::trim) {
        Some(url) if !url.is_empty() => url.to_string(),
        _ => build_magnet(&info_hash, &title),
    };

    let size_bytes = row.size_bytes.as_ref().map_or(0, FlexNum::as_u64);
    let seeds = row.seeds.as_ref().map_or(0, FlexNum::as_u32);
    let added = row.date_released_unix.unwrap_or(0);

    Some(TorrentItem {
        magnet: Some(magnet),
        info_hash,
        title,
        size_bytes,
        size: format_bytes(size_bytes),
        seeds: seeds.to_string(),
        seeds_n: seeds,
        leechers: row.peers.as_ref().map_or(0, FlexNum::as_u32),
        added,
        date: format_date(added),
        // No verifiable per-torrent page (see the module doc).
        download_url: String::new(),
        page_url: String::new(),
        source: "eztv".to_string(),
        group: Some(Group::TV),
        query: String::new(),
    })
}

pub struct EztvSearcher {
    client: reqwest::Client,
}

impl Default for EztvSearcher {
    fn default() -> Self {
        Self::new()
    }
}

impl EztvSearcher {
    pub const HOME_URL: &str = "https://eztvx.to";

    pub fn new() -> Self {
        // The shared browser-like client: the HTML side of this
        Self {
            client: browser_client(),
        }
    }
}

#[async_trait]
impl Source for EztvSearcher {
    fn id(&self) -> &'static str {
        "eztv"
    }

    fn label(&self) -> &'static str {
        "EZTV"
    }

    fn groups(&self) -> &'static [Group] {
        // TV only, and the row attribution agrees (`Group::TV`).
        &[Group::TV]
    }

    fn home_url(&self) -> &'static str {
        Self::HOME_URL
    }

    fn requires_browser(&self) -> bool {
        false
    }

    fn supports_browse(&self) -> bool {
        true
    }

    async fn ensure_logged_in(&self, _auth: &AuthContext, _log: &LogFn) -> Result<bool> {
        Ok(true)
    }

    async fn search(&self, req: &SearchRequest) -> Result<SearchPage> {
        // The wave-1 decision: say why, don't show an empty table.
        if !req.query.trim().is_empty() {
            anyhow::bail!(
                "EZTV has no search: its API ignores the `search` parameter \
                 (verified live). Leave the query empty to browse the newest releases."
            );
        }

        let url = torrents_url(req.offset);
        let options = FetchOptions {
            retries: 1,
            ..FetchOptions::default()
        };
        let response = fetch_resilient(&url, || self.client.get(&url), &options).await?;
        anyhow::ensure!(
            response.status().is_success(),
            "EZTV returned {}",
            response.status()
        );
        let body = response.text().await?;
        parse_page(&body, req.offset)
    }

    async fn download_torrent(&self, _url: &str) -> Result<Vec<u8>> {
        // Magnet-only, as every wave-1 JSON source: the download key
        anyhow::bail!(
            "EZTV rows carry a magnet, not a .torrent file -- stream it, \
             or save the link as a .magnet file"
        )
    }
}
