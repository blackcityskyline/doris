use doris::sources::source::Group;
use doris::ui::theme::Theme;
use doris::ui::zones::{
    button_spans, zone_buttons, zone_title, zone_title_width, FrameSlot, ZoneId, ZoneLayout,
};
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

// Phase 10 (REFACTOR_PLAN.md): key_char/label/from_key all read one
// ZONE_ROWS entry. Guard against a variant with no row (or two variants
// sharing a digit) -- that would silently break the `1`-`5` zone keys.
#[test]
fn test_every_zone_has_one_complete_row() {
    let keys: Vec<char> = ZoneId::all().iter().map(|z| z.key_char()).collect();
    for &id in ZoneId::all() {
        assert_ne!(id.label(), "", "{id:?} has no label");
        assert_ne!(id.key_char(), '\0', "{id:?} has no key");
    }
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    let unique = sorted.len();
    sorted.dedup();
    assert_eq!(unique, sorted.len(), "two zones share a digit key");
    // all() is the render-side list; it must match the row set exactly,
    // or a zone would render but be unreachable from the keyboard.
    assert_eq!(ZoneId::all().len(), keys.len());
}

#[test]
fn test_default_layout_visibility() {
    let zones = ZoneLayout::new();
    assert!(zones.is_visible(ZoneId::Results));
    assert!(zones.is_visible(ZoneId::Torrent));
    assert!(zones.is_visible(ZoneId::Log));
    assert!(zones.is_visible(ZoneId::Trackers));
    assert_eq!(zones.focused, ZoneId::Results);
    assert_eq!(zones.fullscreen, None);
}

#[test]
fn test_toggle_flips_visibility_and_focuses_when_shown() {
    let mut zones = ZoneLayout::new();
    zones.set_visible(ZoneId::Log, false);
    assert!(!zones.is_visible(ZoneId::Log));

    zones.toggle(ZoneId::Log);
    assert!(zones.is_visible(ZoneId::Log));
    assert_eq!(zones.focused, ZoneId::Log);

    zones.toggle(ZoneId::Log);
    assert!(!zones.is_visible(ZoneId::Log));
}

/// The panel renamed to Trackers sits at `3` and Log moved to the last
/// slot `4`, so the zone keyboard reads Results / Torrent / Trackers /
/// Log in order. Digits `1`-`4` are the whole zone keyboard.
#[test]
fn test_trackers_is_three_and_log_is_four() {
    assert_eq!(ZoneId::Trackers.label(), "Trackers");
    assert_eq!(ZoneId::from_key('3'), Some(ZoneId::Trackers));
    assert_eq!(ZoneId::from_key('4'), Some(ZoneId::Log));
    assert_eq!(ZoneId::from_key('5'), None);
    assert_eq!(
        ZoneId::all(),
        &[
            ZoneId::Results,
            ZoneId::Torrent,
            ZoneId::Trackers,
            ZoneId::Log
        ]
    );
    // The superscript in the frame title is `id as u8`, so the
    // discriminant has to agree with the key digit.
    assert_eq!(ZoneId::Trackers as u8, 3);
    assert_eq!(ZoneId::Log as u8, 4);
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
    // `1,3` names zones by key digit: Results and Trackers (Log moved
    // to `4` when Trackers took `3`).
    let mut zones = ZoneLayout::new();
    zones.apply_preset("1,4");
    assert!(zones.is_visible(ZoneId::Results));
    assert!(!zones.is_visible(ZoneId::Torrent));
    assert!(zones.is_visible(ZoneId::Log));
    assert!(!zones.is_visible(ZoneId::Trackers));
}

#[test]
fn test_apply_preset_all_five() {
    let mut zones = ZoneLayout::new();
    zones.apply_preset("1,2,3,4,5");
    for &id in ZoneId::all() {
        assert!(zones.is_visible(id), "{:?} should be visible", id);
    }
}

#[test]
fn test_apply_preset_ignores_unknown_characters() {
    let mut zones = ZoneLayout::new();
    zones.apply_preset("1,4,9,x");
    assert!(zones.is_visible(ZoneId::Results));
    assert!(zones.is_visible(ZoneId::Log));
    assert!(!zones.is_visible(ZoneId::Torrent));
    assert!(!zones.is_visible(ZoneId::Trackers));
}

