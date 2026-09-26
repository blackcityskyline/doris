//! The chromedriver cache contract: one patched driver per browser
//! major, and a driver is only used for the major it reports.
//!
//! Regression tests for the defect behind `get_or_patch_chromedriver`
//! returning a single un-suffixed cached file for every browser: a 152
//! driver handed to Helium 154 died with "This version of ChromeDriver
//! only supports Chrome version 152" (live, 25.09.2026), so switching
//! the browser priority in Options broke session startup.
//!
//! Everything here runs offline against temp files: the binaries are
//! stand-ins that print a version line, exactly what `driver_serves`
//! asks the real ones.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use doris::browser::cdp::{
    adopt_legacy_download, driver_serves, has_patched_chromedriver, patched_chromedriver_path,
};

/// A directory of its own per test, cleared first so a leftover from an
/// earlier run cannot answer for the current one.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("doris-cd-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// A stand-in driver that prints `version_line` when asked its version.
fn fake_driver(dir: &Path, name: &str, version_line: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\necho '{}'\n", version_line)).expect("write");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

#[test]
fn patched_cache_is_keyed_by_browser_major() {
    let dir = scratch("keyed");

    let v152 = patched_chromedriver_path(&dir, 152);
    let v153 = patched_chromedriver_path(&dir, 153);
    let v154 = patched_chromedriver_path(&dir, 154);

    assert_ne!(v152, v153, "two majors must never share a cache file");
    assert_ne!(v153, v154);
    assert_eq!(
        v152.file_name().unwrap().to_string_lossy(),
        "chromedriver_patched-152",
        "the un-suffixed name is the defect, not the contract"
    );

    // Nothing cached yet: no major may claim a driver it does not have.
    assert!(!has_patched_chromedriver(&dir, 152));

    fs::write(&v152, b"driver for 152").expect("write cache");
    assert!(has_patched_chromedriver(&dir, 152));
    assert!(
        !has_patched_chromedriver(&dir, 154),
        "a 152 file must not answer for 154 -- that is the live failure"
    );

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_driver_serves_only_its_own_major() {
    let dir = scratch("serves");

    // The exact line this machine's cached driver prints.
    let driver_152 = fake_driver(
        &dir,
        "chromedriver-152",
        "ChromeDriver 152.0.7977.82 (d04cdb24d67b081f6cf80200ffc5233f44b61109)",
    );
    assert!(driver_serves(&driver_152, 152));
    assert!(
        !driver_serves(&driver_152, 154),
        "Helium 154 must not be started with the 152 driver"
    );
    assert!(!driver_serves(&driver_152, 153));

    let driver_154 = fake_driver(&dir, "chromedriver-154", "ChromeDriver 154.0.8037.57 (abc)");
    assert!(driver_serves(&driver_154, 154));
    assert!(!driver_serves(&driver_154, 152));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_binary_without_a_readable_version_serves_nothing() {
    let dir = scratch("no-version");

    // Answers, but with no version in the answer.
    let silent = fake_driver(&dir, "silent", "");
    assert!(!driver_serves(&silent, 152));

    // Not a version line at all.
    let junk = fake_driver(&dir, "junk", "usage: chromedriver [options]");
    assert!(!driver_serves(&junk, 152));

    // Absent and not executable: both must read as "does not serve"
    // rather than as an error the caller has to unwind.
    assert!(!driver_serves(&dir.join("missing"), 152));
    let unreadable = fake_driver(&dir, "unreadable", "ChromeDriver 152.0.7977.82");
    fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o644)).expect("chmod");
    assert!(!driver_serves(&unreadable, 152));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_un_keyed_download_moves_only_to_the_major_it_serves() {
    let root = scratch("adopt");
    let legacy_dir = root.join("chromedriver-linux64");
    fs::create_dir_all(&legacy_dir).expect("legacy dir");
    let legacy = fake_driver(&legacy_dir, "chromedriver", "ChromeDriver 152.0.7977.82 (abc)");

    // Another browser's major: nothing moves, and 152 keeps the file it
    // was downloaded for.
    assert!(adopt_legacy_download(&root, 154).is_none());
    assert!(legacy.exists(), "the download must stay for its own browser");

    // Its own major: moved under the keyed path, where `find_or_download`
    // will look for it, and still answering as 152 after the move.
    let adopted = adopt_legacy_download(&root, 152).expect("adopted");
    assert_eq!(adopted, root.join("152/chromedriver-linux64/chromedriver"));
    assert!(adopted.exists());
    assert!(driver_serves(&adopted, 152));
    assert!(!legacy.exists(), "the un-keyed path is left behind");

    // Nothing left to adopt for a second caller.
    assert!(adopt_legacy_download(&root, 152).is_none());

    let _ = fs::remove_dir_all(&root);
}
