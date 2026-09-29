//! Offline tests for the 1337x parser, built from pages this network
//! actually served on 25.09.2026 (see the source module doc for the
//! probes behind each one). The fixture keeps the site's real spelling:
//! the `table-list` table, `coll-2 seeds` / `coll-3 leeches` /
//! `coll-date` / `coll-4 size mob-uploader` cells, the icon anchor that
//! precedes the title link, the uploader cell the parser does not
//! read -- and a *raw* `&` in a title, because the live pages carry no
//! entities at all and a decoder that treated `&H` as one would mangle
//! it. Rows are wrapped between tags for the column limit; only
//! whitespace moves, which `strip_html` collapses back out.
//!
//! The three shapes that must never be confused: a query with no
//! matches (200 + the table header and no rows), a page with no
//! results table at all (a challenge that answered 200, a moved
//! layout), and the featured links sitting *above* the table on
//! `/home/` -- to a user the first two both read "found nothing", and
//! the third would be a row pointing at somebody else's navigation.

use std::sync::Arc;

use doris::sources::format::format_date;
use doris::sources::models::TorrentItem;
use doris::sources::source::{AuthContext, Group, LogFn, Source};
use doris::sources::x1337x::{
    browse_url, date_from_detail, filter_rows, magnet_from_detail, parse_rows, search_url,
    stamp_category, to_browse_page, to_page, X1337xSearcher, PAGE_SIZE,
};

/// The mirror every live probe answered on.
const HOST: &str = "www.1337xx.to";

/// What `https://www.1337xx.to/search/dune/1/` served, trimmed to the
/// chrome and the rows the parser reads.
const SEARCH_PAGE: &str = r#"<!DOCTYPE html>
<html>
<head><title>Download dune Torrents | 1337x</title></head>
<body>
<div class="table-list-wrap">
<table class="table-list table table-responsive table-striped">
<thead>
<tr>
<th class="coll-1 name">name</th>
<th class="coll-2">se</th>
<th class="coll-3">le</th>
<th class="coll-date">time</th>
<th class="coll-4"><span class="size">size</span> <span class="info">info</span></th>
<th class="coll-5">uploader</th>
</tr>
</thead>
<tbody>
<tr>
<td class="coll-1 name"><a href="/sub/movies/HD/1/" class="icon"><i class="flaticon-hd"></i></a><a
        href="/torrent/5020920/Dune-2021-1080p-WEBRip-DD5-1-x264-SHITBOX/">
        Dune.2021.1080p.WEBRip.DD5.1.x264-SHITBOX</a></td>
<td class="coll-2 seeds">7312</td>
<td class="coll-3 leeches">899</td>
<td class="coll-date">Oct. 01st  '22</td>
<td class="coll-4 size mob-uploader">10.6 GB</td>
<td class="coll-5 uploader"><a href="/user/Cristie/">Cristie</a>
</td>
</tr>
<tr>
<td class="coll-1 name"><a href="/sub/movies/HD/1/" class="icon"><i class="flaticon-hd"></i></a><a
        href="/torrent/5026025/Dune-2021-1080p-WEBRip-x264/">
        Dune.2021.1080p.WEBRip.x264</a></td>
<td class="coll-2 seeds">5965</td>
<td class="coll-3 leeches">1140</td>
<td class="coll-date">May. 10th  '22</td>
<td class="coll-4 size mob-uploader">3 GB</td>
<td class="coll-5 uploader"><a href="/user/TheMorozko/">TheMorozko</a>
</td>
</tr>
<tr>
<td class="coll-1 name"><a href="/sub/movies/HEVC-x265/1/" class="icon">
        <i class="flaticon-hd"></i></a>
<a href="/torrent/5099999/Dune-Part-Two-2024-REPACK-2160p/">
        Dune.Part.Two.2024.REPACK.2160p.UPSCALE.WEB.HEVC.10Bit.AAC.2.0-R&H.mkv</a></td>
<td class="coll-2 seeds">0</td>
<td class="coll-3 leeches">4</td>
<td class="coll-date">Jun. 26th  '24</td>
<td class="coll-4 size mob-uploader">21.4 GB</td>
<td class="coll-5 uploader"><a href="/user/Somebody/">Somebody</a>
</td>
</tr>
</tbody>
</table>
</div>
</body>
</html>
"#;

