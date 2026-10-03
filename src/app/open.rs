//! Handing the terminal to another program.
//!
//! A terminal file manager needs the real terminal: raw mode off, alternate
//! screen left, and both restored afterwards whatever the child did. The GUI
//! ones need neither -- they are spawned and left running, and doris keeps
//! drawing, which is the difference between a key that opens a directory and
//! a key that closes the app.
//!
//! Every failure path restores. Leaving doris in raw mode with no alternate
//! screen is worse than not having opened anything, so the child is waited
//! on inside a scope that cannot return early.

use crate::config::Config;
use crate::tui::Terminal;
use anyhow::{Context, Result};
use std::process::Command;

pub fn open_path(
    terminal: &mut Terminal,
    config: &Config,
    program: &str,
    path: &str,
) -> Result<()> {
    let mut child = match Command::new(program).arg(path).spawn() {
        Ok(child) => child,
        Err(e) => {
            // Nothing was started, so nothing has to be put back.
            crate::log::log("files", &format!("{program}: {e}"));
            return Ok(());
        }
    };

    // A GUI manager draws its own window and does not read this terminal, so
    // doris stays exactly as it was and the child is simply not waited for.
    if crate::app::files::GUI_MANAGERS.contains(&program) || program == "xdg-open" {
        crate::log::log("files", &format!("opened {path} with {program}"));
        return Ok(());
    }

    let restore = |terminal: &mut Terminal| {
        let _ = crate::tui::restore(terminal);
        match crate::tui::init(!config.false_tty, !config.disable_mouse) {
            Ok(fresh) => {
                *terminal = fresh;
                Ok(())
            }
            Err(e) => Err(e),
        }
    };

    let _ = crate::tui::restore(terminal);
    let status = child.wait();
    restore(terminal)?;

    match status {
        Ok(status) => crate::log::log("files", &format!("{program} on {path} exited {status}")),
        Err(e) => {
            let _ = child.kill();
            crate::log::log("files", &format!("{program}: {e}"));
        }
    }
    Ok(())
}

/// Whether the program needs the terminal, asked separately from whether it
/// exists so the two decisions are not one `if`.
pub fn takes_terminal(program: &str) -> bool {
    crate::app::files::TUI_MANAGERS.contains(&program)
}

/// The directory a torrent's files are in, checked before anything spawns.
pub fn existing_dir(dir: &str) -> Result<&str> {
    std::path::Path::new(dir)
        .exists()
        .then_some(dir)
        .with_context(|| format!("{dir} is not there"))
}
