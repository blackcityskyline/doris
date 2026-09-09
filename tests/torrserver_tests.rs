use doris::torrserver::api::TorrentInfo;

// TorrServer's Go structs have no `json` tags, so its default JSON output
// uses the exact (capitalized) Go field names. This test locks in that
// assumption -- see the doc comment on TorrentInfo for sources and the
// caveat that some forks may differ slightly. If a real TorrServer
// instance's `/torrents` response doesn't match this shape, this is the
// test (and the #[serde(rename = ...)] list in torrserver/api.rs) to fix.
const SAMPLE_LIST_JSON: &str = r#"
[
  {
    "Name": "Big Buck Bunny",
    "Hash": "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c",
    "TorrentSize": 1000000000,
    "LoadedSize": 250000000,
    "DownloadSpeed": 1048576.0,
    "UploadSpeed": 65536.0,
    "TotalPeers": 12,
    "ActivePeers": 5,
    "ConnectedSeeders": 3,
    "TorrentStatusString": "Downloading"
  }
]
"#;

#[test]
fn test_parse_torrent_list_response() {
    let list: Vec<TorrentInfo> = serde_json::from_str(SAMPLE_LIST_JSON).unwrap();
    assert_eq!(list.len(), 1);
    let t = &list[0];
    assert_eq!(t.name, "Big Buck Bunny");
    assert_eq!(t.hash, "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c");
    assert_eq!(t.total_size, 1_000_000_000);
    assert_eq!(t.loaded_size, 250_000_000);
    assert_eq!(t.total_peers, 12);
    assert_eq!(t.status_string, "Downloading");
    assert!((t.progress() - 0.25).abs() < 1e-9);
}

#[test]
fn test_parse_empty_torrent_list() {
    let list: Vec<TorrentInfo> = serde_json::from_str("[]").unwrap();
    assert!(list.is_empty());
}

#[test]
fn test_torrent_info_missing_fields_default_instead_of_failing() {
    // A fork with a slightly different shape (extra or missing fields)
    // should still parse -- unknown fields are ignored by default, and
    // every field on TorrentInfo has #[serde(default)].
    let json = r#"{"Hash": "abc123", "SomeExtraField": 42}"#;
    let t: TorrentInfo = serde_json::from_str(json).unwrap();
    assert_eq!(t.hash, "abc123");
    assert_eq!(t.total_size, 0);
    assert_eq!(t.progress(), 0.0);
}

#[test]
fn test_progress_clamped_and_no_division_by_zero() {
    let t = TorrentInfo { total_size: 0, loaded_size: 500, ..Default::default() };
    assert_eq!(t.progress(), 0.0);

    let t = TorrentInfo { total_size: 100, loaded_size: 200, ..Default::default() };
    assert_eq!(t.progress(), 1.0);
}