#[test]
fn test_apply_preset_moves_focus_off_a_now_hidden_zone() {
    let mut zones = ZoneLayout::new();
    zones.focused = ZoneId::Torrent;
    zones.apply_preset("1,4"); // hides Torrent
    assert!(
        zones.is_visible(zones.focused),
        "focus must land on a visible zone"
    );
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
    // Default visible order: Results, Torrent, Trackers, Log.
    zones.focused = ZoneId::Results;
    zones.focus_next();
    assert_eq!(zones.focused, ZoneId::Torrent);
    zones.focus_next();
    assert_eq!(zones.focused, ZoneId::Trackers);
    zones.focus_next();
    assert_eq!(zones.focused, ZoneId::Log);
    zones.focus_next(); // wraps back to Results
    assert_eq!(zones.focused, ZoneId::Results);
}

#[test]
fn test_focus_prev_skips_hidden_zones_and_wraps() {
    let mut zones = ZoneLayout::new();
    zones.focused = ZoneId::Results;
    zones.focus_prev(); // wraps to the last visible zone (Log)
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
/// "1,2,3,4" preset: two zones used to each independently claim the
/// *entire* leftover height instead of splitting it, so their combined
/// area ran past the bottom of the terminal and panicked ratatui with an
/// out-of-bounds buffer write. This asserts the actual invariant that
/// bug violated, across a range of terminal sizes, so any future
/// regression here fails a test instead of crashing the TUI.
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
                id,
                zone_area,
                bottom,
                h
            );
            let right = zone_area.x + zone_area.width;
            assert!(
                right <= w,
                "{:?} area {:?} extends past terminal width {}",
                id,
                zone_area,
                w
            );
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

/// btop's box title: superscript number + bold, the label plain -- ours
/// used to be one flat string, so nothing distinguished the zone number
/// from its name. The colours are the theme's structure tokens: the
/// number `secondary`, the label `primary`.
///
/// The label is split at its first letter: that letter is the key that
/// opens the zone's detail view (`L`/`T`/`R`), so it carries the same
/// `on_hover` + bold mark a frame button's hotkey carries, and the rest
/// of the word stays plain `primary`.
#[test]
fn test_zone_title_marks_the_number_secondary_and_the_label_primary() {
    let theme = Theme::dark();
    let line = zone_title(ZoneId::Results, &theme);
    let spans = line.spans;

    assert_eq!(spans.len(), 6);
    assert_eq!(spans[1].content.to_string(), "\u{00B9}");

    let number = spans[1].style;
    assert_eq!(number.fg, Some(theme.secondary_color()));
    assert!(number.add_modifier.contains(Modifier::BOLD));

    // The detail-view key leads the word...
    assert_eq!(spans[3].content.to_string(), "R");
    assert_eq!(spans[3].style.fg, Some(theme.on_hover_color()));
    assert!(spans[3].style.add_modifier.contains(Modifier::BOLD));

    // ...and the rest of the label is still plain primary.
    assert_eq!(spans[4].content.to_string(), "esults");
    let label = spans[4].style;
    assert_eq!(label.fg, Some(theme.primary_color()));
    assert!(!label.add_modifier.contains(Modifier::BOLD));
}

/// Every zone whose label starts with its detail key gets the mark;
/// Trackers has no detail view, so its label stays one plain span.
#[test]
fn test_zone_title_marks_the_detail_key_of_each_zone_that_has_one() {
    let theme = Theme::dark();
    for (id, key) in [
        (ZoneId::Log, "L"),
        (ZoneId::Torrent, "T"),
        (ZoneId::Results, "R"),
    ] {
        let spans = zone_title(id, &theme).spans;
        assert_eq!(spans.len(), 6, "{:?} splits its label", id);
        assert_eq!(spans[3].content.to_string(), key, "{:?} leads", id);
        assert_eq!(
            spans[3].style.fg,
            Some(theme.on_hover_color()),
            "{:?} hotkey is on_hover",
            id
        );
        assert!(spans[3].style.add_modifier.contains(Modifier::BOLD));
    }

    let trackers = zone_title(ZoneId::Trackers, &theme).spans;
    assert_eq!(trackers.len(), 5, "Trackers has no detail view to name");
    assert_eq!(trackers[3].content.to_string(), "Trackers");
    assert!(!trackers[3].style.add_modifier.contains(Modifier::BOLD));
}

