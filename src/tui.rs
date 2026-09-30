use anyhow::Result;
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::CrosstermBackend;
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};

pub type Terminal = ratatui::Terminal<CrosstermBackend<io::Stdout>>;

/// `disambiguate_escape_codes`: Esc arrives as its own event instead of
/// racing the ones that follow it, and -- the reason this wave pushed
/// for it -- modified keys such as Shift+Enter arrive as *modified*
/// keys. Terminals that do not implement the protocol ignore the push.
const KEY_FLAGS: KeyboardEnhancementFlags = KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES;

/// Whether `init` pushed the flags, so `restore` pops exactly what was
/// pushed and a plain VT gets no sequence it never saw arrive.
static ENHANCED: AtomicBool = AtomicBool::new(false);

pub fn init(enhance_keys: bool) -> Result<Terminal> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    enter(&mut stdout, enhance_keys)?;
    ENHANCED.store(enhance_keys, Ordering::Relaxed);
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

pub fn restore(terminal: &mut Terminal) -> Result<()> {
    disable_raw_mode()?;
    leave(
        terminal.backend_mut(),
        ENHANCED.swap(false, Ordering::Relaxed),
    )?;
    terminal.show_cursor()?;
    Ok(())
}

/// Alternate screen and mouse capture, then the keyboard protocol --
/// last, so a terminal that rejects nothing else still has the screen
/// it is about to draw on.
fn enter<W: Write>(w: &mut W, enhance_keys: bool) -> io::Result<()> {
    execute!(w, EnterAlternateScreen, EnableMouseCapture)?;
    if enhance_keys {
        execute!(w, PushKeyboardEnhancementFlags(KEY_FLAGS))?;
    }
    Ok(())
}

/// The reverse order: drop the protocol while the user can still see
/// what happens, then leave the screen. Popping after
/// `LeaveAlternateScreen` would be the same bytes with the wrong
/// window of attention.
fn leave<W: Write>(w: &mut W, enhanced: bool) -> io::Result<()> {
    if enhanced {
        execute!(w, PopKeyboardEnhancementFlags)?;
    }
    execute!(w, LeaveAlternateScreen, DisableMouseCapture)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Alternate screen, mouse capture and -- when asked -- the keyboard
    /// enhancement protocol, in that order.
    ///
    /// The protocol is the whole reason this is a function and not an
    /// `execute!` at the call site: without it a terminal reports
    /// Shift+Enter as a plain Enter (or says nothing at all), and
    /// `Shift+Enter` in the key table is then a lie the tests cannot
    /// catch, because crossterm never sees the modifier.
    #[test]
    fn entering_the_screen_requests_the_keyboard_protocol() {
        let mut buf = Vec::new();
        enter(&mut buf, true).expect("the byte sequence is infallible");
        let out = String::from_utf8_lossy(&buf);

        assert!(
            out.contains("\x1b[?1049h"),
            "alternate screen first: {out:?}"
        );
        assert!(
            out.contains("\x1b[?1000h"),
            "mouse capture, as before: {out:?}"
        );
        assert!(
            out.contains("\x1b[>1u"),
            "push keyboard enhancement flags (disambiguate escape codes): {out:?}"
        );
    }

    #[test]
    fn a_false_tty_gets_nothing_it_cannot_understand() {
        let mut buf = Vec::new();
        enter(&mut buf, false).expect("the byte sequence is infallible");
        let out = String::from_utf8_lossy(&buf);

        assert!(!out.contains("\x1b[>"), "no push: {out:?}");
        assert!(out.contains("\x1b[?1049h"), "but still the screen: {out:?}");
    }

    #[test]
    fn leaving_pops_the_flags_it_pushed() {
        let mut buf = Vec::new();
        leave(&mut buf, true).expect("the byte sequence is infallible");
        let out = String::from_utf8_lossy(&buf);

        assert!(
            out.contains("\x1b[<1u"),
            "every push owes a pop, or the terminal keeps sending keys \
             the next program never asked for: {out:?}"
        );
        assert!(
            out.contains("\x1b[?1049l"),
            "alternate screen left: {out:?}"
        );
        assert!(
            !out.contains("\x1b[?1049l\x1b[<1u"),
            "pop before leaving the screen, while the user is still \
             looking at it: {out:?}"
        );
    }

    #[test]
    fn a_terminal_without_the_protocol_is_left_alone() {
        let mut buf = Vec::new();
        leave(&mut buf, false).expect("the byte sequence is infallible");
        let out = String::from_utf8_lossy(&buf);

        assert!(!out.contains("\x1b[<"), "no pop: {out:?}");
        assert!(
            out.contains("\x1b[?1049l"),
            "but the screen still goes: {out:?}"
        );
    }
}
