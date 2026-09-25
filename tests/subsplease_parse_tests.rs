//! Fixture tests for the SubsPlease source (ROADMAP.md B8 wave 1).
//!
//! The fixture is the live document's shape (checked against
//! `subsplease.org/api/` on 25.09.2026), truncated where the payload
//! is repetitive: magnets keep their `xt` (hash), `dn`, `xl` (size)
//! and one tracker, because those four are what the parser reads.

use doris::search::source::{self, Group, SourceEnv};
use doris::search::subsplease::{api_url, parse_rows};

/// One episode in three resolutions (the 1080/720/480 set), one with a
/// single named resolution, one whose magnet is missing entirely, and
/// one whose magnet carries no resolution at all.
const BODY: &str = r#"{
  "Sousou no Frieren S2 - 01-10": {
    "time": "05/29/26",
    "release_date": "Fri, 29 May 2026 17:05:35 +0000",
    "show": "Sousou no Frieren S2",
    "episode": "01-10",
    "downloads": [
      {
        "res": "480",
        "magnet": "magnet:?xt=urn:btih:RG4AGFQUMB4MGRUYN745JBZDJAOFWMNY&dn=batch480&xl=3895551825&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce"
      },
      {
        "res": "720",
        "magnet": "magnet:?xt=urn:btih:RG4AGFQUMB4MGRUYN745JBZDJAOFWMNY&dn=batch720&xl=7488913906&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce"
      },
      {
        "res": "1080",
        "magnet": "magnet:?xt=urn:btih:XQSSGH3FPUIV4YPGRUHBETJ76XWBAH3J&dn=batch1080&xl=14669545971&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce"
      }
    ],
    "xdcc": "%22%5BSubsPlease%5D%22",
    "image_url": "/wp-content/uploads/2026/01/154528.jpg",
    "page": "sousou-no-frieren-s2"
  },
  "Ordinary Show - 05": {
    "release_date": "Mon, 01 Jun 2026 12:00:00 +0000",
    "show": "Ordinary Show",
    "episode": "05",
    "downloads": [
      {
        "res": "720",
        "magnet": "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=ep05&xl=1234567"
      }
    ]
  },
  "Magnetless Show - 01": {
    "release_date": "Mon, 01 Jun 2026 12:00:00 +0000",
    "show": "Magnetless Show",
    "episode": "01",
    "downloads": [
      { "res": "1080" }
    ]
  },
  "Unlabelled Show - 02": {
    "release_date": "Mon, 01 Jun 2026 12:00:00 +0000",
    "show": "Unlabelled Show",
    "episode": "02",
    "downloads": [
      {
        "magnet": "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=ep02"
      }
    ]
  }
}"#;

fn rows() -> Vec<doris::search::models::TorrentItem> {
    parse_rows(BODY).expect("the fixture parses")
}

#[test]
fn test_one_row_per_episode_taking_the_best_resolution() {
    let rows = rows();
    assert_eq!(rows.len(), 3, "the magnetless entry must be skipped");

    let best = rows
        .iter()
        .find(|r| r.title.contains("Frieren"))
        .expect("the batch entry is there");
    assert_eq!(best.title, "Sousou no Frieren S2 - 01-10 [1080p]");
    assert_eq!(
        best.size_bytes, 14_669_545_971,
        "the 1080 entry's `xl`, not the 480 one's"
    );
    assert_eq!(best.size, "13.66 GB");
    assert_eq!(best.page_url, "https://subsplease.org/shows/sousou-no-frieren-s2/");
}

/// The wave-1 decision, stated as the rows: three resolutions in, one
/// row out, and it is the best one.
#[test]
fn test_the_three_resolutions_collapse_into_a_single_row() {
    let rows = rows();
    assert_eq!(
        rows.iter().filter(|r| r.title.contains("Frieren")).count(),
        1,
        "480/720/1080 must not become three rows: {:?}",
        rows.iter().map(|r| &r.title).collect::<Vec<_>>()
    );
}

/// SubsPlease ships base32 hashes inside the magnet (`btih:` + 32
/// chars); `info_hash` promises lowercase hex, so the conversion is
/// load-bearing for dedup and for every hash-keyed feature.
#[test]
fn test_a_base32_hash_becomes_lowercase_hex_and_the_magnet_is_untouched() {
    let best = rows()
        .into_iter()
        .find(|r| r.title.contains("Frieren"))
        .expect("row");

    assert_eq!(best.info_hash, "bc25231f657d115e61e68d0e124d3ff5ec101f69");
    assert_eq!(best.info_hash.len(), 40);
    let magnet = best.magnet.as_deref().expect("the magnet is the payload");
    assert!(
        magnet.contains("xt=urn:btih:XQSSGH3FPUIV4YPGRUHBETJ76XWBAH3J"),
        "the magnet keeps the source's own base32 spelling: {}",
        magnet
    );
    assert!(magnet.contains("xl=14669545971"), "size stays inside it");
    assert!(magnet.contains("tr="), "and the trackers it came with");
}

