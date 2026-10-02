use doris::sources::models::TorrentItem;
use doris::transmission::adopt::{btih_from_magnet, hash_of, row_for_hash, to_row};
use doris::transmission::Download;
use doris::ui::view::DownloadRow;

/// The hash is the join. A search row and a download meet on it and on
/// nothing else, so these three are what decide whether a row the user
/// pressed play on is fetched a second time.
fn download(hash: &str) -> Download {
    Download {
        id: 7,
        hash: hash.into(),
        name: "Oblivion (2013) [1080p]".into(),
        fraction: 0.5,
        download_speed: 1000,
        total_size: 2000,
        status: 4,
        downloaded: 1000,
        uploaded: 250,
        eta: 120,
        dir: "/home/u/Downloads".into(),
        ..Download::default()
    }
}

/// A daemon reports its hash in mixed case across versions, and a search
/// row reports it lower-case. Comparing them raw would miss half of them.
#[test]
fn test_a_download_is_recognised_by_its_hash_in_either_case() {
    let rows = vec![to_row(&download(
        "045E85F2EBC24A875A64FE2E9AC9B61F7AAD0499",
    ))];
    assert_eq!(rows[0].hash, "045e85f2ebc24a875a64fe2e9ac9b61f7aad0499");
    assert!(
        row_for_hash(&rows, "045e85f2ebc24a875a64fe2e9ac9b61f7aad0499").is_some(),
        "and the row carries it normalised"
    );
    assert!(row_for_hash(&rows, "045e85f2ebc24a875a64fe2e9ac9b61f7aad0500").is_none());
}

/// An empty hash is not a wildcard. A tracker that reports no info hash
/// must not be recognised as already downloaded, or the one source that
/// needs a fetch is the one source that never gets one.
#[test]
fn test_an_empty_hash_matches_nothing() {
    let rows = vec![to_row(&download(""))];
    assert!(row_for_hash(&rows, "").is_none());
    assert!(row_for_hash(&[], "045e85f2").is_none());
}

/// Trackers disagree about whether a row carries the hash in its own field
/// or only inside its magnet, so both are read, and the magnet wins only
/// when the field is empty.
#[test]
fn test_a_rows_hash_comes_from_the_field_or_out_of_its_magnet() {
    let magnet = "magnet:?xt=urn:btih:045E85F2EBC24A875A64FE2E9AC9B61F7AAD0499&dn=x";
    let row = TorrentItem {
        magnet: Some(magnet.into()),
        ..TorrentItem::default()
    };
    assert_eq!(
        hash_of(&row),
        "045e85f2ebc24a875a64fe2e9ac9b61f7aad0499",
        "a magnet with no hash field still carries one"
    );

    let both = TorrentItem {
        info_hash: "AAAA".into(),
        magnet: Some(magnet.into()),
        ..TorrentItem::default()
    };
    assert_eq!(hash_of(&both), "aaaa", "the field is the field");

    assert_eq!(
        hash_of(&TorrentItem::default()),
        "",
        "and neither means no hash"
    );
}

/// A magnet has its parameters in any order and any case, so the search is
/// for the parameter and not for a fixed position.
#[test]
fn test_the_hash_is_found_wherever_the_magnet_puts_it() {
    assert_eq!(
        btih_from_magnet("magnet:?xt=urn:btih:ABCDEF&dn=name&tr=udp%3A%2F%2Fx"),
        Some("abcdef".into())
    );
    assert_eq!(
        btih_from_magnet("magnet:?dn=name&xt=urn:btih:ABCDEF"),
        Some("abcdef".into())
    );
    assert_eq!(btih_from_magnet("magnet:?dn=name"), None);
    assert_eq!(btih_from_magnet("not a magnet"), None);
}

/// The panel's row carries what a list line shows and the detail tabs need,
/// which is three fields more than the old single-status panel could hold.
#[test]
fn test_the_row_carries_what_the_panel_shows() {
    let row = to_row(&download("045e85f2ebc24a875a64fe2e9ac9b61f7aad0499"));
    assert_eq!(row.id, 7);
    assert_eq!(row.total_size, 2000);
    assert_eq!(row.left, 0);
    assert_eq!(row.dir, "/home/u/Downloads");
    assert_eq!(row.percent(), 50.0);
    assert_eq!(row.ratio(), Some(0.25), "250 uploaded over 1000 downloaded");
    assert_eq!(row.state(), "downloading");
    // The same words the CLI prints, from the same numbers.
    assert_eq!(row.state(), download("x").state());
}

/// A file resumed from disk has reported over 100%, and a panel showing
/// `120%` is showing a bug rather than a fact.
#[test]
fn test_progress_over_one_is_shown_as_a_hundred() {
    let mut d = download("x");
    d.fraction = 1.4;
    assert_eq!(to_row(&d).percent(), 100.0);
}

/// A panel row and a `TorrentStatus` answer the same questions, so the two
/// must not drift: this pins the state word, which is what both the panel
/// and the CLI print from the same number.
#[test]
fn test_the_state_word_is_the_same_wherever_it_is_printed() {
    use doris::transmission::state_of;
    for code in 0..=6 {
        let mut d = download("x");
        d.status = code;
        assert_eq!(to_row(&d).state(), state_of(code), "status {code}");
    }
}

/// A row with nothing downloaded has no ratio, and `0.00` would look like
/// a measurement.
#[test]
fn test_a_fresh_download_has_no_ratio() {
    let mut d = download("x");
    d.downloaded = 0;
    assert_eq!(to_row(&d).ratio(), None);
    assert_eq!(DownloadRow::default().percent(), 0.0);
}
