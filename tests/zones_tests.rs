use doris::ui::theme::Theme;
use doris::ui::zones::{button_spans, zone_buttons, zone_title, zone_title_width, ZoneId, ZoneLayout};
use ratatui::layout::Rect;
use ratatui::style::Modifier;

#[test]
fn test_zone_id_key_char_and_from_key_round_trip() {
    for &id in ZoneId::all() {
        let c = id.key_char();
        assert_eq!(ZoneId::from_key(c), Some(id));
    }
    assert_eq!(ZoneId::from_key('9'), None);
    assert_eq!(ZoneId::from_key('x'), None);
}

#[test]
fn test_default_layout_visibility() {
    let zones = ZoneLayout::new();
    assert!(zones.is_visible(ZoneId::Results));
    assert!(zones.is_visible(ZoneId::Torrent));
    assert!(zones.is_visible(ZoneId::Log));
    assert!(!zones.is_visible(ZoneId::Extra));
    assert_eq!(zones.focused, ZoneId::Results);
    assert_eq!(zones.fullscreen, None);
}

#[test]
fn test_toggle_flips_visibility_and_focuses_when_shown() {
    let mut zones = ZoneLayout::new();
    assert!(!zones.is_visible(ZoneId::Extra));

    zones.toggle(ZoneId::Extra);
    assert!(zones.is_visible(ZoneId::Extra));
    assert_eq!(zones.focused, ZoneId::Extra);

    zones.toggle(ZoneId::Extra);
    assert!(!zones.is_visible(ZoneId::Extra));
}

#[test]
fn test_toggle_on_fullscreened_zone_exits_fullscreen() {
    let mut zones = ZoneLayout::new();
    zones.set_fullscreen(Some(ZoneId::Results));
    assert_eq!(zones.fullscreen, Some(ZoneId::Results));

    zones.toggle(ZoneId::Results);
    assert_eq!(zones.fullscreen, None);
}

#[test]
fn test_set_visible_directly_controls_state() {
    let mut zones = ZoneLayout::new();
    zones.set_visible(ZoneId::Results, false);
    assert!(!zones.is_visible(ZoneId::Results));
    zones.set_visible(ZoneId::Results, true);
    assert!(zones.is_visible(ZoneId::Results));
}

#[test]
fn test_set_visible_false_clears_matching_fullscreen() {
    let mut zones = ZoneLayout::new();
    zones.set_fullscreen(Some(ZoneId::Log));
    zones.set_visible(ZoneId::Log, false);
    assert_eq!(zones.fullscreen, None);
}

#[test]
fn test_apply_preset_shows_exactly_the_named_zones() {
    let mut zones = ZoneLayout::new();
    zones.apply_preset("1,3");
    assert!(zones.is_visible(ZoneId::Results));
    assert!(!zones.is_visible(ZoneId::Torrent));
    assert!(zones.is_visible(ZoneId::Log));
    assert!(!zones.is_visible(ZoneId::Extra));
}

#[test]
fn test_apply_preset_all_four() {
    let mut zones = ZoneLayout::new();
    zones.apply_preset("1,2,3,4");
    for &id in ZoneId::all() {
        assert!(zones.is_visible(id), "{:?} should be visible", id);
    }
}

#[test]
fn test_apply_preset_ignores_unknown_characters() {
    let mut zones = ZoneLayout::new();
    zones.apply_preset("1,3,9,x");
    assert!(zones.is_visible(ZoneId::Results));
    assert!(zones.is_visible(ZoneId::Log));
    assert!(!zones.is_visible(ZoneId::Torrent));
    assert!(!zones.is_visible(ZoneId::Extra));
}

#[test]
fn test_apply_preset_moves_focus_off_a_now_hidden_zone() {
    let mut zones = ZoneLayout::new();
    zones.focused = ZoneId::Torrent;
    zones.apply_preset("1,3"); // hides Torrent
    assert!(zones.is_visible(zones.focused), "focus must land on a visible zone");
    assert_ne!(zones.focused, ZoneId::Torrent);
}

#[test]
fn test_apply_preset_keeps_focus_if_still_visible() {
    let mut zones = ZoneLayout::new();
    zones.focused = ZoneId::Results;
    zones.apply_preset("1,3");
    assert_eq!(zones.focused, ZoneId::Results);
}

