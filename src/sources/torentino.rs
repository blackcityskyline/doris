//! Torentino -- a DLE-based games tracker -- over its HTML //! B8 wave 3), probed live on
//! 26.09.2026.

use anyhow::{bail, Result};
use async_trait::async_trait;
use regex::Regex;
use scraper::{Html, Selector};
use std::sync::OnceLock;

use super::format::unescape_entities;
use super::models::TorrentItem;
use super::net::{browser_client, fetch_resilient, FetchOptions};
use super::source::{AuthContext, Group, LogFn, SearchPage, SearchRequest, Source};

/// Every CSS selector this file needs, compiled once.
struct Selectors {
    entry: Selector,
    title: Selector,
    date: Selector,
    size: Selector,
    pages: Selector,
    link: Selector,
    download: Selector,
}

fn selectors() -> Option<&'static Selectors> {
    static SEL: OnceLock<Option<Selectors>> = OnceLock::new();
    SEL.get_or_init(|| {
        Some(Selectors {
            entry: Selector::parse("div[id^='entryID']").ok()?,
            title: Selector::parse("h2 a").ok()?,
            date: Selector::parse(".short_cat span").ok()?,
            size: Selector::parse(".size_file").ok()?,
            pages: Selector::parse("div.pages").ok()?,
            link: Selector::parse("a").ok()?,
            download: Selector::parse("a[href^='/load/0-0-0-']").ok()?,
        })
    })
    .as_ref()
}

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
        // The shared browser-like client: the HTML side of this
        // host 403s anything that does not look like a browser.
        Self {
            client: browser_client(),
        }
    }

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
        Ok(SearchPage {
            items,
            has_more,
            next_offset: None,
        })
    }

    /// The `.torrent` bytes for a row.
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
            bail!(
                "torentino download {} answered HTTP {}",
                file_url,
                file_status
            );
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

/// The results rows: every `<div id="entryID<N>">` on the page, read for the title link, the
/// date and the size cell.
pub fn parse_results(html: &str) -> Vec<TorrentItem> {
    let document = Html::parse_document(html);
    let Some(sel) = selectors() else {
        return Vec::new();
    };

    let mut items = Vec::new();
    for entry in document.select(&sel.entry) {
        let title_el = match entry.select(&sel.title).next() {
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
            .select(&sel.date)
            .next()
            .map(|el| el.text().collect::<String>())
            .and_then(|text| parse_date(&text))
            .unwrap_or_default();
        let size = entry
            .select(&sel.size)
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

/// Whether the pagination block offers a next page.
pub fn has_next_page(html: &str) -> bool {
    let document = Html::parse_document(html);
    let Some(sel) = selectors() else {
        return false;
    };
    document
        .select(&sel.pages)
        .any(|pages| pages.select(&sel.link).next().is_some())
}

/// The item page's file link: `/load/0-0-0-<id>-<n>`, which 301s to
/// the `.torrent` under `/_ld/`.
pub fn find_download_link(html: &str) -> Option<String> {
    let document = Html::parse_document(html);
    let sel = selectors()?;
    document
        .select(&sel.download)
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
        // so browse is not claimed.
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
