//! Rutor search + download (host: rutor.info). Plain, unauthenticated HTTP -- no browser
//! session, no login, no cookies (confirmed live, September 2026), which makes it the lightest
//! `Source` in the tree.

use anyhow::Result;
use regex::Regex;
use scraper::{Html, Selector};

use std::collections::HashSet;
use std::sync::OnceLock;

use crate::sources::models::TorrentItem;
use crate::sources::net::{browser_client, fetch_resilient, FetchOptions};
use crate::sources::source::{Group, SearchPage};

/// The rubric id behind each group this source declares -- one table for both halves of B6: the
/// id the search URL is asked with, and the category a returned row claims, so the two can
/// never drift apart.
pub const GROUP_IDS: [(Group, &[i64]); 4] = [
    (Group::Movies, &[1, 5, 7, 12]),
    (Group::TV, &[4, 6, 15, 16]),
    (Group::Games, &[8]),
    (Group::Anime, &[10]),
];

/// The ids a category search fans out over, or an empty list for a group rutor does not
/// declare.
pub fn group_ids(group: Group) -> &'static [i64] {
    GROUP_IDS
        .iter()
        .find(|(known, _)| *known == group)
        .map(|(_, ids)| *ids)
        .unwrap_or(&[])
}

pub struct RutorSearcher {
    client: reqwest::Client,
}

impl Default for RutorSearcher {
    fn default() -> Self {
        Self::new()
    }
}

pub const BROWSE_URL: &str = "https://rutor.info/";

/// A real rutor search results page is large (many rows); a tiny
/// response on a 2xx status is a strong sign of a challenge/interstitial
/// page rather than genuinely zero matches.
const EMPTY_PAGE_THRESHOLD: usize = 2000;

impl RutorSearcher {
    pub const HOME_URL: &'static str = "https://rutor.info/";
    /// Search/download host.
    const BASE: &'static str = "https://rutor.info";
    /// Rutor's search pages hold a fixed 100 rows, verified live while fixing the zero-results
    /// bug: `matrix` reports 219 hits and comes back 100/100/22/0 rows on pages 1/2/3/4 (page 0
    /// is a synonym of page 1).
    pub const PAGE_SIZE: usize = 100;

    pub fn new() -> Self {
        // The shared browser-like client: User-Agent plus the
        Self {
            client: browser_client(),
        }
    }

    pub async fn search(&self, query: &str) -> Result<Vec<TorrentItem>> {
        Ok(self.search_page(query, 0, None).await?.items)
    }

    /// Rutor matches a multi-word query as a strict AND over *all* of its words, in any order
    /// -- and words its index doesn't contain make the whole query return zero hits.
    pub async fn search_page(
        &self,
        query: &str,
        offset: usize,
        category: Option<Group>,
    ) -> Result<SearchPage> {
        if query.trim().is_empty() {
            // Browse: the homepage's latest releases -- one mixed
            let (status, html) = self.fetch_url(BROWSE_URL).await?;
            if !status.is_success() {
                anyhow::bail!("rutor returned HTTP {} for its homepage", status);
            }
            let items = parse_results(&html);
            return Ok(SearchPage {
                items,
                has_more: false,
                next_offset: None,
            });
        }
        if !offset.is_multiple_of(Self::PAGE_SIZE) {
            // The app advances `offset` by however many rows came back,
            crate::log::log(
                "rutor",
                &format!(
                    "offset {} is past a partial final page; no more results",
                    offset,
                ),
            );
            return Ok(SearchPage::default());
        }
        let page = (offset / Self::PAGE_SIZE) + 1;

        let ids: &[i64] = match category {
            Some(group) => group_ids(group),
            None => &[0],
        };
        crate::log::log(
            "rutor",
            &format!(
                "page {} category {:?} -> rubric ids {:?}",
                page, category, ids,
            ),
        );
        let mut per_id = Vec::with_capacity(ids.len());
        for &cat in ids {
            per_id.push(self.search_one_category(page, query, cat).await?);
        }
        Ok(to_page(per_id, category, offset))
    }

