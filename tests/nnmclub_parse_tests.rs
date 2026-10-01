//! Offline tests for the nnmclub HTML parser, built from the pages the host actually served
//! this network on 25.09.2026 -- including the two shapes that must never be confused: a query
//! with no matches (200 + `Не найдено` inside the results table) and a page where the results
//! table is missing altogether (a challenge, a moved layout, a login wall). To a user both read
//! as "found nothing"; only one of them is.

use std::sync::Arc;

use doris::sources::format::unescape_entities;
use doris::sources::models::TorrentItem;
use doris::sources::nnmclub::{
    browse_url, group_for_forum, parse_rows, search_url, to_page, NnmclubSearcher, GROUP_FORUMS,
    PAGE_SIZE,
};
use doris::sources::source::{AuthContext, Group, LogFn, Source};

/// One results table, four result rows, and the header/footer the
/// site wraps them in.
const TABLE: &str = r#"<table class="forumline tablesorter" cellspacing="1">
<tr><th>Тема</th><th>Размер</th></tr>
<tr>
 <td align="center"><img src="/i/icon_minipost.gif" alt="1"></td>
 <td align="center"><a class="gen" href="tracker.php?f=905">Театр</a></td>
 <td title="" class="genmed"><a class="genmed topictitle"
    href="viewtopic.php?t=32097"><b>№ 13 / Out of oder</b></a></td>
 <td align="center"><a class="genmed" href="tracker.php?pid=57179">Turbot</a></td>
 <td align="center" nowrap="nowrap"><a href="download.php?id=31201"
    rel="nofollow" class="genmed">[ <b>DL</b> ]</a></td>
 <td align="center" title=" 20.6&nbsp;KB/s " class="gensmall">
    <u>1175605248</u> 1.09 GB</td>
 <td align="center" title="Seeders" class="seedmed"><b>1</b></td>
 <td align="center" title="Leechers" class="leechmed"><b>0</b></td>
 <td align="center" title="6122 Просмотров" class="gensmall">12</td>
 <td align="center" nowrap="nowrap" title="Торрент-файл добавлен"
    class="gensmall"><u>1722797089</u> 04-08-2024<br>21:44</td>
</tr>
<tr>
 <td align="center"><a class="gen" href="tracker.php?f=906">Аниме</a></td>
 <td class="genmed"><a class="genmed topictitle"
    href="viewtopic.php?t=1849851"><b>Frieren: Beyond
    Journey&#039;s End (2026)</b></a></td>
 <td align="center"><a class="genmed" href="tracker.php?pid=11">MoscowGolem</a></td>
 <td align="center" nowrap="nowrap"><a href="download.php?id=1403094"
    class="genmed">[ <b>DL</b> ]</a></td>
 <td align="center" title=" 8.42&nbsp;MB/s " class="gensmall">
    <u>16902730601</u> 15.7 GB</td>
 <td align="center" title="Seeders" class="seedmed"><b>39</b></td>
 <td align="center" title="Leechers" class="leechmed"><b>3</b></td>
 <td align="center" title="15 Просмотров" class="gensmall">0</td>
 <td align="center" nowrap="nowrap" title="Торрент-файл добавлен"
    class="gensmall"><u>1775392714</u> 05-04-2026<br>15:38</td>
</tr>
<tr>
 <td class="genmed"><a class="genmed topictitle"
    href="viewtopic.php?t=1900000"><b>Row the site gave no date to</b></a></td>
 <td align="center" nowrap="nowrap"><a href="download.php?id=1500000"
    class="genmed">[ <b>DL</b> ]</a></td>
 <td align="center" title=" 1.4&nbsp;MB/s " class="gensmall">
    <u>1500000000</u> 1.40 GB</td>
 <td align="center" title="Seeders" class="seedmed"><b>7</b></td>
 <td align="center" title="Leechers" class="leechmed"><b>2</b></td>
</tr>
<tr>
 <td class="genmed"><a class="genmed topictitle"
    href="viewtopic.php?t=1700000"><b>Dead torrent nobody seeds</b></a></td>
 <td align="center" nowrap="nowrap"><a href="download.php?id=1700000"
    class="genmed">[ <b>DL</b> ]</a></td>
 <td align="center" title="" nowrap="nowrap" class="gensmall">
    <u>700000000</u> 668 MB</td>
 <td align="center" title=" Last seen:   29-03-2020" class="seedmed"></td>
 <td align="center" title="Leechers" class="leechmed"><b>0</b></td>
 <td align="center" nowrap="nowrap" title="Торрент-файл добавлен"
    class="gensmall"><u>1600000000</u> 13-09-2020<br>10:00</td>