/// The Log frame no longer advertises `detail`: `L` moved into the
/// zone's own title, and two marks for one key is one too many.
#[test]
fn test_the_log_frame_carries_no_detail_button() {
    let buttons = zone_buttons(ZoneId::Log);
    assert!(
        buttons.is_empty(),
        "the Log frame must not advertise a detail button"
    );
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
        let buttons = zone_buttons(id);
        for button in &buttons {
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
                Some(theme.on_hover_color()),
                "{}: hotkey is on_hover",
                text
            );
        }
    }
}

/// The filter is a state toggle like `group`, so it lives in the same
/// right-hand cluster beside it instead of standing alone on the left,
/// and it is spelled in lowercase like every other legend word -- the
/// hotkey `f` is what marks it, not a capital letter.
#[test]
fn test_the_filter_button_sits_on_the_right_next_to_group() {
    let buttons = zone_buttons(ZoneId::Results);
    let filter = buttons
        .iter()
        .find(|b| b.key == 'f')
        .expect("Results has a filter button");
    assert_eq!(
        filter.slot,
        FrameSlot::TopRight,
        "filter moved beside group"
    );
    assert_eq!(filter.label, "filter", "lowercase, like `group`/`pause`");

    let group = buttons
        .iter()
        .find(|b| b.key == 'g')
        .expect("Results has a group button");
    assert_eq!(filter.slot, group.slot, "they share the right cluster");
}

/// The category name is centred in a slot as wide as the widest
/// category, so the arrows hold the same columns no matter which one
/// shows: `◀..TV..▶` beside `◀Movies▶`, not `◀TV    ▶`.
#[test]
fn test_the_category_name_is_centred_in_its_slot() {
    let mut app = doris::ui::app::App::new("http://127.0.0.1:8090".into(), None);
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    let area = app.zones.get_area(ZoneId::Results);
    let config = doris::config::Config::default();

    let layout = app.frame_layout(ZoneId::Results, area, &config);
    let label = layout
        .buttons
        .iter()
        .find(|(b, _)| b.is_category())
        .map(|(b, _)| b.label.clone())
        .expect("the Results frame has a category button");

    let width = app
        .group_tabs
        .iter()
        .map(|g| g.map_or("all", Group::label).chars().count())
        .max()
        .unwrap_or(3);
    let name = app.active_group.map_or("all", Group::label);
    assert_eq!(
        label,
        format!("◀ {:^width$} ▶", name, width = width),
        "centred in the fixed slot, extra space to the right"
    );
    assert_eq!(
        label.chars().count(),
        1 + 1 + width + 1 + 1,
        "the arrows stay in the same columns whatever the name"
    );
}

/// The category button's arrows are the mouse targets, so both take the
/// `on_hover` + bold treatment and the name between them stays
/// `primary` -- btop draws its sortable column headers the same way.
/// The button is built in `frame_layout` (its label names the current
/// category), so that is where the test reads it from.
#[test]
fn test_the_category_button_highlights_both_arrows() {
    let theme = Theme::dark();
    let mut app = doris::ui::app::App::new("http://127.0.0.1:8090".into(), None);
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    let area = app.zones.get_area(ZoneId::Results);
    let config = doris::config::Config::default();

    let layout = app.frame_layout(ZoneId::Results, area, &config);
    let button = layout
        .buttons
        .iter()
        .find(|(b, _)| b.is_category())
        .map(|(b, _)| b)
        .expect("the Results frame has a category button");

    let spans = button_spans(&theme, button, false);
    assert_eq!(spans.len(), 3, "arrow / name / arrow");
    assert_eq!(spans[0].content.to_string(), "◀");
    assert_eq!(spans[2].content.to_string(), "▶");
    for arrow in [&spans[0], &spans[2]] {
        assert_eq!(
            arrow.style.fg,
            Some(theme.on_hover_color()),
            "an arrow is a mouse target, drawn like a hotkey"
        );
        assert!(
            arrow.style.add_modifier.contains(Modifier::BOLD),
            "and bold"
        );
    }
    assert_eq!(
        spans[1].style.fg,
        Some(theme.primary_color()),
        "the name is display-only"
    );
}

