//! Contrast of every bundled theme, by the role each colour plays. WCAG 2.1: 4.5:1 for text,
//! 3:1 for non-text (borders, keybind glyphs, anything that carries meaning without being
//! read).

use doris::ui::theme::Theme;

const TEXT: f64 = 4.5;
const NON_TEXT: f64 = 3.0;

/// Relative luminance, WCAG 2.1.
fn lum(rgb: (u8, u8, u8)) -> f64 {
    fn chan(c: u8) -> f64 {
        let c = f64::from(c) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    let (r, g, b) = rgb;
    0.2126 * chan(r) + 0.7152 * chan(g) + 0.0722 * chan(b)
}

fn ratio(a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
    let (la, lb) = (lum(a), lum(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn rgb(def: &doris::ui::theme::ColorDef) -> (u8, u8, u8) {
    (def.r, def.g, def.b)
}

/// Every role that sits on the theme background, with the threshold that
/// role has to clear.
const ON_BACKGROUND: &[(&str, f64)] = &[
    ("main_fg", TEXT),
    ("title", NON_TEXT),
    ("hi_fg", NON_TEXT),
    ("div_line", NON_TEXT),
    ("inactive_fg", NON_TEXT),
    ("graph_text", NON_TEXT),
];

#[test]
fn every_bundled_theme_is_legible_on_its_own_background() {
    let themes = Theme::load_themes();
    assert!(
        themes.len() >= 30,
        "the bundled themes went missing: only {}",
        themes.len()
    );

    let mut failures = Vec::new();
    for t in &themes {
        let bg = rgb(&t.main_bg);
        for (role, need) in ON_BACKGROUND {
            let color = match *role {
                "main_fg" => rgb(&t.main_fg),
                "title" => rgb(&t.title),
                "hi_fg" => rgb(&t.hi_fg),
                "div_line" => rgb(&t.div_line),
                "inactive_fg" => rgb(&t.inactive_fg),
                "graph_text" => rgb(&t.graph_text),
                other => panic!("no accessor for {other}"),
            };
            let got = ratio(color, bg);
            if got < *need {
                failures.push(format!(
                    "{}: {role} is {got:.2}:1 against its background, needs {need}:1",
                    t.name
                ));
            }
        }
    }

    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// The row under the cursor is the one the user reads most, and it has its own background --
/// not the panel's.
#[test]
fn the_cursor_row_is_legible_on_its_own_highlight() {
    let mut failures = Vec::new();
    for t in &Theme::load_themes() {
        let got = ratio(rgb(&t.selected_fg), rgb(&t.selected_bg));
        if got < TEXT {
            failures.push(format!(
                "{}: the cursor row is {got:.2}:1, needs {TEXT}:1",
                t.name
            ));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

/// The focused zone has to look different from the ones that are not.
#[test]
fn focus_is_not_carried_by_colour_alone() {
    use doris::ui::layout::{zone_title, ZoneId};

    let theme = Theme::default();
    let text = |line: ratatui::text::Line| -> String {
        line.spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>()
    };

    for &id in ZoneId::all() {
        let focused = zone_title(id, &theme, true);
        let plain = zone_title(id, &theme, false);

        assert!(
            text(focused.clone()).contains('▸'),
            "{id:?} must mark itself when focused: {:?}",
            text(focused.clone())
        );
        assert!(
            !text(plain.clone()).contains('▸'),
            "{id:?} must not carry the marker when it is not"
        );
        assert_eq!(
            focused.width(),
            plain.width(),
            "{id:?}: the frame legend must not shift when focus moves"
        );
    }
}
