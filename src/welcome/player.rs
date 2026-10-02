//! Drawing the greeting.
//!
//! Plain bytes on stdout, before the terminal goes into the alternate
//! screen. That ordering is the whole integration: nothing here touches
//! the ratatui terminal, so there is no state to restore, no half-drawn
//! frame to survive a panic, and the greeting is still on the primary
//! screen when doris quits and the alt screen goes away.

use std::io::{IsTerminal, Write};

use crate::config::Config;
use crate::tui::{SYNC_BEGIN, SYNC_END};
use crate::welcome::{self, Template};

const HIDE_CURSOR: &str = "\x1b[?25l";
const SHOW_CURSOR: &str = "\x1b[?25h";
/// Clear and go home: the first frame gets a clean screen to draw on.
const CLEAR_HOME: &str = "\x1b[2J\x1b[H";

/// Play the configured greeting, if there is one to play.
///
/// Every condition here is a reason not to draw, and none of them is an
/// error: a greeting is decoration, so the worst it may do is not appear.
/// The three that matter are the obvious ones -- switched off, not a
/// terminal, `false_tty` -- plus one that is easy to miss. `false_tty`
/// exists for the places where the UI is drawn into something that is
/// not a terminal at all, and an animation there would put a second of
/// sleeping between a command and its output.
pub fn play(config: &Config) {
    if !config.welcome_enabled || config.false_tty {
        return;
    }
    let mut stdout = std::io::stdout();
    if !stdout.is_terminal() {
        return;
    }
    let Some(template) = welcome::by_name(&config.welcome_template) else {
        return;
    };
    let Ok((width, height)) = crossterm::terminal::size() else {
        return;
    };
    if !template.fits(width, height) {
        return;
    }
    let plays = welcome::plays(
        config.welcome_frame_ms,
        config.welcome_duration_ms,
        template.frames.len(),
    );
    let _ = draw(
        &mut stdout,
        &template,
        &config.welcome_text,
        plays,
        config.welcome_frame_ms,
    );
}

/// Write the animation out.
///
/// `plays` is how many times the whole thing repeats and `frame_ms` the
/// wait between frames; both come from [`welcome::plays`] so the timing
/// decision is one pure function and this is only the writing. Every
/// frame is followed by a newline and the next one starts by moving back
/// up over it, which is why [`Template`] pads its frames: a frame smaller
/// than the last would otherwise leave the tail of it on screen.
pub fn draw(
    out: &mut impl Write,
    template: &Template,
    text: &str,
    plays: usize,
    frame_ms: u64,
) -> std::io::Result<()> {
    let height = template.height();
    if height == 0 || plays == 0 {
        return Ok(());
    }
    out.write_all(HIDE_CURSOR.as_bytes())?;
    out.write_all(SYNC_BEGIN.as_bytes())?;
    out.write_all(CLEAR_HOME.as_bytes())?;

    let mut first = true;
    for _ in 0..plays {
        for frame in &template.frames {
            if !first {
                out.write_all(format!("\x1b[{height}A").as_bytes())?;
            }
            first = false;
            for line in frame {
                out.write_all(welcome::render_line(line, text).as_bytes())?;
                out.write_all(b"\r\n")?;
            }
            out.flush()?;
            if frame_ms > 0 {
                std::thread::sleep(std::time::Duration::from_millis(frame_ms));
            }
        }
    }

    out.write_all(SYNC_END.as_bytes())?;
    // Down past the art, so the greeting stays on the screen and the
    // cursor does not land on top of it. The UI takes the alternate
    // screen next, so this is where the shell's own output resumes.
    out.write_all(format!("\x1b[{height}B").as_bytes())?;
    out.write_all(SHOW_CURSOR.as_bytes())?;
    out.flush()
}