</tr>
</table>
"#;

/// The live zero-match page: same table, `Не найдено` instead of rows.
const NO_MATCHES: &str = r#"<table class="forumline tablesorter" cellspacing="1">
<tr><td colspan="11" height="32"><span class="gen">Не найдено</span></td></tr>
</table>
"#;

fn rows() -> Vec<TorrentItem> {
    parse_rows(TABLE).expect("the captured results table parses")
}

#[test]
fn test_every_field_the_live_row_carried_is_on_the_item() {
    let rows = rows();
    assert_eq!(rows.len(), 4, "header, footer and cells are not rows");

    let first = &rows[0];
    assert_eq!(first.title, "№ 13 / Out of oder");
    assert_eq!(first.source, "nnmclub");
    // The site's own rendering of these bytes is "1.09 GB"; ours
    assert_eq!(first.size_bytes, 1_175_605_248);
    assert_eq!(first.size, "1.09 GB");
    assert_eq!(first.seeds, "1");
    assert_eq!(first.seeds_n, 1);
    assert_eq!(first.leechers, 0);
    assert_eq!(first.added, 1_722_797_089);
    assert_eq!(first.date, "2024-08-04");
    assert_eq!(
        first.download_url,
        "https://nnmclub.to/forum/download.php?id=31201"
    );
    assert_eq!(
        first.page_url,
        "https://nnmclub.to/forum/viewtopic.php?t=32097"
    );
    // The decision behind wave 3: no detail fan-out, so the row carries
    assert_eq!(first.magnet, None);
    assert_eq!(first.info_hash, "");
    // B6: the row's own forum cell decides. This row's cell says
    assert_eq!(first.group, Some(Group::Movies));
}

#[test]
fn test_a_title_keeps_its_entities_out_of_the_way() {
    let rows = rows();
    assert_eq!(
        rows[1].title, "Frieren: Beyond Journey's End (2026)",
        "&#039; decoded, tags and the whitespace they break are collapsed"
    );
    assert_eq!(rows[1].size_bytes, 16_902_730_601);
    assert_eq!(rows[1].size, "15.74 GB");
    assert_eq!(rows[1].seeds_n, 39);
    assert_eq!(rows[1].leechers, 3);
    assert_eq!(rows[1].added, 1_775_392_714);
    assert_eq!(rows[1].date, "2026-04-05", "the site's 05-04-2026, as UTC");

    // The decoder this shares with nyaa stays one copy (wave 3 commit).
    assert_eq!(unescape_entities("a &amp; b &#039;c&#039;"), "a & b 'c'");
}

#[test]
fn test_the_two_u_values_cannot_swap_and_one_may_be_absent() {
    let rows = rows();
    // Row 3 has a size in `<u>` but no added cell at all: the size is
    let undated = &rows[2];
    assert_eq!(undated.size_bytes, 1_500_000_000);
    assert_eq!(undated.size, "1.40 GB");
    assert_eq!(undated.added, 0);
    assert_eq!(undated.date, "", "unknown is not a date");
    assert_eq!(undated.seeds_n, 7, "everything else on the row parses");
}

#[test]
fn test_a_row_that_cannot_become_one_costs_one_row() {
    // No download link, no topic id, no title: each of these is a
    let mut broken = String::from(TABLE);
    // Both markers present, so the row reaches the parser proper --
    broken.push_str("<tr><td><a href=\"viewtopic.php?t=\"> </a>");
    broken.push_str("<td><a href=\"download.php?id=9\">DL</a></td></tr>\n");
    let rows = parse_rows(&broken).expect("the table still parses");
    assert_eq!(rows.len(), 4, "the malformed row is dropped, not fatal");
}

#[test]
fn test_no_matches_is_an_empty_page_not_a_failure() {
    // Live: HTTP 200, the results table present, `Не найдено` inside.
    let rows = parse_rows(NO_MATCHES).expect("a miss is not a failure");
    assert!(rows.is_empty());
}

