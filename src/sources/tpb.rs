//! The Pirate Bay through apibay.org's JSON. apibay is the front door torio uses too, and it
//! has one quirk worth the module doc: **the same field arrives as a string and as a number
//! depending on the endpoint** -- checked live 25.09.2026: `q.php` answers
//! `"size":"1992277407"`, the precompiled top-100 lists answer `"size":3808117223`.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::Deserialize;

use super::format::{format_bytes, format_date};
use super::magnet::build_magnet;
use super::models::{FlexNum, TorrentItem};
use super::net::{browser_client, fetch_resilient, FetchOptions};
use super::source::{AuthContext, Group, LogFn, SearchPage, SearchRequest, Source};

pub const API: &str = "https://apibay.org";

/// apibay's answer to a query that matched nothing: one row that is a
/// placeholder, not a result (live: `{"id":"0", "info_hash":"000...0",
/// "name":"No results returned"}`).
const ZERO_HASH: &str = "0000000000000000000000000000000000000000";

/// Top-100 lists, one per group this source declares -- the browse URLs
/// (empty query) mirror torio's `TOP_MOVIES`/`TOP_TV`.
pub const TOP_MOVIES_URL: &str = "https://apibay.org/precompiled/data_top100_207.json";
pub const TOP_TV_URL: &str = "https://apibay.org/precompiled/data_top100_208.json";

#[derive(Debug, Deserialize)]
struct ApibayItem {
    id: Option<FlexNum>,
    name: Option<String>,
    info_hash: Option<String>,
    seeders: Option<FlexNum>,
    leechers: Option<FlexNum>,
    size: Option<FlexNum>,
    added: Option<FlexNum>,
    category: Option<FlexNum>,
}

/// The apibay category ids behind each group this source declares -- the single source of truth
/// for both halves of B6: the `cat=` list the server is asked to trim by, and the mapping a
/// returned row's own `category` is read back through.
const GROUP_CATS: [(Group, &[i64]); 2] = [
    (Group::Movies, &[201, 202, 207, 209, 211]),
    (Group::TV, &[205, 208, 212]),
];

/// The ids apibay is asked for when it should trim to `group`, and
/// nothing at all for a group this source never declares -- the
/// orchestrator only asks a source for groups it registered, so the
/// empty list is a safe landing rather than a branch with a user
/// behind it.
fn group_cats(group: Group) -> &'static [i64] {
    GROUP_CATS
        .iter()
        .find(|(known, _)| *known == group)
        .map(|(_, ids)| *ids)
        .unwrap_or(&[])
}

pub fn search_url(query: &str, category: Option<Group>) -> String {
    let mut url = format!("{}/q.php?q={}", API, urlencoding::encode(query.trim()));
    if let Some(group) = category {
        let ids: Vec<String> = group_cats(group).iter().map(i64::to_string).collect();
        if !ids.is_empty() {
            url.push_str("&cat=");
            url.push_str(&ids.join(","));
        }
    }
    url
}

/// TPB's category -> [`Group`] mapping, read back through [`GROUP_CATS`] -- the same list the
/// server filter is built from.
fn group_for_category(category: i64) -> Option<Group> {
    GROUP_CATS
        .iter()
        .find(|(_, ids)| ids.contains(&category))
        .map(|(group, _)| *group)
}

pub fn parse_rows(body: &str) -> Result<Vec<TorrentItem>> {
    let items: Vec<ApibayItem> =
        serde_json::from_str(body).map_err(|e| anyhow!("apibay response did not parse: {}", e))?;
    Ok(items.iter().filter_map(to_row).collect())
}

