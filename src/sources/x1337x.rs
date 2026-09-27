//! 1337x over its search HTML (ROADMAP.md B8 wave 3), shaped by live
//! probes on 25.09.2026 -- three of which are the reason this source
//! differs from every other one in the tree.
//!
//! - **Mirrors, and why `requires_browser` is false.** torio's four
//!   hosts (`1337x.to`, `1337x.st`, `x1337x.ws`, `1337xx.to`) were all
//!   probed from this network: the first three answer **403 with a
//!   Cloudflare JS challenge** (`cf-mitigated: challenge`, "Just a
//!   moment...") to any plain client, while `1337xx.to` 301s to
//!   `www.1337xx.to`, and that one answers **200 on every path
//!   checked** -- search, category-search, detail, `/home/`,
//!   `/popular-*` -- to the very same client. So the challenge belongs
//!   to those mirrors rather than to the site or the network, the
//!   mirror that answered leads the failover order, and there is no
//!   browser to launch (decision; `requires_browser` said `true` in the
//!   registry until these probes came back).
//!
//! - **The category slot is the site's own path** (B6, live
//!   26.09.2026): `/category-search/<q>/<label>/<page>/` answers with
//!   rows whose `/sub/` links are *all* the requested label -- `matrix`
//!   came back 20/20 `movies`, 20/20 `tv`, 11/11 `games`, 1/1 `anime`,
//!   while a made-up label answers 0 rows -- and page 2 is disjoint
//!   from page 1, so the same page cursor keeps its meaning. Browse
//!   has the matching slot in `/popular-<label>/` (22/21/23/2 rows,
//!   likewise all one `/sub/`). `None` keeps `/search/` and `/home/`.
//!   A row fetched *inside* a selected category claims that category;
//!   with no selection there is nothing to claim, and reading an
//!   unfiltered row's `/sub/` link was left undone rather than
//!   guessed (the honesty gap `source.rs` pins down).
//!
//! - **The row carries the whole table.** Title plus
//!   `/torrent/<id>/<slug>/`, seeders, leechers, size, uploader *and
//!   the date* (`Oct. 01st  '22` -- the same shape torio reads off the
//!   detail page as "Date uploaded"). Every column fills from the
//!   search page alone, so neither torio's eight detail fetches per
//!   search nor the one shape below need to be in anybody's way.
//!
//! - **The list's freshest rows show a time instead of a date**
//!   (decision with the user): 23 of the 672 live `coll-date` cells
//!   read `03:15am` -- 11 of the 78 rows on `/home/`, and none at all
//!   on any search page that day -- and fetching those rows' own pages
//!   showed `Date uploaded: Sep. 23rd '26` behind every one of them.
//!   The day exists; the list just does not spell it out for recent
//!   uploads. torio never meets this because it reads the date off a
//!   detail page for *every* row, while here it is **those rows and
//!   only those** -- detected by the list having given them no `added`
//!   -- that fetch their own page, four at a time, for `Date
//!   uploaded`. Nothing extra for a query, a handful of requests for
//!   browse, and the date sort keeps working for exactly the rows a
//!   user sorts toward.
//!
//! - **No `.torrent` exists anywhere.** 1337x is an aggregator: a
//!   detail page answers 200 with a magnet (verified live on Dune 2021,
//!   btih `4d165eae...`) and no download link at all. The link
//!   therefore arrives from `resolve_magnet` at *play* time -- one
//!   request for the one row somebody picks, instead of torio's fan-out
//!   that leaves the un-fetched rows unplayable (decision, the commit
//!   that added that method). A row arrives with neither `magnet` nor
//!   `download_url` nor `info_hash`, which `dedupe_by_hash` lets
//!   through untouched, as with nnmclub.
//!
//! - **The engine ORs a multi-word query, so the client filters**
//!   (decision): live, `frieren 2026` and `2026 frieren` both return
//!   60 rows containing "2026" and *zero* containing "frieren", while
//!   `frieren` alone returns 20 of 20. Hence a one-word query is
//!   trusted exactly as answered -- the engine also matches on
//!   metadata, 8 of the 20 rows for `frieren crack` carry neither word
//!   in the title, and filtering those away would delete rows the site
//!   vouched for -- and a longer query keeps only the rows carrying
//!   every meaningful word (stop words, as torio lists them).
//!
//!   torio runs the same test over its rows, but it reads one
//!   *category* page, keeps at most eight rows and never pages -- so
//!   it never meets what a page-wide filter does here. Live:
//!   `witcher s03` keeps 4 of 20 (the filter earning its keep), while
//!   `dune 1080p`, `frieren 2026` and `frieren crack` keep **none** on
//!   every page checked (5 pages of `dune 1080p`, 12 of
//!   `frieren 2026`, 3 of `frieren crack` -- and `/category-search/`
//!   answers the same junk). An empty table is not an answer either:
//!   the TUI's auto-paging refuses to ask for page 2 from an empty
//!   list (`needs_more`) and has no key of its own, so the user would
//!   be stuck on nothing. Hence the fallback, decided with the user:
//!   **a filter that would take every row leaves the page exactly as
//!   the site answered it**, with a line in the log saying so -- and
//!   only a page the site itself left empty stays empty.
//!
//! - **A page number in an offset costume.** `/search/<q>/<N>/` steps
//!   by pages of 20 (pages 1 and 2 of `dune` share zero rows), so
//!   `next_offset = offset + 20` is spelled out rather than counted
//!   from the rows, and `has_more` is read off the *server's* page
//!   **before** the filter: a full page trimmed to three rows is still
//!   a full page, or pagination dies the first time somebody searches
//!   two words -- the nnmclub dead-row lesson, applied to a second
//!   mechanism.
//!
//! - **Browse is `/home/`.** 78 rows across the site's per-category
//!   sections, the same markup, and no pager at all -- so browse never
//!   promises a next page.
//!
//! - **A miss is a miss, and a block is an error.** A query with no
//!   matches answers 200 with the table header present and no rows
//!   (live: `zzqqxxnothing123`), so zero rows mean an empty page; a
//!   page with no `table-list` at all is something else -- a challenge
//!   that answered 200, a moved layout -- and is an error, which is
//!   also what lets `first_ok` try the next mirror instead of calling
//!   all of them "no results".