#[test]
fn test_a_torrent_nobody_seeds_is_a_row_with_zero_seeds_not_a_missing_one() {
    // Found live, 25.09.2026: 35 of the 50 rows on a browse page are
    let items = rows();
    let dead = items
        .iter()
        .find(|row| row.title.contains("Dead torrent"))
        .expect("the dead row survived");
    assert_eq!(dead.seeds_n, 0, "an empty cell is the 0 it means");
    assert_eq!(dead.seeds, "0");
    assert_eq!(dead.leechers, 0);
    // Everything else on the row still parses, which is the point: a
    assert_eq!(dead.size_bytes, 700_000_000);
    assert_eq!(dead.added, 1_600_000_000);
    assert_eq!(dead.date, "2020-09-13");
    assert!(dead.download_url.ends_with("download.php?id=1700000"));
}

#[test]
fn test_a_page_without_the_results_table_says_so_instead_of_crying_empty() {
    // Live shape of everything that is *not* the tracker answering:
    let blocked = "<!DOCTYPE html><html lang=en><title>Just a moment...</title>\
                   <p>Performing security verification</p>";
    let err = parse_rows(blocked).expect_err("no table means no answer");
    assert!(
        err.to_string().contains("no results table"),
        "the refusal names what is missing: {}",
        err
    );
}

#[test]
fn test_the_urls_are_the_ones_that_were_fetched() {
    assert_eq!(
        search_url("frieren", 0, None),
        "https://nnmclub.to/forum/tracker.php?f=-1&nm=frieren"
    );
    // Page two was fetched live for both shapes and came back disjoint
    assert_eq!(
        search_url("frieren 2026", 50, None),
        "https://nnmclub.to/forum/tracker.php?f=-1&nm=frieren%202026&start=50"
    );
    assert_eq!(
        browse_url(0, None),
        "https://nnmclub.to/forum/tracker.php?f=-1&o=2&sd=desc"
    );
    assert_eq!(
        browse_url(50, None),
        "https://nnmclub.to/forum/tracker.php?f=-1&o=2&sd=desc&start=50"
    );
    // An empty query is browse -- the URL behind `supports_browse`,
    assert_eq!(search_url("   ", 0, None), browse_url(0, None));
    assert_eq!(
        search_url("   ", 0, Some(Group::Games)),
        browse_url(0, Some(Group::Games))
    );
}

/// The whole category decision in one assertion per group: one request, every id of that group
/// as `f%5B%5D=`, nothing else in the URL.
#[test]
fn test_a_selected_group_asks_for_exactly_its_own_forums_in_one_request() {
    for (group, ids) in GROUP_FORUMS {
        let params = ids
            .iter()
            .map(|id| format!("f%5B%5D={}", id))
            .collect::<Vec<_>>()
            .join("&");
        let expected = format!("https://nnmclub.to/forum/tracker.php?{}&nm=matrix", params);
        let url = search_url("matrix", 0, Some(group));
        assert_eq!(
            url,
            expected,
            "{:?} must ask for its {} forums and no others",
            group,
            ids.len()
        );
    }
    // Browse narrows the same way, cursor included.
    let anime = GROUP_FORUMS
        .iter()
        .find(|(group, _)| *group == Group::Anime)
        .map_or(&[][..], |(_, ids)| ids);
    let params = anime
        .iter()
        .map(|id| format!("f%5B%5D={}", id))
        .collect::<Vec<_>>()
        .join("&");
    assert_eq!(
        browse_url(50, Some(Group::Anime)),
        format!(
            "https://nnmclub.to/forum/tracker.php?{}&o=2&sd=desc&start=50",
            params
        )
    );
}