    /// One rubric id's page: the fetch, the parse, and the relaxed
    /// fallback that works around rutor's strict-AND semantics -- split
    /// out of `search_page` unchanged so every id of a fan-out gets the
    /// exact same treatment, including the fallback.
    async fn search_one_category(
        &self,
        page: usize,
        query: &str,
        category: i64,
    ) -> Result<Vec<TorrentItem>> {
        let (status, html) = self.fetch_page(page, category, query).await?;
        if !status.is_success() {
            // Parsing an error/challenge page always finds zero results;
            anyhow::bail!("rutor returned HTTP {} for {}", status, query);
        }

        let strict = parse_results(&html);
        if !strict.is_empty() {
            return Ok(strict);
        }

        let (kept, dropped) = split_query(query);
        if dropped.is_empty() || kept.is_empty() {
            // Nothing was dropped, or everything was: no better query to
            return Ok(strict);
        }

        let relaxed = kept.join(" ");
        crate::log::log(
            "rutor",
            &format!(
                "0 hits for {:?}; retrying as {:?} (rutor does not index \
             {:?} and ANDs every query word)",
                query, relaxed, dropped,
            ),
        );
        let (status, html) = self.fetch_page(page, category, &relaxed).await?;
        if !status.is_success() {
            crate::log::log(
                "rutor",
                &format!("relaxed query {:?} failed with HTTP {}", relaxed, status,),
            );
            return Ok(strict);
        }

        let items = parse_results(&html);
        if items.is_empty() {
            return Ok(items);
        }
        // Rows that really mention the dropped words are the user's
        let exact: Vec<TorrentItem> = items
            .iter()
            .filter(|it| dropped.iter().all(|w| title_has_word(&it.title, w)))
            .cloned()
            .collect();
        if !exact.is_empty() {
            crate::log::log(
                "rutor",
                &format!(
                    "{}/{} relaxed rows also mention the dropped words",
                    exact.len(),
                    items.len(),
                ),
            );
            return Ok(exact);
        }
        crate::log::log(
            "rutor",
            &format!(
                "no relaxed row mentions {:?}; keeping all {} rows",
                dropped,
                items.len(),
            ),
        );
        Ok(items)
    }

    /// The search URL for one rubric id: `/search/{page}/{cat}/000/0/ {query}` -- `cat` is
    /// rutor's own rubric slot, `0` meaning "all categories" (its spelling, not ours).
    pub fn search_url(page: usize, category: i64, query: &str) -> String {
        format!(
            "{}/search/{}/{}/000/0/{}",
            Self::BASE,
            page,
            category,
            urlencoding::encode(query)
        )
    }

    /// One GET of a search page plus the unconditional diagnostic log
    /// line (kept out of `search_one_category` so the strict and the
    /// relaxed attempt share the exact same request shape).
    async fn fetch_page(
        &self,
        page: usize,
        category: i64,
        query: &str,
    ) -> Result<(reqwest::StatusCode, String)> {
        let url = Self::search_url(page, category, query);
        self.fetch_url(&url).await
    }

    /// One GET of any URL on this source's site, with the shared client and the unconditional
    /// diagnostic log line.
    async fn fetch_url(&self, url: &str) -> Result<(reqwest::StatusCode, String)> {
        // Accept/Accept-Language come from the shared client; the
        let response = fetch_resilient(
            url,
            || self.client.get(url).header("Referer", Self::BASE),
            &FetchOptions::default(),
        )
        .await?;

        let status = response.status();
        let html = response.text().await?;
        let matched = count_title_links(&html);

        crate::log::log(
            "rutor",
            &format!(
                "GET {} -> status={} body_len={} title_links={}",
                url,
                status,
                html.len(),
                matched,
            ),
        );

        if matched == 0 && html.len() < EMPTY_PAGE_THRESHOLD {
            // A real rutor search results page is large (many rows); a
            let snippet: String = html.chars().take(500).collect();
            crate::log::log(
                "rutor",
                &format!("suspiciously small body, first 500 chars: {}", snippet,),
            );
        }

        Ok((status, html))
    }

    pub async fn download_torrent(&self, url: &str) -> Result<Vec<u8>> {
        // Same resilient path as search: a.torrent fetch that hits a
        let response =
            fetch_resilient(url, || self.client.get(url), &FetchOptions::default()).await?;
        let status = response.status();
        if !status.is_success() {
            // This used to be unchecked, so a mirror answering
            anyhow::bail!("rutor download {} answered HTTP {}", url, status);
        }
        let bytes = response.bytes().await?;
        Ok(bytes.to_vec())
    }
}

