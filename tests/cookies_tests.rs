use doris::sources::cookies::*;
use serial_test::serial;

/// The payload `Browser::add_cookies` wants, in a form Chrome accepts.
/// The leading dot of a Netscape domain is the *Set-Cookie* spelling and
/// `Network.setCookie` rejects it ("invalid cookie domain"), so the domain
/// goes in bare -- live, this is what kept the session an app run saved
/// from being re-injected at all, and every run from re-logging-in.
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

/// A saved cookie file is a live login: `bb_session` is the session, and
/// anyone who reads it can act as the user on rutracker. `save_to_file`
/// went through `fs::write`, which creates 0644 under the default umask
/// 022 -- readable by every account on the machine. 0600 is the whole
/// protection here, so the file is created with it and *repaired* on a
/// file that already exists: the mode argument only applies at creation,
/// so a store written by an older build keeps its 0644 otherwise.
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