#[test]
fn test_focus_next_skips_hidden_zones_and_wraps() {
    let mut zones = ZoneLayout::new();
    // Default visible order: Results, Torrent, Log (Extra hidden).
    zones.focused = ZoneId::Results;
    zones.focus_next();
    assert_eq!(zones.focused, ZoneId::Torrent);
    zones.focus_next();
    assert_eq!(zones.focused, ZoneId::Log);
    zones.focus_next(); // wraps back to Results, skipping hidden Extra
    assert_eq!(zones.focused, ZoneId::Results);
}

#[test]
fn test_focus_prev_skips_hidden_zones_and_wraps() {
    let mut zones = ZoneLayout::new();
    zones.focused = ZoneId::Results;
    zones.focus_prev(); // wraps to the last visible zone (Log), skipping Extra
    assert_eq!(zones.focused, ZoneId::Log);
}

#[test]
fn test_focus_next_noop_when_nothing_visible() {
    let mut zones = ZoneLayout::new();
    for &id in ZoneId::all() {
        zones.set_visible(id, false);
    }
    let before = zones.focused;
    zones.focus_next();
    assert_eq!(zones.focused, before);
}

/// Regression test for the crash reported after enabling the default
/// "1,2,3,4" preset: Results and Extra used to each independently claim
/// the *entire* leftover height instead of splitting it, so their
/// combined area ran past the bottom of the terminal and panicked
/// ratatui with an out-of-bounds buffer write. This asserts the actual
/// invariant that bug violated, across a range of terminal sizes, so any
/// future regression here fails a test instead of crashing the TUI.
#[test]
fn test_all_zones_visible_never_exceeds_terminal_height() {
    for &(w, h) in &[(80u16, 24u16), (100, 30), (141, 35), (60, 15), (200, 50)] {
        let mut zones = ZoneLayout::new();
        zones.apply_preset("1,2,3,4");
        let area = Rect::new(0, 0, w, h);
        zones.update_areas(area);

        for &id in ZoneId::all() {
            let zone_area = zones.get_area(id);
            let bottom = zone_area.y + zone_area.height;
            assert!(
                bottom <= h,
                "{:?} area {:?} extends to row {} but terminal is only {} rows tall",
                id, zone_area, bottom, h
            );
            let right = zone_area.x + zone_area.width;
            assert!(right <= w, "{:?} area {:?} extends past terminal width {}", id, zone_area, w);
        }
    }
}

#[test]
fn test_update_areas_with_only_results_visible() {
    let mut zones = ZoneLayout::new();
    for &id in ZoneId::all() {
        zones.set_visible(id, id == ZoneId::Results);
    }
    let area = Rect::new(0, 0, 80, 24);
    zones.update_areas(area);

    let results_area = zones.get_area(ZoneId::Results);
    assert!(results_area.height > 0);
    assert!(results_area.y + results_area.height <= 24);

    // Hidden zones should have an empty (zero-sized) area, not a stale
    // or overlapping one.
    for &id in ZoneId::all() {
        if id != ZoneId::Results {
            assert_eq!(zones.get_area(id), Rect::default());
        }
    }
}

#[test]
fn test_update_areas_fullscreen_gives_full_area_to_one_zone_only() {
    let mut zones = ZoneLayout::new();
    zones.set_fullscreen(Some(ZoneId::Log));
    let area = Rect::new(0, 0, 80, 24);
    zones.update_areas(area);

    assert_eq!(zones.get_area(ZoneId::Log), area);
    for &id in ZoneId::all() {
        if id != ZoneId::Log {
            assert_eq!(zones.get_area(id), Rect::default());
        }
    }
}

#[test]
fn test_update_areas_with_nothing_visible_does_not_panic() {
    let mut zones = ZoneLayout::new();
    for &id in ZoneId::all() {
        zones.set_visible(id, false);
    }
    let area = Rect::new(0, 0, 80, 24);
    zones.update_areas(area); // must not panic
    for &id in ZoneId::all() {
        assert_eq!(zones.get_area(id), Rect::default());
    }
}

// --- the frame legend (btop's buttons drawn on the border) ----------------