/// A named resolution wins when it exists; when no named one carries a
/// magnet, any magnet does -- with `?p`, torio's placeholder.
#[test]
fn test_falls_back_from_best_to_any_usable_magnet() {
    let rows = rows();

    let single = rows
        .iter()
        .find(|r| r.title.starts_with("Ordinary Show"))
        .expect("row");
    assert_eq!(single.title, "Ordinary Show - 05 [720p]");
    assert_eq!(single.size_bytes, 1_234_567);
    assert_eq!(single.size, "1.18 MB");

    let unlabelled = rows
        .iter()
        .find(|r| r.title.starts_with("Unlabelled Show"))
        .expect("row");
    assert_eq!(
        unlabelled.title, "Unlabelled Show - 02 [?p]",
        "an unlabelled magnet still becomes a row"
    );
    assert_eq!(unlabelled.size_bytes, 0, "no xl -> size unknown");
    assert_eq!(unlabelled.size, "0 B");
}

#[test]
fn test_an_entry_without_a_magnet_is_skipped() {
    assert!(
        rows().iter().all(|r| !r.title.contains("Magnetless")),
        "an entry whose only download lacks a magnet owes no row"
    );
}

/// A miss answers `[]` where the success case is an object (live
/// checked). Reading that as a parse error would show "source broken"
/// where the truth is "nothing found".
#[test]
fn test_a_miss_is_an_empty_array_not_a_parse_error() {
    let rows = parse_rows("[]").expect("an empty array is a valid answer");
    assert!(rows.is_empty());
}

/// The API grows fields; an entry that no longer fits `SpEntry` costs
/// its own row rather than the page.
#[test]
fn test_a_malformed_entry_costs_one_row_not_the_page() {
    let rows = parse_rows(
        r#"{
          "broken": 42,
          "Ordinary Show - 05": {
            "release_date": "Mon, 01 Jun 2026 12:00:00 +0000",
            "show": "Ordinary Show",
            "episode": "05",
            "downloads": [
              {"res": "720",
               "magnet": "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=x&xl=9"}
            ]
          }
        }"#,
    )
    .expect("the document parses");

    assert_eq!(rows.len(), 1, "the bad entry is dropped, the good kept");
    assert_eq!(rows[0].title, "Ordinary Show - 05 [720p]");
}

#[test]
fn test_every_row_is_magnet_only_and_attributed_to_anime() {
    for row in rows() {
        assert_eq!(row.source, "subsplease");
        assert_eq!(row.group, Some(Group::Anime), "{}", row.title);
        assert_eq!(
            row.download_url, "",
            "the download key writes a .magnet for these rows"
        );
        assert!(
            row.magnet.is_some(),
            "the magnet is the only payload this API gives"
        );
        assert_eq!(
            row.seeds, "",
            "no seed data here: empty reads as unknown, \"0\" would claim dead"
        );
    }
}

#[test]
fn test_dates_come_from_the_rfc2822_release_date() {
    let best = rows()
        .into_iter()
        .find(|r| r.title.contains("Frieren"))
        .expect("row");

    assert_eq!(best.added, 1_780_074_335);
    assert_eq!(best.date, "2026-05-29", "rendered in UTC, not the local zone");
}

#[test]
fn test_an_empty_query_browses_latest_and_a_query_searches() {
    assert_eq!(api_url(""), "https://subsplease.org/api/?tz=UTC&f=latest");
    assert_eq!(api_url("   "), "https://subsplease.org/api/?tz=UTC&f=latest");
    assert_eq!(
        api_url("frieren"),
        "https://subsplease.org/api/?tz=UTC&f=search&s=frieren"
    );
    assert_eq!(
        api_url("the matrix"),
        "https://subsplease.org/api/?tz=UTC&f=search&s=the%20matrix"
    );
}

// --- registry ---------------------------------------------------------------

#[test]
fn test_subsplease_is_registered_as_an_anime_browser_free_source() {
    let info = source::get_source("subsplease").expect("must be listed");
    assert!(info.implemented);
    assert_eq!(info.label, "SubsPlease");
    assert_eq!(info.groups, &[Group::Anime]);
    assert!(!info.requires_browser);
    assert!(info.home_url.starts_with("http"), "{}", info.home_url);

    let built = source::build_source("subsplease", SourceEnv { browser: None })
        .expect("an implemented browser-free source must build offline");
    assert_eq!(built.id(), info.id);
    assert_eq!(built.label(), info.label);
    assert_eq!(built.home_url(), info.home_url);
    assert_eq!(built.groups(), info.groups, "registry and impl must agree");
    assert!(built.supports_browse(), "empty query -> f=latest");
}
