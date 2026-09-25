//! NNM-Club over its tracker HTML (ROADMAP.md B8 wave 3), parsed from
//! the markup as it came back live on 25.09.2026: windows-1251 in both
//! the header and the `<meta>`, cloudflare-fronted, but a browser UA
//! alone was enough -- no JS challenge, no login, no cookie jar.
//!
//! What the live pages established, and what the code therefore does:
//!
//! - **The search page is enough.** Each result row carries its title
//!   (and `viewtopic.php?t=`), a `download.php?id=` link, raw bytes
//!   inside `<u>`, seeders, leechers, and the topic's own timestamp in
//!   a second `<u>` -- so no row needs a second request. torio fetches
//!   up to eight *detail* pages per search just to scrape a magnet out
//!   of them; doris does not have to: `spawn_stream` already falls back
//!   to downloading the `.torrent` and handing the bytes to TorrServer,
//!   and `download.php?id=` was checked live to answer
//!   `302 -> application/x-bittorrent` with a real bencoded file.
//!   Decision with the user; the cost is that rows carry no
//!   `info_hash`, which `dedupe_by_hash` explicitly lets through
//!   untouched rather than collapsing.
//!
//! - **One URL for a query, one for browse, `start=` for page two.**
//!   `f=-1&nm=<q>` for a query (multi-word: the site matches *every*
//!   token -- live-checked, 2/2 rows for "frieren 2026"), and
//!   `f=-1&o=2&sd=desc` for browse (50 newest topics). Both paginate at
//!   50, verified by fetching page 2 of each and comparing topic ids:
//!   zero overlap. Hence `has_more` on a full page and
//!   `next_offset = offset + 50`, spelled out rather than derived from
//!   the row count so a single dropped row cannot misalign the cursor
//!   -- the lesson yts taught this codebase.
//!
//! - **No group per row** (decision): a row *does* show its category in
//!   Russian ("Аниме с озвучкой (FullHD)"), but mapping a live
//!   inventory of those strings is B6's job; the source declares the
//!   four groups it spans, like rutracker and rutor do.
//!
//! - **Dead rows are rows.** Roughly two thirds of a browse page came
//!   back with no seeders, and the site spells that by replacing the
//!   seeder cell's `title="Seeders"` with `title=" Last seen: ..."`
//!   and emptying the cell (`class="seedmed"` stays). They are kept as
//!   rows with `seeds = 0` -- which is also what keeps a full page
//!   reading as a full page, and pagination alive with it.
//!   See `nnmclub_parse_tests` for the regression.
//!
//! - **A miss is a miss, and a block is an error.** A query with no
//!   matches answers 200 with the results table present and
//!   `Не найдено` inside it (live), so zero rows mean an empty page;
//!   a page with no results table at all means something else -- a
//!   challenge, a moved layout, a login wall -- and says so, because to
//!   a user those three all look like "the tracker found nothing".

use std::sync::OnceLock;

use anyhow::{Result, bail, ensure};
use async_trait::async_trait;
use regex::Regex;

use super::format::{format_bytes, format_date, unescape_entities};
use super::models::TorrentItem;
use super::net::{FetchOptions, browser_client, fetch_resilient};
use super::source::{AuthContext, Group, LogFn, SearchPage, SearchRequest, Source};

/// The three torio splits (`nnm-movies`/`nnm-tv`/`nnm-games`) plus the
/// anime forums the live search page was actually returning rows from.
const GROUPS: &[Group] = &[Group::Movies, Group::TV, Group::Games, Group::Anime];

/// Rows the site puts on one page, live on both a broad query and
/// browse (50 each, page 2 disjoint). A full page is what "may be
/// more" means here.
pub const PAGE_SIZE: usize = 50;

/// `f=-1`: every forum, which is the id the site's own navigation
/// links as "all".
const ALL_FORUMS: &str = "f=-1";

/// Site root; every path below lives under `/forum/`.
pub const FORUM: &str = "https://nnmclub.to/forum/";

