//! Rutor.org search + download. Unlike Rutracker, this needs no browser
//! session, no login, and no cookies -- confirmed live (September 2026):
//! search results and .torrent downloads are both plain, unauthenticated
//! HTTP GETs. This makes it a much lighter-weight `Source` than
//! Rutracker's, and a good first proof that the `Source` abstraction from
//! ROADMAP.md Phase 3 actually pays off: adding a second real source
//! didn't require touching the browser layer at all.
//!
//! Page structure (verified by fetching https://rutor.org/ and a live
//! search results page while writing this, not guessed at from memory):
//! - Search: `GET {BASE}/search/{page}/{category}/000/0/{urlencoded query}`,
//!   `page` starts at 1, `category` 0 = all categories.
//! - Each result row has a title link `<a href="/torrent/{id}">`, a
//!   download link `<a href="/download/{id}">`, and a magnet link
//!   `<a href="/magnet/{id}">` -- no slug in any of the three, just the
//!   numeric id.
//! - Size ("2.27 GB"), a seed count after an `alt="S"` up-arrow icon, and
//!   a peer/leech count after an `alt="L"` down-arrow icon all live in the
//!   same table row as the title link.
//!
//! If rutor.org changes its markup, this is the file (and
//! `tests/rutor_parse_tests.rs`, which pins down the exact row shape seen
//! live) to fix -- same spirit as the TorrServer JSON-shape caveat
//! elsewhere in ROADMAP.md.

use anyhow::Result;
use regex::Regex;
use scraper::{Html, Selector};

use crate::search::models::TorrentItem;

pub struct RutorSearcher {
    client: reqwest::Client,
}

impl Default for RutorSearcher {
    fn default() -> Self {
        Self::new()
    }
}

impl RutorSearcher {
    pub const HOME_URL: &'static str = "https://rutor.org/";
    const BASE: &'static str = "https://rutor.org";
    /// Rutor doesn't report a page size anywhere obvious; this is only
    /// used to translate this app's "offset" pagination convention
    /// (0, 50, 100, ...) into rutor's own 1-based page numbers for
    /// "load more". Being slightly off just means the next "page" of
    /// results overlaps or skips a few rows, not a crash or a wrong URL.
    const ASSUMED_PAGE_SIZE: usize = 50;

    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .user_agent("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/152.0.0.0 Safari/537.36")
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
        }
    }

    pub async fn search(&self, query: &str) -> Result<Vec<TorrentItem>> {
        self.search_page(query, 0).await
    }

    pub async fn search_page(&self, query: &str, offset: usize) -> Result<Vec<TorrentItem>> {
        let page = (offset / Self::ASSUMED_PAGE_SIZE) + 1;
        let encoded = urlencoding::encode(query);
        let url = format!("{}/search/{}/0/000/0/{}", Self::BASE, page, encoded);

        // Headers beyond User-Agent: some sites gate on Accept/Referer
        // too, and a request missing everything a real browser always
        // sends is an easy bot-detection signal. Logged unconditionally
        // to crate::log (~/.local/share/doris/doris.log) since the
        // Source trait's search_page has no log-callback parameter to
        // surface this in the UI's own Detailed Log panel the way
        // Rutracker's AUTH steps do -- if this ever returns zero results
        // again, that file is the first thing to check.
        let response = self.client.get(&url)
            .header("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
            .header("Accept-Language", "ru-RU,ru;q=0.9,en-US;q=0.8,en;q=0.7")
            .header("Referer", Self::BASE)
            .send()
            .await?;

        let status = response.status();
        let html = response.text().await?;
        let matched = count_title_links(&html);

        crate::log::log("rutor", &format!(
            "GET {} -> status={} body_len={} title_links={}",
            url, status, html.len(), matched,
        ));

        if !status.is_success() {
            // Parsing an error/challenge page always finds zero results;
            // say so explicitly instead of silently returning an empty
            // Vec indistinguishable from "no matches for this query".
            anyhow::bail!("rutor returned HTTP {} for {}", status, url);
        }

        if matched == 0 && html.len() < 2000 {
            // A real rutor search results page is large (many rows); a
            // tiny response on a 2xx status is a strong sign of a
            // challenge/interstitial page rather than genuinely zero
            // matches. Log a snippet so the actual page content (rather
            // than just its length) is on hand next time this happens.
            let snippet: String = html.chars().take(500).collect();
            crate::log::log("rutor", &format!("suspiciously small body, first 500 chars: {}", snippet));
        }

        Ok(parse_results(&html))
    }

    pub async fn download_torrent(&self, url: &str) -> Result<Vec<u8>> {
        let bytes = self.client.get(url).send().await?.bytes().await?;
        Ok(bytes.to_vec())
    }
}

