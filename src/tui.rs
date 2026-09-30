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

/// DEC private mode 2026, synchronized output: the terminal buffers
/// everything a frame writes and presents it in one go, so a frame is
/// never shown half-drawn. Without it a redraw that takes longer than
/// one frame period (a search answering, a directory with many rows)
/// shows the old and the new content side by side on the way through.
///
/// Terminals that do not implement it ignore the pair, which is why the
/// option exists at all: some users see the tearing, some would rather
/// have the few microseconds back.
///
/// These are two functions rather than a guard object because a guard
/// would have to hold the backend borrowed across `Terminal::draw`, and
/// the draw cannot then run. Split this way the borrow ends before the
/// frame and starts again after, and the closing sequence is written
/// after `draw` returns whatever it returned -- a guard's one advantage
/// was the same, and the two borrows give it without the type.
pub const SYNC_BEGIN: &str = "\x1b[?2026h";
pub const SYNC_END: &str = "\x1b[?2026l";

/// Write a raw sequence straight to the terminal, past ratatui's buffer.
///
/// Errors are dropped: a terminal that will not take the mode gets no
/// synchronized output, which is the state it was already in.
fn write_raw(w: &mut impl Write, seq: &str) {
    let _ = w.write_all(seq.as_bytes());
    let _ = w.flush();
}

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

/// Start buffering this frame. Paired with [`end_sync`], which must be
/// called even if the draw in between failed.
pub fn begin_sync(terminal: &mut Terminal) {
    write_raw(terminal.backend_mut(), SYNC_BEGIN);
}

/// Stop buffering, so the terminal presents the frame. Skipping this after
/// a [`begin_sync`] leaves the terminal in a state the user cannot get
/// out of.
pub fn end_sync(terminal: &mut Terminal) {
    write_raw(terminal.backend_mut(), SYNC_END);
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

    /// Synchronized output is one private mode set and reset. Getting the
    /// parameters wrong is not a syntax error the terminal complains
    /// about -- it is a mode that never turns on, or worse one that turns
    /// on and is never reset, leaving the terminal buffering with no way
    /// out. So the bytes are pinned.
    #[test]
    fn synchronized_output_is_a_dec_2026_pair() {
        assert_eq!(SYNC_BEGIN, "\x1b[?2026h", "begin: set mode 2026");
        assert_eq!(SYNC_END, "\x1b[?2026l", "end: reset the same mode");
    }

    /// Every begin owes an end. `write_raw` is what both go through, and
    /// it drops a write error rather than propagating it -- a terminal
    /// that will not take the mode just does not get the feature -- so
    /// what has to be checked here is that it writes and flushes, since a
    /// buffered mode change that never reaches the terminal is the same
    /// as no feature at all.
    #[test]
    fn the_sequence_is_written_and_flushed() {
        /// A writer that records what it was asked to do.
        struct Recorder {
            bytes: Vec<u8>,
            flushes: usize,
        }
        impl Write for Recorder {
            fn write(&mut self, b: &[u8]) -> io::Result<usize> {
                self.bytes.extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> io::Result<()> {
                self.flushes += 1;
                Ok(())
            }
        }

        let mut r = Recorder {
            bytes: Vec::new(),
            flushes: 0,
        };
        write_raw(&mut r, SYNC_BEGIN);
        write_raw(&mut r, SYNC_END);

        assert_eq!(
            String::from_utf8_lossy(&r.bytes),
            format!("{SYNC_BEGIN}{SYNC_END}"),
            "both, in order"
        );
        assert_eq!(r.flushes, 2, "each one is flushed as it is written");
    }
}
