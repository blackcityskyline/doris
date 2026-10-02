//! The palette accents -- `primary`, `secondary`, `error`
//! -- are optional theme fields, and that is what makes the colour
//! distribution one rule for every theme: a file that spells them out
//! (the noctalia template does) uses its own values, a file that omits
//! them (every bundled theme does) falls back to the classic field each
//! token replaced, so nothing changes for themes that never heard of
//! the accents.

use doris::ui::layout::{zone_border_color, ZoneId};
use doris::ui::theme::{ColorDef, Theme};
use ratatui::style::Color;

/// No accents in the file: each token falls back to whatever drew
/// those elements before the tokens existed.
#[test]
fn test_missing_tokens_fall_back_to_the_classic_fields() {
    // The tokens are unset here on purpose rather than by relying on
    // `Theme::dark()`: the built-in theme names its own accents, and a
    // test that read them off it would pin whatever it happened to name.
    let theme = Theme {
        primary: None,
        secondary: None,
        error: None,
        ..Theme::dark()
    };
    assert_eq!(theme.primary_color(), theme.hi_fg.to_color());
    assert_eq!(theme.secondary_color(), theme.hi_fg.to_color());
    assert_eq!(theme.error_color(), Color::Red);
}

/// A theme that does spell them out wins over the fallback.
#[test]
fn test_tokens_in_the_theme_win_over_the_fallback() {
    let mut theme = Theme::dark();
    theme.primary = Some(ColorDef::new(1, 2, 3));
    theme.secondary = Some(ColorDef::new(4, 5, 6));
    theme.error = Some(ColorDef::new(7, 8, 9));

    assert_eq!(theme.primary_color(), Color::Rgb(1, 2, 3));
    assert_eq!(theme.secondary_color(), Color::Rgb(4, 5, 6));
    assert_eq!(theme.error_color(), Color::Rgb(7, 8, 9));
}

/// New fields must not make an old theme file stop parsing: a failed
/// parse drops the theme from the picker silently (`load_themes_from`
/// keeps the error to itself), and most bundled themes predate the
/// accents.
///
/// Two themes now name a `primary` -- one whose `hi_fg` is pure white,
/// one whose `title` and `hi_fg` both are -- so the "none of them name it"
/// claim this test used to make is exactly the white-banner bug. What must
/// hold is that a theme file *without* the field still parses and still
/// resolves an accent, which is what the count and the accent tests cover.
#[test]
fn test_every_bundled_theme_still_loads_without_the_tokens() {
    let themes = Theme::load_themes_from(None);
    assert!(
        themes.len() >= 40,
        "only {} bundled themes loaded",
        themes.len()
    );
    assert!(
        themes.iter().all(|t| t.secondary.is_none()),
        "no bundled theme names `secondary` yet, so a file naming it is new ground"
    );
    for t in &themes {
        if t.primary.is_none() {
            assert_eq!(
                t.primary_color(),
                t.hi_fg.to_color(),
                "a theme that names no accent falls back to its highlight colour"
            );
        }
    }
}

/// The frame under the cursor takes the structure accent; every other
/// frame stays on the divider line.
#[test]
fn test_the_focused_frame_is_primary_and_the_rest_div_line() {
    let theme = Theme::dark();
    assert_eq!(
        zone_border_color(ZoneId::Results, ZoneId::Results, &theme),
        theme.primary_color()
    );
    assert_eq!(
        zone_border_color(ZoneId::Log, ZoneId::Results, &theme),
        theme.div_line.to_color()
    );
}

/// No *bundled* theme's accent comes out near-white.
///
/// The four accents fall back to a classic field when a theme file does not
/// name them, and for `primary` the classic field used to be `title` -- the
/// near-white a theme draws its headings in. So a theme that named no accent
/// of its own got a white one: a white ASCII banner, white panel titles, and
/// a menu whose picked item is the same colour as the two unpicked ones.
/// Every bundled theme is such a theme; they predate the accents.
///
/// `load_themes_from(None)` rather than `load_themes()`, and that is the
/// point of the test rather than a detail of it. `load_themes()` reads
/// `~/.config/doris/themes/`, so a file somebody's own machine happens to
/// carry decides whether the suite passes here. A gate that depends on files
/// outside the repository is not a gate, it is a report about one
/// developer's home directory -- and it failed exactly that way, twice, on a
/// scratch file of mine.
///
/// A user's own theme may still make its accent whatever it likes; that is
/// their file, and the near-white is their choice to make.
#[test]
fn no_bundled_theme_has_an_accent_that_comes_out_near_white() {
    let themes = Theme::load_themes_from(None);
    assert!(themes.len() >= 40, "only {} bundled themes", themes.len());
    let white: Vec<&str> = themes
        .iter()
        .filter(|t| near_white(t.primary_color()))
        .map(|t| t.name.as_str())
        .collect();
    assert!(
        white.is_empty(),
        "these bundled themes' accent is near-white, so every `primary` on \
         screen comes out invisible: {white:?}"
    );
}

/// An accent that matches `main_fg` is as invisible as a white one: a
/// frame word then reads as ordinary body text.
#[test]
fn no_theme_has_an_accent_that_is_its_body_colour() {
    let themes = Theme::load_themes();
    let flat: Vec<&str> = themes
        .iter()
        .filter(|t| t.primary_color() == t.main_fg.to_color())
        .map(|t| t.name.as_str())
        .collect();
    assert!(
        flat.is_empty(),
        "these themes' accent is their own body colour, so a frame word \
         reads as ordinary text: {flat:?}"
    );
}

fn near_white(c: ratatui::style::Color) -> bool {
    match c {
        ratatui::style::Color::Rgb(r, g, b) => r > 200 && g > 200 && b > 200,
        _ => false,
    }
}
