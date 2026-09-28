//! Torentino -- a DLE-based games tracker -- over its HTML (ROADMAP.md
//! B8 wave 3), probed live on 26.09.2026.
//!
//! What the live host established, and what the code therefore does:
//!
//! - **Search is a POST, not a GET**: `/load` with
//!   `do=search&subaction=search&a=2`, answered with the results as HTML
//!   rows under `<div id="entryID<N>">`. A plain browser-like client
//!   gets a 200 with the results -- no challenge, no login.
//! - **The `.torrent` link lives on the item page, not in the search
//!   row** -- the row's own "Скачать торрент" button links the item page
//!   itself. So `download_torrent` fetches the item page and follows the
//!   `/load/0-0-0-<id>-<n>` link it finds there (`301 -> /_ld/.../*.torrent`,
//!   a real bencoded file). That is why no bencode crate is needed: the
//!   bytes travel the existing `.torrent -> upload_torrent` path (B7), and
//!   no magnet is ever built because the site serves none. Rows carry no
//!   `info_hash`, which `dedupe_by_hash` explicitly lets through.
//! - **The pagination block never offered a next page** in four probes
//!   ("matrix" 1 row, "gta" 34, "игра" 5, "скачать" 40), so `has_more`
//!   reads the block and defaults to false -- the honest answer when the
//!   site does not say there is a next page.
//! - **Rows claim Games**: the site is a games tracker, top to bottom.

use anyhow::{Result, bail};
use async_trait::async_trait;
use regex::Regex;
use scraper::{Html, Selector};

use super::format::unescape_entities;
use super::models::TorrentItem;
use super::net::{FetchOptions, browser_client, fetch_resilient};
use super::source::{AuthContext, Group, LogFn, SearchPage, SearchRequest, Source};

/// Site root; the search endpoint is `/load` under it.
pub const HOME_URL: &str = "https://torentino.org/";

/// The search endpoint (the front page's own form: `action="/load"`,
/// `do=search`, `subaction=search`, `a=2`).
pub const SEARCH_URL: &str = "https://torentino.org/load";

pub struct TorentinoSearcher {
    client: reqwest::Client,
}

impl Default for TorentinoSearcher {
    fn default() -> Self {
        Self::new()
    }
}

impl TorentinoSearcher {
    pub fn new() -> Self {
        // The shared browser-like client (B5): the HTML side of this
        // host 403s anything that does not look like a browser.
        Self { client: browser_client() }
    }

    /// One page of results for `query`. An empty query is refused with
    /// the reason (B9's browse needs a feed this site has never shown):
    /// "no results" and "this search needs terms" are different facts.
    pub async fn search_page(&self, query: &str, _offset: usize) -> Result<SearchPage> {
        if query.trim().is_empty() {
            bail!("Torentino's search needs terms -- there is no browse feed to fall back to");
        }
        let form = format!(
            "query={}&do=search&subaction=search&a=2&sfSbm=",
            urlencoding::encode(query)
        );
        let response = fetch_resilient(
            SEARCH_URL,
            || {
                self.client
                    .post(SEARCH_URL)
                    .header("Content-Type", "application/x-www-form-urlencoded")
                    .header("Referer", HOME_URL)
                    .body(form.clone())
            },
            &FetchOptions::default(),
        )
        .await?;
        let status = response.status();
        let html = response.text().await?;
        if !status.is_success() {
            bail!("torentino returned HTTP {} for {:?}", status, query);
        }
        let items = parse_results(&html);
        let has_more = has_next_page(&html);
        Ok(SearchPage { items, has_more, next_offset: None })
    }

    /// The `.torrent` bytes for a row. The row's `download_url` is the
    /// item page, and the file link lives on it: fetch the page, take the
    /// `/load/0-0-0-...` link, follow it (301 -> the file). No bencode
    /// parsing -- the bytes are handed to TorrServer as-is.
    pub async fn download_torrent(&self, url: &str) -> Result<Vec<u8>> {
        let item_url = resolve_url(url);
        let item = fetch_resilient(
            &item_url,
            || self.client.get(&item_url).header("Referer", HOME_URL),
            &FetchOptions::default(),
        )
        .await?;
        let item_status = item.status();
        let item_html = item.text().await?;
        if !item_status.is_success() {
            bail!("torentino item page {} answered HTTP {}", url, item_status);
        }
        let file_url = match find_download_link(&item_html) {
            Some(href) => href,
            None => bail!("no .torrent link on {}", url),
        };
        let file = fetch_resilient(
            &file_url,
            || self.client.get(&file_url).header("Referer", HOME_URL),
            &FetchOptions::default(),
        )
        .await?;
        let file_status = file.status();
        // The final URL after redirects: the check below reads it, and
        // `bytes()` consumes the response.
        let final_url = file.url().to_string();
        let bytes = file.bytes().await?;
        if !file_status.is_success() {
            bail!("torentino download {} answered HTTP {}", file_url, file_status);
        }
        // The link 301s to `/_ld/.../<name>.torrent` for a real file, and
        // to a `.txt` placeholder ("ИГРА ПОКА НЕ ВЫШЛА") for a game that
        // is not out yet -- uploading that to TorrServer would fail, so
        // the final URL's extension is the honesty check.
        if !final_url.ends_with(".torrent") {
            bail!(
                "torentino's file link resolved to {}, not a .torrent \
                 (the game is likely not out yet)",
                final_url
            );
        }
        Ok(bytes.to_vec())
    }
}