/// The page the query URL was verified against: results ordered by the
/// tracker, tokens matched server-side (all of a row's words present).
pub fn search_url(query: &str, offset: usize) -> String {
    let query = query.trim();
    if query.is_empty() {
        return browse_url(offset);
    }
    with_offset(
        format!(
            "{}tracker.php?{}&nm={}",
            FORUM,
            ALL_FORUMS,
            urlencoding::encode(query)
        ),
        offset,
    )
}

/// The browse URL (live: 50 rows, `o=2&sd=desc` = newest topic first),
/// the empty-query entry point behind `supports_browse`.
pub fn browse_url(offset: usize) -> String {
    with_offset(
        format!("{}tracker.php?{}&o=2&sd=desc", FORUM, ALL_FORUMS),
        offset,
    )
}

/// `start=` only when there is a second page to ask for: page one was
/// verified *without* the parameter, and `start=0` was never seen.
fn with_offset(url: String, offset: usize) -> String {
    if offset == 0 {
        url
    } else {
        format!("{}&start={}", url, offset)
    }
}

/// The regexes the parser needs, built once. Patterns rather than hand
/// scanning because the two cells worth reading are distinguished by
/// Russian `title=` attributes the site writes for us (verified live),
/// and a pattern documents that better than an index arithmetic.
struct Patterns {
    /// The results table, and only that one.
    table: Regex,
    /// One row inside it. The table's rows are flat -- no nested
    /// `<tr>`, live -- which is what makes this split safe.
    row: Regex,
    /// `<u>12345678</u>`: the site wraps both a size in raw bytes and
    /// a unix timestamp in `<u>`.
    u_digits: Regex,
    /// Tags, for the title text.
    tags: Regex,
}

impl Patterns {
    fn build() -> Option<Self> {
        Some(Self {
            table: Regex::new(
                r#"<table class="forumline tablesorter"[^>]*>([\s\S]*?)</table>"#,
            )
            .ok()?,
            row: Regex::new(r"<tr[^>]*>([\s\S]*?)</tr>").ok()?,
            u_digits: Regex::new(r"<u>(\d+)</u>").ok()?,
            tags: Regex::new(r"<[^>]+>").ok()?,
        })
    }
}

/// Compiled patterns, or `None` if any of them could not be built --
/// which callers turn into an error rather than an empty result.
fn patterns() -> Option<&'static Patterns> {
    static PATTERNS: OnceLock<Option<Patterns>> = OnceLock::new();
    PATTERNS.get_or_init(Patterns::build).as_ref()
}