/// A pressed toggle is drawn bold, the whole word -- btop wraps `pause`
/// in `Fx::b` while `pause_proc_list` is on.
#[test]
fn test_active_button_bolds_the_whole_word() {
    let theme = Theme::dark();
    let buttons = zone_buttons(ZoneId::Torrent);
    let button = buttons
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
/// that cannot trail the key instead -- see `FrameButton::text`.
#[test]
fn test_primary_buttons_lead_with_their_hotkey() {
    let primary = [
        (ZoneId::Results, 'f'),
        (ZoneId::Results, 'g'),
        (ZoneId::Torrent, 'p'),
        (ZoneId::Torrent, 'd'),
    ];
    for (id, key) in primary {
        let buttons = zone_buttons(id);
        let button = buttons
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

// --- layout tiling (rows via `,`, columns via `|`) --------------------------

/// The spec is written the way it is read: rows separated by `,`,
/// columns inside a row by `|`. The old flat list `"1,2,3,4"` still
/// parses -- it is four rows of one cell each -- so an existing
/// `config.toml` keeps meaning "every zone, stacked full width".
#[test]
fn test_a_preset_spec_writes_rows_and_columns() {
    let mut zones = ZoneLayout::new();
    zones.apply_preset("1,3|4");

    assert!(zones.is_visible(ZoneId::Results));
    assert!(zones.is_visible(ZoneId::Trackers));
    assert!(zones.is_visible(ZoneId::Log));
    assert!(!zones.is_visible(ZoneId::Torrent), "row 2 is `3|4`");

    assert_eq!(
        zones.grid,
        vec![vec![ZoneId::Results], vec![ZoneId::Trackers, ZoneId::Log]],
        "row 1 is Results alone, row 2 is Trackers beside Log"
    );
}

/// The default tiling the plan asks for: Results across the top, then
/// Trackers | Log underneath, Torrent switched off.
#[test]
fn test_the_default_tiling_is_results_over_trackers_and_log() {
    let mut zones = ZoneLayout::new();
    zones.apply_preset("1,3|4");
    zones.update_areas(Rect::new(0, 0, 100, 30));

    // 27 rows below the search bar, split over two rows: 14 then 13.
    assert_eq!(zones.get_area(ZoneId::Results), Rect::new(0, 3, 100, 14));
    // The second row splits its width between its two cells.
    assert_eq!(zones.get_area(ZoneId::Trackers), Rect::new(0, 17, 50, 13));
    assert_eq!(zones.get_area(ZoneId::Log), Rect::new(50, 17, 50, 13));
    assert_eq!(zones.get_area(ZoneId::Torrent), Rect::default());
}

/// An old flat spec still stacks every zone at full width, which is
/// exactly what it meant before there were rows.
#[test]
fn test_a_flat_spec_still_stacks_zones_full_width() {
    let mut zones = ZoneLayout::new();
    zones.apply_preset("1,2,3,4");
    zones.update_areas(Rect::new(0, 0, 100, 30));

    // 27 rows over four zones: 7, 7, 7, 6 -- the first three get the
    // remainder, so no row of the terminal is wasted.
    assert_eq!(zones.get_area(ZoneId::Results), Rect::new(0, 3, 100, 7));
    assert_eq!(zones.get_area(ZoneId::Torrent), Rect::new(0, 10, 100, 7));
    assert_eq!(zones.get_area(ZoneId::Trackers), Rect::new(0, 17, 100, 7));
    assert_eq!(zones.get_area(ZoneId::Log), Rect::new(0, 24, 100, 6));
}

/// A cell switched off hands its width to the rest of its row, so the
/// tiling never leaves a hole.
#[test]
fn test_hiding_a_cell_gives_its_width_to_its_row() {
    let mut zones = ZoneLayout::new();
    zones.apply_preset("1,3|4");
    zones.set_visible(ZoneId::Trackers, false);
    zones.update_areas(Rect::new(0, 0, 100, 30));

    assert_eq!(zones.get_area(ZoneId::Log), Rect::new(0, 17, 100, 13));
    assert_eq!(zones.get_area(ZoneId::Trackers), Rect::default());
}

/// A whole row switched off hands its height to the rows that remain.
#[test]
fn test_hiding_a_row_gives_its_height_to_the_rest() {
    let mut zones = ZoneLayout::new();
    zones.apply_preset("1,3|4");
    zones.set_visible(ZoneId::Trackers, false);
    zones.set_visible(ZoneId::Log, false);
    zones.update_areas(Rect::new(0, 0, 100, 30));

    assert_eq!(
        zones.get_area(ZoneId::Results),
        Rect::new(0, 3, 100, 27),
        "Results takes everything below the search bar"
    );
}

/// The zone keys keep working after a preset: a zone the spec never
/// names still gets room, as a row of its own at the bottom.
#[test]
fn test_a_zone_shown_by_hand_gets_its_own_row() {
    let mut zones = ZoneLayout::new();
    zones.apply_preset("1,3|4");
    zones.toggle(ZoneId::Torrent);
    zones.update_areas(Rect::new(0, 0, 100, 30));

    // Three rows over 27: 9 each.
    assert_eq!(zones.get_area(ZoneId::Results), Rect::new(0, 3, 100, 9));
    assert_eq!(zones.get_area(ZoneId::Trackers), Rect::new(0, 12, 50, 9));
    assert_eq!(zones.get_area(ZoneId::Log), Rect::new(50, 12, 50, 9));
    assert_eq!(zones.get_area(ZoneId::Torrent), Rect::new(0, 21, 100, 9));
}

/// A terminal too short for the tiling still gets a layout: nothing
/// panics and nothing is drawn outside the frame.
#[test]
fn test_the_tiling_survives_a_tiny_terminal() {
    let mut zones = ZoneLayout::new();
    zones.apply_preset("1,3|4");
    zones.update_areas(Rect::new(0, 0, 40, 6));

    for &id in ZoneId::all() {
        let area = zones.get_area(id);
        assert!(
            area.x + area.width <= 40,
            "{:?} runs off the right edge",
            id
        );
        assert!(
            area.y + area.height <= 6,
            "{:?} runs off the bottom edge",
            id
        );
    }
}

/// The search bar is not part of any spec: it is always the top rows
/// at full width, whatever the tiling says.
#[test]
fn test_the_search_bar_is_outside_every_spec() {
    for spec in ["1,3|4", "1,2,3,4", "1,3"] {
        let mut zones = ZoneLayout::new();
        zones.apply_preset(spec);
        zones.update_areas(Rect::new(0, 0, 100, 30));
        for &id in ZoneId::all() {
            let area = zones.get_area(id);
            if area.height == 0 {
                continue;
            }
            assert!(
                area.y >= doris::ui::zones::SEARCH_BAR_HEIGHT,
                "{:?} starts inside the search bar under `{}`",
                id,
                spec
            );
        }
    }
}

/// Fullscreen outranks any spec: one zone gets the whole frame, the
/// others get nothing.
#[test]
fn test_fullscreen_overrides_the_tiling() {
    let mut zones = ZoneLayout::new();
    zones.apply_preset("1,3|4");
    zones.set_fullscreen(Some(ZoneId::Torrent));
    zones.update_areas(Rect::new(0, 0, 100, 30));

    assert_eq!(zones.get_area(ZoneId::Torrent), Rect::new(0, 0, 100, 30));
    assert_eq!(zones.get_area(ZoneId::Results), Rect::default());
    assert_eq!(zones.get_area(ZoneId::Log), Rect::default());
}

/// A layout that has never had a spec applied still tiles: the fresh
/// state is one full-width row per zone, exactly what the flat default
/// spec would give.
#[test]
fn test_a_fresh_layout_stacks_every_zone() {
    let mut zones = ZoneLayout::new();
    zones.update_areas(Rect::new(0, 0, 100, 30));

    assert!(zones.is_visible(ZoneId::Torrent), "all four start on");
    assert_eq!(zones.get_area(ZoneId::Results), Rect::new(0, 3, 100, 7));
    assert_eq!(zones.get_area(ZoneId::Log), Rect::new(0, 24, 100, 6));
}