/// A query nobody could match, live as `zzqqxxnothing123`: 200, the
/// table header, zero rows. An empty page, not an error.
const EMPTY_PAGE: &str = r#"<!DOCTYPE html>
<html>
<head><title>Download zzqqxxnothing123 Torrents | 1337x</title></head>
<body>
<div class="table-list-wrap">
<table class="table-list table table-responsive table-striped">
<thead>
<tr>
<th class="coll-1 name">name</th>
<th class="coll-2">se</th>
<th class="coll-3">le</th>
<th class="coll-date">time</th>
<th class="coll-4"><span class="size">size</span> <span class="info">info</span></th>
<th class="coll-5">uploader</th>
</tr>
</thead>
<tbody>
</tbody>
</table>
</div>
</body>
</html>
"#;

/// What the three challenged mirrors answer instead of markup (the live
/// 403 body's title), and what any 200-but-not-1337x page looks like
/// to this parser.
const NO_TABLE_PAGE: &str = r#"<!DOCTYPE html><html lang="en-US"><head>
<title>Just a moment...</title>
<meta name="robots" content="noindex,nofollow">
</head><body><h1>Performing security verification</h1></body></html>
"#;

/// The top of `/home/`: the "Most Popular" navigation -- real
/// `/torrent/` links, but *above* `table-list`, so not results -- and
/// then three rows of the first sections, live: the second one with
/// the seeders the site renders as `0` (4 of 201 cells seen that day),
/// the third one with a *time* where the date cell should be (11 of
/// the 78 rows on that page, every one of them a `Sep. 23rd '26`
/// upload once its own page was fetched).
const BROWSE_PAGE: &str = r#"<!DOCTYPE html>
<html>
<head><title>Download verified torrents: movies, music, games, software | 1337x</title></head>
<body>
<ul class="featured-list">
<li><a title="Legend 2026 1080p AMZN WEB-DL"
    href="/torrent/6725508/Legend-Of-The-White-Dragon-2026-1080p-AMZN/">
    Legend.Of.The.White.Dragon.2026</a></li>
<li><a title="Idiots 2026 1080p AMZN WEB-DL"
    href="/torrent/6725071/Idiots-2026-1080p-AMZN-WEB-DL/">
    Idiots.2026</a></li>
</ul>
<div class="table-list-wrap">
<table class="table-list table table-responsive table-striped">
<thead>
<tr>
<th class="coll-1 name">name</th>
<th class="coll-2">se</th>
<th class="coll-3">le</th>
<th class="coll-date">time</th>
<th class="coll-4"><span class="size">size</span> <span class="info">info</span></th>
<th class="coll-5">uploader</th>
</tr>
</thead>
<tbody>
<tr>
<td class="coll-1 name"><a href="/sub/tv/HEVC-x265/1/" class="icon">
        <i class="flaticon-hd"></i></a>
<a href="/torrent/6721684/Reacher-S04E08-1080p-WEBRip-10Bit-DDP5-1-x265-NeoNoir/">
        Reacher.S04E08.1080p.WEBRip.10Bit.DDP5.1.x265-NeoNoir</a></td>
<td class="coll-2 seeds">2665</td>
<td class="coll-3 leeches">3246</td>
<td class="coll-date">Sep. 16th  '26</td>
<td class="coll-4 size mob-uploader">949.8 MB</td>
<td class="coll-5 uploader"><a href="/user/NeoNoir/">NeoNoir</a>
</td>
</tr>
<tr>
<td class="coll-1 name"><a href="/sub/tv/HD/1/" class="icon">
        <i class="flaticon-hd"></i></a>
<a href="/torrent/6724782/Ice-Cream-Man-2026-1080p-AMZN/">
        Ice.Cream.Man.2026.1080p.AMZN.WEB-DL.H264-KyoGo</a></td>
<td class="coll-2 seeds">0</td>
<td class="coll-3 leeches">12</td>
<td class="coll-date">Sep. 20th  '26</td>
<td class="coll-4 size mob-uploader">8.1 GB</td>
<td class="coll-5 uploader"><a href="/user/KyoGo/">KyoGo</a>
</td>
</tr>
<tr>
<td class="coll-1 name"><a href="/sub/movies/HEVC-x265/1/" class="icon">
        <i class="flaticon-hd"></i></a>
<a href="/torrent/6725454/Super-Troopers-3-2026-1080p-WEB-DL-HEVC-x265-5-1-BONE/">
        Super.Troopers.3.2026.1080p.WEB-DL.HEVC.x265.5.1-BONE</a></td>