/// Quick standalone count of title-link matches, used only to log
/// diagnostics in `search_page` before the full (row-by-row) parse runs.
pub fn count_title_links(html: &str) -> usize {
    let document = Html::parse_document(html);
    match Selector::parse(r#"a[href^="/torrent/"]"#) {
        Ok(sel) => document.select(&sel).count(),
        Err(_) => 0,
    }
}

/// Pulled out of the impl block so it's plain-function testable against
/// saved HTML fixtures without needing a `RutorSearcher` (and therefore a
/// network client) at all.
pub fn parse_results(html: &str) -> Vec<TorrentItem> {
    let document = Html::parse_document(html);
    // Matching on the href *prefix* rather than any CSS class: class names
    // are far more likely to change across a template refresh than the
    // URL scheme every row's title link has to use to point at a real
    // torrent page.
    let title_sel = match Selector::parse(r#"a[href^="/torrent/"]"#) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    // Rust's regex crate is Unicode-aware by default, so \s already
    // matches a literal non-breaking space (U+00A0) if the parser decoded
    // the &nbsp; entity to one; the literal "&nbsp;" alternative covers
    // the case where it didn't.
    let size_re = Regex::new(r"(\d+(?:[.,]\d+)?)(?:\s|&nbsp;)*(TB|GB|MB|KB)").ok();
    let seeds_re = Regex::new(r#"alt="S"[^>]*>\s*(\d+)"#).ok();
    let peers_re = Regex::new(r#"alt="L"[^>]*>\s*(\d+)"#).ok();
    // "07 Сен 25" / "31 Окт 20" style short Russian date, always the very
    // first text in the row.
    let date_re = Regex::new(r"(\d{2}\s+[А-Яа-я]{3}\s+\d{2})").ok();

    let mut items = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    for title_el in document.select(&title_sel) {
        let href = match title_el.value().attr("href") {
            Some(h) => h,
            None => continue,
        };
        let id = href
            .trim_start_matches("/torrent/")
            .split('/')
            .next()
            .unwrap_or("")
            .to_string();
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        // Rutor's per-row markup repeats the title link's id nowhere else
        // dangerous, but be defensive against any future duplicate anchor
        // pointing at the same torrent within one row.
        if !seen_ids.insert(id.clone()) {
            continue;
        }

        let title = title_el.text().collect::<String>().trim().to_string();
        if title.is_empty() {
            continue;
        }

        let row_html = enclosing_row_html(title_el).unwrap_or_default();

        let size = size_re.as_ref()
            .and_then(|re| re.captures(&row_html))
            .map(|c| format!("{} {}", c[1].replace(',', "."), &c[2]))
            .unwrap_or_default();
        let seeds = seeds_re.as_ref()
            .and_then(|re| re.captures(&row_html))
            .map(|c| c[1].to_string())
            .unwrap_or_default();
        let peers = peers_re.as_ref()
            .and_then(|re| re.captures(&row_html))
            .map(|c| c[1].to_string())
            .unwrap_or_default();
        let date = date_re.as_ref()
            .and_then(|re| re.captures(&row_html))
            .map(|c| c[1].to_string())
            .unwrap_or_default();

        items.push(TorrentItem {
            title,
            size,
            seeds,
            download_url: format!("{}/download/{}", RutorSearcher::BASE, id),
            query: String::new(),
            date,
            page_url: format!("{}/torrent/{}", RutorSearcher::BASE, id),
            source: "rutor".to_string(),
        });
        let _ = peers; // peers isn't a TorrentItem field today; kept for a future column.
    }
    items
}

/// Walk up from a title `<a>` to its enclosing `<tr>` and return that
/// row's outer HTML, so size/seeds/date can be read from a small, known
/// snippet instead of guessed at by absolute position in the whole
/// document. Falls back to `None` (never panics) if the DOM shape isn't
/// what's expected -- the caller just gets blank size/seeds/date for that
/// row rather than a crash.
fn enclosing_row_html(el: scraper::ElementRef) -> Option<String> {
    for ancestor in el.ancestors() {
        if let Some(element) = ancestor.value().as_element() {
            if element.name() == "tr" {
                return scraper::ElementRef::wrap(ancestor).map(|r| r.html());
            }
        }
    }
    None
}