/// btop's box title: superscript number in `hi_fg` + bold, the label in
/// `title` (`btop_draw.cpp:290` + `:332`). Ours used to be one flat
/// string, so nothing distinguished the zone number from its name.
#[test]
fn test_zone_title_marks_the_number_hi_fg_and_the_label_title() {
    let theme = Theme::dark();
    let line = zone_title(ZoneId::Results, &theme);
    let spans = line.spans;

    assert_eq!(spans.len(), 5);
    assert_eq!(spans[1].content.to_string(), "\u{00B9}");

    let number = spans[1].style;
    assert_eq!(number.fg, Some(theme.hi_fg.to_color()));
    assert!(number.add_modifier.contains(Modifier::BOLD));

    assert_eq!(spans[3].content.to_string(), "Results");
    let label = spans[3].style;
    assert_eq!(label.fg, Some(theme.title.to_color()));
    assert!(!label.add_modifier.contains(Modifier::BOLD));
}

/// `zone_title_width` is what positions the whole frame legend, so it has
/// to describe the line that is actually drawn -- off by one and every
/// button would start one column late.
#[test]
fn test_zone_title_width_matches_the_drawn_title() {
    for &id in ZoneId::all() {
        let theme = Theme::dark();
        let drawn: usize = zone_title(id, &theme)
            .spans
            .iter()
            .map(|s| s.content.chars().count())
            .sum();
        assert_eq!(drawn as u16, zone_title_width(id), "{:?}", id);
    }
}

/// The highlight marks the hotkey, not the alphabet: btop spells it
/// "pa**u**se" because `p` was taken, so the styled span has to land on
/// exactly the character that triggers the button, and the three spans
/// have to reassemble the word unchanged.
#[test]
fn test_button_spans_put_the_hotkey_on_the_key_character() {
    let theme = Theme::dark();
    for &id in ZoneId::all() {
        for button in zone_buttons(id) {
            let text = button.text();
            let spans = button_spans(&theme, button, false);

            assert_eq!(spans.len(), 3, "{}: {}", id.label(), text);
            let joined: String = spans.iter().map(|s| s.content.to_string()).collect();
            assert_eq!(joined, text, "{}: the spans reassemble the word", text);
            assert_eq!(
                spans[1].content.to_string(),
                button.key.to_string(),
                "{}: the hotkey span must be the key itself",
                text
            );
            assert_eq!(
                spans[1].style.fg,
                Some(theme.hi_fg.to_color()),
                "{}: hotkey is hi_fg",
                text
            );
        }
    }
}

/// A pressed toggle is drawn bold, the whole word -- btop wraps `pause`
/// in `Fx::b` while `pause_proc_list` is on.
#[test]
fn test_active_button_bolds_the_whole_word() {
    let theme = Theme::dark();
    let button = zone_buttons(ZoneId::Torrent)
        .iter()
        .find(|b| b.key == 'p')
        .expect("Torrent has a pause button");

    let idle = button_spans(&theme, button, false);
    let active = button_spans(&theme, button, true);
    // Idle: only the hotkey is bold, the word around it is not.
    assert!(!idle[0].style.add_modifier.contains(Modifier::BOLD));
    assert!(!idle[2].style.add_modifier.contains(Modifier::BOLD));
    assert!(idle[1].style.add_modifier.contains(Modifier::BOLD));
    // Active: the whole word goes bold.
    assert!(active[0].style.add_modifier.contains(Modifier::BOLD));
    assert!(active[2].style.add_modifier.contains(Modifier::BOLD));
    // The hotkey stays bold either way -- it is the key either way.
    assert!(active[1].style.add_modifier.contains(Modifier::BOLD));
}

/// The convention the plan asks for: primary functions use their first
/// letter for the hotkey, as long as it isn't already taken. The words
/// that cannot (`source` is `]`, `play` is Enter, `info` is `v`) trail
/// the key instead -- see `FrameButton::text`.
#[test]
fn test_primary_buttons_lead_with_their_hotkey() {
    let primary = [
        (ZoneId::Results, 'F'),
        (ZoneId::Results, 'd'),
        (ZoneId::Results, 'g'),
        (ZoneId::Torrent, 'p'),
        (ZoneId::Torrent, 'd'),
    ];
    for (id, key) in primary {
        let button = zone_buttons(id)
            .iter()
            .find(|b| b.key == key)
            .unwrap_or_else(|| panic!("{:?} has no '{}' button", id, key));
        let first = button.label.chars().next().expect("label is not empty");
        assert_eq!(
            first.to_ascii_lowercase(),
            key.to_ascii_lowercase(),
            "{} should lead with its hotkey so the border reads as a mnemonic",
            button.label
        );
    }
}
