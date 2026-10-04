//! Starting and stopping the TorrServer the streaming path needs.
//!
//! The dangerous operation here is not starting a process. It is stopping
//! one: a pid is a number, and stopping whatever number a file happens to
//! hold is how a program kills something it never started. So the rules are
//! checked here rather than in a comment -- a pid file that does not exist
//! means there is nothing of ours to stop, and a pid whose process is not
//! TorrServer is not ours either, whatever the file says.

use doris::torrserver::service;
use serial_test::serial;

/// A pid that is certainly not running, so the tests never signal a stranger.
const NO_SUCH_PID: i32 = i32::MAX;

/// A `$HOME` of this test's own, so every path under it is scratch.
///
/// `state_dir()` is `~/.local/share/doris` and two of these tests *write*
/// there: one deleted the pid file if it found one, another overwrote it.
/// That is the user's own state on a machine where doris has started its own
/// TorrServer -- which is the designed behaviour, it is meant to outlive
/// doris -- and deleting that pid file quietly makes doris unable to stop the
/// process it started. It also made this file's own tests lie: the
/// missing-binary test asserted its error message and got "already started
/// here", which is a true answer about the machine and the wrong one to be
/// testing.
///
/// `#[serial]` because `$HOME` is process-wide: these four tests change it,
/// and a `#[serial]` only orders against other `#[serial]` tests.
fn scratch_home(name: &str) {
    let dir = std::env::temp_dir().join(format!("doris-tsvc-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join(".local/share/doris")).expect("a scratch state dir");
    std::env::set_var("HOME", &dir);
}

/// Nothing of ours is running: stopping says so, and does not invent a pid.
///
/// The message is the whole point. "Stopped" when nothing was started is a
/// lie a user acts on -- they go and look for a service that is still up.
#[test]
#[serial]
fn stopping_without_a_pid_stops_nothing_and_says_so() {
    scratch_home("stopping_without_a_pid");
    assert_eq!(
        service::owned_pid(),
        None,
        "a scratch state dir has nothing in it, which is the premise here"
    );
    let what = service::stop();
    assert!(
        what.contains("nothing of ours"),
        "the answer names that nothing was stopped: {what}"
    );
    assert!(
        !what.contains("stopped (pid"),
        "and never claims a pid it did not signal: {what}"
    );
}

/// A pid file naming a process that is not TorrServer is a stale claim.
///
/// Two ways it happens: the server was restarted by something else and the
/// number was reused, or the file was written by an older version. Either
/// way, signalling it would stop an unrelated program.
#[test]
#[serial]
fn a_pid_that_is_not_torrserver_is_never_stopped() {
    scratch_home("a_pid_that_is_not");
    // This process: definitely running, definitely not TorrServer.
    let mine = std::process::id() as i32;
    assert!(mine != NO_SUCH_PID);
    std::fs::create_dir_all(service::pid_file().parent().expect("a parent dir")).unwrap();
    std::fs::write(service::pid_file(), format!("{mine}\n")).expect("write the stale pid");

    assert_eq!(
        service::owned_pid(),
        None,
        "a pid file holding our own pid is not a claim about TorrServer -- \
         and this binary's own name contains \"torrserver\", which is how a \
         `contains` check came to claim it was the server"
    );
    let what = service::stop();
    assert!(
        what.contains("nothing of ours"),
        "and stopping it leaves us alone: {what}"
    );
    assert_eq!(
        mine,
        std::process::id() as i32,
        "this process is still here"
    );

    // A pid nobody has: the same answer, and no signal sent to the void.
    std::fs::write(service::pid_file(), format!("{NO_SUCH_PID}\n")).expect("write");
    assert_eq!(service::owned_pid(), None);
    assert!(service::stop().contains("nothing of ours"));
    let _ = std::fs::remove_file(service::pid_file());
}

/// Where the pid file is, and where the cache goes.
///
/// Both are answers to "where does this put things", and both have to be
/// somewhere that does not depend on the working directory: a detached
/// process has no working directory anybody chose, and its own default is
/// `./settings` there.
#[test]
fn the_pid_file_and_the_cache_live_beside_doris_state() {
    let pid = service::pid_file();
    assert_eq!(
        pid,
        doris::log::state_dir().join("torrserver.pid"),
        "a pid is state, and state belongs beside the program that owns it"
    );
    assert_eq!(
        service::data_dir(""),
        doris::log::state_dir().join("torrserver"),
        "and so does the cache: TorrServer's own default is relative"
    );
    assert_eq!(
        service::data_dir("/mnt/media/ts"),
        std::path::PathBuf::from("/mnt/media/ts"),
        "an explicit directory wins"
    );
}

/// Which binary, and the failure a user can act on.
///
/// The error is the deliverable here: "not on PATH" with no next step is the
/// message that sends someone to the web instead of to the two lines that
/// would fix it.
#[test]
#[serial]
fn a_missing_binary_is_reported_with_the_way_out() {
    scratch_home("a_missing_binary");
    let err = service::start("/nowhere/torrserver", "", false)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("torrserver_path") || err.contains("install"),
        "the error names a way to fix it: {err}"
    );
}

/// Starting when something already answers does nothing at all.
///
/// Two servers on one port is not a louder failure, it is a process that
/// answers the wrong requests -- and the pid file would then name a process
/// that lost the race, which is the state the stop rule exists to avoid.
#[test]
#[serial]
fn starting_with_something_already_answering_starts_nothing() {
    scratch_home("starting_with_something");
    let what = service::start("/nowhere/torrserver", "", true).unwrap();
    assert!(what.contains("already answering"), "and says why: {what}");
    assert_eq!(
        service::owned_pid(),
        None,
        "no pid written for a server that was not ours to start"
    );
}

/// An explicit path that is not there is not silently a PATH lookup.
///
/// Otherwise `torrserver_path = "/opt/torr/torrserver"` on a machine where
/// the binary moved to `/usr/bin` keeps working from somewhere the user did
/// not write, which is a configuration that cannot be reasoned about.
#[test]
fn an_explicit_path_is_not_replaced_by_a_path_lookup() {
    assert_eq!(
        service::binary("/definitely/not/here"),
        None,
        "a named path that does not exist is not a request for the PATH one"
    );
    assert_eq!(
        service::binary("   "),
        service::binary(""),
        "whitespace is empty"
    );
}
