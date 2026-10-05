//! SubsPlease's JSON API ported from torio's `subsplease.ts`. The API is a map keyed by
//! `"<show> - <episode>"`, and three live facts (checked 25.09.2026) shape the code: - **A miss
//! answers `[]`, not `{}`** -- an array where the success case is an object.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::Deserialize;

use super::format::{format_bytes, format_date};
use super::magnet::parse_magnet;
use super::models::TorrentItem;
use super::net::{browser_client, fetch_resilient, FetchOptions};
use super::source::{AuthContext, Group, LogFn, SearchPage, SearchRequest, Source};

pub const API: &str = "https://subsplease.org/api/";

/// The show page a row links to, e.g.
const SHOWS_URL: &str = "https://subsplease.org/shows/";

/// Best resolution wins, exactly torio's preference order: SubsPlease
/// releases one episode in up to three sizes and the table gets one
/// row for it (the wave-1 decision).
const RES_PREFERENCE: [&str; 3] = ["1080", "720", "480"];

#[derive(Debug, Deserialize)]
struct SpDownload {
    res: Option<String>,
    magnet: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SpEntry {
    show: Option<String>,
    episode: Option<String>,
    release_date: Option<String>,
    page: Option<String>,
    downloads: Option<Vec<SpDownload>>,
}

/// The endpoint for one request: `f=search&s=` for a query, `f=latest`
/// for browse -- which is what makes an empty query *mean* something
/// here (`Source::supports_browse`).
pub fn api_url(query: &str) -> String {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        format!("{}?tz=UTC&f=latest", API)
    } else {
        format!("{}?tz=UTC&f=search&s={}", API, urlencoding::encode(trimmed))
    }
}

/// torio's `pickBest`: 1080, then 720, then 480, then -- if none of the
/// named resolutions carry a magnet -- whatever magnet exists at all.
fn pick_best(downloads: &[SpDownload]) -> Option<&SpDownload> {
    for res in RES_PREFERENCE {
        if let Some(found) = downloads
            .iter()
            .find(|d| d.res.as_deref() == Some(res) && d.magnet.is_some())
        {
            return Some(found);
        }
    }
    downloads.iter().find(|d| d.magnet.is_some())
}

/// The magnet's `xl=<bytes>` parameter -- SubsPlease is the only wave-1 source whose size
/// travels *inside* the magnet, because the API sends no size of its own.
fn size_from_magnet(magnet: &str) -> u64 {
    for prefix in ["?xl=", "&xl="] {
        let Some(at) = magnet.find(prefix) else {
            continue;
        };
        let digits: String = magnet[at + prefix.len()..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if let Ok(size) = digits.parse::<u64>() {
            return size;
        }
    }
    0
}

pub fn parse_rows(body: &str) -> Result<Vec<TorrentItem>> {
    let document: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| anyhow!("SubsPlease response did not parse: {}", e))?;

    let map = match document {
        // A miss answers `[]` (live-checked) -- that is "no results",
        serde_json::Value::Array(_) => return Ok(Vec::new()),
        serde_json::Value::Object(map) => map,
        other => anyhow::bail!("SubsPlease sent an unexpected document: {}", other),
    };

    let mut rows = Vec::new();
    for value in map.values() {
        // A malformed entry costs one row, not the whole page: the API
        let Ok(entry) = serde_json::from_value::<SpEntry>(value.clone()) else {
            continue;
        };
        if let Some(row) = to_row(&entry) {
            rows.push(row);
        }
    }
    Ok(rows)
}

/// One entry -> its single (best-resolution) row, or `None` when the
/// entry has no usable magnet at all.
fn to_row(entry: &SpEntry) -> Option<TorrentItem> {
    let download = pick_best(entry.downloads.as_deref().unwrap_or_default())?;
    let magnet = download.magnet.as_deref()?;
    let parsed = parse_magnet(magnet)?;

    let show = entry
        .show
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Unknown");
    let episode = entry
        .episode
        .as_deref()
        .filter(|e| !e.trim().is_empty())
        .map(|e| format!(" - {}", e))
        .unwrap_or_default();
    let resolution = download.res.as_deref().unwrap_or("?");
    // torio's name: `${show}${ep} [${res}p]`.
    let title = format!("{}{} [{}p]", show, episode, resolution);

    let added = entry
        .release_date
        .as_deref()
        .and_then(|raw| chrono::DateTime::parse_from_rfc2822(raw.trim()).ok())
        .map(|dt| dt.timestamp())
        .unwrap_or(0);
    let page = entry.page.as_deref().filter(|p| !p.is_empty());

    let size_bytes = size_from_magnet(magnet);

    Some(TorrentItem {
        // The original magnet, trackers and `xl` intact (B7 keeps
        magnet: Some(parsed.magnet.clone()),
        info_hash: parsed.info_hash.clone(),
        title,
        size_bytes,
        size: format_bytes(size_bytes),
        // No seed/peer data in this API: "" is the display string for
        seeds: String::new(),
        seeds_n: 0,
        leechers: 0,
        added,
        date: format_date(added),
        // Magnet-only: the download key writes `<title>.magnet`.
        download_url: String::new(),
        page_url: page
            .map(|p| format!("{}{}/", SHOWS_URL, p))
            .unwrap_or_default(),
        source: "subsplease".to_string(),
        group: Some(Group::Anime),
        query: String::new(),
        ..Default::default()
    })
}

pub struct SubsPleaseSearcher {
    client: reqwest::Client,
}

impl Default for SubsPleaseSearcher {
    fn default() -> Self {
        Self::new()
    }
}

impl SubsPleaseSearcher {
    pub const HOME_URL: &str = "https://subsplease.org";

    pub fn new() -> Self {
        Self {
            client: browser_client(),
        }
    }
}

#[async_trait]
impl Source for SubsPleaseSearcher {
    fn id(&self) -> &'static str {
        "subsplease"
    }

    fn label(&self) -> &'static str {
        "SubsPlease"
    }

    fn groups(&self) -> &'static [Group] {
        // SubsPlease is anime-only, so the group is not a filter here
        &[Group::Anime]
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
        let url = api_url(&req.query);
        // One attempt, torio's `retries: 1`: there is no mirror list
        let options = FetchOptions {
            retries: 1,
            ..FetchOptions::default()
        };
        let response = fetch_resilient(&url, || self.client.get(&url), &options).await?;
        anyhow::ensure!(
            response.status().is_success(),
            "SubsPlease returned {}",
            response.status()
        );
        let body = response.text().await?;

        Ok(SearchPage {
            items: parse_rows(&body)?,
            // The API has no cursor: `f=search` returns every match,
            has_more: false,
            next_offset: None,
        })
    }

    async fn download_torrent(&self, _url: &str) -> Result<Vec<u8>> {
        // Magnets only (see the module doc): the download key writes a
        anyhow::bail!(
            "SubsPlease rows carry a magnet, not a .torrent file -- stream it, \
             or save the link as a .magnet file"
        )
    }
}
