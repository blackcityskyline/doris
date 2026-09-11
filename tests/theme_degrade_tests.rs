use doris::ui::theme::degrade_color;
use ratatui::style::Color;

#[test]
fn test_named_colors_pass_through_unchanged() {
    // Only Color::Rgb should ever be degraded -- named/basic colors (used
    // e.g. for focus highlights) are already safe on any terminal.
    for c in [Color::Yellow, Color::Cyan, Color::Green, Color::Red, Color::White, Color::Reset] {
        assert_eq!(degrade_color(c, true), c);
        assert_eq!(degrade_color(c, false), c);
    }
}

#[test]
fn test_pure_black_and_white_degrade_correctly() {
    assert_eq!(degrade_color(Color::Rgb(0, 0, 0), true), Color::Black);
    assert_eq!(degrade_color(Color::Rgb(255, 255, 255), true), Color::White);
    // Without bright colors allowed, pure white should fall back to the
    // closest *basic* color (light Gray), not stay unmatched.
    assert_eq!(degrade_color(Color::Rgb(255, 255, 255), false), Color::Gray);
}

#[test]
fn test_pure_red_degrades_to_red_family() {
    let degraded = degrade_color(Color::Rgb(255, 0, 0), true);
    assert!(matches!(degraded, Color::Red | Color::LightRed));
}

#[test]
fn test_false_tty_never_returns_bright_variants() {
    // Colors that are clearly "bright" (e.g. a light pastel) must still
    // resolve to one of the 8 basic colors when bright variants are
    // disallowed (False tty mode).
    let bright_pastel = Color::Rgb(255, 200, 200);
    let degraded = degrade_color(bright_pastel, false);
    let basic_only = [
        Color::Black, Color::Red, Color::Green, Color::Yellow,
        Color::Blue, Color::Magenta, Color::Cyan, Color::Gray,
    ];
    assert!(basic_only.contains(&degraded), "expected a basic color, got {:?}", degraded);
}

#[test]
fn test_truecolor_off_can_return_bright_variants() {
    // A near-white color should be allowed to resolve to White/LightGray
    // when bright variants ARE allowed (Truecolor=false but not False tty).
    let near_white = Color::Rgb(250, 250, 250);
    assert_eq!(degrade_color(near_white, true), Color::White);
}

#[test]
fn test_degradation_is_deterministic() {
    // Same input, same output every time -- important since this runs on
    // every frame render.
    let c = Color::Rgb(123, 45, 67);
    let a = degrade_color(c, true);
    let b = degrade_color(c, true);
    assert_eq!(a, b);
}
