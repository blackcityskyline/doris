//! The palette accents -- `primary`, `secondary`, `error`, `on_hover`
//! -- are optional theme fields, and that is what makes the colour
//! distribution one rule for every theme: a file that spells them out
//! (the noctalia template does) uses its own values, a file that omits
//! them (every bundled theme does) falls back to the classic field each
//! token replaced, so nothing changes for themes that never heard of
//! the accents.

use doris::ui::theme::{ColorDef, Theme};
use doris::ui::zones::{zone_border_color, ZoneId};
use ratatui::style::Color;

/// No accents in the file: each token falls back to whatever drew
/// those elements before the tokens existed.
#[test]
fn test_missing_tokens_fall_back_to_the_classic_fields() {
    let theme = Theme::dark();
    assert_eq!(theme.primary_color(), theme.title.to_color());
    assert_eq!(theme.secondary_color(), theme.hi_fg.to_color());
    assert_eq!(theme.on_hover_color(), theme.hi_fg.to_color());
    assert_eq!(theme.error_color(), Color::Red);
}

/// A theme that does spell them out wins over the fallback.
#[test]
fn test_tokens_in_the_theme_win_over_the_fallback() {
    let mut theme = Theme::dark();
    theme.primary = Some(ColorDef::new(1, 2, 3));
    theme.secondary = Some(ColorDef::new(4, 5, 6));
    theme.error = Some(ColorDef::new(7, 8, 9));
    theme.on_hover = Some(ColorDef::new(10, 11, 12));

    assert_eq!(theme.primary_color(), Color::Rgb(1, 2, 3));
    assert_eq!(theme.secondary_color(), Color::Rgb(4, 5, 6));
    assert_eq!(theme.error_color(), Color::Rgb(7, 8, 9));
    assert_eq!(theme.on_hover_color(), Color::Rgb(10, 11, 12));
}

/// New fields must not make an old theme file stop parsing: a failed
/// parse drops the theme from the picker silently (`load_themes_from`
/// keeps the error to itself), and every bundled theme is an old file.
#[test]
fn test_every_bundled_theme_still_loads_without_the_tokens() {
    let themes = Theme::load_themes_from(None);
    assert!(
        themes.len() >= 40,
        "only {} bundled themes loaded",
        themes.len()
    );
    assert!(themes.iter().all(|t| t.primary.is_none()));
    assert!(themes.iter().all(|t| t.on_hover.is_none()));
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
