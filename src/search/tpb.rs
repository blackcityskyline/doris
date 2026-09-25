//! The Pirate Bay through apibay.org's JSON (ROADMAP.md B8 wave 1).
//!
//! apibay is the front door torio uses too, and it has one quirk worth
//! the module doc: **the same field arrives as a string and as a number
//! depending on the endpoint** -- checked live 25.09.2026:
//! `q.php` answers `"size":"1992277407"`, the precompiled top-100 lists
//! answer `"size":3808117223`. A parser that only reads one spelling
//! silently zeroes the other, so [`ApibayItem`] types those fields
//! against [`Num`] and accepts both.
//!
//! Two more live facts shape the code:
//!
//! - **A query returns at most 100 rows and there is no cursor**:
//!   `page=`/`start=` are ignored (verified: both return the identical
//!   first 100). So `has_more` is always `false` -- not "there is
//!   nothing more" but "this API will not hand over more", and paging
//!   would need a different endpoint.
//! - **"No results" is a row, not an empty array**: `id == "0"` with an
//!   all-zero `info_hash` and the name "No results returned". Rendering
//!   it would show one fake result for every miss.
//!
//! Both search and browse are magnet-only (`download_url == ""`), which
//! routes the download key to a `.magnet` file; see `magnet_only_download`.

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::Deserialize;

use super::format::{format_bytes, format_date};
use super::magnet::build_magnet;
use super::models::TorrentItem;
use super::net::{FetchOptions, browser_client, fetch_resilient};
use super::source::{AuthContext, Group, LogFn, SearchPage, SearchRequest, Source};

/// The API base. Single host: apibay *is* the service, so there is no
/// mirror list to fail over to and no `first_ok` in this file.
pub const API: &str = "https://apibay.org";

/// apibay's answer to a query that matched nothing: one row that is a
/// placeholder, not a result (live: `{"id":"0", "info_hash":"000...0",
/// "name":"No results returned"}`).
const ZERO_HASH: &str = "0000000000000000000000000000000000000000";

/// Top-100 lists, one per group this source declares -- the browse URLs
/// (empty query) mirror torio's `TOP_MOVIES`/`TOP_TV`.
pub const TOP_MOVIES_URL: &str = "https://apibay.org/precompiled/data_top100_207.json";
pub const TOP_TV_URL: &str = "https://apibay.org/precompiled/data_top100_208.json";

/// apibay's numeric-ish fields, which are strings on `q.php` and
/// numbers on the top-100 endpoints. Untagged so serde picks by JSON
/// type, with the parse falling back to 0 on garbage -- a malformed
/// count should cost one row's detail, not the whole page.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum Num {
    Int(i64),
    Str(String),
}

impl Num {
    fn as_i64(&self) -> i64 {
        match self {
            Num::Int(n) => *n,
            Num::Str(s) => s.trim().parse::<i64>().unwrap_or(0),
        }
    }

    fn as_u64(&self) -> u64 {
        self.as_i64().max(0) as u64
    }

    fn as_u32(&self) -> u32 {
        self.as_i64().max(0) as u32
    }
}

/// One apibay row. Fields are optional because the two endpoints do not
/// agree on which ones they send (`num_files`/`username`/`imdb` come and
/// go); a missing field degrades, it does not fail the page.
#[derive(Debug, Deserialize)]
struct ApibayItem {
    id: Option<Num>,
    name: Option<String>,
    info_hash: Option<String>,
    seeders: Option<Num>,
    leechers: Option<Num>,
    size: Option<Num>,
    added: Option<Num>,
    category: Option<Num>,
}

/// The search URL for one query. `cat=all`: B8 wave 1 deliberately does
/// not filter -- category filtering is B6's job, and the row's own
/// `category` is already carried into `TorrentItem::group`.
pub fn search_url(query: &str) -> String {
    format!("{}/q.php?q={}", API, urlencoding::encode(query.trim()))
}