<td class="coll-2 seeds">378</td>
<td class="coll-3 leeches">100</td>
<td class="coll-date">03:15am</td>
<td class="coll-4 size mob-uploader">1.7 GB</td>
<td class="coll-5 uploader"><a href="/user/bone111/">bone111</a>
</td>
</tr>
</tbody>
</table>
</div>
</body>
</html>
"#;

/// A minimal row, for the filter and cursor tests.
fn row(title: &str) -> TorrentItem {
    TorrentItem {
        title: title.to_string(),
        ..Default::default()
    }
}

#[test]
fn test_a_row_carries_every_column_from_the_search_page_alone() {
    let rows = parse_rows(SEARCH_PAGE, HOST).expect("the live search page parses");
    assert_eq!(
        rows.len(),
        3,
        "the header row has no torrent link and is not one"
    );

    let first = &rows[0];
    assert_eq!(first.title, "Dune.2021.1080p.WEBRip.DD5.1.x264-SHITBOX");
    assert_eq!(
        first.page_url,
        "https://www.1337xx.to/torrent/5020920/Dune-2021-1080p-WEBRip-DD5-1-x264-SHITBOX/",
        "the row's own page, on the mirror that served it"
    );
    assert_eq!(first.seeds, "7312");
    assert_eq!(first.seeds_n, 7312);
    assert_eq!(first.leechers, 899);
    assert_eq!(
        first.size, "10.6 GB",
        "the site's own display string, kept as written"
    );
    // Latin units are SI in this codebase (`GB` = 1e9), a torio
    // compatibility the row's display string makes visible for the
    // first time -- nnmclub handed over bytes and never had a unit.
    assert_eq!(first.size_bytes, 10_600_000_000);
    // The date comes out of the row itself, where torio pays for a
    // detail page to read the same value.
    assert_eq!(format_date(first.added), "2022-10-01");
    assert_eq!(first.date, "2022-10-01");
    // Nothing to play yet: the link lives on the row's own page.
    assert_eq!(first.magnet, None);
    assert!(first.download_url.is_empty());
    assert!(first.info_hash.is_empty());
    assert_eq!(first.source, "1337x");
    assert_eq!(first.group, None, "groups are declared, never guessed (B6)");
}

#[test]
fn test_a_raw_ampersand_in_a_title_survives_decoding() {
    let rows = parse_rows(SEARCH_PAGE, HOST).expect("the live search page parses");
    assert_eq!(
        rows[2].title, "Dune.Part.Two.2024.REPACK.2160p.UPSCALE.WEB.HEVC.10Bit.AAC.2.0-R&H.mkv",
        "live titles carry raw `&`, and no entity table touches it"
    );
    // ...while the icon anchor in front of the link is still not part
    // of the title.
    assert!(!rows[2].title.contains("flaticon"));
}

#[test]
fn test_a_row_with_zero_seeders_is_still_a_row() {
    let rows = parse_rows(BROWSE_PAGE, HOST).expect("the live /home/ page parses");
    let dead = &rows[1];
    assert_eq!(
        dead.title,
        "Ice.Cream.Man.2026.1080p.AMZN.WEB-DL.H264-KyoGo"
    );
    assert_eq!(
        dead.seeds_n, 0,
        "4 of 201 live seed cells read 0; a dead row keeps its row"
    );
    assert_eq!(dead.leechers, 12);
}

#[test]
fn test_a_freshest_row_keeps_its_row_and_waits_for_its_day() {
    let rows = parse_rows(BROWSE_PAGE, HOST).expect("the live /home/ page parses");
    let fresh = &rows[2];
    assert_eq!(
        fresh.title,
        "Super.Troopers.3.2026.1080p.WEB-DL.HEVC.x265.5.1-BONE"
    );
    assert_eq!(
        fresh.seeds_n, 378,
        "a time in the date cell costs the row nothing else"
    );
    assert_eq!(fresh.size, "1.7 GB");
    // The list wrote a time, so the row has no day -- and *that* is
    // what tells `search` to go ask this row's own page for one. It is
    // not an error, and it is not today either: the live page behind
    // this very row said `Sep. 23rd '26`, two days before the probe.
    assert_eq!(fresh.added, 0, "no day was claimed by the list");
    assert_eq!(fresh.date, "", "and nothing is printed until one arrives");
    assert!(
        fresh.page_url.contains("/torrent/6725454/"),
        "the page to ask is already on the row: {}",
        fresh.page_url
    );
}

