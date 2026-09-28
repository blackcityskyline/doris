//! Fixture tests for the apibay-backed TPB source (ROADMAP.md B8 wave 1).
//!
//! Both live endpoint shapes are pinned side by side on purpose: `q.php`
//! sends every numeric-ish field as a *string*, the precompiled top-100
//! lists send them as *numbers* (checked 25.09.2026). A parser that
//! reads only one spelling zeroes the other silently, which is exactly
//! the kind of bug a single-fixture test never catches.

use doris::sources::source::{self, Group, SourceEnv};
use doris::sources::tpb::{parse_rows, search_url, TOP_MOVIES_URL, TOP_TV_URL};

/// `q.php` shape: strings everywhere, `info_hash` uppercase, and the
/// "No results" placeholder as a third row (that is how apibay answers
/// a miss -- not with `[]`).
const SEARCH_BODY: &str = r#"[
  {
    "id": "7349687",
    "name": "The Matrix (1999) 1080p BrRip x264 - 1.85GB - YIFY",
    "info_hash": "D7A46713EAEE18C746B3254B7D1492A50FD9D6CE",
    "leechers": "120",
    "seeders": "793",
    "size": "1992277407",
    "num_files": "6",
    "username": "YIFY",
    "added": "1339543961",
    "status": "vip",
    "category": "207",
    "imdb": "tt0133093"
  },
  {
    "id": "84496597",
    "name": "Lanterns S01E06 Bad Optics 1080p HEVC x265-MeGusta",
    "info_hash": "CCD4835CC1E6686ACD42C4E0283B2F2F204C2240",
    "leechers": "4268",
    "seeders": "4010",
    "size": "539157470",
    "num_files": "1",
    "username": "jajaja",
    "added": "1789954501",
    "status": "vip",
    "category": "208",
    "imdb": "tt26545992"
  },
  {
    "id": "0",
    "name": "No results returned",
    "info_hash": "0000000000000000000000000000000000000000",
    "leechers": "0",
    "seeders": "0",
    "num_files": "0",
    "size": "0",
    "username": "",
    "added": "0",
    "status": "member",
    "category": "0",
    "imdb": "",
    "total_found": "1"
  }
]"#;

/// `data_top100_207/208.json` shape: the same fields as *numbers*.
const TOP100_BODY: &str = r#"[
  {
    "id": 83970962,
    "info_hash": "9D86667F49F42712909C2888D346B37A17C44191",
    "category": 207,
    "name": "Spider-Man: Brand New Day 2026.1080p.HQ Pre.Multi.AAC 2.0.x264",
    "status": "vip",
    "num_files": 1,
    "size": 3808117223,
    "seeders": 6003,
    "leechers": 5610,
    "username": "Anonymous",
    "added": 1785517204,
    "imdb": "tt22084616"
  },
  {
    "id": 84496597,
    "info_hash": "CCD4835CC1E6686ACD42C4E0283B2F2F204C2240",
    "category": 208,
    "name": "Lanterns S01E06 Bad Optics 1080p HEVC x265-MeGusta",
    "status": "vip",
    "num_files": 0,
    "size": 539157470,
    "seeders": 4010,
    "leechers": 4268,
    "username": "jajaja",
    "added": 1789954501,
    "anon": 0,
    "imdb": "tt26545992"
  }
]"#;

#[test]
fn test_string_typed_endpoint_yields_rows_with_every_number_read() {
    let rows = parse_rows(SEARCH_BODY).expect("q.php fixture parses");
    assert_eq!(rows.len(), 2, "the placeholder row must not be counted");

    let first = &rows[0];
    assert_eq!(
        first.title,
        "The Matrix (1999) 1080p BrRip x264 - 1.85GB - YIFY"
    );
    assert_eq!(
        first.info_hash, "d7a46713eaee18c746b3254b7d1492a50fd9d6ce",
        "apibay shouts the hash; info_hash promises lowercase"
    );
    assert_eq!(first.size_bytes, 1_992_277_407, "read from \"1992277407\"");
    assert_eq!(first.size, "1.86 GB");
    assert_eq!(first.seeds_n, 793);
    assert_eq!(first.seeds, "793");
    assert_eq!(first.leechers, 120);
    assert_eq!(first.added, 1_339_543_961);
    assert_eq!(first.date, "2012-06-12");
    assert_eq!(first.group, Some(Group::Movies), "category 207");
    assert_eq!(
        first.page_url,
        "https://thepiratebay.org/description.php?id=7349687"
    );
}

