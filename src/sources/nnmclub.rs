//! NNM-Club over its tracker HTML, parsed from the markup as it came back live on 25.09.2026:
//! windows-1251 in both the header and the `<meta>`, cloudflare-fronted, but a browser UA alone
//! was enough -- no JS challenge, no login, no cookie jar. Five things the live pages
//! established, which the code therefore does: - **One request per search.** Each row carries
//! its title, a `download.php?id=` link, raw bytes, seeders, leechers and its own timestamp, so
//! no row needs a second request; `download.php?id=` was checked live to answer `302 ->
//! application/x-bittorrent` with a real bencoded file.

use std::sync::OnceLock;

use anyhow::{bail, ensure, Result};
use async_trait::async_trait;
use regex::Regex;

use super::format::{format_bytes, format_date};
use super::models::TorrentItem;
use super::net::{browser_client, fetch_resilient, FetchOptions};
use super::source::{AuthContext, Group, LogFn, SearchPage, SearchRequest, Source};

/// The groups this source declares -- exactly the groups the forum
/// table below spans, asserted by a test against `GROUP_FORUMS` so the
/// two cannot drift apart.
const GROUPS: &[Group] = &[Group::Movies, Group::TV, Group::Games, Group::Anime];

/// Rows the site puts on one page, live on both a broad query and browse (50 each, page 2
/// disjoint).
pub const PAGE_SIZE: usize = 50;

/// `f=-1`: every forum, which is the id the site's own navigation
/// links as "all".
const ALL_FORUMS: &str = "f=-1";

/// Every forum one group asks the tracker for -- leaf ids read off the live `<select
/// name="f[]">` on `tracker.php` (698 options in 18 optgroups).
pub const GROUP_FORUMS: [(Group, &[i32]); 4] = [
    (
        Group::Movies,
        &[
            216, 270, 218, 219, 954, 217, 1293, 1298, 318, 320, 677, 1177, 319, 678, 885, 908,
            1310, 909, 910, 911, 912, 220, 221, 222, 882, 889, 224, 225, 226, 227, 1296, 891, 1299,
            682, 694, 884, 1211, 693, 913, 228, 1150, 1311, 1313, 1312, 256, 257, 258, 883, 955,
            905, 271, 1210, 264, 265, 272, 1262, 266, 1294, 724, 731, 1345, 733, 1346, 1329, 1330,
            1331, 1332, 1340, 658, 890, 1336, 1337, 1338, 1339, 660, 232, 91, 668, 892, 1143,
        ],
    ),
    (
        Group::TV,
        &[
            1219, 1221, 1220, 722, 768, 1344, 779, 1288, 787, 1141, 777, 786, 776, 785, 775, 1265,
            1242, 1140, 782, 773, 1142, 772, 771, 783, 1144, 804, 1290, 1300, 784, 774, 922, 770,
            1320, 780, 781, 1322, 769, 799, 800, 791, 793, 794, 796, 795, 713, 706, 577, 894, 578,
            580, 579, 953, 581, 806, 714, 761, 809, 924, 812, 576, 590, 591, 588, 589, 598, 652,
            599, 959, 956, 597, 593, 594, 819, 595, 587, 584, 586, 585, 600, 596, 1295, 614, 603,
            1308, 1309, 1206, 1194, 1062, 974, 609, 1263, 951, 975, 608, 607, 606, 750, 605, 604,
            950, 610, 613, 612, 653, 654, 611, 656, 669, 802, 400,
        ],
    ),
    (
        Group::Games,
        &[
            410, 411, 412, 1008, 415, 746, 428, 1009, 413, 414, 1010, 1012, 1014, 416, 1013, 1015,
            268, 1016, 1041, 1018, 1017, 972, 971, 970, 969, 968, 1146, 418, 1061, 1060, 1059,
            1058, 1057, 1056, 1054, 1053, 1052, 1051, 1050, 1049, 1048, 1047, 1046, 1045, 1044,
            382, 390, 387, 388, 1264, 1318, 385, 386, 848, 1321, 383, 384, 1292, 389, 391, 93,
        ],
    ),
    (
        Group::Anime,
        &[
            615, 616, 617, 648, 619, 620, 623, 622, 621, 632, 624, 627, 626, 625, 644, 628, 635,
            634, 638, 646, 645, 639, 640, 169,
        ],
    ),
];

