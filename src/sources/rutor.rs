//! Rutor search + download (host: rutor.info). Plain, unauthenticated
//! HTTP -- no browser session, no login, no cookies (confirmed live,
//! September 2026), which makes it the lightest `Source` in the tree.
//!
//! **Why rutor.info and not rutor.org.** Both mirrors serve identical
//! results and share torrent ids, but rutor.org answers `302 -> /login`
//! for `/download/{id}` and `/magnet/{id}` to a logged-out client -- a
//! download there returns an HTML login page, and an HTML login page was
//! what once got uploaded to TorrServer. rutor.info answers `200
//! application/x-bittorrent` on the same path and carries an inline
//! magnet per row, which rutor.org does not.
//!
//! **A category is a fan-out over rubric ids, not a comma list.** The
//! search URL filters server-side, while a row carries no category at
//! all -- the rubric is named only on the torrent's own page. So a
//! selected group becomes one GET per id of `GROUP_IDS`, asked one after
//! another (rutor answers 503 under load, so no burst), merged and
//! deduped by `page_url`, and each row claims the category that fetched
//! it. A comma list is not a shortcut: live, `cat=1,5` answered
//! byte-for-byte the rows of `cat=1` and silently lost all 96 of `cat=5`,
//! while an unknown id answers 0 rows rather than everything. No
//! selection keeps `cat=0` and rows claim nothing.
//!
//! Markup notes worth knowing before changing the parser: the counts sit
//! in the title's own row behind `alt="S"` / `alt="L"` icons with a
//! literal `&nbsp;` entity and an extra `<span>` before peers; the date
//! cell is the first `<td>` and uses `06 Сен 26` on one mirror and
//! `06&nbsp;Сен&nbsp;26` on the other (both accepted, normalised to
//! spaces); a page holds exactly 100 rows; and rows carry no category,
//! which is why the fan-out above claims one on their behalf.
//!
//! If the markup changes, this file and `tests/rutor_parse_tests.rs`,
//! which pins the exact row shape seen live, are what to fix.

use anyhow::Result;
use regex::Regex;
use scraper::{Html, Selector};

use std::collections::HashSet;
use std::sync::OnceLock;

use crate::sources::models::TorrentItem;
use crate::sources::net::{browser_client, fetch_resilient, FetchOptions};
use crate::sources::source::{Group, SearchPage};

/// The rubric id behind each group this source declares -- one table
/// for both halves of B6: the id the search URL is asked with, and the
/// category a returned row claims, so the two can never drift apart.
///
/// Every id was read off the live site on 26.09.2026 by fetching a row
/// from each rubric and taking the name its own page prints ("Категория
///..."), rather than guessed from the URL:
///
/// - Movies: 1 Зарубежные фильмы, 5 Наши фильмы, 7 Мультипликация,
///   12 Научно-популярные фильмы -- the four buckets holding films
///   (decision with the user: cartoons and documentaries count).
/// - TV: 4 Зарубежные сериалы, 16 Наши сериалы, 6 Телевизор, 15 Юмор --
///   series plus everything else broadcast (decision with the user).
/// - Games: 8 Игры. Anime: 10 Аниме.
///
/// Everything else stays out -- 2 Музыка, 9 Софт, 11 Книги, 13 Спорт и
/// Здоровье, 14 Хозяйство и Быт, 17 Иностранные релизы and the
/// remaining buckets -- because a row fetched from them would be shown
/// under a group this registry never promised for rutor.
pub const GROUP_IDS: [(Group, &[i64]); 4] = [
    (Group::Movies, &[1, 5, 7, 12]),
    (Group::TV, &[4, 6, 15, 16]),
    (Group::Games, &[8]),
    (Group::Anime, &[10]),
];

/// The ids a category search fans out over, or an empty list for a
/// group rutor does not declare. The orchestrator only asks a source
/// for groups it registered, so the empty list is a safe landing rather
/// than a branch with a user behind it.
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

