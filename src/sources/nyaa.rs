//! Nyaa's RSS feed, parsed from the markup as it actually came back on
//! 25.09.2026 -- one live response of 75 items, captured before
//! `ddos-guard` started answering 504 to this network on every path.
//!
//! - **One `<item>` per release, everything we show inside it.** All of
//!   title, hash, size, seeders, leechers, category id, pubDate and both
//!   links were present in 75 of 75, so all are filled in -- links
//!   included, because a link we *saw* is markup, and whether it answers
//!   is the same question as whether search itself answers.
//! - **No magnets**: 0 of 75 carry one, so a row's magnet is built from
//!   its hash with the shared trackers.
//! - **Every category is requested** (`c=0_0`), so a row's group comes
//!   from its own `nyaa:categoryId` rather than from the query: `1_*`
//!   *is* anime on the site, and whatever else `c=0_0` drags in (Audio)
//!   has no group to claim and stays `None`.
//!
//! What could **not** be verified while the host was down, and is
//! consequently not claimed anywhere in this file: pagination (`&p=2`),
//! an empty-query feed, and the reachability of the `.torrent` and view
//! links. Hence `has_more: false` -- one feed page with an unknown
//! cursor, not "there is no second page" -- and `supports_browse` false.

use anyhow::Result;
use async_trait::async_trait;

use super::format::{format_bytes, format_date, parse_size, unescape_entities};
use super::magnet::{build_magnet, is_info_hash, normalize_info_hash};
use super::models::TorrentItem;
use super::net::{browser_client, fetch_resilient, FetchOptions};
use super::source::{AuthContext, Group, LogFn, SearchPage, SearchRequest, Source};

/// The RSS endpoint: `page=rss` selects the feed, `c`/`f` scope it.
pub const RSS: &str = "https://nyaa.si/?page=rss";

/// `c=0_0` = every category (wave-2 decision, matching torio). The
/// alternative, `c=1_0` for anime-only, would also drop nyaa's own
/// soundtrack and audiobook uploads of anime -- something
/// filtering should decide per search, not something the source should
/// bake in.
const ALL_CATEGORIES: &str = "0_0";

/// One feed page: 75 items on the live query, and no cursor we have
/// seen move (see the module doc). The UI must not offer "load more"
/// into a page whose existence we never confirmed.
const PAGE_ITEMS: usize = 75;

/// How hard nyaa tries -- one attempt, deliberately not torio's
/// default of five (wave-2 decision, taken after measuring: a 504 from
/// `ddos-guard` costs ~16 s *per attempt* here, and the orchestrator
/// gives a source 25 s total, so five retries could never report their
/// own outcome -- the user would just read `timed out after 25s` with
/// the cause hidden behind it). One attempt spends ~16 s and names the
/// status, which is the difference between "nyaa is blocked" and "the
/// app is broken".
///
/// The download path uses the same budget: nothing downstream waits
/// 96 s for it either, and a user who wants a second try can press the
/// key again.
pub fn fetch_options() -> FetchOptions {
    FetchOptions {
        retries: 0,
        ..FetchOptions::default()
    }
}

/// The feed URL for `query`. Public so the fixture tests and the live
/// test build exactly what `search` sends.
pub fn feed_url(query: &str) -> String {
    format!(
        "{}&q={}&c={}&f=0",
        RSS,
        urlencoding::encode(query.trim()),
        ALL_CATEGORIES
    )
}

/// The text inside `<name>...</name>` of one item, tolerating
/// attributes on the opening tag (`<guid isPermaLink="true">`) and an
/// optional CDATA wrapper -- torio's `tag()` rule, for feeds that spell
/// the same field two ways.
fn tag(item: &str, name: &str) -> Option<String> {
    let open = format!("<{}", name);
    let open_at = item.find(&open)?;
    let after = open_at + open.len();
    // The tag name has to *end* here: `<link>` matches, `<linkfoo>`
    // does not, and `<nyaa:size>` matches only when asked for whole.
    match item.as_bytes().get(after) {
        Some(&b'>') | Some(&b' ') | Some(&b'\n') | Some(&b'\t') => {}
        _ => return None,
    }
    let gt = after + item[after..].find('>')?;
    let close = format!("</{}", name);
    let end = gt + 1 + item[gt + 1..].find(&close)?;
    let inner = &item[gt + 1..end];
    let inner = inner.strip_prefix("<![CDATA[").unwrap_or(inner);
    let inner = inner.strip_suffix("]]>").unwrap_or(inner);
    Some(inner.trim().to_string())
}

/// `nyaa:categoryId` -> the group the row may claim. `1_*` is the
/// site's own Anime branch (1_2 English-translated, 1_3 Non-English,
/// 1_4 Raw were all in the live feed); `2_*` and the rest are not
/// Anime, and the `Group` enum has no honest name for them, so they
/// stay `None` -- visible only under the "all" tab (B6 owns the rest).
fn group_from_category(category_id: &str) -> Option<Group> {
    if category_id.starts_with("1_") {
        Some(Group::Anime)
    } else {
        None
    }
}

/// `<pubDate>` -> unix seconds, `0` when absent or unparseable.
///
/// nyaa writes RFC-2822 with a `-0000` offset ("Sat, 12 Sep 2026
/// 15:45:00 -0000", the live sample), which means "offset unknown"
/// rather than a real zone; `chrono` reads it directly, and the
/// `+0000` retry below exists only in case a stricter parser ever
/// disagrees -- it says the same thing about the instant.
fn parse_pubdate(raw: &str) -> i64 {
    let raw = raw.trim();
    if raw.is_empty() {
        return 0;
    }
    if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(raw) {
        return dt.timestamp();
    }
    if let Some(rest) = raw.strip_suffix("-0000") {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc2822(&format!("{}+0000", rest)) {
            return dt.timestamp();
        }
    }
    0
}

