//! Rutor search + download (host: rutor.info). Unlike Rutracker, this
//! needs no browser session, no login, and no cookies -- confirmed live
//! (September 2026): search results and .torrent downloads are both
//! plain, unauthenticated HTTP GETs. This makes it a much lighter-weight
//! `Source` than Rutracker's, and a good first proof that the `Source`
//! abstraction from ROADMAP.md Phase 3 actually pays off: adding a second
//! real source didn't require touching the browser layer at all.
//!
//! Why rutor.info and not rutor.org (live check, 25.09.2026): both
//! mirrors serve identical search results and share torrent ids, but
//! rutor.org now answers `302 -> /login` for `/download/{id}` and
//! `/magnet/{id}` to a logged-out client -- downloading there returns an
//! HTML login page instead of a .torrent, which is exactly what got
//! uploaded to TorrServer. On rutor.info the same `/download/{id}`
//! answers `200 application/x-bittorrent` (redirecting to
//! `d.rutor.info`), and rows carry an inline `magnet:?xt=urn:btih:...`
//! link rutor.org does not have.
//!
//! Page structure (verified by fetching both mirrors and a live search
//! results page while writing this, not guessed at from memory):
//! - Search: `GET {BASE}/search/{page}/{category}/000/0/{urlencoded query}`,
//!   `page` starts at 1, `category` 0 = all categories. The site's own
//!   advanced form (`/search`) builds the same URL: its third segment is
//!   `{search_method}{search_in}0` (000 = "фразу полностью", в титуле)
//!   and the fourth is a sort id -- neither changes what matches, see
//!   [`RutorSearcher::search_page`] for rutor's real (AND, stopword-
//!   sensitive) semantics and the fallback built on top of them.
//! - Each result row has a title link `<a href="/torrent/{id}[/{slug}]">`
//!   (only rutor.info adds the slug; the id is the first path segment
//!   either way), a download link (protocol-relative
//!   `//d.rutor.info/download/{id}` on rutor.info), and a magnet link
//!   (`magnet:?xt=urn:btih:{40 hex}...` inline on rutor.info). The
//!   parser builds download/page URLs from the numeric id plus
//!   [`RutorSearcher::BASE`] instead of copying the row's hrefs, so the
//!   same code works on either mirror.
//! - Size ("2.27 GB" / "82.73&nbsp;MB"), a seed count after an
//!   `alt="S"` up-arrow icon, and a peer/leech count after an
//!   `alt="L"` down-arrow icon all live in the same table row as the
//!   title link. Live markup wraps the counts as
//!   `<img ... alt="S">&nbsp;6` (seeds) and
//!   `<img ... alt="L"><span class="red">&nbsp;2</span>` (peers) -- note
//!   the literal `&nbsp;` entity and the extra `<span>` before peers.
//! - The date cell is the first `<td>`: `06 Сен 26` with plain spaces on
//!   rutor.org, `06&nbsp;Сен&nbsp;26` (entity separators) on
//!   rutor.info -- the parser accepts both and normalizes to spaces.
//! - A page holds a fixed 100 rows (verified: "matrix" = 219 hits ->
//!   pages of 100/100/22/0 rows; page 0 and page 1 are the same page).
//!
//! If rutor changes its markup, this is the file (and
//! `tests/rutor_parse_tests.rs`, which pins down the exact row shape seen
//! live) to fix -- same spirit as the TorrServer JSON-shape caveat
//! elsewhere in ROADMAP.md.

use anyhow::Result;
use regex::Regex;
use scraper::{Html, Selector};