/// TPB's category -> [`Group`] mapping, restricted to the two groups
/// this source declares (`Group::Movies`, `Group::TV` -- the pair the
/// wave-1 decision set). Everything else -- games (301-309), music
/// (101-109), apps, books, and the unclassified leftovers -- stays
/// `None`: unattributed rows show only in the "all" view, and claiming
/// a group the registry does not promise would put them somewhere the
/// source was never asked to speak for.
fn group_for_category(category: i64) -> Option<Group> {
    match category {
        // 201 Movies, 202 DVD, 207 HD Movies, 209 3D -- torio's
        // MOVIE_CATS, mapped rather than filtered.
        201 | 202 | 207 | 209 => Some(Group::Movies),
        // 205 TV shows, 208 TV episodes -- torio's TV_CATS.
        205 | 208 => Some(Group::TV),
        _ => None,
    }
}

/// apibay JSON -> rows, with the placeholder row dropped. Public so the
/// fixture tests can exercise the real parser with no network (the same
/// split `yts::parse_page` has).
pub fn parse_rows(body: &str) -> Result<Vec<TorrentItem>> {
    let items: Vec<ApibayItem> = serde_json::from_str(body)
        .map_err(|e| anyhow!("apibay response did not parse: {}", e))?;
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
    if item.id.as_ref().map(Num::as_i64).unwrap_or(0) <= 0 {
        return None;
    }

    let title = item
        .name
        .as_deref()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("Unknown")
        .to_string();
    let size_bytes = item.size.as_ref().map_or(0, Num::as_u64);
    let seeds = item.seeders.as_ref().map_or(0, Num::as_u32);
    let added = item.added.as_ref().map_or(0, Num::as_i64);
    let category = item.category.as_ref().map_or(0, Num::as_i64);
    let id = item.id.as_ref().map(Num::as_i64).unwrap_or(0);

    Some(TorrentItem {
        magnet: Some(build_magnet(&info_hash, &title)),
        title: title.clone(),
        size: format_bytes(size_bytes),
        seeds: seeds.to_string(),
        size_bytes,
        seeds_n: seeds,
        leechers: item.leechers.as_ref().map_or(0, Num::as_u32),
        added,
        date: format_date(added),
        info_hash,
        // apibay serves magnets only; there is no file to fetch, so the
        // download key writes `<title>.magnet` instead of refusing.
        download_url: String::new(),
        // Live-verified: this form answers 200 (a bare `/torrent/<id>`
        // 302s to it).
        page_url: format!("https://thepiratebay.org/description.php?id={}", id),
        source: "tpb".to_string(),
        group: group_for_category(category),
        query: String::new(),
    })
}

pub struct TpbSearcher {
    client: reqwest::Client,
}

impl TpbSearcher {
    pub const HOME_URL: &str = "https://thepiratebay.org";

    pub fn new() -> Self {
        // The shared browser-like client (B5): apibay answers a
        // library UA with 403 (checked live) -- this is not paranoia.
        Self { client: browser_client() }
    }

    /// One apibay document -> its rows' source data, or a real error.
    async fn fetch_items(&self, url: &str) -> Result<Vec<ApibayItem>> {
        // One attempt per fetch, torio's `retries: 1`: there is no
        // second host to fall back to, and apibay is fast to re-ask.
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
        // torio's (tpb-movies + tpb-tv), without category filtering.
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
            // Browse: both top-100 lists, movies and episodes together.
            // Two requests rather than a merged endpoint, because apibay
            // has none -- and 100+100 rows arrive before the table can
            // finish drawing the first 100 anyway.
            let mut items = self.fetch_items(TOP_MOVIES_URL).await?;
            items.extend(self.fetch_items(TOP_TV_URL).await?);
            items
        } else {
            self.fetch_items(&search_url(&query)).await?
        };

        let items: Vec<TorrentItem> = items.iter().filter_map(to_row).collect();
        Ok(SearchPage {
            items,
            // No cursor exists on either path (see the module doc): a
            // query tops out at 100 rows, the top-100 lists are fixed.
            // `next_offset: None` keeps the row-based default, which is
            // never consulted while `has_more` says "stop".
            has_more: false,
            next_offset: None,
        })
    }

    async fn download_torrent(&self, _url: &str) -> Result<Vec<u8>> {
        // Same refusal as YTS, for the same reason: apibay serves
        // magnets. The download key short-circuits to a `.magnet` file
        // before this is reached; what lands here is a fallback path
        // that deserves a plain explanation rather than junk bytes.
        anyhow::bail!(
            "TPB rows carry a magnet, not a .torrent file -- stream it, \
             or save the link as a .magnet file"
        )
    }
}
