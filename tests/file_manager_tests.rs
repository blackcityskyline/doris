//! Which file manager `o` opens a download in.
//!
//! The setting is a promise about what a key will run, so what is tested
//! here is the promise: a name that is on the list is the name that runs,
//! a name that is not installed says so rather than running something else
//! silently, and `auto` cannot land on a manager that opens a window when a
//! terminal one is there -- because a window opened from the alternate
//! screen is a window nobody can see.

use doris::app::files::{self, Manager, AUTO};

/// A manager that is definitely not on this machine, for the "not installed"
/// half of the promise. `definitely-not-a-file-manager` is the kind of name
/// a typo produces.
const MISSING: &str = "definitely-not-a-file-manager";

/// Everything on the list can be chosen, and every key a name of.
#[test]
fn every_manager_is_reachable_by_its_key() {
    for manager in files::MANAGERS {
        assert_eq!(
            files::by_key(manager.key).map(|m| m.program),
            Some(manager.program),
            "{} answers to its own key",
            manager.key
        );
    }
    assert!(
        files::by_key(AUTO).is_none(),
        "`auto` is a choice, not a manager: nothing should run because of it"
    );
}

/// The setting and the list are one thing. A name the Options modal cycles
/// onto that `pick` does not know is a setting that cannot be honoured.
#[test]
fn the_choosable_keys_are_exactly_the_list() {
    let keys = files::keys();
    assert_eq!(keys[0], AUTO, "`auto` is offered first");
    assert_eq!(
        keys.len(),
        files::MANAGERS.len() + 1,
        "one `auto` and every manager, no more"
    );
    for manager in files::MANAGERS {
        assert!(
            keys.contains(&manager.key),
            "{} is not choosable",
            manager.key
        );
    }
}

/// A named manager that is installed is what runs, even when it is not the
/// one `auto` would have picked.
///
/// The probe is the *last* installed manager on the list, found rather than
/// hard-coded: anything earlier would also be what `auto` returns, and a
/// test that cannot tell those apart is a test that says nothing.
#[test]
fn a_named_installed_manager_is_the_one_that_runs() {
    let installed: Vec<&Manager> = files::MANAGERS
        .iter()
        .filter(|m| which::which(m.program).is_ok())
        .collect();
    let last = installed
        .last()
        .expect("a machine with no file manager at all is not this one");
    assert_ne!(
        installed.first().map(|m| m.program),
        Some(last.program),
        "need a machine with two, or the test cannot see the difference"
    );

    let (manager, missing) = files::pick(last.key);
    assert_eq!(
        manager.map(|m| m.program),
        Some(last.program),
        "the named one, not the first installed"
    );
    assert_eq!(missing, None, "and nothing is reported missing");
}

/// A named manager that is not installed must not be silently replaced.
///
/// The alternative -- fall through to whatever is installed without saying
/// so -- is how a user ends up with a setting they changed and a program
/// they did not choose, and no way to tell which happened.
#[test]
fn a_named_manager_that_is_not_installed_says_so_and_falls_back() {
    let (manager, missing) = files::pick(MISSING);
    assert_eq!(
        missing,
        Some(MISSING),
        "the name that could not be honoured is named"
    );
    assert!(
        manager.is_some(),
        "and something still opens, so the key is not dead"
    );
    assert_ne!(
        manager.map(|m| m.key),
        Some(MISSING),
        "but not the one that is not there"
    );
}

/// `auto` walks the list in order and takes the first that is installed, so
/// it never returns a name that is not there.
#[test]
fn auto_never_names_something_that_is_not_installed() {
    let (manager, missing) = files::pick(AUTO);
    assert_eq!(missing, None, "auto has nothing to report missing");
    let manager = manager.expect("xdg-open is always somewhere, or the code lies");
    assert!(
        which::which(manager.program).is_ok(),
        "auto returned {}, which is not installed",
        manager.program
    );
}

/// An empty setting is `auto` rather than nothing: a config written before
/// this field existed has no value for it, and a file manager that stopped
/// working because a field is missing is a file manager that stopped.
#[test]
fn an_empty_setting_is_auto() {
    let (from_empty, missing) = files::pick("");
    let (from_auto, _) = files::pick(AUTO);
    assert_eq!(
        from_empty.map(|m| m.program),
        from_auto.map(|m| m.program),
        "an absent value means the same as `auto`"
    );
    assert_eq!(missing, None);
}

/// The two kinds really are two kinds, and the terminal ones really are the
/// ones that take the terminal.
///
/// This is what decides whether the alternate screen is handed over: a
/// desktop manager run inside it opens a window nobody can see, and the
/// whole reason the table carries a flag is that the flag is read.
#[test]
fn terminal_and_desktop_managers_do_not_overlap() {
    for manager in files::tui_managers() {
        assert!(
            manager.takes_terminal,
            "{} is listed as terminal",
            manager.key
        );
    }
    for manager in files::gui_managers() {
        assert!(
            !manager.takes_terminal,
            "{} is listed as desktop",
            manager.key
        );
    }
    assert_eq!(
        files::tui_managers().count() + files::gui_managers().count(),
        files::MANAGERS.len(),
        "every manager is in exactly one of the two lists"
    );
}

/// The Options modal says what it means.
///
/// `yazi (not installed)` on a machine without yazi is the difference
/// between a setting and a guess: the panel names the manager the key will
/// try, and says when the promise cannot be kept.
#[test]
fn the_options_panel_says_when_a_choice_cannot_be_kept() {
    assert_eq!(files::display_name(AUTO), AUTO);
    assert_eq!(
        files::display_name(""),
        AUTO,
        "an unset value reads as auto"
    );

    let unknown = files::display_name("not-a-manager");
    assert!(
        unknown.contains("unknown"),
        "a name off the list says so: {unknown}"
    );
    assert!(
        unknown.contains("not-a-manager"),
        "and keeps the name the user typed: {unknown}"
    );

    for manager in files::MANAGERS {
        let shown = files::display_name(manager.key);
        let installed = which::which(manager.program).is_ok();
        assert_eq!(
            shown.contains("not installed"),
            !installed,
            "{} shows `{shown}` but installed={installed}",
            manager.key
        );
        if installed {
            assert!(
                shown.contains(manager.key),
                "an installed manager is named plainly: {shown}"
            );
        }
    }
}

/// `xdg-open` is the fallback and not on the list: it is what the session
/// already uses, so it is the right last resort and the wrong thing to offer
/// as a choice -- a user who picked a manager picked *a* manager.
#[test]
fn the_fallback_is_not_offered_as_a_choice() {
    assert!(
        !files::MANAGERS.iter().any(|m| m.program == "xdg-open"),
        "xdg-open is a fallback, not an option"
    );
    assert!(
        !files::keys().contains(&"xdg-open"),
        "and it is not in the cycle"
    );
}

/// The speed ramp is in the same file and must stay sorted and positive: a
/// ramp that goes down is a `-` that makes things faster.
#[test]
fn the_speed_ramp_goes_up() {
    let steps = files::SPEED_STEPS;
    assert!(steps.len() > 1, "a ramp needs steps");
    for pair in steps.windows(2) {
        assert!(pair[0] < pair[1], "the ramp turns back at {pair:?}");
    }
    assert!(steps.iter().all(|s| *s > 0), "a limit of zero is a stop");
}
