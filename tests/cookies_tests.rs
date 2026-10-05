use doris::sources::cookies::*;
use serial_test::serial;

/// The payload `Browser::add_cookies` wants, in a form Chrome accepts.
#[test]
fn test_to_json_drops_the_set_cookie_dot_from_the_domain() {
    let cookie = Cookie {
        domain: ".rutracker.org".to_string(),
        path: "/forum/".to_string(),
        secure: true,
        name: "bb_session".to_string(),
        value: "abc123".to_string(),
    };
    let json = cookie.to_json();
    assert_eq!(json["domain"], "rutracker.org");
    assert_eq!(json["name"], "bb_session");
    assert_eq!(json["value"], "abc123");
    assert_eq!(json["path"], "/forum/");
    assert_eq!(json["secure"], true);

    // A domain that is already bare survives untouched.
    let bare = Cookie {
        domain: "rutracker.org".to_string(),
        ..cookie
    };
    assert_eq!(bare.to_json()["domain"], "rutracker.org");
}

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
    let content =
        "# This is a comment\n# Another comment\n.rutracker.org\tTRUE\t/\tTRUE\t0\tsession\tval\n";
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

/// A saved cookie file is a live login: `bb_session` is the session, and anyone who reads it
/// can act as the user on rutracker.
#[test]
#[cfg(unix)]
fn test_saved_cookies_are_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = std::env::temp_dir().join(format!("doris-cook-perm-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cookies.txt");

    save_to_file(&path, &[]).unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "a fresh save must be owner-only");

    // An already-world-readable file is tightened, not left as it was.
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    save_to_file(&path, &[]).unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "an existing 0644 file must be repaired");

    std::fs::remove_dir_all(&dir).unwrap();
}

/// The jar is one file for every source, and a browser handed a cookie for a
/// site it was not opened on answers an error -- measured on 05.10.2026, where
/// signing into ext failed in ten seconds with rutracker's cookies in the jar
/// and succeeded in twenty-one with an empty one.
#[test]
fn test_each_source_takes_only_the_cookies_of_its_own_site() {
    let jar = parse_netscape(
        "# Netscape HTTP Cookie File\n\
         .rutracker.org\tTRUE\t/\tTRUE\t0\tbb_guid\t123\n\
         .rutracker.org\tTRUE\t/\tTRUE\t0\tcf_clearance\tabc\n\
         .ext.to\tTRUE\t/\tTRUE\t0\t__LOGIN\ttorum\n\
         .ext.to\tTRUE\t/\tTRUE\t0\tPHPSESSID\tdeadbeef\n\
         .nyaa.si\tTRUE\t/\tFALSE\t0\tuser\tn\n",
    );

    let names = |host: &str| -> Vec<String> {
        for_domain(&jar, host)
            .iter()
            .map(|c| c.name.clone())
            .collect()
    };
    assert_eq!(names("ext.to"), vec!["__LOGIN", "PHPSESSID"]);
    assert_eq!(names("rutracker.org"), vec!["bb_guid", "cf_clearance"]);
    assert_eq!(names("nyaa.si").len(), 1, "a third source keeps its own");
    assert!(
        names("yts.mx").is_empty(),
        "and a source with nothing in the jar gets nothing, not everything"
    );
}

/// A leading dot means the cookie covers subdomains, which is how a jar written
/// by the browser reads: `.rutracker.org` belongs to `forum.rutracker.org`.
#[test]
fn test_a_leading_dot_still_matches_a_subdomain() {
    let jar = parse_netscape(
        "# Netscape HTTP Cookie File\n\
         .rutracker.org\tTRUE\t/\tTRUE\t0\tbb_guid\t123\n\
         ext.to\tTRUE\t/\tTRUE\t0\t__LOGIN\tt\n",
    );
    let count = |host: &str| for_domain(&jar, host).len();
    assert_eq!(count("forum.rutracker.org"), 1);
    assert_eq!(count("www.ext.to"), 1);
    assert_eq!(
        count("notrutracker.org"),
        0,
        "a suffix that is not a label boundary is a different site"
    );
    assert_eq!(
        count("rutracker.org.evil.example"),
        0,
        "and a host that merely starts with ours is not ours"
    );
}

/// One jar, several trackers: writing one site's cookies must not take the
/// others' away. It did, on 05.10.2026 -- signing into ext replaced the file
/// with ext's six cookies and took rutracker's four with them, so the next
/// rutracker search asked for a password the user had given an hour earlier.
#[test]
fn test_saving_one_site_leaves_the_others_in_the_jar() {
    let dir = std::env::temp_dir().join(format!("doris-jar-merge-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cookies.txt");

    let jar = |domain: &str, name: &str, value: &str| Cookie {
        domain: domain.to_string(),
        path: "/".to_string(),
        secure: true,
        name: name.to_string(),
        value: value.to_string(),
    };

    save_for_domain(
        &path,
        "rutracker.org",
        &[
            jar(".rutracker.org", "bb_guid", "1"),
            jar(".rutracker.org", "cf_clearance", "rt"),
        ],
    )
    .expect("rutracker's cookies are saved");
    save_for_domain(
        &path,
        "ext.to",
        &[
            jar(".ext.to", "__LOGIN", "torum"),
            jar(".ext.to", "PHPSESSID", "dead"),
            jar(".ext.to", "cf_clearance", "ext"),
        ],
    )
    .expect("ext's cookies are saved");

    let after = load_from_file(&path).expect("the jar reads back");
    let names = |host: &str| -> Vec<String> {
        for_domain(&after, host)
            .iter()
            .map(|c| c.name.clone())
            .collect()
    };
    assert_eq!(names("rutracker.org"), vec!["bb_guid", "cf_clearance"]);
    assert_eq!(
        names("ext.to"),
        vec!["__LOGIN", "PHPSESSID", "cf_clearance"]
    );

    // And a second save replaces that site's own cookies rather than piling a
    // second copy of the same name next to the first.
    save_for_domain(&path, "ext.to", &[jar(".ext.to", "PHPSESSID", "fresh")]).unwrap();
    let after = load_from_file(&path).expect("the jar reads back");
    let sessions: Vec<&str> = for_domain(&after, "ext.to")
        .iter()
        .filter(|c| c.name == "PHPSESSID")
        .map(|c| c.value.as_str())
        .collect();
    assert_eq!(sessions, vec!["fresh"], "one session per name, the newest");
    assert_eq!(
        for_domain(&after, "rutracker.org").len(),
        2,
        "and the other site is still there after it"
    );

    std::fs::remove_dir_all(&dir).unwrap();
}