/// The table the whole feature rests on: four disjoint lists, spot-
/// checked against the names the live tree carried, and the same four
/// groups the source declares.
#[test]
fn test_the_forum_table_is_disjoint_and_maps_the_live_sections() {
    for (i, (left, left_ids)) in GROUP_FORUMS.iter().enumerate() {
        for (right, right_ids) in GROUP_FORUMS.iter().skip(i + 1) {
            let shared: Vec<i32> = left_ids
                .iter()
                .filter(|id| right_ids.contains(id))
                .copied()
                .collect();
            assert!(
                shared.is_empty(),
                "{:?} and {:?} share a forum: {:?}",
                left,
                right,
                shared
            );
        }
    }

    // Sections named in the live tree (26.09.2026) and their group.
    assert_eq!(group_for_forum(905), Some(Group::Movies), "Театр");
    assert_eq!(group_for_forum(227), Some(Group::Movies), "Кино");
    assert_eq!(group_for_forum(768), Some(Group::TV), "Сериалы");
    assert_eq!(group_for_forum(410), Some(Group::Games), "Win Игры");
    assert_eq!(group_for_forum(621), Some(Group::Anime), "Аниме (HD)");
    // The archives the user folded into their rubric.
    assert_eq!(group_for_forum(91), Some(Group::Movies), "Архив Кино");
    assert_eq!(group_for_forum(668), Some(Group::Movies), "Архив классики");
    assert_eq!(group_for_forum(169), Some(Group::Anime), "Архив Аниме");
    assert_eq!(group_for_forum(93), Some(Group::Games), "Архив Игр");
    assert_eq!(group_for_forum(669), Some(Group::TV), "Архив Док/TV");
    // Checked live and kept out: MP3, books, music, and the fixture's
    assert_eq!(group_for_forum(734), None, "Классика для мам = музыка");
    assert_eq!(group_for_forum(738), None, "Образование = книги");
    assert_eq!(group_for_forum(92), None, "Архив Музыки");
    assert_eq!(group_for_forum(906), None, "not in the live tree");

    // What the source declares is what the table spans -- the two
    let declared = NnmclubSearcher::new().groups();
    let from_table: Vec<Group> = GROUP_FORUMS.iter().map(|(g, _)| *g).collect();
    assert_eq!(declared, from_table.as_slice());
}

/// Per-row attribution: the row's own forum cell decides, and a forum
/// the table does not know claims nothing rather than something wrong.
#[test]
fn test_a_row_claims_the_group_of_its_own_forum_and_nothing_more() {
    let rows = rows();
    assert_eq!(rows.len(), 4, "the fixture's four rows");
    assert_eq!(
        rows[0].group,
        Some(Group::Movies),
        "f=905 is Театр, and theatre sits with cinema"
    );
    assert_eq!(
        rows[1].group, None,
        "f=906 is not in the table, so this row belongs to \"all\" only"
    );
    assert_eq!(
        rows[2].group, None,
        "no forum cell in this row, so nothing to attribute it by"
    );
    assert_eq!(rows[3].group, None, "and none in the dead-torrent row");
}

#[test]
fn test_a_full_page_offers_the_next_one_and_a_short_one_does_not() {
    let template = rows()[0].clone();
    let full: Vec<TorrentItem> = (0..PAGE_SIZE).map(|_| template.clone()).collect();
    let page = to_page(full.clone(), 0);
    assert!(page.has_more, "50 rows is what the site counts as a page");
    assert_eq!(page.next_offset, Some(PAGE_SIZE), "start= steps by pages");

    // The cursor is spelled out, not derived from the row count: a
    let page = to_page(full, 100);
    assert_eq!(page.next_offset, Some(150));

    let short: Vec<TorrentItem> = (0..PAGE_SIZE - 1).map(|_| template.clone()).collect();
    let page = to_page(short, 50);
    assert!(!page.has_more, "a short page is the last page");
    assert_eq!(page.next_offset, None);
}

#[tokio::test]
async fn test_the_source_declares_what_the_live_probe_showed() {
    let nnm = NnmclubSearcher::new();
    assert_eq!(nnm.id(), "nnmclub");
    assert_eq!(nnm.label(), "NNM-Club");
    assert_eq!(nnm.home_url(), "https://nnmclub.to");
    // A browser UA alone got 200 from every page -- cloudflare included.
    assert!(!nnm.requires_browser());
    // `f=-1&o=2&sd=desc` answered 50 newest topics, and `start=50`
    assert!(nnm.supports_browse());
    // Declared, per wave-3 decision: the four forums the site spans.
    assert_eq!(
        nnm.groups(),
        &[Group::Movies, Group::TV, Group::Games, Group::Anime]
    );
    // No login wall appeared on any page fetched that day.
    let log: LogFn = Arc::new(|_| {});
    let logged_in = nnm
        .ensure_logged_in(&AuthContext::default(), &log)
        .await
        .expect("the tracker answered without a login");
    assert!(logged_in);
}
