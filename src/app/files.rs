//! The file managers `o` can open a download in, and which one it picks.
//!
//! Two kinds, and the difference is the whole reason they are not one list.
//! A terminal manager draws a full-screen browser and needs the terminal
//! handed to it and taken back; a desktop manager opens a window of its own
//! and doris carries on drawing. Running the second kind inside the
//! alternate screen puts it somewhere nobody can see, which is what a file
//! manager that does not appear looks like.
//!
//! The order of `MANAGERS` is the order of preference for `auto`, terminal
//! managers first: on a machine with both, the one that opens *where the
//! user already is* is the one that keeps the session.

/// One file manager: the name the config uses, the binary to run, and
/// whether it needs the terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Manager {
    /// The value `file_manager` holds.
    pub key: &'static str,
    /// What the Options modal shows.
    pub label: &'static str,
    /// The binary to run.
    pub program: &'static str,
    /// `true` when it draws in this terminal and must be given it.
    pub takes_terminal: bool,
}

/// The list the config chooses from, in preference order for `auto`.
pub const MANAGERS: &[Manager] = &[
    Manager {
        key: "yazi",
        label: "yazi (terminal)",
        program: "yazi",
        takes_terminal: true,
    },
    Manager {
        key: "tfm",
        label: "tfm (terminal)",
        program: "tfm",
        takes_terminal: true,
    },
    Manager {
        key: "elio",
        label: "elio (terminal)",
        program: "elio",
        takes_terminal: true,
    },
    Manager {
        key: "lf",
        label: "lf (terminal)",
        program: "lf",
        takes_terminal: true,
    },
    Manager {
        key: "ranger",
        label: "ranger (terminal)",
        program: "ranger",
        takes_terminal: true,
    },
    Manager {
        key: "nnn",
        label: "nnn (terminal)",
        program: "nnn",
        takes_terminal: true,
    },
    Manager {
        key: "vis",
        label: "vis (terminal)",
        program: "vis",
        takes_terminal: true,
    },
    Manager {
        key: "nautilus",
        label: "nautilus (desktop)",
        program: "nautilus",
        takes_terminal: false,
    },
    Manager {
        key: "thunar",
        label: "thunar (desktop)",
        program: "thunar",
        takes_terminal: false,
    },
    Manager {
        key: "pcmanfm",
        label: "pcmanfm (desktop)",
        program: "pcmanfm",
        takes_terminal: false,
    },
    Manager {
        key: "dolphin",
        label: "dolphin (desktop)",
        program: "dolphin",
        takes_terminal: false,
    },
    Manager {
        key: "nemo",
        label: "nemo (desktop)",
        program: "nemo",
        takes_terminal: false,
    },
    Manager {
        key: "caja",
        label: "caja (desktop)",
        program: "caja",
        takes_terminal: false,
    },
];

/// The value that means "whichever is installed".
pub const AUTO: &str = "auto";

/// Terminal managers, for the ones that must be given the terminal.
pub fn tui_managers() -> impl Iterator<Item = &'static Manager> {
    MANAGERS.iter().filter(|m| m.takes_terminal)
}

/// Desktop managers, for the ones that are spawned and left alone.
pub fn gui_managers() -> impl Iterator<Item = &'static Manager> {
    MANAGERS.iter().filter(|m| !m.takes_terminal)
}

/// The names `file_manager` accepts, `auto` first.
///
/// The Options modal cycles over exactly this list, so a value that cannot
/// be chosen with `←`/`→` cannot be chosen at all.
pub fn keys() -> Vec<&'static str> {
    std::iter::once(AUTO)
        .chain(MANAGERS.iter().map(|m| m.key))
        .collect()
}

/// The manager named by `configured`, if it is on the list.
///
/// A name that is not on the list is `None` rather than a guess: the
/// setting is a closed list because every entry says whether it needs the
/// terminal, and a name from outside it has no answer to give.
pub fn by_key(configured: &str) -> Option<&'static Manager> {
    let wanted = configured.trim();
    MANAGERS.iter().find(|m| m.key == wanted)
}

