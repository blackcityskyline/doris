//! Opening a download, and the file managers that can be opened.
//!
//! The order is the argument: a terminal file manager takes the terminal
//! over, so it is only run when the user asked for one explicitly, and the
//! GUI ones are spawned and left alone. A machine with `yazi` installed
//! should not lose its doris session every time `o` is pressed.
//!
//! `xdg-open` is the last resort and the only one that needs nothing
//! installed -- on a desktop it hands the path to whatever the session
//! already uses, which is the file manager the user chose for every other
//! file.

/// Terminal file managers, best first: each of these draws a full-screen
/// browser and takes the terminal while it does.
pub const TUI_MANAGERS: &[&str] = &["yazi", "tfm", "elio", "lf", "ranger", "nnn", "vis"];

/// GUI file managers, best first. Spawned and not waited for, so the order
/// only decides which one claims the path.
pub const GUI_MANAGERS: &[&str] = &["nautilus", "thunar", "pcmanfm", "dolphin", "nemo", "caja"];

/// The download rates a person picks from, in bytes per second.
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

/// Which program opens `dir`, and whether it takes the terminal.
///
/// `None` for the GUI ones is the point: they are spawned and doris keeps
/// drawing, which is the difference between `o` being useful and `o`
/// closing the app.
pub fn pick(dir: &str) -> Option<&'static str> {
    for candidate in TUI_MANAGERS.iter().chain(GUI_MANAGERS.iter()) {
        if which(candidate) {
            return Some(candidate);
        }
    }
    // Nothing installed that we know by name: hand it to the session.
    if std::path::Path::new(dir).exists() {
        Some("xdg-open")
    } else {
        None
    }
}

/// On PATH, and executable.
///
/// `which` is already a dependency and already used to find browsers, so
/// this is the same question asked twice rather than a second way to ask
/// it.
fn which(program: &str) -> bool {
    which::which(program).is_ok()
}