#[test]
fn test_the_upload_day_comes_from_the_rows_own_page() {
    // Live detail page of the row above: the day the list would not
    // spell out, in the same `Mon. DDth  'YY` the list uses for older
    // rows.
    let detail = r#"<ul class="list"><li><strong>Downloads</strong><span>630</span></li>
<li><strong>Last checked</strong><span>Sep. 23rd '26</span></li>
<li><strong>Date uploaded</strong><span>Sep. 23rd  '26</span> </li>
<li><strong>Seeders</strong> <span class="seeds">378</span></li></ul>"#;
    let stamp = date_from_detail(detail).expect("the page carries the field");
    assert_eq!(format_date(stamp), "2026-09-23");
}

#[test]
fn test_a_detail_page_without_a_readable_day_adds_no_day() {
    assert_eq!(
        date_from_detail("<p>A page that is not a torrent page</p>"),
        None,
        "an unreadable page leaves the row's date empty"
    );
    // `Last checked` is the neighbour field; reading it would put the
    // wrong day on the row.
    let neighbour = "<li><strong>Last checked</strong><span>Sep. 23rd '26</span></li>";
    assert_eq!(date_from_detail(neighbour), None);
    // A time where the date should be (should the site ever move the
    // format) parses to nothing rather than to 1970-01-01.
    let time = "<strong>Date uploaded</strong><span>03:15am</span>";
    assert_eq!(date_from_detail(time), None);
}

#[test]
fn test_a_miss_is_an_empty_page_not_an_error() {
    let rows = parse_rows(EMPTY_PAGE, HOST).expect("a page with no matches is still a page");
    assert!(rows.is_empty());
    let page = to_page(rows, "zzqqxxnothing123", 0);
    assert!(page.items.is_empty(), "and no fallback invents rows for it");
    assert!(!page.has_more);
    assert_eq!(page.next_offset, None);
}

#[test]
fn test_a_page_without_the_results_table_is_an_error() {
    let err = parse_rows(NO_TABLE_PAGE, HOST).expect_err("no table means we are not on 1337x");
    let message = err.to_string();
    assert!(
        message.contains("no results table"),
        "the error must say what was missing, got: {}",
        message
    );
    assert!(
        message.contains("blocked") || message.contains("moved"),
        "and name the candidate reasons, got: {}",
        message
    );
}

#[test]
fn test_the_featured_links_above_the_table_are_not_rows() {
    let rows = parse_rows(BROWSE_PAGE, HOST).expect("the live /home/ page parses");
    assert_eq!(
        rows.len(),
        3,
        "the two nav links above `table-list` leaked in"
    );
    assert!(rows
        .iter()
        .all(|r| r.title != "Legend.Of.The.White.Dragon.2026"));
    assert_eq!(
        rows[0].title,
        "Reacher.S04E08.1080p.WEBRip.10Bit.DDP5.1.x265-NeoNoir"
    );
}

#[test]
fn test_one_word_is_taken_exactly_as_the_site_answered_it() {
    // Live: `frieren` alone came back 20 of 20 -- but the engine also
    // matches on metadata, so a row carrying the word nowhere in its
    // title can still be a row the site vouched for. Filtering a
    // one-word answer would delete those (8 of the 20 rows for
    // `frieren crack` matched through metadata that day).
    let items = vec![row("Hogwarts.Legacy.Deluxe.Edition-EMPRESS")];
    let kept = filter_rows(&items, "crack");
    assert_eq!(
        kept.len(),
        1,
        "a trusted answer is not re-checked against its own title"
    );
}

#[test]
fn test_two_words_keep_only_the_rows_that_carry_both() {
    // Live: `frieren crack` returned 20 rows, 12 with "crack" in the
    // title, none with "frieren" -- and 8 with neither word.
    let items = vec![
        row("Frieren.Crack.RELEASE-GRP"),
        row("Some.Crack.To.Go"),
        row("Frieren.Beyond.Journey.End"),
        row("Unrelated entirely"),
    ];
    let kept = filter_rows(&items, "frieren crack");
    let titles: Vec<&str> = kept.iter().map(|i| i.title.as_str()).collect();
    assert_eq!(titles, vec!["Frieren.Crack.RELEASE-GRP"]);
}

#[test]
fn test_stop_words_are_not_insisted_on() {
    let items = vec![
        row("The.Witcher.S03.1080p"),
        // No `the` anywhere -- kept all the same, which is the whole
        // point: a stop word is not part of what the row must carry.
        row("Witcher.S01.720p"),
        row("Totally.Unrelated"),
    ];
    let kept = filter_rows(&items, "the witcher");
    let titles: Vec<&str> = kept.iter().map(|i| i.title.as_str()).collect();
    assert_eq!(
        titles,
        vec!["The.Witcher.S03.1080p", "Witcher.S01.720p"],
        "`the` matches nothing of its own; only `witcher` is insisted on"
    );
}

