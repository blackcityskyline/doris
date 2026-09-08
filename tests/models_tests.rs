use doris::search::models::*;

#[test]
fn test_torrent_item_full_json() {
    let json = r#"{
        "title": "Ubuntu 24.04 LTS",
        "size": "4.2 GB",
        "seeds": "150",
        "download_url": "/forum/dl.php?t=12345",
        "query": "ubuntu",
        "date": "01-Янв-26",
        "page_url": "viewtopic.php?t=12345"
    }"#;
    let item: TorrentItem = serde_json::from_str(json).unwrap();
    assert_eq!(item.title, "Ubuntu 24.04 LTS");
    assert_eq!(item.size, "4.2 GB");
    assert_eq!(item.seeds, "150");
    assert_eq!(item.download_url, "/forum/dl.php?t=12345");
    assert_eq!(item.date, "01-Янв-26");
}

#[test]
fn test_torrent_item_minimal_json() {
    let json = r#"{"title": "Test"}"#;
    let item: TorrentItem = serde_json::from_str(json).unwrap();
    assert_eq!(item.title, "Test");
    assert_eq!(item.size, "");
    assert_eq!(item.seeds, "");
    assert_eq!(item.download_url, "");
    assert_eq!(item.date, "");
    assert_eq!(item.page_url, "");
}

#[test]
fn test_torrent_item_vec_deserialize() {
    let json = r#"[
        {"title": "First", "seeds": "10", "size": "1 GB"},
        {"title": "Second", "seeds": "5", "size": "2 GB"}
    ]"#;
    let items: Vec<TorrentItem> = serde_json::from_str(json).unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].title, "First");
    assert_eq!(items[1].title, "Second");
}

#[test]
fn test_torrent_item_empty_array() {
    let json = "[]";
    let items: Vec<TorrentItem> = serde_json::from_str(json).unwrap();
    assert_eq!(items.len(), 0);
}

#[test]
fn test_resolve_url_absolute() {
    assert_eq!(
        resolve_url("https://rutracker.org/forum/dl.php?t=123"),
        "https://rutracker.org/forum/dl.php?t=123"
    );
}

#[test]
fn test_resolve_url_root_relative() {
    assert_eq!(
        resolve_url("/forum/dl.php?t=123"),
        "https://rutracker.org/forum/dl.php?t=123"
    );
}

#[test]
fn test_resolve_url_bare_path() {
    assert_eq!(
        resolve_url("dl.php?t=123"),
        "https://rutracker.org/forum/dl.php?t=123"
    );
}

#[test]
fn test_resolve_url_bare_viewtopic() {
    assert_eq!(
        resolve_url("viewtopic.php?t=123&start=0"),
        "https://rutracker.org/forum/viewtopic.php?t=123&start=0"
    );
}

#[test]
fn test_resolve_url_with_query_params() {
    assert_eq!(
        resolve_url("dl.php?t=4682196"),
        "https://rutracker.org/forum/dl.php?t=4682196"
    );
}