use std::sync::OnceLock;

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
    pub const HOME_URL: &'static str = "https://rutor.info/";
    /// Search/download host. rutor.info, not rutor.org: since
    /// 25.09.2026 the .org mirror answers `302 -> /login` for
    /// `/download/{id}`, so an unauthenticated download returns an HTML
    /// login page instead of a .torrent (see the module docs).
    const BASE: &'static str = "https://rutor.info";
    /// Rutor's search pages hold a fixed 100 rows, verified live while
    /// fixing the zero-results bug: `matrix` reports 219 hits and comes
    /// back 100/100/22/0 rows on pages 1/2/3/4 (page 0 is a synonym of
    /// page 1). Used to translate this app's "offset" pagination
    /// convention (0, 100, 200, ...) into rutor's 1-based page numbers
    /// for "load more".
    const PAGE_SIZE: usize = 100;

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

    /// Rutor matches a multi-word query as a strict AND over *all* of its
    /// words, in any order -- and words its index doesn't contain make the
    /// whole query return zero hits. Two classes of words are effectively
    /// unmatchable (both confirmed live while fixing this):
    ///
    /// - English/Russian stopwords ("the", "of", "a", "it", "am", ...):
    ///   they are stripped from the index but not from the query, so even
    ///   `live the matrix` returns 0 while `live matrix` returns the very
    ///   torrent that contains "The" in its title.
    /// - Rare tokens no title contains ("z", "qq"), which is why the
    ///   natural query `world war z` always came back empty.
    ///
    /// So when the literal query finds nothing, retry once with the
    /// unmatchable words removed and then prefer the rows that do mention
    /// them -- strict precision when rutor allows it, relaxed-but-useful
    /// rows otherwise, instead of a guaranteed empty result list.
    pub async fn search_page(&self, query: &str, offset: usize) -> Result<Vec<TorrentItem>> {
        if offset % Self::PAGE_SIZE != 0 {
            // The app advances `offset` by however many rows came back,
            // so an offset that isn't on a page boundary means the
            // previous fetch was a partial (= final) page. Returning
            // nothing here both avoids re-reading that page and lets the
            // caller's `count < 50` check flip `all_loaded`.
            crate::log::log("rutor", &format!(
                "offset {} is past a partial final page; no more results",
                offset,
            ));
            return Ok(Vec::new());
        }
        let page = (offset / Self::PAGE_SIZE) + 1;

        let (status, html) = self.fetch_page(page, query).await?;
        if !status.is_success() {
            // Parsing an error/challenge page always finds zero results;
            // say so explicitly instead of silently returning an empty
            // Vec indistinguishable from "no matches for this query".
            anyhow::bail!("rutor returned HTTP {} for {}", status, query);
        }

        let strict = parse_results(&html);
        if !strict.is_empty() {
            return Ok(strict);
        }

        let (kept, dropped) = split_query(query);
        if dropped.is_empty() || kept.is_empty() {
            // Nothing was dropped, or everything was: no better query to
            // try, so the literal one's empty answer is the real answer.
            return Ok(strict);
        }

        let relaxed = kept.join(" ");
        crate::log::log("rutor", &format!(
            "0 hits for {:?}; retrying as {:?} (rutor does not index \
             {:?} and ANDs every query word)",
            query, relaxed, dropped,
        ));
        let (status, html) = self.fetch_page(page, &relaxed).await?;
        if !status.is_success() {
            crate::log::log("rutor", &format!(
                "relaxed query {:?} failed with HTTP {}", relaxed, status,
            ));
            return Ok(strict);
        }

        let items = parse_results(&html);
        if items.is_empty() {
            return Ok(items);
        }
        // Rows that really mention the dropped words are the user's
        // actual intent ("the matrix" -> "Матрица / The Matrix"), so
        // promote them; if none do, keeping the relaxed rows is still
        // strictly better than showing nothing.
        let exact: Vec<TorrentItem> = items.iter()
            .filter(|it| dropped.iter().all(|w| title_has_word(&it.title, w)))
            .cloned()
            .collect();
        if !exact.is_empty() {
            crate::log::log("rutor", &format!(
                "{}/{} relaxed rows also mention the dropped words",
                exact.len(), items.len(),
            ));
            return Ok(exact);
        }
        crate::log::log("rutor", &format!(
            "no relaxed row mentions {:?}; keeping all {} rows",
            dropped, items.len(),
        ));
        Ok(items)
    }

    /// One GET of a search page plus the unconditional diagnostic log
    /// line (kept out of `search_page` so the strict and the relaxed
    /// attempt share the exact same request shape).
    async fn fetch_page(&self, page: usize, query: &str) -> Result<(reqwest::StatusCode, String)> {
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

        if matched == 0 && html.len() < 2000 {
            // A real rutor search results page is large (many rows); a
            // tiny response on a 2xx status is a strong sign of a
            // challenge/interstitial page rather than genuinely zero
            // matches. Log a snippet so the actual page content (rather
            // than just its length) is on hand next time this happens.
            let snippet: String = html.chars().take(500).collect();
            crate::log::log("rutor", &format!(
                "suspiciously small body, first 500 chars: {}", snippet,
            ));
        }

        Ok((status, html))
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

/// Words rutor's index skips even though they appear in (English and
/// Russian) titles all the time, so AND-ing them into a query can only
/// ever produce zero hits -- the reason `world war z` and friends used to
/// come back empty. Kept deliberately small and only for function words:
/// dropping a real content word on the fallback path costs precision,
/// dropping a stopword cannot, because rutor could not have matched it
/// anyway.
pub const STOPWORDS: &[&str] = &[
    // English
    "a", "an", "the", "of", "to", "it", "i", "am", "is", "are", "be",
    "in", "on", "at", "by", "for", "and", "or", "but", "not", "with",
    "from", "this", "that", "as", "my", "your",
    // Russian
    "и", "в", "во", "на", "с", "со", "к", "ко", "о", "об", "от", "до",
    "для", "по", "из", "не", "ни", "что", "как", "за", "у", "же", "бы",
    "то",
];

/// Split a query into the words rutor can actually match and the ones
/// that would poison the whole AND (see [`STOPWORDS`] and the
/// [`RutorSearcher::search_page`] docs). Punctuation is trimmed off the
/// edges first so `"the,"` and `the` are treated the same. Public for
/// `tests/rutor_parse_tests.rs`; the fallback in `search_page` is its
/// only in-crate user.
pub fn split_query(query: &str) -> (Vec<String>, Vec<String>) {
    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    for word in query.split_whitespace() {
        let core: String = word
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_string();
        // rutor's own help text says the minimum query length is 2, and
        // every observed <=2-char token ("it", "am", "z", "qq", ...) was
        // unmatchable -- no point sending them back.
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

/// Whole-word, case-insensitive containment check used to decide whether
/// a relaxed row really mentions the words that had to be dropped from
/// the query ("the" in "Матрица / The Matrix (1999)" yes, "the" in
/// "Theatre" no). Public for `tests/rutor_parse_tests.rs`.
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
    // are far more likely to change across a template refresh than the
    // URL scheme every row's title link has to use to point at a real
    // torrent page.
    let title_sel = match Selector::parse(r#"a[href^="/torrent/"]"#) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    // Inline magnet links (rutor.info puts `magnet:?xt=urn:btih:...`
    // right in the row; rutor.org only had an `/magnet/{id}` endpoint,
    // so rows there simply have no magnet and leave the field empty).
    let magnet_sel = match Selector::parse(r#"a[href^="magnet:"]"#) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    // Live markup puts the counts right after the icons as
    // `alt="S">&nbsp;6` / `alt="L"><span class="red">&nbsp;2</span>`:
    // an explicit `&nbsp;` alternative is required between the `>` and
    // the digits (a plain `\s*` matched nothing -- that's why seeds used
    // to come back empty), and the peers variant may skip one wrapper
    // `<span>` before the number. Seeds deliberately don't skip tags so
    // they can never bleed into the leech count that follows.
    // Units come in both Latin (`2.27 GB`) and Cyrillic (`2,27 ГБ`)
    // spellings depending on the row -- the Cyrillic variant used to parse
    // to an empty size (B0.6). `(?i)` covers lower-case spellings too
    // (`гб`, `mb`), which the old pattern also missed.
    let size_re = Regex::new(
        r"(?i)(\d+(?:[.,]\d+)?)(?:\s|&nbsp;)*(TB|GB|MB|KB|ТБ|ГБ|МБ|КБ)",
    )
    .ok();
    let seeds_re = Regex::new(r#"alt="S"[^>]*>(?:\s|&nbsp;)*(\d+)"#).ok();
    let peers_re = Regex::new(r#"alt="L"[^>]*>(?:<[^>]*>|\s|&nbsp;)*(\d+)"#).ok();
    // "07 Сен 25" / "31 Окт 20" style short Russian date, always the very
    // first text in the row. rutor.info separates the parts with the
    // literal `&nbsp;` entity where rutor.org used plain spaces, so both
    // spellings match and the capture is normalized to spaces below.
    let date_re = Regex::new(r"(\d{2}(?:\s|&nbsp;)+[А-Яа-я]{3}(?:\s|&nbsp;)+\d{2})").ok();

    let mut items = Vec::new();
    let mut seen_ids = std::collections::HashSet::new();

    for title_el in document.select(&title_sel) {
        // Only rows of the search-results table count. The page also
        // links to `/torrent/{id}` from `table#news_table` (the tracker's
        // news posts -- ids like 472), and on rutor.info those hrefs are
        // relative, so the title-link selector catches them too; a news
        // row has no size/seeds/date and must not become a result.
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
        // dangerous, but be defensive against any future duplicate anchor
        // pointing at the same torrent within one row.
        if !seen_ids.insert(id.clone()) {
            continue;
        }

        let title = title_el.text().collect::<String>().trim().to_string();
        if title.is_empty() {
            continue;
        }

        let row_html = row.html();

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
        // Numeric twins derived from the display strings above (B1), then
        // the two fields rutor hands over directly: the added timestamp
        // (from the date cell) and the info hash inside the row's magnet.
        item.fill_from_display();
        item.added = parse_added(&item.date);
        item.info_hash = info_hash_from_magnet(item.magnet.as_deref().unwrap_or(""));
        // `group` stays `None`: the search URL carries category 0 ("all
        // categories"), so nothing here can attribute a row to a group --
        // that's B6, which passes a real category down to this parser.
        items.push(item);
    }
    items
}

/// Walk up from a title `<a>` to its enclosing `<tr>` and return that
/// row element, so size/seeds/date can be read from a small, known
/// snippet instead of guessed at by absolute position in the whole
/// document. Returns `None` (never panics) when the DOM shape isn't what
/// was expected -- the caller skips that link entirely.
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

/// Whether a row belongs to the search-results table. The page also
/// lists the tracker's news posts in `table#news_table`, whose links use
/// the same `/torrent/{id}` shape as real results (ids like 472) but have
/// no size/seeds/date -- exactly those rows are skipped. Everything else
/// that carries a numeric id is treated as a result, so a future change
/// to the results rows' `gai`/`tum` classes cannot silently turn a
/// working search into zero results.
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

/// The row's inline magnet URI, when it has one. Read from the DOM
/// attribute (entity-decoded at parse time) rather than regexing the
/// row's serialized HTML, where the `&dn=`/`&tr=` query parts come back
/// as `&amp;dn=`/`&amp;tr=` and would corrupt the stored URI.
fn magnet_href(row: &scraper::ElementRef, sel: &Selector) -> Option<String> {
    row.select(sel)
        .next()
        .and_then(|a| a.value().attr("href").map(str::to_string))
}

/// `xt=urn:btih:{40 hex}` inside a magnet URI -> the lower-case hash, or
/// `""`. Only the 40-hex form is accepted: base32 hashes need
/// normalizing, which is B7's `normalize_info_hash` job -- guessing in
/// two places is how a wrong hash silently defeats dedup (B4).
fn info_hash_from_magnet(magnet: &str) -> String {
    info_hash_re()
        .and_then(|re| re.captures(magnet))
        .map(|caps| caps[1].to_lowercase())
        .unwrap_or_default()
}

fn info_hash_re() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)xt=urn:btih:([a-f0-9]{40})").ok()).as_ref()
}

/// `06 Сен 26` -> unix seconds of that UTC day, `0` when the date can't
/// be read. Port of torio's `parseRutorDate`: three-letter Russian month
/// abbreviations, two-digit years read as 20xx.
fn parse_added(date: &str) -> i64 {
    const RU_MONTHS: [&str; 12] = [
        "Янв", "Фев", "Мар", "Апр", "Май", "Июн",
        "Июл", "Авг", "Сен", "Окт", "Ноя", "Дек",
    ];
    let Some(caps) = added_re().and_then(|re| re.captures(date)) else {
        return 0;
    };
    let day: i64 = match caps[1].parse::<i64>() {
        Ok(d) => d,
        Err(_) => return 0,
    };
    let Some(month) = RU_MONTHS.iter().position(|m| m.eq_ignore_ascii_case(&caps[2]))
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

/// Days since 1970-01-01 for a civil (year, month, day) -- Howard
/// Hinnant's `days_from_civil`. No date crate is in the dependency list
/// (AGENTS.md: don't add dependencies speculatively) and this is the
/// whole algorithm.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (month + 9) % 12; // March = 0 .. February = 11
    let doy = (153 * mp + 2) / 5 + day - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}