use std::sync::OnceLock;

use anyhow::{Context, Result, bail, ensure};
use async_trait::async_trait;
use regex::Regex;

use super::format::{format_date, parse_size, unescape_entities};
use super::models::TorrentItem;
use super::net::{FetchOptions, browser_client, fetch_resilient, first_ok};
use super::source::{AuthContext, Group, LogFn, SearchPage, SearchRequest, Source};

/// Mirror hosts, the live-verified answer first (module doc): the two
/// `1337xx.to` spellings serve real markup, torio's other three are
/// behind the challenge and stay as the fallback a mirror outage would
/// need.
pub const HOSTS: &[&str] = &[
    "www.1337xx.to",
    "1337xx.to",
    "1337x.to",
    "1337x.st",
    "x1337x.ws",
];

/// Rows the site puts on one search page, live on pages 1-3 of `dune`
/// (20 each, page 2 disjoint from page 1). A full page is what "may be
/// more" means here -- browse never has one, see `to_browse_page`.
pub const PAGE_SIZE: usize = 20;

/// The site's categories that *have* a [`Group`] to go to -- and
/// exactly the four the site's own `/category-search/<q>/<label>/` and
/// `/popular-<label>/` paths spell the same way (live 26.09.2026), so
/// a declared group is a URL this source can really be trimmed by.
/// Music, Documentaries, Applications, Other and XXX have none, and an
/// unfiltered row's `/sub/<category>/` link is still uninventoried:
/// such a row keeps `group = None`, like nnmclub's.
const GROUPS: &[Group] = &[Group::Movies, Group::TV, Group::Games, Group::Anime];

/// Words that carry no match of their own once the engine ORs them
/// (torio's `STOP` set).
const STOP: &[&str] = &["the", "a", "an", "of", "and", "or", "to"];

/// The string every results page carries before its rows -- search,
/// category-search, and each of `/home/`'s sections. Its absence means
/// we are not reading 1337x.
const TABLE: &str = "table-list";

/// How many undated rows' own pages are read at once (module doc):
/// this runs before the page reaches the screen, so the user waits for
/// the lot -- four at a time rounds the delay down without turning a
/// browse into a burst.
const DATE_FETCHES: usize = 4;

