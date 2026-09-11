use doris::ui::zones::{ZoneId, ZoneLayout};
use ratatui::layout::Rect;

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
