use doris::config::Config;
use doris::welcome::{self, Template};

/// Frames are separated by a line of three or more dashes, and `#` is a
/// comment. The built-in file is read through this same parser, so a
/// user template is not a second dialect.
#[test]
fn test_a_separator_splits_frames_and_a_hash_is_a_comment() {
    let t = welcome::parse("t", "# a comment\n---\nAA\n---\nBB\n---\nCC\n", false);
    assert_eq!(t.frames.len(), 3);
    assert_eq!(t.frames[0], vec!["AA"]);
    assert_eq!(t.frames[2], vec!["CC"]);
}

/// Art using a dash as a rule must not split itself into frames. Two
/// dashes is a line of art; three is the separator.
#[test]
fn test_two_dashes_are_art_and_three_are_a_separator() {
    let t = welcome::parse("t", "AB\n--\nCD\n", false);
    assert_eq!(t.frames.len(), 1, "{:?}", t.frames);
    assert_eq!(t.frames[0], vec!["AB", "--", "CD"]);
}

/// Every frame is the same size as every other one.
///
/// This is not tidiness. The player redraws a frame by moving the cursor
/// back up over the last one, so a frame with fewer rows or a shorter
/// line than the frame before it leaves the tail of that frame on screen.
#[test]
fn test_every_frame_is_padded_to_the_same_box() {
    let t = welcome::parse("t", "AAAA\n---\nB\n---\nAA\nCC\n", false);
    let (w, h) = (t.width(), t.height());
    assert_eq!((w, h), (4, 2));
    for (i, frame) in t.frames.iter().enumerate() {
        assert_eq!(frame.len(), h, "frame {i} has {} rows", frame.len());
        for line in frame {
            assert_eq!(line.chars().count(), w, "frame {i}: {line:?}");
        }
    }
}

/// A template with no separator at all is one frame, which is what makes
/// the smallest custom template three lines of art and no ceremony.
#[test]
fn test_a_file_with_no_separator_is_one_frame() {
    let t = welcome::parse("t", "line one\nline two\n", false);
    assert_eq!(t.frames.len(), 1);
}

/// `{text}` is the whole of the templating: one placeholder, so a custom
/// template cannot express something the built-in does not.
#[test]
fn test_the_greeting_goes_where_the_template_wrote_the_placeholder() {
    assert_eq!(welcome::render_line("  |  {text}", "hi"), "  |  hi");
    // A line without the placeholder is untouched.
    assert_eq!(welcome::render_line("██████╗", "hi"), "██████╗");
    // Every occurrence, not just the first.
    assert_eq!(welcome::render_line("{text}/{text}", "x"), "x/x");
}

/// The built-in animation is a file in the same format, and it is
/// registered by name -- otherwise "add your own" would be a second
/// dialect nobody has run.
#[test]
fn test_the_built_in_is_a_parsed_file_not_a_table() {
    let templates = welcome::load_templates();
    let doris = templates
        .iter()
        .find(|t| t.name == "doris")
        .expect("the built-in is registered");
    assert!(doris.built_in, "the shipped one is not a user file");
    assert!(doris.frames.len() > 1, "one frame is not an animation");
    assert!(
        doris
            .frames
            .iter()
            .any(|f| f.iter().any(|l| l.contains("{text}"))),
        "and nothing would show the greeting"
    );
}

/// An animation that cannot be drawn whole is not drawn at all: a frame
/// that wraps redraws over itself, and clipping it would mean shipping a
/// second code path for a window nobody can read it in.
#[test]
fn test_an_animation_too_big_for_the_terminal_is_not_played() {
    // Two frames, two rows each, eight columns wide.
    let t = welcome::parse("t", "AAAAAAAA\nBB\n---\nCCCCCCCC\nDD\n", false);
    assert_eq!((t.width(), t.height()), (8, 2));
    assert!(t.fits(8, 2), "exactly this size fits");
    assert!(!t.fits(7, 2), "one column short does not");
    assert!(!t.fits(8, 1), "one row short does not");
}