/// One `<item>` -> its row, or `None` when the item cannot become one
/// (no hash, a hash that is not a hash, no title). A single malformed
/// item costs one row, never the page.
fn to_row(item: &str) -> Option<TorrentItem> {
    let title = unescape_entities(&tag(item, "title")?);
    if title.is_empty() {
        return None;
    }

    let info_hash = normalize_info_hash(&tag(item, "nyaa:infoHash")?);
    if !is_info_hash(&info_hash) {
        return None;
    }

    let size_text = tag(item, "nyaa:size").unwrap_or_default();
    let size_bytes = parse_size(&size_text);
    let seeds = tag(item, "nyaa:seeders").unwrap_or_default();
    let leechers = tag(item, "nyaa:leechers").unwrap_or_default();
    let category = tag(item, "nyaa:categoryId").unwrap_or_default();
    let added = parse_pubdate(&tag(item, "pubDate").unwrap_or_default());

    Some(TorrentItem {
        // nyaa ships no magnet (0 of 75 live), so this one is built
        // from the verified hash; the download key still has a real
        // `.torrent` to fetch below.
        magnet: Some(build_magnet(&info_hash, &title)),
        info_hash,
        title,
        size: format_bytes(size_bytes),
        size_bytes,
        seeds: seeds.trim().to_string(),
        seeds_n: seeds.trim().parse::<u32>().unwrap_or(0),
        leechers: leechers.trim().parse::<u32>().unwrap_or(0),
        added,
        date: format_date(added),
        download_url: tag(item, "link").unwrap_or_default(),
        page_url: tag(item, "guid").unwrap_or_default(),
        source: "nyaa".to_string(),
        group: group_from_category(&category),
        query: String::new(),
    })
}

/// The feed -> rows. Public so the fixture tests exercise the real
/// parser with no network, as with `yts::parse_page` / `tpb::parse_rows`.
pub fn parse_items(body: &str) -> Result<Vec<TorrentItem>> {
    // A `ddos-guard` challenge or an error page is not "no results":
    // saying so out loud is the difference between "the tracker is
    // empty" and "we did not reach the tracker".
    if !body.contains("<rss") && !body.contains("<item>") {
        anyhow::bail!("nyaa returned a document that is not an RSS feed");
    }
    let rows: Vec<TorrentItem> = body.split("<item>").skip(1).filter_map(to_row).collect();
    Ok(rows)
}

/// The rows -> the page the UI sees. Split out so the two claims this
/// source makes about paging are testable without the network: one
/// feed page, no cursor (`SearchPage::has_more` lives here, not inside
/// `search`, where only a live run could reach it).
pub fn to_page(items: Vec<TorrentItem>) -> SearchPage {
    SearchPage {
        items,
        // One feed page, and no cursor we have seen move. Claiming
        // `true` would invite "load more" into a page whose URL we have
        // never seen answered (the module doc's follow-up).
        has_more: false,
        next_offset: None,
    }
}

/// How many items the live feed put on one page -- the number the
/// module doc quotes, kept as one constant rather than a comment.
pub fn page_items() -> usize {
    PAGE_ITEMS
}

pub struct NyaaSearcher {
    client: reqwest::Client,
}

impl Default for NyaaSearcher {
    fn default() -> Self {
        Self::new()
    }
}

impl NyaaSearcher {
    pub const HOME_URL: &str = "https://nyaa.si";

    pub fn new() -> Self {
        Self {
            client: browser_client(),
        }
    }
}

#[async_trait]
impl Source for NyaaSearcher {
    fn id(&self) -> &'static str {
        "nyaa"
    }

    fn label(&self) -> &'static str {
        "Nyaa"
    }

    fn groups(&self) -> &'static [Group] {
        // Anime is what the tracker *is*; the all-category query above
        // is why individual rows still carry their own group (or none).
        &[Group::Anime]
    }

    fn home_url(&self) -> &'static str {
        Self::HOME_URL
    }

    fn requires_browser(&self) -> bool {
        false
    }

    fn supports_browse(&self) -> bool {
        // Not claimed: the empty-query feed was never answered live
        // (B8 wave 2 decision -- see the module doc). Wiring it up is
        // browse work, once the host answers again.
        false
    }

    async fn ensure_logged_in(&self, _auth: &AuthContext, _log: &LogFn) -> Result<bool> {
        Ok(true)
    }

    async fn search(&self, req: &SearchRequest) -> Result<SearchPage> {
        let query = req.query.trim();
        anyhow::ensure!(
            !query.is_empty(),
            "Nyaa has no live-verified browse feed, so an empty query has nothing \
             honest to return -- type a query"
        );

        let url = feed_url(query);
        let response = fetch_resilient(&url, || self.client.get(&url), &fetch_options()).await?;
        let status = response.status();
        anyhow::ensure!(status.is_success(), "Nyaa returned {}", status);
        let body = response.text().await?;

        Ok(to_page(parse_items(&body)?))
    }

    async fn download_torrent(&self, url: &str) -> Result<Vec<u8>> {
        // Rows carry the `.torrent` link the feed ships (75 of 75
        // live). The status check matters for the same reason rutor's
        // has one: a block page must never reach TorrServer as a
        //.torrent file.
        let response = fetch_resilient(url, || self.client.get(url), &fetch_options()).await?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("nyaa download {} answered HTTP {}", url, status);
        }
        Ok(response.bytes().await?.to_vec())
    }
}