/// The search URL for `host`. `offset` counts rows and the site counts
/// pages, so the page number is derived rather than stored -- which is
/// also what keeps a restarted search on the site's grid.
///
/// A selected category picks the site's own server-side slot: the
/// path becomes `/category-search/<q>/<label>/<page>/`, spelled with
/// [`Group::label`] -- the very word the tabs show, which live
/// 26.09.2026 is also the word the site filters by (every row of the
/// `Movies` answer carried `/sub/movies/`; a label the site does not
/// know answers zero rows, so a typo cannot pass for a hit). `None`
/// keeps the plain `/search/`, whose rows claim nothing (see
/// [`stamp_category`]).
///
/// An empty query goes to browse, whose URL takes the same category
/// (`/home/` vs `/popular-<label>/`).
pub fn search_url(host: &str, query: &str, offset: usize, category: Option<Group>) -> String {
    let query = query.trim();
    if query.is_empty() {
        return browse_url(host, category);
    }
    // torio's spelling: `+` between the words (verified live to behave
    // the same as `%20`, but the reference implementation uses this).
    let encoded = urlencoding::encode(query).replace("%20", "+");
    match category {
        Some(group) => format!(
            "https://{}/category-search/{}/{}/{}/",
            host,
            encoded,
            group.label(),
            page_of(offset)
        ),
        None => format!("https://{}/search/{}/{}/", host, encoded, page_of(offset)),
    }
}

/// The browse URL: the site's front page of popular rows, all sections
/// on one page, live -- or, with a category selected, the matching
/// `/popular-<label>/` section (live 26.09.2026: `/popular-movies`
/// answered 22 rows, all of them `/sub/movies/`, and likewise for tv,
/// games and anime), so browse is trimmed by the same selection a
/// search is.
pub fn browse_url(host: &str, category: Option<Group>) -> String {
    match category {
        Some(group) => {
            format!("https://{}/popular-{}", host, group.label().to_lowercase())
        }
        None => format!("https://{}/home/", host),
    }
}

/// `/search/<q>/<page>/` counts pages from 1; our offsets count rows
/// from 0. Floored, so an offset that does not land on a page
/// boundary still asks for a page that exists.
fn page_of(offset: usize) -> usize {
    offset / PAGE_SIZE + 1
}

/// The path half of an absolute URL. A row's page URL belongs to the
/// mirror that served it, and every mirror serves the same paths
/// (live), so the same path on another host is still the same page --
/// which is what lets `resolve_magnet` fail over after its own mirror
/// goes down.
fn path_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let at = rest.find('/')?;
    Some(rest[at..].to_string())
}

/// The regexes the parser needs, built once: the row split, and the
/// tags that come off a title or a cell.
struct Patterns {
    /// One row. The tables are flat -- no nested `<tr>`, live -- which
    /// is what makes this split safe.
    row: Regex,
    /// Tags, for title and cell text.
    tags: Regex,
    /// The magnet on a detail page. Case-insensitive because torio's
    /// matcher is, and a site that writes `MAGNET:?XT=` would otherwise
    /// be read as a page with no link at all.
    magnet: Regex,
}

impl Patterns {
    fn build() -> Option<Self> {
        Some(Self {
            row: Regex::new(r"<tr[^>]*>([\s\S]*?)</tr>").ok()?,
            tags: Regex::new(r"<[^>]+>").ok()?,
            magnet: Regex::new(r#"(?i)magnet:\?xt=urn:btih:[^"'<>\s]+"#).ok()?,
        })
    }
}

/// Compiled patterns, or `None` if any of them could not be built --
/// which callers turn into an error rather than an empty result.
fn patterns() -> Option<&'static Patterns> {
    static PATTERNS: OnceLock<Option<Patterns>> = OnceLock::new();
    PATTERNS.get_or_init(Patterns::build).as_ref()
}