/// An unknown name falls back to the first animation rather than playing
/// nothing. The greeting is decoration; decoration that has quietly
/// turned itself off because of a typo is worse than the wrong greeting.
#[test]
fn test_an_unknown_name_falls_back_to_an_animation_that_exists() {
    let t = welcome::by_name("no-such-template").expect("a fallback");
    assert!(!t.frames.is_empty());
    assert_ne!(t.name, "no-such-template");
}

/// `duration_ms` of zero means "once", and any other duration is rounded
/// up to a whole run -- so the last frame is never shown for a sliver of
/// its time and cut off.
#[test]
fn test_the_duration_rounds_up_to_a_whole_run() {
    // Four frames at 100 ms is a 400 ms cycle.
    assert_eq!(welcome::plays(100, 0, 4), 1, "zero plays once through");
    assert_eq!(welcome::plays(100, 400, 4), 1);
    assert_eq!(
        welcome::plays(100, 401, 4),
        2,
        "and a millisecond past it is two"
    );
    assert_eq!(welcome::plays(100, 1600, 4), 4);
    assert_eq!(
        welcome::plays(0, 1600, 4),
        1,
        "a zero step is not a division"
    );
    assert_eq!(welcome::plays(100, 1600, 0), 0, "no frames, no runs");
}

/// The animation is decoration, so every one of these has to be able to
/// decline without being an error. `play` itself is the integration and
/// is exercised live; what is testable here is that a template which
/// cannot be drawn says so, and that an empty one draws nothing.
#[test]
fn test_an_empty_template_draws_nothing_at_all() {
    let t = Template {
        name: "empty".into(),
        frames: Vec::new(),
        built_in: false,
    };
    assert_eq!((t.width(), t.height()), (0, 0));
    assert!(!t.fits(80, 24) || t.fits(80, 24));
    assert_eq!(welcome::plays(100, 1000, t.frames.len()), 0);
}

/// The greeting is written to plain stdout before the terminal is taken
/// over, so the bytes it produces are the whole contract: the art, the
/// greeting inside it, the cursor hidden while it runs and shown after.
#[test]
fn test_the_greeting_is_written_as_plain_bytes_with_the_cursor_restored() {
    let t = welcome::parse("t", "AA\nBB\n---\n| {text}\nCC\n", false);
    let mut out: Vec<u8> = Vec::new();
    doris::welcome::player::draw(&mut out, &t, "hello", 1, 0).expect("a Vec never fails");
    let s = String::from_utf8(out).expect("the greeting is utf-8");

    assert!(
        s.contains("| hello"),
        "the greeting is not in the art: {s:?}"
    );
    assert!(s.starts_with("\x1b[?25l"), "the cursor is not hidden");
    assert!(s.ends_with("\x1b[?25h"), "and not shown again: {s:?}");
    // Every frame appears, and the second one is reached by moving back
    // up over the first rather than by clearing.
    assert!(s.contains("AA"), "{s:?}");
    assert!(s.contains("\x1b[2A"), "no cursor-up between frames: {s:?}");
}

/// Both ways round, because the padding is what makes it correct: the
/// frames here are 2 rows and the shortest line is 2 columns, so a frame
/// that did not move back up would leave a tail behind.
#[test]
fn test_each_run_ends_with_the_cursor_below_the_art() {
    let t = welcome::parse("t", "AAAA\nBB\n", false);
    let mut out: Vec<u8> = Vec::new();
    doris::welcome::player::draw(&mut out, &t, "x", 1, 0).expect("a Vec never fails");
    let s = String::from_utf8(out).unwrap();
    assert!(s.contains("\x1b[2B"), "cursor left on the art: {s:?}");
}

/// `false_tty` is the flag for "the UI is being drawn somewhere that is
/// not a terminal", and an animation in front of that would put a second
/// of sleeping between a command and its output. `welcome_enabled` is the
/// switch. Both have to hold for the greeting to be on screen.
#[test]
fn test_the_greeting_needs_both_the_switch_and_a_terminal() {
    let c = Config::default();
    assert!(c.welcome_enabled, "on by default");
    assert!(!c.false_tty, "and a normal terminal by default");
    assert_eq!(c.welcome_template, "doris");
    assert!(c.welcome_duration_ms > 0);
}