/// The browse URL: the homepage index. Live 26.09.2026 it
/// answers 149 rows of the latest releases with the same row markup
/// as the search results, and it has no pager -- so browse is one
/// page and `has_more` is false. The category is not honoured: the
/// homepage is one mixed list, which is why the `b` key returns the
/// view to "all" before searching.
pub const BROWSE_URL: &str = "https://rutor.info/";

/// A real rutor search results page is large (many rows); a tiny
/// response on a 2xx status is a strong sign of a challenge/interstitial
/// page rather than genuinely zero matches.
const EMPTY_PAGE_THRESHOLD: usize = 2000;

impl RutorSearcher {
    pub const HOME_URL: &'static str = "https://rutor.info/";
    /// Search/download host. rutor.info, not rutor.org: since
    /// 25.09.2026 the.org mirror answers `302 -> /login` for
    /// `/download/{id}`, so an unauthenticated download returns an HTML
    /// login page instead of a.torrent (see the module docs).
    const BASE: &'static str = "https://rutor.info";
    /// Rutor's search pages hold a fixed 100 rows, verified live while
    /// fixing the zero-results bug: `matrix` reports 219 hits and comes
    /// back 100/100/22/0 rows on pages 1/2/3/4 (page 0 is a synonym of
    /// page 1). Used to translate this app's "offset" pagination
    /// convention (0, 100, 200,...) into rutor's 1-based page numbers
    /// for "load more".
    /// Rows per results page -- the unit `SearchRequest::offset` counts
    /// in, and what `Source::search` uses to decide `has_more`.
    pub const PAGE_SIZE: usize = 100;

    pub fn new() -> Self {
        // The shared browser-like client: User-Agent plus the
        // Accept/Accept-Language pair rutor used to set per request.
        Self {
            client: browser_client(),
        }
    }

    pub async fn search(&self, query: &str) -> Result<Vec<TorrentItem>> {
        Ok(self.search_page(query, 0, None).await?.items)
    }