#[test]
fn test_numeric_endpoint_parses_through_the_same_row_builder() {
    let rows = parse_rows(TOP100_BODY).expect("top-100 fixture parses");
    assert_eq!(rows.len(), 2);

    let movies = &rows[0];
    assert_eq!(movies.size_bytes, 3_808_117_223, "read from 3808117223");
    assert_eq!(movies.size, "3.55 GB");
    assert_eq!(movies.seeds_n, 6003, "a number, not the string \"6003\"");
    assert_eq!(movies.added, 1_785_517_204);
    assert_eq!(movies.date, "2026-07-31");
    assert_eq!(movies.group, Some(Group::Movies));

    let episodes = &rows[1];
    assert_eq!(episodes.group, Some(Group::TV), "category 208");
    assert_eq!(episodes.size, "514.18 MB");
    assert_eq!(episodes.date, "2026-09-21");
}

#[test]
fn test_every_row_is_magnet_only_and_carries_its_magnet() {
    for body in [SEARCH_BODY, TOP100_BODY] {
        for row in parse_rows(body).expect("fixture parses") {
            assert_eq!(row.source, "tpb");
            assert_eq!(
                row.download_url, "",
                "apibay serves magnets: the download key writes a .magnet"
            );
            let magnet = row.magnet.as_deref().expect("a magnet is the payload");
            assert!(
                magnet.contains(&format!("xt=urn:btih:{}", row.info_hash)),
                "magnet and info_hash must agree: {} vs {}",
                magnet,
                row.info_hash
            );
        }
    }
}

#[test]
fn test_the_no_results_placeholder_never_becomes_a_row() {
    // A miss answers with one row that *looks* like a result: id "0",
    // all-zero hash, name "No results returned". Rendering it would
    // show a fake torrent for every failed search.
    let rows = parse_rows(
        r#"[
        {"id":"0","name":"No results returned",
         "info_hash":"0000000000000000000000000000000000000000",
         "size":"0","seeders":"0","leechers":"0","added":"0","category":"0"}
    ]"#,
    )
    .expect("the placeholder body parses");
    assert!(
        rows.is_empty(),
        "got: {:?}",
        rows.iter().map(|r| &r.title).collect::<Vec<_>>()
    );

    // The same rule on a real answer, where the placeholder is a
    // third row -- it must not cost the two real ones.
    let mixed = parse_rows(SEARCH_BODY).expect("mixed body parses");
    assert_eq!(mixed.len(), 2);
}

#[test]
fn test_missing_optional_fields_degrade_instead_of_failing_the_page() {
    let rows = parse_rows(r#"[{"info_hash":"ABCDEF0123456789ABCDEF0123456789ABCDEF01","id":"5"}]"#)
        .expect("a bare row still parses");

    let row = rows.first().expect("one row survives");
    assert_eq!(row.title, "Unknown");
    assert_eq!(row.size_bytes, 0);
    assert_eq!(row.size, "0 B", "an unknown size still looks like a size");
    assert_eq!(row.seeds, "0");
    assert_eq!(row.date, "", "an unknown date must not read 1970-01-01");
    assert_eq!(row.group, None, "no category -> unattributed");
}

/// Category mapping, driven through the public parser rather than by
/// widening the API just to poke at a private function.
fn group_of(category: i64) -> Option<Group> {
    let body = format!(
        r#"[{{"id":"1","name":"x","info_hash":"abcdef0123456789abcdef0123456789abcdef01",
            "category":{},"size":"1","seeders":"1","leechers":"0","added":"0"}}]"#,
        category
    );
    parse_rows(&body)
        .expect("mapping fixture parses")
        .pop()
        .expect("one row")
        .group
}

