//! Fixture tests for the YTS source.
//!
//! The JSON is reconstructed from the live API's shape (checked against
//! `yts.gg`/`movies-api.accel.li` on 25.09.2026), not saved off the
//! wire -- the same convention `rutor_parse_tests.rs` states: a fixture
//! should pin the *fields we rely on*, so a fixture that carries a
//! server's whole payload only adds noise to diff.

use doris::sources::source::{self, SourceEnv};
use doris::sources::yts::{list_movies_url, parse_page, PAGE_SIZE};

/// One movie in three shapes: two hashed torrents plus one without a
/// hash, a movie with no torrents at all, and a movie whose torrent
/// carries no quality/type tags.
const PAGE_ONE: &str = r#"{
  "status": "ok",
  "status_message": "Query was successful",
  "data": {
    "movie_count": 120,
    "limit": 50,
    "page_number": 1,
    "movies": [
      {
        "id": 59406,
        "url": "https://yts.gg/movies/matrix-generation-2024",
        "title": "Matrix: Generation",
        "title_long": "Matrix: Generation (2024)",
        "date_uploaded_unix": 1705959944,
        "torrents": [
          {
            "hash": "937C8886C8FD31240898B0DE40DE9E104A926F7E",
            "quality": "720p",
            "type": "web",
            "seeds": 7,
            "peers": 2,
            "size": "511 MB",
            "size_bytes": 511568773
          },
          {
            "quality": "1080p",
            "type": "bluray",
            "seeds": 3,
            "peers": 1,
            "size_bytes": 1992277407
          },
          {
            "hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "quality": "2160p",
            "type": "web",
            "seeds": 1,
            "peers": 0,
            "size_bytes": 4000000000
          }
        ]
      },
      {
        "title": "A Movie With No Torrents",
        "date_uploaded_unix": 0,
        "torrents": []
      },
      {
        "title_long": "Third Movie (2026)",
        "date_uploaded_unix": 1700000000,
        "torrents": [
          {
            "hash": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "size_bytes": 700
          }
        ]
      }
    ]
  }
}"#;

fn page(offset: usize) -> doris::sources::source::SearchPage {
    parse_page(PAGE_ONE, offset).expect("the fixture parses")
}

#[test]
fn test_one_row_per_hashed_torrent_with_the_quality_tag_in_the_title() {
    let page = page(0);

    assert_eq!(page.items.len(), 3, "two hashless torrents must be skipped");
    let first = &page.items[0];
    assert_eq!(first.title, "Matrix: Generation (2024) [720p web]");
    assert_eq!(
        first.info_hash, "937c8886c8fd31240898b0de40de9e104a926f7e",
        "the API shouts the hash; info_hash promises lowercase hex"
    );
    assert_eq!(first.size_bytes, 511_568_773);
    assert_eq!(first.size, "487.87 MB", "display size comes from bytes");
    assert_eq!(first.seeds_n, 7);
    assert_eq!(first.seeds, "7");
    assert_eq!(first.leechers, 2);
    assert_eq!(first.added, 1705959944);
    assert_eq!(first.date, "2024-01-22");
}

#[test]
fn test_a_torrent_without_quality_tags_keeps_the_plain_title() {
    let page = page(0);

    let last = page.items.last().expect("the third movie has one torrent");
    assert_eq!(last.title, "Third Movie (2026)", "no tags -> no brackets");
}

#[test]
fn test_hashless_torrents_are_skipped_not_rendered() {
    let page = page(0);

    assert!(
        page.items.iter().all(|item| !item.title.contains("1080p")),
        "the unhashed 1080p torrent must not become a row: {:?}",
        page.items.iter().map(|i| &i.title).collect::<Vec<_>>()
    );
    //...and neither must a movie with no torrents at all.
    assert!(
        page.items.iter().all(|i| !i.title.contains("No Torrents")),
        "an empty torrent list yields no rows"
    );
}