#[test]
fn test_a_page_nothing_answers_comes_back_empty() {
    // 20 rows from the server, none of which carries both words --
    // live, that is `dune 1080p` on five pages in a row and `frieren
    // 2026` on twelve. The page is NOT handed back raw: that fallback
    // is where the "Games" tab full of Sims/GTA RELOADED repacks came
    // from, rows the query never asked for. An empty table is the
    // honest answer, and it is no longer a dead end -- `needs_more` no
    // longer needs rows to scroll, so Down fetches page 2.
    let raw: Vec<TorrentItem> = (0..PAGE_SIZE).map(|i| row(&format!("Row {}", i))).collect();
    let page = to_page(raw, "frieren crack", 40);
    assert!(
        page.items.is_empty(),
        "no row answered, so no row is shown: {:?}",
        page.items.iter().map(|i| &i.title).collect::<Vec<_>>()
    );
    assert!(page.has_more, "a full page is still a full page");
    assert_eq!(
        page.next_offset,
        Some(60),
        "offset + PAGE_SIZE, not offset + survivors"
    );
}

#[test]
fn test_a_page_some_rows_answer_keeps_only_those() {
    // The other half of the same decision, live as `witcher s03`:
    // 4 of the 20 rows on the page carry both words, so the filter
    // does its job and the cursor keeps walking the site's grid.
    let mut raw: Vec<TorrentItem> = (0..PAGE_SIZE).map(|i| row(&format!("Row {}", i))).collect();
    for index in [3usize, 7, 11, 18] {
        raw[index].title = format!("The.Witcher.S03.Part{}.720p", index);
    }
    let page = to_page(raw, "witcher s03", 0);
    assert_eq!(page.items.len(), 4, "only the rows that answer both words");
    assert!(page.has_more, "the other 16 rows are still the site's page");
    assert_eq!(page.next_offset, Some(PAGE_SIZE));
}

#[test]
fn test_a_short_page_promises_nothing() {
    let raw: Vec<TorrentItem> = (0..7).map(|i| row(&format!("Row {}", i))).collect();
    let page = to_page(raw, "row", 20);
    assert!(!page.has_more);
    assert_eq!(page.next_offset, None);
}

#[test]
fn test_browse_never_promises_a_second_page() {
    // `/home/` answered 78 rows on one page with no pager -- so it
    // must not go through `to_page`, where 78 >= 20 would promise a
    // second fetch of the same URL forever.
    let rows: Vec<TorrentItem> = (0..78).map(|i| row(&format!("Row {}", i))).collect();
    let page = to_browse_page(rows);
    assert_eq!(page.items.len(), 78);
    assert!(!page.has_more);
    assert_eq!(page.next_offset, None);
}

#[test]
fn test_the_cursor_is_the_sites_page_number() {
    // `/search/<q>/<page>/` counts pages from 1, offsets count rows
    // from 0, and the words go in the path the way torio spells them.
    assert_eq!(
        search_url(HOST, "dune 1080p", 0, None),
        "https://www.1337xx.to/search/dune+1080p/1/"
    );
    assert_eq!(
        search_url(HOST, "  dune  ", 20, None),
        "https://www.1337xx.to/search/dune/2/",
        "and the query is trimmed before it is built"
    );
    assert_eq!(
        search_url(HOST, "a/b", 40, None),
        "https://www.1337xx.to/search/a%2Fb/3/",
        "slashes in a query stay in the path, not in the routing"
    );
    assert_eq!(browse_url(HOST, None), "https://www.1337xx.to/home/");
    assert_eq!(
        search_url(HOST, "", 0, None),
        browse_url(HOST, None),
        "an empty query is browse, as everywhere else"
    );
}