pub fn to_page(
    per_id: Vec<Vec<TorrentItem>>,
    category: Option<Group>,
    offset: usize,
) -> SearchPage {
    let has_more = per_id
        .iter()
        .any(|rows| rows.len() >= RutorSearcher::PAGE_SIZE);
    let mut seen = HashSet::new();
    let mut items = Vec::new();
    for rows in per_id {
        for mut row in rows {
            if !seen.insert(row.page_url.clone()) {
                continue;
            }
            if row.group.is_none() {
                row.group = category;
            }
            items.push(row);
        }
    }
    SearchPage {
        items,
        has_more,
        next_offset: has_more.then_some(offset + RutorSearcher::PAGE_SIZE),
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

/// Words rutor's index skips even though they appear in (English and Russian) titles all the
/// time, so AND-ing them into a query can only ever produce zero hits -- the reason `world war
/// z` and friends used to come back empty.
pub const STOPWORDS: &[&str] = &[
    // English
    "a", "an", "the", "of", "to", "it", "i", "am", "is", "are", "be", "in", "on", "at", "by", "for",
    "and", "or", "but", "not", "with", "from", "this", "that", "as", "my", "your",
    // Russian
    "и", "в", "во", "на", "с", "со", "к", "ко", "о", "об", "от", "до", "для", "по", "из", "не",
    "ни", "что", "как", "за", "у", "же", "бы", "то",
];

/// Split a query into the words rutor can actually match and the ones that would poison the
/// whole AND (see [`STOPWORDS`] and the [`RutorSearcher::search_page`] docs).
pub fn split_query(query: &str) -> (Vec<String>, Vec<String>) {
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    for word in query.split_whitespace() {
        let core: String = word
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_string();
        // rutor's own help text says the minimum query length is 2, and
        let too_short = core.chars().count() <= 2;
        let is_stopword = STOPWORDS.contains(&core.to_lowercase().as_str());
        if core.is_empty() || too_short || is_stopword {
            dropped.push(core);
        } else {
            kept.push(core);
        }
    }
    (kept, dropped)
}

/// Whole-word, case-insensitive containment check used to decide whether a relaxed row really
/// mentions the words that had to be dropped from the query ("the" in "Матрица / The Matrix
/// (1999)" yes, "the" in "Theatre" no).
pub fn title_has_word(title: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let title: Vec<char> = title.to_lowercase().chars().collect();
    let word: Vec<char> = word.to_lowercase().chars().collect();
    if word.len() > title.len() {
        return false;
    }
    for start in 0..=(title.len() - word.len()) {
        if title[start..start + word.len()] != word[..] {
            continue;
        }
        let end = start + word.len();
        let left_ok = start == 0 || !title[start - 1].is_alphanumeric();
        let right_ok = end == title.len() || !title[end].is_alphanumeric();
        if left_ok && right_ok {
            return true;
        }
    }
    false
}

/// Pulled out of the impl block so it's plain-function testable against
/// saved HTML fixtures without needing a `RutorSearcher` (and therefore a
/// network client) at all.
pub fn parse_results(html: &str) -> Vec<TorrentItem> {
    let document = Html::parse_document(html);
    // Matching on the href *prefix* rather than any CSS class: class names
    let title_sel = match Selector::parse(r#"a[href^="/torrent/"]"#) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    // Inline magnet links (rutor.info puts `magnet:?xt=urn:btih:...`
    let magnet_sel = match Selector::parse(r#"a[href^="magnet:"]"#) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    // Live markup puts the counts right after the icons as
    let size_re = Regex::new(r"(?i)(\d+(?:[.,]\d+)?)(?:\s|&nbsp;)*(TB|GB|MB|KB|ТБ|ГБ|МБ|КБ)").ok();
    let seeds_re = Regex::new(r#"alt="S"[^>]*>(?:\s|&nbsp;)*(\d+)"#).ok();
    let peers_re = Regex::new(r#"alt="L"[^>]*>(?:<[^>]*>|\s|&nbsp;)*(\d+)"#).ok();
    // "07 Сен 25" / "31 Окт 20" style short Russian date, always the very
    let date_re = Regex::new(r"(\d{2}(?:\s|&nbsp;)+[А-Яа-я]{3}(?:\s|&nbsp;)+\d{2})").ok();

    let mut items = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    for title_el in document.select(&title_sel) {
        // Only rows of the search-results table count. The page also
        let Some(row) = enclosing_row(title_el) else {
            continue;
        };
        if !is_results_row(&row) {
            continue;
        }

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
        if !seen_ids.insert(id.clone()) {
            continue;
        }

        let title = title_el.text().collect::<String>().trim().to_string();
        if title.is_empty() {
            continue;
        }

        let row_html = row.html();

        let size = size_re
            .as_ref()
            .and_then(|re| re.captures(&row_html))
            .map(|c| format!("{} {}", c[1].replace(',', "."), &c[2]))
            .unwrap_or_default();
        let seeds = seeds_re
            .as_ref()
            .and_then(|re| re.captures(&row_html))
            .map(|c| c[1].to_string())
            .unwrap_or_default();
        let peers = peers_re
            .as_ref()
            .and_then(|re| re.captures(&row_html))
            .map(|c| c[1].to_string())
            .unwrap_or_default();
        let date = date_re
            .as_ref()
            .and_then(|re| re.captures(&row_html))
            .map(|c| c[1].replace("&nbsp;", " "))
            .unwrap_or_default();

        let mut item = TorrentItem {
            title,
            size,
            seeds,
            leechers: peers.parse::<u32>().unwrap_or(0),
            download_url: format!("{}/download/{}", RutorSearcher::BASE, id),
            query: String::new(),
            date,
            page_url: format!("{}/torrent/{}", RutorSearcher::BASE, id),
            source: "rutor".to_string(),
            magnet: magnet_href(&row, &magnet_sel),
            ..Default::default()
        };
        // Numeric twins derived from the display strings above, then
        item.fill_from_display();
        item.added = parse_added(&item.date);
        item.info_hash = info_hash_from_magnet(item.magnet.as_deref().unwrap_or(""));
        // `group` stays `None`: the search URL carries category 0 ("all
        items.push(item);
    }
    items
}

/// Walk up from a title `<a>` to its enclosing `<tr>` and return that row element, so
/// size/seeds/date can be read from a small, known snippet instead of guessed at by absolute
/// position in the whole document.
fn enclosing_row(el: scraper::ElementRef) -> Option<scraper::ElementRef> {
    for ancestor in el.ancestors() {
        if let Some(element) = ancestor.value().as_element() {
            if element.name() == "tr" {
                return scraper::ElementRef::wrap(ancestor);
            }
        }
    }
    None
}

/// Whether a row belongs to the search-results table.
fn is_results_row(row: &scraper::ElementRef) -> bool {
    for ancestor in row.ancestors() {
        if let Some(element) = ancestor.value().as_element() {
            if element.name() == "table" {
                let table_id = scraper::ElementRef::wrap(ancestor)
                    .and_then(|t| t.value().attr("id").map(str::to_string))
                    .unwrap_or_default();
                return table_id != "news_table";
            }
        }
    }
    true
}

/// The row's inline magnet URI, when it has one.
fn magnet_href(row: &scraper::ElementRef, sel: &Selector) -> Option<String> {
    row.select(sel)
        .next()
        .and_then(|a| a.value().attr("href").map(str::to_string))
}

/// `xt=urn:btih:{40 hex}` inside a magnet URI -> the lower-case hash, or `""`.
fn info_hash_from_magnet(magnet: &str) -> String {
    info_hash_re()
        .and_then(|re| re.captures(magnet))
        .map(|caps| caps[1].to_lowercase())
        .unwrap_or_default()
}

fn info_hash_re() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)xt=urn:btih:([a-f0-9]{40})").ok())
        .as_ref()
}

/// `06 Сен 26` -> unix seconds of that UTC day, `0` when the date can't be read.
fn parse_added(date: &str) -> i64 {
    const RU_MONTHS: [&str; 12] = [
        "Янв", "Фев", "Мар", "Апр", "Май", "Июн", "Июл", "Авг", "Сен", "Окт", "Ноя", "Дек",
    ];
    let Some(caps) = added_re().and_then(|re| re.captures(date)) else {
        return 0;
    };
    let day: i64 = match caps[1].parse::<i64>() {
        Ok(d) => d,
        Err(_) => return 0,
    };
    let Some(month) = RU_MONTHS
        .iter()
        .position(|m| m.eq_ignore_ascii_case(&caps[2]))
    else {
        return 0;
    };
    let year: i64 = match caps[3].parse::<i64>() {
        Ok(y) => 2000 + y,
        Err(_) => return 0,
    };
    days_from_civil(year, month as i64 + 1, day) * 86_400
}

fn added_re() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(\d{1,2})(?:\s|&nbsp;)+([А-Яа-я]{3})(?:\s|&nbsp;)+(\d{2})").ok())
        .as_ref()
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (month + 9) % 12; // March = 0 .. February = 11
    let doy = (153 * mp + 2) / 5 + day - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}