    /// Rutor matches a multi-word query as a strict AND over *all* of its
    /// words, in any order -- and words its index doesn't contain make the
    /// whole query return zero hits. Two classes of words are effectively
    /// unmatchable (both confirmed live while fixing this):
    ///
    /// - English/Russian stopwords ("the", "of", "a", "it", "am",...):
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
    /// One page of results, fanned out over the selected category's
    /// rubric ids: one request per id of [`GROUP_IDS`], asked one after
    /// another (rutor answers 503 under load, so no burst), or a single
    /// request with `cat=0` when no category is selected.
    ///
    /// `has_more` and the cursor are decided per id by [`to_page`]: a
    /// merged row count would lie, because four partial pages can add up
    /// past `PAGE_SIZE` while not one of them has a next page.
    pub async fn search_page(
        &self,
        query: &str,
        offset: usize,
        category: Option<Group>,
    ) -> Result<SearchPage> {
        if query.trim().is_empty() {
            // Browse: the homepage's latest releases -- one mixed
            // list, no pager, no category. Rows claim no group, which
            // is why the `b` key returns the view to "all" first.
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
            // so an offset that isn't on a page boundary means the
            // previous fetch was a partial (= final) page. Returning
            // nothing here both avoids re-reading that page and makes
            // `Source::search` report `has_more: false`, which flips
            // `all_loaded`.
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
        // actual intent ("the matrix" -> "Матрица / The Matrix"), so
        // promote them; if none do, keeping the relaxed rows is still
        // strictly better than showing nothing.
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

    /// The search URL for one rubric id: `/search/{page}/{cat}/000/0/
    /// {query}` -- `cat` is rutor's own rubric slot, `0` meaning "all
    /// categories" (its spelling, not ours). Public so tests can pin the
    /// category slot without the network, the way the other sources'
    /// URL builders are.
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

    /// One GET of any URL on this source's site, with the shared client
    /// and the unconditional diagnostic log line. Browse reuses it:
    /// the homepage is not a search URL, but it is fetched and parsed
    /// exactly like one.
    async fn fetch_url(&self, url: &str) -> Result<(reqwest::StatusCode, String)> {
        // Accept/Accept-Language come from the shared client; the
        // only per-request header left is Referer, which names this
        // source's own site. The fetch retries transient failures
        // (rutor occasionally answers 503 under load) and refuses to
        // retry a ddos-guard challenge, which would otherwise burn the
        // whole request budget on a page that never becomes an answer.
        //
        // Logged unconditionally to crate::log
        // (~/.local/share/doris/doris.log) since the Source trait's
        // search_page has no log-callback parameter to surface this in
        // the UI's own Detailed Log panel the way Rutracker's AUTH steps
        // do -- if this ever returns zero results again, that file is
        // the first thing to check.
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
            // tiny response on a 2xx status is a strong sign of a
            // challenge/interstitial page rather than genuinely zero
            // matches. Log a snippet so the actual page content (rather
            // than just its length) is on hand next time this happens.
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
        // transient 503 should retry rather than hand TorrServer a
        // failure page.
        let response =
            fetch_resilient(url, || self.client.get(url), &FetchOptions::default()).await?;
        let status = response.status();
        if !status.is_success() {
            // This used to be unchecked, so a mirror answering
            // `302 -> /login` (or a plain 404) had its HTML body
            // uploaded as a.torrent -- the exact failure that made us
            // move to rutor.info in the first place.
            anyhow::bail!("rutor download {} answered HTTP {}", url, status);
        }
        let bytes = response.bytes().await?;
        Ok(bytes.to_vec())
    }
}

/// One fan-out's worth of pages -> the page the app sees. `per_id` is
/// one `Vec` per rubric id asked for (a single one for an unfiltered
/// `cat=0`), and the rules are:
///
/// - **Merge, then dedup by `page_url`.** Rubrics are disjoint on the
///   live site (`cat=1` and `cat=5` shared zero ids on 26.09.2026), so
///   the dedup is the belt on those braces: a torrent listed twice is
///   still one torrent.
/// - **`has_more` reads each id's own page** -- one full page means at
///   least one rubric has a next one. A *merged* count would answer
///   "more" whenever four partial pages add up past `PAGE_SIZE`, and
///   then promise a page the site does not have.
/// - **The cursor steps by exactly one page** (`offset + PAGE_SIZE`):
///   every id was read at the same page number, so counting the merged
///   rows instead would jump ahead and skip each rubric's rows.
/// - **Rows claim the category that fetched them**. With no
///   category they claim nothing -- the honest reading of an
///   unfiltered row this parser cannot attribute.
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

/// Words rutor's index skips even though they appear in (English and
/// Russian) titles all the time, so AND-ing them into a query can only
/// ever produce zero hits -- the reason `world war z` and friends used to
/// come back empty. Kept deliberately small and only for function words:
/// dropping a real content word on the fallback path costs precision,
/// dropping a stopword cannot, because rutor could not have matched it
/// anyway.
pub const STOPWORDS: &[&str] = &[
    // English
    "a", "an", "the", "of", "to", "it", "i", "am", "is", "are", "be", "in", "on", "at", "by", "for",
    "and", "or", "but", "not", "with", "from", "this", "that", "as", "my", "your",
    // Russian
    "и", "в", "во", "на", "с", "со", "к", "ко", "о", "об", "от", "до", "для", "по", "из", "не",
    "ни", "что", "как", "за", "у", "же", "бы", "то",
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
        // every observed <=2-char token ("it", "am", "z", "qq",...) was
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
    // to an empty size. `(?i)` covers lower-case spellings too
    // (`гб`, `mb`), which the old pattern also missed.
    let size_re = Regex::new(r"(?i)(\d+(?:[.,]\d+)?)(?:\s|&nbsp;)*(TB|GB|MB|KB|ТБ|ГБ|МБ|КБ)").ok();
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
/// normalizing, which is `normalize_info_hash` job -- guessing in
/// two places is how a wrong hash silently defeats dedup.
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

/// `06 Сен 26` -> unix seconds of that UTC day, `0` when the date can't
/// be read. Port of torio's `parseRutorDate`: three-letter Russian month
/// abbreviations, two-digit years read as 20xx.
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