#[test]
fn test_rows_are_magnet_only_and_attributed_to_yts_movies() {
    let page = page(0);

    for item in &page.items {
        assert_eq!(item.source, "yts");
        assert_eq!(item.group, Some(doris::sources::source::Group::Movies));
        assert_eq!(
            item.download_url, "",
            "YTS publishes magnets, not files -- this is what routes the \
             download key to a .magnet file"
        );
        let magnet = item.magnet.as_deref().expect("a magnet is the payload");
        assert!(
            magnet.contains(&format!("xt=urn:btih:{}", item.info_hash)),
            "magnet and info_hash must agree: {} vs {}",
            magnet,
            item.info_hash
        );
    }
    assert_eq!(
        page.items[0].page_url,
        "https://yts.gg/movies/matrix-generation-2024"
    );
}

/// The B8 part-B decision, stated as the numbers: the verdict counts
/// *movies* (`movie_count` 120 against `limit` 50), the cursor counts
/// API pages -- never the rows the page happened to yield.
#[test]
fn test_the_verdict_counts_movies_and_the_cursor_counts_pages() {
    assert_eq!(PAGE_SIZE, 50, "the API's limit");

    let first = page(0);
    assert!(first.has_more, "50 of 120 movies loaded -> more remain");
    assert_eq!(first.next_offset, Some(1));

    let second = page(1);
    assert!(second.has_more, "100 of 120 movies loaded");
    assert_eq!(second.next_offset, Some(2));

    let third = page(2);
    assert!(
        !third.has_more,
        "150 >= 120 movies: the third page is past the end"
    );
    assert_eq!(
        third.next_offset,
        Some(3),
        "the cursor keeps its own unit even on the last page"
    );
}

#[test]
fn test_an_empty_query_browses_by_newest_and_a_query_searches() {
    let browse = list_movies_url("yts.gg", "   ", 0);
    assert!(browse.contains("page_number=1"), "{}", browse);
    assert!(browse.contains("sort_by=date_added"), "{}", browse);
    assert!(!browse.contains("query_term"), "{}", browse);

    let search = list_movies_url("movies-api.accel.li", "the matrix", 1);
    assert!(search.contains("page_number=2"), "{}", search);
    assert!(search.contains("query_term=the%20matrix"), "{}", search);
    assert!(!search.contains("sort_by"), "{}", search);
}

/// An error status must surface rather than render as "no results",
/// because that is what lets `first_ok` try the next mirror instead of
/// showing the user an empty list.
#[test]
fn test_an_error_status_is_an_error_not_an_empty_page() {
    let err = parse_page(
        r#"{"status":"error","status_message":"too many requests"}"#,
        0,
    )
    .expect_err("an error status is not a page");

    assert!(err.to_string().contains("too many requests"), "{}", err);
}

#[test]
fn test_an_unparseable_body_is_an_error_so_failover_can_move_on() {
    let err = parse_page("<html>maintenance</html>", 0)
        .expect_err("an HTML maintenance page is not JSON");

    assert!(err.to_string().contains("did not parse"), "{}", err);
}

/// `data` is optional in the API's own shape (`status: ok` with no
/// results is legal); an empty page is the honest answer there.
#[test]
fn test_a_missing_data_block_is_an_empty_page_not_an_error() {
    let page = parse_page(r#"{"status":"ok"}"#, 4).expect("ok is ok");

    assert!(page.items.is_empty());
    assert!(!page.has_more, "no movies known -> nothing after this page");
    assert_eq!(page.next_offset, Some(5));
}

// --- registry ---------------------------------------------------------------

#[test]
fn test_yts_is_registered_as_an_implemented_browser_free_source() {
    let info = source::get_source("yts").expect("yts must be in KNOWN_SOURCES");
    assert!(info.implemented);
    assert_eq!(info.label, "YTS");
    assert_eq!(info.groups, &[doris::sources::source::Group::Movies]);
    assert!(!info.requires_browser, "a JSON API needs no browser");
    assert!(info.home_url.starts_with("http"), "{}", info.home_url);

    let built = source::build_source("yts", SourceEnv { browser: None })
        .expect("an implemented browser-free source must build offline");
    assert_eq!(built.id(), info.id);
    assert_eq!(built.label(), info.label);
    assert_eq!(built.home_url(), info.home_url);
    // B9 consumes this: an empty query is a *browse* here, and YTS
    // answers it with `sort_by=date_added`.
    assert!(built.supports_browse(), "empty query -> newest movies");
    assert!(!built.requires_browser());
}
