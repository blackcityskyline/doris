//! Fixture tests for the EZTV source.
//!
//! The fixture is the live response's shape (checked against
//! `eztvx.to/api/get-torrents` on 25.09.2026): `size_bytes` arrives as
//! a *string* while `seeds`/`peers` arrive as numbers, and both
//! spellings of every numeric field must read.

use doris::sources::eztv::{parse_page, torrents_url, EztvSearcher, PAGE_SIZE};
use doris::sources::source::{self, Group, SearchRequest, Source, SourceEnv};

/// Two usable rows (one with a shipped magnet, one rebuilt from an
/// uppercase hash; `size_bytes` once as a string and once as a number)
/// plus three rows that cannot become results: a missing hash, an
/// empty hash, and a hash that is not a hash.
const BODY: &str = r#"{
  "torrents_count": 1085539,
  "limit": 100,
  "page": 1,
  "torrents": [
    {
      "id": 3158388,
      "hash": "e5ffd046a810a0fedbf213cfdb717fb8ede83060",
      "filename": "hells.kitchen.s25e01.1080p.web.h264-hotdogwater[EZTVx.to].mkv",
      "title": "Hells Kitchen S25E01 1080p WEB H264-HOTDOGWATER EZTV",
      "magnet_url": "magnet:?xt=urn:btih:e5ffd046a810a0fedbf213cfdb717fb8ede83060&dn=hk&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce",
      "season": "25",
      "episode": "1",
      "seeds": 0,
      "peers": 3,
      "date_released_unix": 1790331023,
      "size_bytes": "2529563471"
    },
    {
      "hash": "ABCDEF0123456789ABCDEF0123456789ABCDEF01",
      "filename": "no.magnet.here.mkv",
      "title": "No Magnet S01E02 720p WEB H264-TEST EZTV",
      "seeds": "7",
      "peers": "2",
      "date_released_unix": 1700000000,
      "size_bytes": 12345
    },
    {
      "title": "A Row With No Hash At All",
      "magnet_url": "magnet:?xt=urn:btih:e5ffd046a810a0fedbf213cfdb717fb8ede83060",
      "size_bytes": "10"
    },
    {
      "hash": "",
      "title": "A Row With An Empty Hash",
      "magnet_url": "magnet:?xt=urn:btih:e5ffd046a810a0fedbf213cfdb717fb8ede83060",
      "size_bytes": "10"
    },
    {
      "hash": "xyz",
      "title": "A Row Whose Hash Is Not A Hash",
      "magnet_url": "magnet:?xt=urn:btih:xyz",
      "size_bytes": "10"
    }
  ]
}"#;

fn page(offset: usize) -> doris::sources::source::SearchPage {
    parse_page(BODY, offset).expect("the fixture parses")
}

#[test]
fn test_rows_carry_title_hash_size_seeds_and_date() {
    let page = page(0);
    assert_eq!(page.items.len(), 2, "three unusable rows must be skipped");

    let first = &page.items[0];
    assert_eq!(
        first.title,
        "Hells Kitchen S25E01 1080p WEB H264-HOTDOGWATER EZTV"
    );
    assert_eq!(first.info_hash, "e5ffd046a810a0fedbf213cfdb717fb8ede83060");
    assert_eq!(first.size_bytes, 2_529_563_471, "read from \"2529563471\"");
    assert_eq!(first.size, "2.36 GB");
    assert_eq!(first.seeds_n, 0, "a real zero, not a missing field");
    assert_eq!(first.seeds, "0");
    assert_eq!(first.leechers, 3);
    assert_eq!(first.added, 1_790_331_023);
    assert_eq!(first.date, "2026-09-25");
    assert_eq!(first.group, Some(Group::TV));
    assert_eq!(first.source, "eztv");
}

#[test]
fn test_an_uppercase_hash_is_lowered_and_a_number_spelled_size_reads() {
    let rows = page(0).items;
    let second = rows
        .iter()
        .find(|r| r.title.starts_with("No Magnet"))
        .expect("row");

    assert_eq!(second.info_hash, "abcdef0123456789abcdef0123456789abcdef01");
    assert_eq!(second.size_bytes, 12_345, "a JSON number, not a string");
    assert_eq!(second.size, "12.06 KB");
    assert_eq!(second.seeds_n, 7, "a *quoted* seeds value still reads");
}

/// `magnet_url` is normally shipped, but the hash alone is enough to
/// rebuild one -- torio's fallback, and the reason a row without a
/// magnet is not dropped.
#[test]
fn test_a_row_without_a_shipped_magnet_gets_one_built_from_its_hash() {
    let second = page(0)
        .items
        .into_iter()
        .find(|r| r.title.starts_with("No Magnet"))
        .expect("row");
    let magnet = second.magnet.as_deref().expect("built, not missing");

    assert!(
        magnet.contains("xt=urn:btih:abcdef0123456789abcdef0123456789abcdef01"),
        "{}",
        magnet
    );
    assert!(magnet.contains("dn="), "and a display name: {}", magnet);
}

#[test]
fn test_rows_without_a_usable_hash_never_reach_the_table() {
    let first = page(0);
    let titles: Vec<&str> = first.items.iter().map(|r| r.title.as_str()).collect();
    for unusable in [
        "A Row With No Hash At All",
        "A Row With An Empty Hash",
        "A Row Whose Hash Is Not A Hash",
    ] {
        assert!(
            !titles.contains(&unusable),
            "{} became a row: {:?}",
            unusable,
            titles
        );
    }
}

