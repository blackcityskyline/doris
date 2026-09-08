use doris::search::cookies::*;
use serial_test::serial;

#[test]
fn test_parse_netscape_basic() {
    let content = "# Netscape HTTP Cookie File\n\
        .rutracker.org\tTRUE\t/\tTRUE\t0\tbb_session\tabc123\n\
        .rutracker.org\tTRUE\t/\tFALSE\t0\tbb_data\txyz789\n";
    let cookies = parse_netscape(content);
    assert_eq!(cookies.len(), 2);
    assert_eq!(cookies[0].name, "bb_session");
    assert_eq!(cookies[0].value, "abc123");
    assert_eq!(cookies[0].domain, ".rutracker.org");
    assert_eq!(cookies[0].path, "/");
    assert!(cookies[0].secure);
    assert_eq!(cookies[1].name, "bb_data");
    assert!(!cookies[1].secure);
}

#[test]
fn test_parse_netscape_skips_comments() {
    let content = "# This is a comment\n# Another comment\n.rutracker.org\tTRUE\t/\tTRUE\t0\tsession\tval\n";
    let cookies = parse_netscape(content);
    assert_eq!(cookies.len(), 1);
    assert_eq!(cookies[0].name, "session");
}

#[test]
fn test_parse_netscape_skips_empty_lines() {
    let content = "\n\n\n.rutracker.org\tTRUE\t/\tTRUE\t0\tsession\tval\n\n\n";
    let cookies = parse_netscape(content);
    assert_eq!(cookies.len(), 1);
}

#[test]
fn test_parse_netscape_short_line_ignored() {
    let content = "too\tfew\tcolumns\n";
    let cookies = parse_netscape(content);
    assert_eq!(cookies.len(), 0);
}

#[test]
fn test_parse_netscape_cf_clearance() {
    let content = ".rutracker.org\tTRUE\t/\tTRUE\t0\tcf_clearance\tdef456\n\
        .rutracker.org\tTRUE\t/\tFALSE\t0\tbb_guid\tguid123\n\
        .rutracker.org\tTRUE\t/\tFALSE\t0\tbb_ssl\t1\n";
    let cookies = parse_netscape(content);
    assert_eq!(cookies.len(), 3);
    assert_eq!(cookies[0].name, "cf_clearance");
    assert_eq!(cookies[1].name, "bb_guid");
    assert_eq!(cookies[2].name, "bb_ssl");
}

#[test]
fn test_parse_netscape_empty() {
    let cookies = parse_netscape("");
    assert_eq!(cookies.len(), 0);
}

#[test]
#[serial]
fn test_save_and_reload() {
    let dir = std::env::temp_dir().join("doris-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test_cookies.txt");

    let cookies = vec![
        Cookie {
            domain: "rutracker.org".to_string(),
            path: "/".to_string(),
            secure: true,
            name: "bb_session".to_string(),
            value: "test123".to_string(),
        },
        Cookie {
            domain: ".rutracker.org".to_string(),
            path: "/forum".to_string(),
            secure: false,
            name: "cf_clearance".to_string(),
            value: "cf456".to_string(),
        },
    ];

    save_to_file(&path, &cookies).unwrap();
    let loaded = load_from_file(&path).unwrap();
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].name, "bb_session");
    assert_eq!(loaded[0].value, "test123");
    assert!(loaded[0].domain.starts_with('.'));
    assert_eq!(loaded[1].name, "cf_clearance");

    std::fs::remove_dir_all(&dir).unwrap();
}
