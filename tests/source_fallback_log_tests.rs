//! The log line written when a result row is sent to a source it does
//! not name.
//!
//! In its own file on purpose. These tests set `HOME` so the file
//! logger writes somewhere they can read, and `HOME` is a process-wide
//! variable: another test in the same binary running at the same moment
//! would see the temporary one. `#[serial]` only serialises tests that ask
//! for it, and the other 36 tests in the old file did not -- which showed
//! up as a flake under a full parallel run. One test binary, one process,
//! one `HOME` to change. `#[serial]` still matters *inside* the file:
//! `HOME` is per-process, not per-test, and two tests setting it at the
//! same moment is the same race one file over.

use doris::app::source_id_for;
use doris::sources::models::TorrentItem;
use serial_test::serial;

fn item_with_source(source: &str) -> TorrentItem {
    TorrentItem {
        source: source.to_string(),
        ..Default::default()
    }
}

/// Run `f` with `HOME` pointed at a fresh directory, and hand back what
/// the logger wrote.
fn logged_while<F: FnOnce()>(name: &str, f: F) -> String {
    let dir = std::env::temp_dir().join(format!("doris-log-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let real_home = std::env::var("HOME").ok();
    std::env::set_var("HOME", &dir);
    doris::log::init();

    f();

    let log = std::fs::read_to_string(dir.join(".local/share/doris/doris.log")).unwrap();
    if let Some(home) = real_home {
        std::env::set_var("HOME", home);
    }
    let _ = std::fs::remove_dir_all(&dir);
    log
}

/// A row whose id is not in the registry is sent to rutracker, and that
/// substitution is said out loud: it is the one place a wrong id turns
/// into a request to somebody else's server, and the failure the user
/// sees is a rutracker error about a row they did not ask about.
#[test]
#[serial]
fn a_substituted_source_id_is_reported_not_silent() {
    let log = logged_while("subst", || {
        // A wrong id and an id from before the field existed both
        // substitute.
        source_id_for(&item_with_source("never-heard-of-it"));
        source_id_for(&item_with_source(""));
    });

    assert_eq!(
        log.matches("no registered source").count(),
        2,
        "both substitutions must be reported:\n{log}"
    );
    assert!(log.contains("never-heard-of-it"), "and name the id:\n{log}");
    assert!(
        log.contains("talking to rutracker instead"),
        "and say what happened instead:\n{log}"
    );
}

/// A row that *is* in the registry says nothing -- the log is for the
/// substitution, not a running commentary on every download.
#[test]
#[serial]
fn a_registered_source_id_is_not_reported() {
    let log = logged_while("ok", || {
        source_id_for(&item_with_source("rutor"));
    });

    assert!(
        !log.contains("no registered source"),
        "a row that routes correctly must be quiet:\n{log}"
    );
}