/// The B8-part-B cursor, stated for eztv: it advances by whole API
/// pages, never by the rows that survived -- dropping a hashless row
/// must not slide the next request into the middle of a page.
#[test]
fn test_the_cursor_advances_by_whole_pages_not_by_rows_kept() {
    let first = page(0);
    assert_eq!(first.items.len(), 2, "fewer rows than the page holds");
    assert_eq!(
        first.next_offset,
        Some(PAGE_SIZE),
        "the *page* boundary, not offset + 3"
    );
    assert_eq!(page(100).next_offset, Some(200));

    assert_eq!(PAGE_SIZE, 100, "the API's limit and our page agree");
    assert_eq!(
        torrents_url(0),
        "https://eztvx.to/api/get-torrents?limit=100&page=1"
    );
    assert_eq!(
        torrents_url(200),
        "https://eztvx.to/api/get-torrents?limit=100&page=3"
    );
    // `eztv.re` only 301s (live), and `search=` is ignored (live) --
    // neither may appear in what we ask for.
    let url = torrents_url(0);
    assert!(!url.contains("eztv.re"), "{}", url);
    assert!(!url.contains("search="), "{}", url);
}

/// The verdict is arithmetic over `torrents_count`, not a guess about
/// how full this page happened to be.
#[test]
fn test_the_verdict_comes_from_the_index_count() {
    // BODY says 1085539 total: page 1 has plenty behind it.
    assert!(page(0).has_more, "100 of 1085539 loaded");

    // A small index, so page 2 is already past the end.
    let short = r#"{
      "torrents_count": 150,
      "torrents": [
        {"hash":"e5ffd046a810a0fedbf213cfdb717fb8ede83060","title":"x"}
      ]
    }"#;
    assert!(parse_page(short, 0).expect("page 1").has_more, "100 of 150");
    assert!(
        !parse_page(short, 100).expect("page 2").has_more,
        "200 >= 150: there is no page 3 to ask for"
    );
}

/// With no count in the answer, a full page may have a next one and an
/// empty one certainly does not -- the fallback must not promise more
/// than it could know.
#[test]
fn test_without_a_count_a_full_page_may_continue_and_an_empty_one_may_not() {
    fn body_with(rows: usize) -> String {
        let entries: Vec<String> = (0..rows)
            .map(|i| {
                format!(
                    r#"{{"title":"Show {i}","hash":"{i:040x}","magnet_url":"magnet:?xt=urn:btih:{i:040x}"}}"#
                )
            })
            .collect();
        format!(r#"{{"torrents":[{}]}}"#, entries.join(","))
    }

    let full = parse_page(&body_with(PAGE_SIZE), 0).expect("full page");
    assert!(full.has_more, "a full page may well continue");
    assert_eq!(full.items.len(), PAGE_SIZE);

    let almost = parse_page(&body_with(PAGE_SIZE - 1), 0).expect("short page");
    assert!(!almost.has_more, "a short page says the index ran dry");

    let empty = parse_page(r#"{"torrents":[]}"#, 0).expect("empty page");
    assert!(!empty.has_more);
    assert!(empty.items.is_empty());
}

/// The wave-1 decision, testable offline precisely because the refusal
/// happens *before* any request: an empty table would read as "no
/// results", and that is a different fact from "this API has no search".
#[tokio::test]
async fn test_a_query_is_refused_with_the_reason_instead_of_a_wrong_empty_page() {
    let eztv = EztvSearcher::new();

    let err = eztv
        .search(&SearchRequest::new("breaking bad", 0))
        .await
        .expect_err("a query must not be answered by a browse");

    let message = err.to_string();
    assert!(message.contains("no search"), "{}", message);
    assert!(
        message.contains("ignores"),
        "the message must explain *why*: {}",
        message
    );
    assert!(
        message.contains("empty"),
        "and what to do instead: {}",
        message
    );
}

#[test]
fn test_an_unparseable_body_is_an_error_not_an_empty_page() {
    let err = parse_page("<html>maintenance</html>", 0).expect_err("an HTML page is not JSON");
    assert!(err.to_string().contains("did not parse"), "{}", err);
}

// --- registry ---------------------------------------------------------------

#[test]
fn test_eztv_is_registered_as_a_browser_free_source_declaring_tv() {
    let info = source::get_source("eztv").expect("eztv must be in KNOWN_SOURCES");
    assert!(info.implemented);
    assert_eq!(info.label, "EZTV");
    assert_eq!(info.groups, &[Group::TV]);
    assert!(!info.requires_browser);
    assert!(info.home_url.starts_with("http"), "{}", info.home_url);

    let built = source::build_source("eztv", SourceEnv { browser: None })
        .expect("an implemented browser-free source must build offline");
    assert_eq!(built.id(), info.id);
    assert_eq!(built.label(), info.label);
    assert_eq!(built.home_url(), info.home_url);
    assert_eq!(built.groups(), info.groups, "registry and impl must agree");
    assert!(
        built.supports_browse(),
        "empty query -> the newest releases"
    );
}