/// The group one forum belongs to -- the row's own `tracker.php?f=<id>` cell looked up in
/// `GROUP_FORUMS`.
pub fn group_for_forum(forum: i32) -> Option<Group> {
    GROUP_FORUMS
        .iter()
        .find(|(_, ids)| ids.contains(&forum))
        .map(|(group, _)| *group)
}

/// The forum selector for one request.
fn forum_params(category: Option<Group>) -> String {
    super::source::forum_params(&GROUP_FORUMS, category, ALL_FORUMS)
}

/// Site root; every path below lives under `/forum/`.
pub const FORUM: &str = "https://nnmclub.to/forum/";

/// The page the query URL was verified against: results ordered by the tracker, tokens matched
/// server-side (all of a row's words present).
pub fn search_url(query: &str, offset: usize, category: Option<Group>) -> String {
    let query = query.trim();
    if query.is_empty() {
        return browse_url(offset, category);
    }
    with_offset(
        format!(
            "{}tracker.php?{}&nm={}",
            FORUM,
            forum_params(category),
            urlencoding::encode(query)
        ),
        offset,
    )
}

/// The browse URL (live: 50 rows, `o=2&sd=desc` = newest topic first),
/// the empty-query entry point behind `supports_browse`.
pub fn browse_url(offset: usize, category: Option<Group>) -> String {
    with_offset(
        format!(
            "{}tracker.php?{}&o=2&sd=desc",
            FORUM,
            forum_params(category)
        ),
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

/// The regexes the parser needs, built once.
struct Patterns {
    /// The results table, and only that one.
    table: Regex,
    /// One row inside it.
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
            table: Regex::new(r#"<table class="forumline tablesorter"[^>]*>([\s\S]*?)</table>"#)
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

/// The topic id and the title next to it.
fn strip_html(input: &str) -> String {
    match patterns() {
        Some(p) => super::format::strip_html(input, &p.tags),
        None => input.to_string(),
    }
}

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

/// The row's forum id, read off the `tracker.php?f=<id>` cell the site fills with the section's
/// Russian name -- live present in every row of both a query and browse.
fn forum_of(row: &str) -> Option<i32> {
    let marker = "tracker.php?f=";
    let at = row.find(marker)?;
    let id: String = row[at + marker.len()..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    id.parse().ok()
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

/// Raw bytes and the topic timestamp, both of which the site wraps in `<u>`.
fn size_and_added(row: &str, patterns: &Patterns) -> (u64, i64) {
    let added_cell = cell_span(row, "title=\"Торрент-файл добавлен\"");
    let mut size = 0_u64;
    let mut added = 0_i64;
    for caps in patterns.u_digits.captures_iter(row) {
        let Some(value) = caps.get(1) else { continue };
        let number = value.as_str().parse::<u64>().unwrap_or(0);
        let inside_added =
            added_cell.is_some_and(|(start, end)| value.start() >= start && value.start() < end);
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

/// One result row -> its row, or `None` when the row cannot become one: no topic id, no
/// download link, no title.
fn to_row(row: &str, patterns: &Patterns) -> Option<TorrentItem> {
    let (topic_id, title) = topic(row)?;
    let download_id = download_id(row)?;
    let (size_bytes, added) = size_and_added(row, patterns);
    // The site's *classes*, not its `title=` hints -- the difference
    // was found live and cost real rows: a torrent nobody seeds swaps
    // `title="Seeders"` for `title=" Last seen: 29-03-2020"` and
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
        // The row's own forum, so a tab switch can filter rows the
        // tracker already returned; `None` for sections outside
        // the four groups, which is what keeps "all" honest.
        group: forum_of(row).and_then(group_for_forum),
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

/// The results table -> rows.
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

impl Default for NnmclubSearcher {
    fn default() -> Self {
        Self::new()
    }
}

impl NnmclubSearcher {
    pub const HOME_URL: &str = "https://nnmclub.to";

    pub fn new() -> Self {
        Self {
            client: browser_client(),
        }
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
        // The four groups `GROUP_FORUMS` spans -- declared once, and
        // asserted against the forum table by a test.
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
        let url = search_url(&req.query, offset, req.category);
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
        // handed to TorrServer as a.torrent file.
        let response =
            fetch_resilient(url, || self.client.get(url), &FetchOptions::default()).await?;
        let status = response.status();
        if !status.is_success() {
            anyhow::bail!("nnmclub download {} answered HTTP {}", url, status);
        }
        Ok(response.bytes().await?.to_vec())
    }
}