/// Remove tags and the `&nbsp;` the site uses for spacing, then decode
/// entities and collapse whitespace -- torio's `stripHtml` +
/// `unescapeEntities` in that order, which is what turns
/// `<b>Фрирен&#039;s</b>&nbsp;<span ...>` back into a title.
fn strip_html(input: &str) -> String {
    let patterns = match patterns() {
        Some(p) => p,
        None => return input.to_string(),
    };
    let bare = patterns.tags.replace_all(input, "");
    let bare = bare.replace("&nbsp;", " ").replace('\u{a0}', " ");
    unescape_entities(&bare)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The topic id and the title next to it. The first
/// `viewtopic.php?t=` in a result row *is* the title's link: the row's
/// other links point at `tracker.php?f=` (forum), `?pid=` (author) and
/// `download.php?id=` -- live.
fn topic(row: &str) -> Option<(String, String)> {
    let marker = "viewtopic.php?t=";
    let at = row.find(marker)?;
    let id: String = row[at + marker.len()..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    if id.is_empty() {
        return None;
    }
    let anchor = row[..at].rfind("<a")?;
    let open = anchor + row[anchor..].find('>')?;
    let close = open + row[open..].find("</a>")?;
    let title = strip_html(&row[open + 1..close]);
    if title.is_empty() {
        return None;
    }
    Some((id, title))
}

/// The row's `download.php?id=`, which live-checks to a real
/// `.torrent` -- so this is all the download path needs.
fn download_id(row: &str) -> Option<String> {
    let marker = "download.php?id=";
    let at = row.find(marker)?;
    let id: String = row[at + marker.len()..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    (!id.is_empty()).then_some(id)
}

/// A cell identified by an attribute the site writes on it, with the
/// cell's own span -- the seeds/leechers/added cells all have one.
fn cell_span(row: &str, marker: &str) -> Option<(usize, usize)> {
    let at = row.find(marker)?;
    let start = row[..at].rfind("<td")?;
    let end = start + row[start..].find("</td>")?;
    Some((start, end))
}

/// The first run of digits in a stripped cell (`<b>39</b>` -> 39).
fn first_number(cell: &str) -> u32 {
    let text = strip_html(cell);
    let digits: String = text
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse::<u32>().unwrap_or(0)
}

/// Raw bytes and the topic timestamp, both of which the site wraps in
/// `<u>`. Told apart by the added cell's own marker
/// (`title="Торрент-файл добавлен"`) rather than by position, so the
/// two values cannot swap places if a column moves.
///
/// Without that marker the first `<u>` is still the size (it sits in
/// the column before the timestamp, live); the timestamp is then left
/// at `0` -- "unknown", which is what `format_date` refuses to print
/// as 1970-01-01.
fn size_and_added(row: &str, patterns: &Patterns) -> (u64, i64) {
    let added_cell = cell_span(row, "title=\"Торрент-файл добавлен\"");
    let mut size = 0_u64;
    let mut added = 0_i64;
    for caps in patterns.u_digits.captures_iter(row) {
        let Some(value) = caps.get(1) else { continue };
        let number = value.as_str().parse::<u64>().unwrap_or(0);
        let inside_added = added_cell.is_some_and(|(start, end)| {
            value.start() >= start && value.start() < end
        });
        match added_cell {
            Some(_) if inside_added => added = number as i64,
            Some(_) => {
                if size == 0 {
                    size = number;
                }
            }
            None => {
                if size == 0 {
                    size = number;
                } else if added == 0 {
                    added = number as i64;
                }
            }
        }
    }
    (size, added)
}

/// One result row -> its row, or `None` when the row cannot become
/// one: no topic id, no download link, no title. One malformed row
/// costs one row, never the page.
fn to_row(row: &str, patterns: &Patterns) -> Option<TorrentItem> {
    let (topic_id, title) = topic(row)?;
    let download_id = download_id(row)?;
    let (size_bytes, added) = size_and_added(row, patterns);
    // The site's *classes*, not its `title=` hints -- the difference
    // was found live and cost real rows: a torrent nobody seeds swaps
    // `title="Seeders"` for `title=" Last seen:   29-03-2020"` and
    // leaves the cell body empty, while keeping `class="seedmed"`.
    // Keying on the title lost 35 of 50 browse rows, which in turn
    // read a full page as a short one and switched pagination off;
    // the empty body is the `0` those rows deserve anyway.
    let seeds = first_number(&cell_of(row, "class=\"seedmed\"")?);
    let leechers = first_number(&cell_of(row, "class=\"leechmed\"")?);

    Some(TorrentItem {
        // No magnet in the row (live: zero on the search page) and no
        // hash either -- the `.torrent` link carries both, and the
        // download/stream path is built for exactly that.
        magnet: None,
        info_hash: String::new(),
        title,
        size: format_bytes(size_bytes),
        size_bytes,
        seeds: seeds.to_string(),
        seeds_n: seeds,
        leechers,
        added,
        date: format_date(added),
        download_url: format!("{}download.php?id={}", FORUM, download_id),
        page_url: format!("{}viewtopic.php?t={}", FORUM, topic_id),
        source: "nnmclub".to_string(),
        group: None,
        query: String::new(),
    })
}

/// The full cell a marker sits in, as text -- `None` when the marker
/// is missing, which for seeds/leechers means the page is not the page
/// this parser was written against.
fn cell_of(row: &str, marker: &str) -> Option<String> {
    let (start, end) = cell_span(row, marker)?;
    Some(row[start..end].to_string())
}

/// The results table -> rows. Public so the fixture tests exercise the
/// real parser with no network, as with `yts::parse_page`.
pub fn parse_rows(body: &str) -> Result<Vec<TorrentItem>> {
    let patterns = match patterns() {
        Some(p) => p,
        None => bail!("nnmclub: the parser's patterns failed to build"),
    };
    // The live page always answers with this table -- 50 rows, 9 rows,
    // or just `Не найдено`. Its absence therefore means we are not
    // reading the tracker at all, and saying "no results" would be the
    // one lie the user cannot investigate.
    let Some(table) = patterns.table.captures(body) else {
        bail!("nnmclub: the page has no results table (blocked, moved, or asking for a login)");
    };
    let Some(table) = table.get(1) else {
        bail!("nnmclub: the results table came out empty");
    };

    let mut rows = Vec::new();
    for caps in patterns.row.captures_iter(table.as_str()) {
        let Some(row) = caps.get(1) else { continue };
        let row = row.as_str();
        // Header/footer rows and the `Не найдено` row have neither
        // link, so they fall out here without needing a case each.
        if !row.contains("viewtopic.php?t=") || !row.contains("download.php?id=") {
            continue;
        }
        if let Some(item) = to_row(row, patterns) {
            rows.push(item);
        }
    }
    Ok(rows)
}

/// The rows -> the page, with the cursor spelled out: `start=` steps
/// by whole pages, so the next offset is `offset + PAGE_SIZE` and not
/// `offset + rows.len()` (a dropped row would otherwise walk the
/// cursor off the site's page grid, silently skipping a page).
pub fn to_page(items: Vec<TorrentItem>, offset: usize) -> SearchPage {
    let has_more = items.len() >= PAGE_SIZE;
    SearchPage {
        items,
        has_more,
        next_offset: has_more.then_some(offset + PAGE_SIZE),
    }
}

pub struct NnmclubSearcher {
    client: reqwest::Client,
}

impl NnmclubSearcher {
    pub const HOME_URL: &str = "https://nnmclub.to";

    pub fn new() -> Self {
        Self { client: browser_client() }
    }
}

#[async_trait]
impl Source for NnmclubSearcher {
    fn id(&self) -> &'static str {
        "nnmclub"
    }

    fn label(&self) -> &'static str {
        "NNM-Club"
    }

    fn groups(&self) -> &'static [Group] {
        // Declared, not guessed per row (wave-3 decision): the four
        // forums the site spans, which is what the Options grouping
        // and B6's filtering need from a source.
        GROUPS
    }

    fn home_url(&self) -> &'static str {
        Self::HOME_URL
    }

    fn requires_browser(&self) -> bool {
        // Live: a plain HTTP client with a browser UA got 200 from
        // every page, cloudflare included -- no challenge to solve.
        false
    }

    fn supports_browse(&self) -> bool {
        // Live-verified: `f=-1&o=2&sd=desc` returns 50 newest topics,
        // and `start=50` returns a disjoint second page.
        true
    }

    async fn ensure_logged_in(&self, _auth: &AuthContext, _log: &LogFn) -> Result<bool> {
        Ok(true)
    }

    async fn search(&self, req: &SearchRequest) -> Result<SearchPage> {
        let offset = req.offset;
        let url = search_url(&req.query, offset);
        let response =
            fetch_resilient(&url, || self.client.get(&url), &FetchOptions::default()).await?;
        let status = response.status();
        ensure!(status.is_success(), "NNM returned {}", status);
        let bytes = response.bytes().await?;

        // windows-1251 in the header and in the `<meta>`, live; the
        // replacement keeps an unexpected byte from becoming U+FFFD in
        // the middle of a title instead of dropping the whole page.
        let (body, _, _) = encoding_rs::WINDOWS_1251.decode(&bytes);

        Ok(to_page(parse_rows(&body)?, offset))
    }

    async fn download_torrent(&self, url: &str) -> Result<Vec<u8>> {
        // The row's own `download.php?id=` link (live: 302 ->
        // `application/x-bittorrent`). The status check matters for the
        // same reason rutor's has one: a challenge page must never be
        // handed to TorrServer as a .torrent file.
        let response =
            fetch_resilient(url, || self.client.get(url), &FetchOptions::default()).await?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("nnmclub download {} answered HTTP {}", url, status);
        }
        Ok(response.bytes().await?.to_vec())
    }
}