/// What the Options modal shows for a configured value.
///
/// An uninstalled or misspelled one says so rather than pretending: the
/// setting is a promise about what `o` will run, and the panel should not
/// say `yazi` when `yazi` is not there.
pub fn display_name(configured: &str) -> String {
    let wanted = configured.trim();
    if wanted.is_empty() || wanted == AUTO {
        return AUTO.to_string();
    }
    match by_key(wanted) {
        Some(manager) if which(manager.program) => manager.label.to_string(),
        Some(manager) => format!("{} (not installed)", manager.label),
        None => format!("{wanted} (unknown)"),
    }
}

/// The manager `o` should run for a configured value.
///
/// `auto` walks the list in preference order and takes the first that is
/// installed, because a setting that says `auto` and then requires the user
/// to know which of the twelve is installed is not `auto`. A named manager
/// that is not installed falls back to the same walk and says so, because
/// the alternative is a key that does nothing at all.
/// The name that was asked for and not used, when there was one.
///
/// Borrowed from the caller's own `&str` rather than the table, because an
/// unrecognised name is by definition not in the table.
pub fn pick(configured: &str) -> (Option<&'static Manager>, Option<&str>) {
    let configured = configured.trim();
    if configured.is_empty() || configured == AUTO {
        return (first_installed(), None);
    }
    match by_key(configured) {
        Some(manager) if which(manager.program) => (Some(manager), None),
        Some(manager) => (first_installed(), Some(manager.key)),
        None => (first_installed(), Some(configured)),
    }
}

/// The first installed manager, terminal ones first.
fn first_installed() -> Option<&'static Manager> {
    MANAGERS
        .iter()
        .find(|m| which(m.program))
        .or(Some(&Manager {
            key: "xdg-open",
            label: "xdg-open (desktop session)",
            program: "xdg-open",
            takes_terminal: false,
        }))
}

/// On PATH, and executable.
///
/// `which` is already a dependency and already used to find browsers, so
/// this is the same question asked twice rather than a second way to ask
/// it.
fn which(program: &str) -> bool {
    which::which(program).is_ok()
}

/// Download rates a person picks from, in bytes per second.
///
/// Steps, not a free-text field: `+` and `-` are the only way to reach this
/// from the keyboard, and a ramp nobody can predict is a ramp nobody uses.
/// The top step is not a limit -- `0` is that, and it sits above the ramp.
pub const SPEED_STEPS: &[i64] = &[
    50_000,      // 50 KB/s
    100_000,     // 100 KB/s
    250_000,     // 250 KB/s
    500_000,     // 500 KB/s
    1_000_000,   // 1 MB/s
    2_000_000,   // 2 MB/s
    5_000_000,   // 5 MB/s
    10_000_000,  // 10 MB/s
    25_000_000,  // 25 MB/s
    50_000_000,  // 50 MB/s
    100_000_000, // 100 MB/s
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Every manager has to say whether it takes the terminal, because that
    /// is what decides whether the terminal is handed over. A missing flag
    /// would run a desktop manager inside the alternate screen, which is a
    /// window nobody can see.
    #[test]
    fn every_manager_says_whether_it_needs_the_terminal() {
        for manager in MANAGERS {
            assert!(
                !manager.key.is_empty() && !manager.program.is_empty(),
                "{manager:?} has no name to run"
            );
            assert!(
                manager.label.contains("terminal") || manager.label.contains("desktop"),
                "{} does not say which kind it is",
                manager.label
            );
        }
    }

    /// `auto` has to reach the terminal managers first, or a machine with
    /// both a desktop and a terminal manager opens a second window instead
    /// of taking over the one the user is in.
    #[test]
    fn auto_prefers_a_terminal_manager_over_a_desktop_one() {
        let first_desktop = MANAGERS
            .iter()
            .position(|m| !m.takes_terminal)
            .expect("a desktop manager on the list");
        assert!(
            MANAGERS[..first_desktop].iter().all(|m| m.takes_terminal),
            "the list interleaves and auto could land on a window"
        );
    }

    /// The keys and the list are the same set: a name the Options modal
    /// cycles onto must be one `pick` understands.
    #[test]
    fn every_choosable_key_is_on_the_list() {
        for key in keys() {
            assert!(
                key == AUTO || by_key(key).is_some(),
                "`{key}` can be chosen but nothing answers to it"
            );
        }
        for manager in MANAGERS {
            assert!(
                keys().contains(&manager.key),
                "{} is on the list but cannot be chosen",
                manager.key
            );
        }
    }
}
