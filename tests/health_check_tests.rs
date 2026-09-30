//! The health check's cookie line.
//!
//! It used to name `Path::new("cookies.txt")` inside the check, ignoring
//! `config.cookie_file` and `--cookie-file`. Since the cookie path is
//! configurable -- and now defaults to an absolute path beside the config
//! rather than to the working directory -- a user with a real session
//! somewhere else was told their file was missing while the check measured
//! a file the app would never read.

#![cfg(unix)]

use doris::sources::cookies::{save_to_file, Cookie};
use doris::ui::modals::health::cookie_file_status;
use std::path::PathBuf;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("doris-health-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

fn a_session() -> Vec<Cookie> {
    vec![Cookie {
        domain: ".rutracker.org".to_string(),
        path: "/".to_string(),
        secure: true,
        name: "bb_session".to_string(),
        value: "live-session".to_string(),
    }]
}

#[test]
fn it_reports_the_file_it_was_given_not_one_in_the_working_directory() {
    let dir = scratch("given");
    let mine = dir.join("my-session.txt");
    save_to_file(&mine, &a_session()).unwrap();

    // A `cookies.txt` in the CWD is what the old hardcoded check read.
    // It must not be what this one reads.
    let cwd_has_one = std::path::Path::new("cookies.txt").exists();
    let cwd_cookie = std::path::Path::new("cookies.txt");
    let stashed = dir.join("stashed.txt");
    if cwd_has_one {
        std::fs::rename(cwd_cookie, &stashed).unwrap();
    }
    std::fs::write(cwd_cookie, "# Netscape HTTP Cookie File\n").unwrap();

    let line = cookie_file_status(&mine);

    if cwd_has_one {
        std::fs::rename(&stashed, cwd_cookie).unwrap();
    }
    let _ = std::fs::remove_file(cwd_cookie);

    assert!(
        line.contains('✔') && line.contains("1 cookies"),
        "must report the file it was handed, got: {line}"
    );
    assert!(
        line.contains(&mine.display().to_string()),
        "the line must name the path it checked, got: {line}"
    );
}

#[test]
fn an_empty_file_is_a_warning_not_a_pass() {
    let dir = scratch("empty");
    let path = dir.join("cookies.txt");
    save_to_file(&path, &[]).unwrap();

    let line = cookie_file_status(&path);

    assert!(line.contains('⚠'), "an empty file is not a pass: {line}");
    assert!(line.contains("empty/invalid"), "{line}");
}

/// The one that matters: the check must report the path it is handed.
///
/// Checking `cookie_file_status` alone would not have caught the original
/// bug -- the hardcoded `cookies.txt` lived in `health_check`, upstream of
/// that function, so testing the function passed while the check went on
/// reading a file the app would never use. Reverting the call site to the
/// hardcoded path is what makes this test fail.
///
/// It costs one GET to the configured TorrServer (`is_reachable`, 2s
/// timeout) and a browser probe, because `health_check` does both before
/// it gets to the cookies.
#[tokio::test]
async fn the_check_reports_the_path_it_is_handed() {
    let dir = scratch("wired");
    let mine = dir.join("my-session.txt");
    save_to_file(&mine, &a_session()).unwrap();

    let app = doris::ui::app::App::new("http://127.0.0.1:1".into(), None);
    let lines = app.health_check(Some(&mine)).await;
    let cookie_line = lines
        .iter()
        .find(|l| l.contains("Cookie file"))
        .expect("the check reports on the cookie file");

    assert!(
        cookie_line.contains(&mine.display().to_string()),
        "the check must name the file it was handed, got: {cookie_line}"
    );
    assert!(
        !cookie_line.contains("not found"),
        "a real session must not be reported missing: {cookie_line}"
    );

    // And with no file to look at, it says so instead of inventing one.
    let lines = app.health_check(None).await;
    let cookie_line = lines
        .iter()
        .find(|l| l.contains("Cookie file"))
        .expect("the check reports on the cookie file");
    assert!(
        cookie_line.contains("not saved"),
        "with nothing configured the check must not measure a file nobody named: {cookie_line}"
    );
}

#[test]
fn a_missing_file_names_the_path_it_looked_for() {
    let dir = scratch("missing");
    let path = dir.join("not-written-yet.txt");

    let line = cookie_file_status(&path);

    assert!(line.contains('⚠'), "{line}");
    assert!(
        line.contains("not found") && line.contains(&path.display().to_string()),
        "a user whose file is elsewhere needs to see where the check looked: {line}"
    );
}
