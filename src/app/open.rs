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
    manager: &crate::app::files::Manager,
    path: &str,
) -> Result<()> {
    let program = manager.program;
    let mut command = Command::new(program);
    command.arg(path);

    // A desktop manager draws its own window and needs none of this
    // terminal, so it is given none: inheriting stdout lets one line of its
    // startup output scroll the frame it was opened from, and the user comes
    // back to a detail view that has moved up the screen.
    let desktop = !manager.takes_terminal;
    if desktop {
        command.stdout(std::process::Stdio::null());
        command.stderr(std::process::Stdio::null());
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            // Nothing was started, so nothing has to be put back.
            crate::log::log("files", &format!("{program}: {e}"));
            return Ok(());
        }
    };

    // Not waited for, and the terminal is not touched: doris carries on
    // drawing exactly as it was.
    if desktop {
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

/// The directory a torrent's files are in, checked before anything spawns.
pub fn existing_dir(dir: &str) -> Result<&str> {
    std::path::Path::new(dir)
        .exists()
        .then_some(dir)
        .with_context(|| format!("{dir} is not there"))
}
