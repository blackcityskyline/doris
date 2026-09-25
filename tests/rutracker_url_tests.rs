use doris::search::rutracker::resolve_url;

// Moved here from `models_tests.rs` together with `resolve_url` itself:
// the function hardcodes rutracker's host, so it belongs to the
// rutracker source, not to the source-agnostic models module (B0.5).

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