/// The wave-1 decision in one test, widened by B6's live classification:
/// TPB attributes rows to the two groups it declares (Movies, TV) and
/// leaves everything else -- concerts, animation, the 206 mix, games,
/// music, apps, books, XXX -- unattributed rather than claiming a group
/// the registry does not promise this source speaks for. 211 (UHD films)
/// and 212 (2160p episodes) are the ids torio's lists predate; 203/204/
/// 206 are the ids a naive "everything under 200" rule would swallow.
#[test]
fn test_only_the_declared_groups_are_attributed() {
    for category in [201, 202, 207, 209, 211] {
        assert_eq!(group_of(category), Some(Group::Movies), "cat {}", category);
    }
    for category in [205, 208, 212] {
        assert_eq!(group_of(category), Some(Group::TV), "cat {}", category);
    }
    for category in [101, 203, 204, 206, 301, 401, 505, 601] {
        assert_eq!(
            group_of(category),
            None,
            "cat {} stays unattributed",
            category
        );
    }
}

#[test]
fn test_the_search_url_carries_the_selected_category() {
    let plain = search_url("the matrix", None);
    assert_eq!(plain, "https://apibay.org/q.php?q=the%20matrix");
    assert!(!plain.contains("cat="), "no category selected, no trim");

    assert_eq!(
        search_url("the matrix", Some(Group::Movies)),
        "https://apibay.org/q.php?q=the%20matrix&cat=201,202,207,209,211"
    );
    assert_eq!(
        search_url("the matrix", Some(Group::TV)),
        "https://apibay.org/q.php?q=the%20matrix&cat=205,208,212"
    );
    // The two groups tpb does not declare ask for nothing at all: it is
    // never asked for them (the registry gates the dispatch), so this
    // branch cannot answer with rows claiming someone else's category.
    assert!(!search_url("x", Some(Group::Games)).contains("cat="));
    assert!(!search_url("x", Some(Group::Anime)).contains("cat="));

    assert_eq!(
        search_url("  spaced  ", None),
        "https://apibay.org/q.php?q=spaced"
    );
}

/// B6's acceptance in one direction: every id the server is trimmed by
/// parses back into exactly the group it was asked for. Without this,
/// a stale `cat=` list would fetch rows the view drops as unattributed
/// and report an empty category while the corpus had hits.
#[test]
fn test_every_id_in_the_category_filter_parses_back_into_that_group() {
    for group in [Group::Movies, Group::TV] {
        let url = search_url("x", Some(group));
        let ids = url
            .split("cat=")
            .nth(1)
            .expect("a group tpb declares has a cat= list");
        for id in ids.split(',') {
            assert_eq!(
                group_of(id.parse().expect("numeric id")),
                Some(group),
                "cat id {} fetched for {:?} must be that group's",
                id,
                group
            );
        }
    }
}

// --- registry ---------------------------------------------------------------

#[test]
fn test_tpb_is_registered_as_a_browser_free_source_declaring_movies_and_tv() {
    let info = source::get_source("tpb").expect("tpb must be in KNOWN_SOURCES");
    assert!(info.implemented);
    assert_eq!(info.label, "TPB");
    assert_eq!(
        info.groups,
        &[Group::Movies, Group::TV],
        "one source, both groups"
    );
    assert!(!info.requires_browser, "a JSON API needs no browser");
    assert!(info.home_url.starts_with("http"), "{}", info.home_url);

    let built = source::build_source("tpb", SourceEnv { browser: None })
        .expect("an implemented browser-free source must build offline");
    assert_eq!(built.id(), info.id);
    assert_eq!(built.label(), info.label);
    assert_eq!(built.home_url(), info.home_url);
    assert_eq!(built.groups(), info.groups, "registry and impl must agree");
    assert!(built.supports_browse(), "empty query -> the top-100 lists");
    assert!(!built.requires_browser());
}

#[test]
fn test_browse_is_the_two_top100_lists() {
    assert!(
        TOP_MOVIES_URL.ends_with("data_top100_207.json"),
        "{}",
        TOP_MOVIES_URL
    );
    assert!(
        TOP_TV_URL.ends_with("data_top100_208.json"),
        "{}",
        TOP_TV_URL
    );
}