/// The results rows: every `<div id="entryID<N>">` on the page, read
/// for the title link, the date and the size cell. The download link is
/// deliberately not read here -- it lives on the item page, one fetch
/// per row, and a search that fetched it for every row would cost more
/// than the search itself.
pub fn parse_results(html: &str) -> Vec<TorrentItem> {
    let document = Html::parse_document(html);
    let entry_sel = Selector::parse("div[id^='entryID']").expect("entry selector");
    let title_sel = Selector::parse("h2 a").expect("title selector");
    let date_sel = Selector::parse(".short_cat span").expect("date selector");
    let size_sel = Selector::parse(".size_file").expect("size selector");

    let mut items = Vec::new();
    for entry in document.select(&entry_sel) {
        let title_el = match entry.select(&title_sel).next() {
            Some(el) => el,
            None => continue,
        };
        let title = title_el.text().collect::<String>().trim().to_string();
        let page_url = title_el.value().attr("href").unwrap_or("").to_string();
        if title.is_empty() || page_url.is_empty() {
            continue;
        }
        // The first span reads "| Дата: 29.08.2026, 11:26".
        let date = entry
            .select(&date_sel)
            .next()
            .map(|el| el.text().collect::<String>())
            .and_then(|text| parse_date(&text))
            .unwrap_or_default();
        let size = entry
            .select(&size_sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_string())
            .unwrap_or_default();
        items.push(TorrentItem {
            title: unescape_entities(&title),
            size,
            seeds: String::new(),
            download_url: page_url.clone(),
            query: String::new(),
            date,
            page_url,
            source: "torentino".to_string(),
            group: Some(Group::Games),
            ..Default::default()
        });
    }
    items
}

/// Resolve a row's root-relative link (`/load/...`) against the site
/// root: reqwest needs an absolute URL, and the search rows carry paths.
fn resolve_url(url: &str) -> String {
    if url.starts_with("http") {
        url.to_string()
    } else {
        format!("{}{}", HOME_URL.trim_end_matches('/'), url)
    }
}

/// "29.08.2026, 11:26" -> "2026-08-29", so the date column sorts.
pub fn parse_date(text: &str) -> Option<String> {
    let re = Regex::new(r"(\d{2})\.(\d{2})\.(\d{4})").ok()?;
    let caps = re.captures(text)?;
    Some(format!("{}-{}-{}", &caps[3], &caps[2], &caps[1]))
}

/// Whether the pagination block offers a next page. The current page is
/// a `<b>`; any other page is an `<a>` -- so a block with no links is a
/// single-page answer, which is what every live probe showed.
pub fn has_next_page(html: &str) -> bool {
    let document = Html::parse_document(html);
    let pages_sel = Selector::parse("div.pages").expect("pages selector");
    let link_sel = Selector::parse("a").expect("link selector");
    document
        .select(&pages_sel)
        .any(|pages| pages.select(&link_sel).next().is_some())
}

/// The item page's file link: `/load/0-0-0-<id>-<n>`, which 301s to
/// the `.torrent` under `/_ld/`.
pub fn find_download_link(html: &str) -> Option<String> {
    let document = Html::parse_document(html);
    let link_sel = Selector::parse("a[href^='/load/0-0-0-']").expect("download selector");
    document
        .select(&link_sel)
        .next()
        .and_then(|el| el.value().attr("href"))
        .map(|href| format!("{}{}", HOME_URL.trim_end_matches('/'), href))
}

#[async_trait]
impl Source for TorentinoSearcher {
    fn id(&self) -> &'static str {
        "torentino"
    }

    fn label(&self) -> &'static str {
        "Torentino"
    }

    fn groups(&self) -> &'static [Group] {
        // A games tracker, top to bottom.
        &[Group::Games]
    }

    fn home_url(&self) -> &'static str {
        HOME_URL
    }

    fn requires_browser(&self) -> bool {
        false
    }

    fn supports_browse(&self) -> bool {
        // No "latest" feed has ever been verified on this host, and the
        // homepage is a category listing, not a freshest-first feed --
        // so browse is not claimed (B9).
        false
    }

    async fn ensure_logged_in(&self, _auth: &AuthContext, _log: &LogFn) -> Result<bool> {
        Ok(true)
    }

    async fn search(&self, req: &SearchRequest) -> Result<SearchPage> {
        TorentinoSearcher::search_page(self, &req.query, req.offset).await
    }

    async fn download_torrent(&self, url: &str) -> Result<Vec<u8>> {
        TorentinoSearcher::download_torrent(self, url).await
    }
}