/// One apibay row -> one result, or `None` for the placeholder: an
/// all-zero hash (or a non-positive `id`) is apibay's "nothing found"
/// shaped as a row, and it must never reach the table.
fn to_row(item: &ApibayItem) -> Option<TorrentItem> {
    let raw_hash = item.info_hash.as_deref().unwrap_or_default();
    let info_hash = raw_hash.to_lowercase();
    if info_hash.is_empty() || info_hash == ZERO_HASH {
        return None;
    }
    if item.id.as_ref().map(FlexNum::as_i64).unwrap_or(0) <= 0 {
        return None;
    }

    let title = item
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Unknown")
        .to_string();
    let size_bytes = item.size.as_ref().map_or(0, FlexNum::as_u64);
    let seeds = item.seeders.as_ref().map_or(0, FlexNum::as_u32);
    let added = item.added.as_ref().map_or(0, FlexNum::as_i64);
    let category = item.category.as_ref().map_or(0, FlexNum::as_i64);
    let id = item.id.as_ref().map(FlexNum::as_i64).unwrap_or(0);

    Some(TorrentItem {
        magnet: Some(build_magnet(&info_hash, &title)),
        title: title.clone(),
        size: format_bytes(size_bytes),
        seeds: seeds.to_string(),
        size_bytes,
        seeds_n: seeds,
        leechers: item.leechers.as_ref().map_or(0, FlexNum::as_u32),
        added,
        date: format_date(added),
        info_hash,
        // apibay serves magnets only; there is no file to fetch, so the
        download_url: String::new(),
        // Live-verified: this form answers 200 (a bare `/torrent/<id>`
        page_url: format!("https://thepiratebay.org/description.php?id={}", id),
        source: "tpb".to_string(),
        group: group_for_category(category),
        query: String::new(),
    })
}

pub struct TpbSearcher {
    client: reqwest::Client,
}

impl Default for TpbSearcher {
    fn default() -> Self {
        Self::new()
    }
}

impl TpbSearcher {
    pub const HOME_URL: &str = "https://thepiratebay.org";

    pub fn new() -> Self {
        // The shared browser-like client: apibay answers a
        Self {
            client: browser_client(),
        }
    }

    /// One apibay document -> its rows' source data, or a real error.
    async fn fetch_items(&self, url: &str) -> Result<Vec<ApibayItem>> {
        // One attempt per fetch, torio's `retries: 1`: there is no
        let options = FetchOptions {
            retries: 1,
            ..FetchOptions::default()
        };
        let response = fetch_resilient(url, || self.client.get(url), &options).await?;
        anyhow::ensure!(
            response.status().is_success(),
            "apibay returned {} for {}",
            response.status(),
            url
        );
        let body = response.text().await?;
        let items: Vec<ApibayItem> = serde_json::from_str(&body)
            .map_err(|e| anyhow!("apibay response did not parse: {}", e))?;
        Ok(items)
    }
}

#[async_trait]
impl Source for TpbSearcher {
    fn id(&self) -> &'static str {
        "tpb"
    }

    fn label(&self) -> &'static str {
        "TPB"
    }

    fn groups(&self) -> &'static [Group] {
        // The wave-1 decision: one apibay source covering both of
        &[Group::Movies, Group::TV]
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
        let query = req.query.trim().to_string();
        let items = if query.is_empty() {
            // Browse: the two top-100 lists, trimmed to the category
            let mut urls: Vec<&'static str> = Vec::new();
            match req.category {
                None => {
                    urls.push(TOP_MOVIES_URL);
                    urls.push(TOP_TV_URL);
                }
                Some(Group::Movies) => urls.push(TOP_MOVIES_URL),
                Some(Group::TV) => urls.push(TOP_TV_URL),
                Some(Group::Games) | Some(Group::Anime) => {}
            }
            let mut items = Vec::new();
            for url in urls {
                items.extend(self.fetch_items(url).await?);
            }
            items
        } else {
            self.fetch_items(&search_url(&query, req.category)).await?
        };

        let items: Vec<TorrentItem> = items.iter().filter_map(to_row).collect();
        Ok(SearchPage {
            items,
            // No cursor exists on either path (see the module doc): a
            has_more: false,
            next_offset: None,
        })
    }

    async fn download_torrent(&self, _url: &str) -> Result<Vec<u8>> {
        // Same refusal as YTS, for the same reason: apibay serves
        anyhow::bail!(
            "TPB rows carry a magnet, not a .torrent file -- stream it, \
             or save the link as a .magnet file"
        )
    }
}