/// B6's slot, in the spelling the site answers to (live 26.09.2026):
/// the group's own label names the category path, the *same* label
/// names browse's `/popular-<label>/`, and both keep the page cursor
/// the plain search uses. The labels are `Group::label` -- the words
/// in the category row -- rather than a private spelling that could
/// drift away from what the tabs promise.
#[test]
fn test_a_category_picks_the_sites_category_paths() {
    let expected = [
        (Group::Movies, "Movies", "popular-movies"),
        (Group::TV, "TV", "popular-tv"),
        (Group::Games, "Games", "popular-games"),
        (Group::Anime, "Anime", "popular-anime"),
    ];
    for (group, label, popular) in expected {
        assert_eq!(
            search_url(HOST, "dune 1080p", 20, Some(group)),
            format!(
                "https://www.1337xx.to/category-search/dune+1080p/{}/2/",
                label
            ),
            "the site spells {:?} as {} in the path",
            group,
            label
        );
        assert_eq!(
            browse_url(HOST, Some(group)),
            format!("https://www.1337xx.to/{}", popular),
            "browse is trimmed by the same selection"
        );
    }
    assert_eq!(
        search_url(HOST, "", 0, Some(Group::Games)),
        browse_url(HOST, Some(Group::Games)),
        "an empty query is still browse, now the category's section"
    );
}

/// The rows fetched inside a selected category claim it, and the
/// unfiltered answer claims nothing -- with the parser's own
/// attribution, if it ever produces one, left alone.
#[test]
fn test_rows_claim_the_category_that_fetched_them() {
    let fresh = || vec![row("Some Torrent")];
    assert_eq!(
        stamp_category(fresh(), Some(Group::Games))[0].group,
        Some(Group::Games)
    );
    assert_eq!(stamp_category(fresh(), None)[0].group, None);

    let mut attributed = fresh();
    attributed[0].group = Some(Group::Movies);
    assert_eq!(
        stamp_category(attributed, Some(Group::TV))[0].group,
        Some(Group::Movies),
        "what the row says outranks the URL that fetched it"
    );
}

#[test]
fn test_the_magnet_comes_off_a_detail_page() {
    // Live detail page for Dune 2021: the magnet with its tracker
    // list, written with a raw `&`, inside an anchor.
    let magnet = concat!(
        "magnet:?xt=urn:btih:4D165EAE3C3F1C8FCD467E7A9B21ADD164D6E969",
        "&dn=Dune.2021.1080p.WEBRip.DD5.1.x264-SHITBOX",
        "&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce",
    );
    let detail = format!(
        "<div class=\"buttons\"><a href=\"{}\" rel=\"nofollow\">Download magnet</a></div>",
        magnet
    );
    let found = magnet_from_detail(&detail).expect("the page carries a magnet");
    assert_eq!(found, magnet, "raw `&` passes through untouched");

    // An entity in the link would be a mirror's doing; the decoder
    // turns it into the link TorrServer needs rather than handing over
    // `&amp;tr=`.
    let escaped = "magnet:?xt=urn:btih:abc&dn=x&amp;tr=udp%3A%2F%2Ft%3A1";
    let detail = format!("<a href=\"{}\">magnet</a>", escaped);
    let found = magnet_from_detail(&detail).expect("the page carries a magnet");
    assert!(found.contains("&tr=udp%3A%2F%2Ft%3A1"), "{}", found);
    assert!(!found.contains("&amp;"));

    assert!(
        magnet_from_detail("<p>No link here</p>").is_none(),
        "a page with no magnet says so instead of inventing one"
    );
}

#[test]
fn test_the_source_declares_what_the_probes_found() {
    let source = X1337xSearcher::new();
    assert_eq!(source.id(), "1337x");
    assert_eq!(source.label(), "1337x");
    assert_eq!(source.home_url(), "https://1337x.to");
    // Three of torio's four mirrors answered 403 with a Cloudflare JS
    // challenge; `www.1337xx.to` answered 200 to this same client on
    // every path. A challenge on *those mirrors* is not a session this
    // source needs.
    assert!(!source.requires_browser());
    // `/home/` answered 78 rows in the search page's own markup.
    assert!(source.supports_browse());
    // The four categories the site's nine map onto a `Group`; Music,
    // Documentaries, Applications, Other and XXX have none to go to.
    assert_eq!(
        source.groups(),
        &[Group::Movies, Group::TV, Group::Games, Group::Anime]
    );
}

#[tokio::test]
async fn test_the_source_serves_no_file_and_needs_no_login() {
    let source = X1337xSearcher::new();
    let err = source
        .download_torrent("https://www.1337xx.to/torrent/5020920/x/")
        .await
        .expect_err("an aggregator has no file to hand over");
    assert!(
        err.to_string().contains("no .torrent"),
        "and says why, got: {}",
        err
    );

    // No login wall appeared on any page fetched that day.
    let log: LogFn = Arc::new(|_| {});
    let logged_in = source
        .ensure_logged_in(&AuthContext::default(), &log)
        .await
        .expect("the mirror answered without a login");
    assert!(logged_in);
}