/// Remove tags, then decode entities and collapse whitespace --
/// nnmclub's `strip_html` in the same order, which is what turns
/// `Dune.2021.<span>1080p</span>&amp;Co` back into a title.
fn strip_html(input: &str) -> String {
    let patterns = match patterns() {
        Some(p) => p,
        None => return input.to_string(),
    };
    unescape_entities(&patterns.tags.replace_all(input, ""))
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The row's `/torrent/<id>/<slug>/` link and the title inside it.
/// The anchor before it points at `/sub/<category>/...`, so the link is
/// found by its path rather than by position.
fn title_anchor(row: &str) -> Option<(String, String)> {
    let marker = "href=\"/torrent/";
    let at = row.find(marker)?;
    // Back to where the *value* starts, so the path keeps its
    // `/torrent/` -- the marker is only there to find the right link.
    let start = at + "href=\"".len();
    let quote = row[start..].find('"')? + start;
    let path = row[start..quote].to_string();
    let open = quote + row[quote..].find('>')?;
    let close = open + row[open..].find("</a>")?;
    let title = strip_html(&row[open + 1..close]);
    (!title.is_empty()).then_some((path, title))
}

/// The cell a marker sits in, as it was written -- `None` when the
/// marker is missing, which for the four cells this parser reads means
/// the page is not the page it was written against.
fn cell_of(row: &str, marker: &str) -> Option<String> {
    let at = row.find(marker)?;
    let start = row[..at].rfind("<td")?;
    let end = start + row[start..].find("</td>")?;
    Some(row[start..end].to_string())
}

/// The cell's text rather than its markup (dates and sizes are plain).
fn cell_text(row: &str, marker: &str) -> Option<String> {
    cell_of(row, marker).map(|cell| strip_html(&cell))
}

/// The first run of digits in a stripped cell (`<td class="coll-2
/// seeds">7312</td>` -> 7312).
fn first_number(cell: &str) -> u32 {
    strip_html(cell)
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse::<u32>()
        .unwrap_or(0)
}

/// The digits in a token (`01st` -> `01`, `'22` -> `22`).
fn digits(token: &str) -> String {
    token.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// `oct` from `Oct.` -- the site writes the abbreviation with a dot.
fn month_number(token: &str) -> u32 {
    match token.trim_end_matches('.').to_lowercase().as_str() {
        "jan" => 1, "feb" => 2, "mar" => 3, "apr" => 4,
        "may" => 5, "jun" => 6, "jul" => 7, "aug" => 8,
        "sep" => 9, "oct" => 10, "nov" => 11, "dec" => 12,
        _ => 0,
    }
}

/// `Oct. 01st  '22` -> unix seconds. This is torio's "Date uploaded"
/// from the detail page, found in the row's own `coll-date` cell
/// instead -- so a row's date costs no request. Unparseable is `0`,
/// "unknown", which is what `format_date` refuses to print as
/// 1970-01-01.
fn parse_upload_date(text: &str) -> i64 {
    let mut parts = text.split_whitespace();
    let month = parts.next().map(month_number).unwrap_or(0);
    let day = parts
        .next()
        .map(digits)
        .and_then(|day| day.parse::<u32>().ok())
        .unwrap_or(0);
    let year = parts
        .next()
        .map(|token| digits(token.trim_start_matches('\'')))
        .and_then(|year| year.parse::<i32>().ok())
        .map(|year| if (0..100).contains(&year) { 2000 + year } else { year })
        .unwrap_or(0);
    if month == 0 || day == 0 || year == 0 {
        return 0;
    }
    chrono::NaiveDate::from_ymd_opt(year, month, day)
        .and_then(|date| date.and_hms_opt(0, 0, 0))
        .map(|datetime| datetime.and_utc().timestamp())
        .unwrap_or(0)
}

/// One result row -> a row, or `None` when the row cannot become one:
/// no torrent link, no title, or one of the four cells missing. A
/// malformed row costs one row, never the page.
fn to_row(row: &str, host: &str) -> Option<TorrentItem> {
    let (path, title) = title_anchor(row)?;
    let seeds = first_number(&cell_of(row, "class=\"coll-2 seeds\"")?);
    let leechers = first_number(&cell_of(row, "class=\"coll-3 leeches\"")?);
    let added = parse_upload_date(&cell_text(row, "class=\"coll-date\"")?);
    // The site's own display string ("10.6 GB"), decoded into the
    // numeric twin the table sorts on: re-rendering it through
    // `format_bytes` would only cost a decimal place and change what
    // the user compared against the site.
    let size = cell_text(row, "class=\"coll-4 size")?;
    let size_bytes = parse_size(&size);

    Some(TorrentItem {
        // No link of any kind in the row (live): the magnet is on the
        // row's own page and there is no `.torrent` to fetch, so the
        // play path resolves it -- see `resolve_magnet`.
        magnet: None,
        info_hash: String::new(),
        title,
        size,
        size_bytes,
        seeds: seeds.to_string(),
        seeds_n: seeds,
        leechers,
        added,
        date: format_date(added),
        download_url: String::new(),
        page_url: format!("https://{}{}", host, path),
        source: "1337x".to_string(),
        group: None,
        query: String::new(),
    })
}

/// The results page -> rows. Public so the fixture tests exercise the
/// real parser with no network, as with `yts::parse_page`.
pub fn parse_rows(body: &str, host: &str) -> Result<Vec<TorrentItem>> {
    let patterns = match patterns() {
        Some(p) => p,
        None => bail!("1337x: the parser's patterns failed to build"),
    };
    // Every results page answers with this marker before its rows --
    // 20 rows, 9 rows, or none at all (live for a miss). Its absence
    // means we are not reading 1337x, and saying "no results" would be
    // the one lie the user cannot investigate.
    let Some(at) = body.find(TABLE) else {
        bail!("1337x: the page has no results table (blocked, moved, or asking for a login)");
    };

    let mut rows = Vec::new();
    for caps in patterns.row.captures_iter(&body[at..]) {
        let Some(row) = caps.get(1) else { continue };
        let row = row.as_str();
        // The header row (`<th>`), section headings, and the page
        // chrome all fall out here without needing a case each.
        if !row.contains("href=\"/torrent/") {
            continue;
        }
        if let Some(item) = to_row(row, host) {
            rows.push(item);
        }
    }
    Ok(rows)
}

/// Keep the rows answering the query (module doc: the engine ORs, so
/// the page has to be narrowed here). One word is left exactly as the
/// site answered it -- it matches on metadata too, and filtering a
/// trusted answer would delete rows rather than sharpen them.
pub fn filter_rows(items: &[TorrentItem], query: &str) -> Vec<TorrentItem> {
    let tokens: Vec<String> = query
        .split_whitespace()
        .map(|token| token.to_lowercase())
        .collect();
    if tokens.len() < 2 {
        return items.to_vec();
    }
    let need: Vec<&String> = tokens
        .iter()
        .filter(|token| !STOP.contains(&token.as_str()))
        .collect();
    // All tokens are stop words ("the of"): there is nothing to insist
    // on, so the page stands as answered rather than as empty.
    if need.is_empty() {
        return items.to_vec();
    }
    items
        .iter()
        .filter(|item| {
            let title = item.title.to_lowercase();
            need.iter().all(|word| title.contains(word.as_str()))
        })
        .cloned()
        .collect()
}

/// The rows a page shows: the filter's survivors, or -- when it would
/// take every row -- the page exactly as the site answered it, said
/// out loud in the log (decision with the user, module doc).
///
/// The alternative, an empty table, is a dead end rather than an
/// answer: the TUI loads another page only from a list it can scroll
/// (`needs_more`), and a page nobody can reach is worth nothing to
/// anybody.
pub fn filter_or_raw(raw: Vec<TorrentItem>, query: &str) -> Vec<TorrentItem> {
    let kept = filter_rows(&raw, query);
    if !kept.is_empty() || raw.is_empty() {
        return kept;
    }
    crate::log::log(
        "1337x",
        &format!(
            "no row of this page carries every word of \"{}\" -- showing the {} rows \
             the site answered with instead",
            query,
            raw.len()
        ),
    );
    raw
}

/// The rows fetched *inside* a selected category claim it; with no
/// selection they claim nothing. Live 26.09.2026, every row the
/// category paths answered with carried the matching `/sub/` link
/// (Movies 20 of 20, TV 20 of 20, Games 11 of 11, Anime 1 of 1 for
/// `matrix`; `/popular-games` 23 of 23), so claiming the category that
/// picked the URL is the site's own claim, not a guess. An unfiltered
/// row stays `None` -- the "all" view is the only one it honestly
/// belongs to, and a title is not a category (the honesty gap in
/// `source.rs`).
///
/// `if row.group.is_none()` rather than an overwrite: attribution the
/// parser ever learns from the row itself outranks the URL that
/// fetched it.
pub fn stamp_category(rows: Vec<TorrentItem>, category: Option<Group>) -> Vec<TorrentItem> {
    rows.into_iter()
        .map(|mut row| {
            if row.group.is_none() {
                row.group = category;
            }
            row
        })
        .collect()
}

/// The rows -> a search page, with `has_more` read off the server's
/// page and *then* the filter applied: a full page trimmed to three
/// rows is still a full page, and `next_offset` steps by the site's
/// pages, not by the survivors (the nnmclub dead-row lesson).
pub fn to_page(raw: Vec<TorrentItem>, query: &str, offset: usize) -> SearchPage {
    let has_more = raw.len() >= PAGE_SIZE;
    SearchPage {
        items: filter_or_raw(raw, query),
        has_more,
        next_offset: has_more.then_some(offset + PAGE_SIZE),
    }
}

/// Browse rows -> a page: `/home/` has no pager at all (live: 78 rows
/// in one page of sections), so it never promises a next one -- which
/// is also why a browse page must not go through [`to_page`], where
/// 78 >= 20 would promise one.
pub fn to_browse_page(items: Vec<TorrentItem>) -> SearchPage {
    SearchPage {
        items,
        has_more: false,
        next_offset: None,
    }
}

/// One page over the wire: one retry, then its body or why not. The
/// URL carries the host, so this reads a row's own page (the mirror
/// that just answered us) as happily as one [`first_ok`] picked.
async fn get(client: &reqwest::Client, url: &str) -> Result<String> {
    let options = FetchOptions {
        retries: 1,
        ..FetchOptions::default()
    };
    let response = fetch_resilient(url, || client.get(url), &options).await?;
    let status = response.status();
    ensure!(status.is_success(), "1337x could not read {} (HTTP {})", url, status);
    response
        .text()
        .await
        .context("1337x: reading the page failed")
}

/// The upload day from a detail page:
/// `<strong>Date uploaded</strong><span>Sep. 23rd  '26</span>`, live.
/// `None` when the field is not there or does not parse: a page we
/// could not read must leave the row's date empty, never fill it in.
pub fn date_from_detail(body: &str) -> Option<i64> {
    let at = body.find("Date uploaded")?;
    let open = at + body[at..].find("<span>")? + "<span>".len();
    let close = open + body[open..].find("</span>")?;
    let stamp = parse_upload_date(&strip_html(&body[open..close]));
    (stamp > 0).then_some(stamp)
}

/// The magnet on a detail page, as written there. Live the site writes
/// raw `&` (no `&amp;` anywhere on the page); the shared decoder runs
/// over it anyway, since one mirrored entity in a tracker URL would
/// otherwise be handed to TorrServer as part of the link.
pub fn magnet_from_detail(body: &str) -> Option<String> {
    let found = patterns()?.magnet.find(body)?;
    Some(unescape_entities(found.as_str()))
}

pub struct X1337xSearcher {
    client: reqwest::Client,
}

impl X1337xSearcher {
    /// The canonical name of the site, which is not the mirror the
    /// probes found answering (module doc). Nothing here navigates to
    /// it: `requires_browser` is false, so it is only ever the label a
    /// browser session would have used.
    pub const HOME_URL: &str = "https://1337x.to";

    pub fn new() -> Self {
        Self { client: browser_client() }
    }

    /// The rows the list dated, left alone; the ones it only gave a
    /// time to, sent to their own pages for `Date uploaded` (module
    /// doc). Four at a time, and a row whose page fails to answer
    /// keeps the empty date the list gave it rather than being handed
    /// somebody else's day.
    async fn with_detail_dates(&self, mut rows: Vec<TorrentItem>) -> Vec<TorrentItem> {
        let pending: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter(|(_, item)| item.added == 0)
            .map(|(index, _)| index)
            .collect();
        if pending.is_empty() {
            return rows;
        }

        let mut tasks = tokio::task::JoinSet::new();
        for index in pending {
            while tasks.len() >= DATE_FETCHES {
                let Some(result) = tasks.join_next().await else { break };
                apply_detail_date(&mut rows, result);
            }
            let client = self.client.clone();
            let url = rows[index].page_url.clone();
            tasks.spawn(async move {
                let body = get(&client, &url).await.ok();
                (index, body)
            });
        }
        while let Some(result) = tasks.join_next().await {
            apply_detail_date(&mut rows, result);
        }
        rows
    }
}

/// One detail page's answer, onto the row it belongs to. A fetch that
/// failed, a task that died, or a page without a readable date all
/// leave the row as the list gave it: an empty date, never a guess.
fn apply_detail_date(
    rows: &mut [TorrentItem],
    result: Result<(usize, Option<String>), tokio::task::JoinError>,
) {
    let Ok((index, body)) = result else { return };
    let Some(body) = body else { return };
    if let Some(stamp) = date_from_detail(&body) {
        rows[index].added = stamp;
        rows[index].date = format_date(stamp);
    }
}

#[async_trait]
impl Source for X1337xSearcher {
    fn id(&self) -> &'static str {
        "1337x"
    }

    fn label(&self) -> &'static str {
        "1337x"
    }

    fn groups(&self) -> &'static [Group] {
        GROUPS
    }

    fn home_url(&self) -> &'static str {
        Self::HOME_URL
    }

    fn requires_browser(&self) -> bool {
        // Live: three mirrors behind a Cloudflare JS challenge, one
        // answering 200 to the same plain client on every path -- so
        // the challenge is those mirrors' business, and the browser
        // layer (whose job is a *session*, not a solver) stays out.
        false
    }

    fn supports_browse(&self) -> bool {
        // Live: `/home/` answers with the same row markup as search,
        // 78 rows, one page.
        true
    }

    async fn ensure_logged_in(&self, _auth: &AuthContext, _log: &LogFn) -> Result<bool> {
        Ok(true)
    }

    async fn search(&self, req: &SearchRequest) -> Result<SearchPage> {
        let offset = req.offset;
        let query = req.query.trim().to_string();
        let browse = query.is_empty();
        let filter_query = query.clone();
        let client = self.client.clone();
        let category = req.category;

        // Every host gets the same attempt, and a host that answers
        // with something unparseable moves the search on to the next
        // one -- the definition of "this mirror is not serving 1337x"
        // that lets a challenge page be an error instead of an empty
        // result for every mirror at once.
        let rows = first_ok(HOSTS, move |host| {
            let client = client.clone();
            let query = query.clone();
            let host = host.to_string();
            async move {
                let url = if query.is_empty() {
                    browse_url(&host, category)
                } else {
                    search_url(&host, &query, offset, category)
                };
                let body = get(&client, &url).await?;
                parse_rows(&body, &host)
            }
        })
        .await?;
        // What the category path fetched, the rows claim (B6); the
        // plain search's rows keep their None.
        let rows = stamp_category(rows, category);
        // Rows the list left without a day get it from their own pages
        // here, before anything is shown (module doc); rows it dated
        // already cost nothing.
        let rows = self.with_detail_dates(rows).await;

        if browse {
            Ok(to_browse_page(rows))
        } else {
            Ok(to_page(rows, &filter_query, offset))
        }
    }

    async fn download_torrent(&self, url: &str) -> Result<Vec<u8>> {
        // The site serves no `.torrent` anywhere (live: a detail page
        // carries a magnet and no download link). Rows reach
        // `resolve_magnet` instead, and a caller that got here anyway
        // deserves the reason rather than a fetch of something that is
        // not a torrent.
        bail!("1337x has no .torrent to fetch ({}): its rows play over a magnet", url)
    }

    async fn resolve_magnet(&self, page_url: &str) -> Result<Option<String>> {
        let path = path_of(page_url).context("1337x: not a page URL")?;
        let client = self.client.clone();

        let body: String = first_ok(HOSTS, move |host| {
            let client = client.clone();
            let path = path.clone();
            let host = host.to_string();
            async move {
                let url = format!("https://{}{}", host, path);
                get(&client, &url).await
            }
        })
        .await?;

        magnet_from_detail(&body)
            .context("1337x: the torrent page carries no magnet link")
            .map(Some)
    }
}
