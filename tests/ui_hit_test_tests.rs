use doris::config::Config;
use doris::sources::models::TorrentItem;
use doris::sources::source::Group;
use doris::ui::layout::ZoneId;
use doris::ui::modals::help::sections;
use doris::ui::view::{App as UiApp, UiAction};
use ratatui::layout::Rect;

/// A config as a first run gets it: every implemented source switched on.
fn test_config() -> Config {
    let mut config = Config::default();
    doris::sources::source::first_run_config(&mut config);
    config
}

fn make_app() -> UiApp {
    UiApp::new("http://127.0.0.1:8090".to_string(), None)
}

fn make_results(n: usize) -> Vec<TorrentItem> {
    (0..n)
        .map(|i| TorrentItem {
            title: format!("Torrent {}", i),
            size: format!("{} GB", i + 1),
            seeds: format!("{}", i * 10),
            date: format!("01-Jan-2{}", i),
            download_url: format!("/forum/dl.php?t={}", 1000 + i),
            page_url: format!("viewtopic.php?t={}", 1000 + i),
            query: "test".into(),
            ..Default::default()
        })
        .collect()
}

// --- search_box_at ---------------------------------------------------------

/// The input box owns the top three rows of the frame and nothing else: `render_search_bar`
/// draws into exactly that, and `update_areas` starts the zones below it.
#[test]
fn test_search_box_covers_the_top_three_rows_only() {
    let app = make_app();
    for row in 0..3 {
        assert!(app.search_box_at(row), "row {} is the input box", row);
    }
    assert!(!app.search_box_at(3));
    assert!(!app.search_box_at(40));
}

/// Something can paint over the box: fullscreen stretches a zone across the whole frame and the
/// detail log takes it too.
#[test]
fn test_search_box_is_not_a_target_when_something_covers_it() {
    let mut app = make_app();

    app.zones.fullscreen = Some(ZoneId::Results);
    assert!(!app.search_box_at(0));

    app.zones.fullscreen = None;
    app.detail_view = Some(ZoneId::Log);
    assert!(!app.search_box_at(0));
}

// --- zone_at ------------------------------------------------------------

#[test]
fn test_zone_at_finds_the_containing_zone() {
    let mut app = make_app();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));

    let results_area = app.zones.get_area(ZoneId::Results);
    assert_eq!(
        app.zone_at(results_area.y, results_area.x),
        Some(ZoneId::Results)
    );

    let torrent_area = app.zones.get_area(ZoneId::Torrent);
    assert_eq!(
        app.zone_at(torrent_area.y, torrent_area.x),
        Some(ZoneId::Torrent)
    );
}

#[test]
fn test_zone_at_returns_none_outside_all_zones() {
    let mut app = make_app();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    // Far outside the terminal entirely.
    assert_eq!(app.zone_at(1000, 1000), None);
}

#[test]
fn test_zone_at_only_hits_fullscreened_zone() {
    let mut app = make_app();
    app.zones.set_fullscreen(Some(ZoneId::Log));
    app.zones.update_areas(Rect::new(0, 0, 80, 24));

    // Every point in the full terminal area should resolve to Log...
    assert_eq!(app.zone_at(10, 10), Some(ZoneId::Log));
    //...since every other zone's area is zeroed out while fullscreened.
    let torrent_area_before = app.zones.get_area(ZoneId::Torrent);
    assert_eq!(torrent_area_before, Rect::default());
}

// --- click_at -------------------------------------------------------------

#[test]
fn test_click_at_focuses_the_clicked_zone() {
    let mut app = make_app();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    assert_eq!(app.zones.focused, ZoneId::Results);

    let log_area = app.zones.get_area(ZoneId::Log);
    let mut config = test_config();
    app.click_at(log_area.y, log_area.x, &mut config);
    assert_eq!(app.zones.focused, ZoneId::Log);
}

#[test]
fn test_click_at_results_header_row_does_not_select_a_row() {
    let mut app = make_app();
    app.results = make_results(5);
    app.filtered_indices = (0..5).collect();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));

    let results_area = app.zones.get_area(ZoneId::Results);
    let before = app.selected;
    // +1 for the border, then the table's own header row -- see
    let mut config = test_config();
    app.click_at(results_area.y + 1, results_area.x, &mut config); // table header row, not a data row
    assert_eq!(app.selected, before);
}

#[test]
fn test_click_at_results_data_row_selects_that_item() {
    let mut app = make_app();
    app.results = make_results(5);
    app.filtered_indices = (0..5).collect();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));

    let results_area = app.zones.get_area(ZoneId::Results);
    // y+1 = table header, y+2 = first data row (index 0).
    let mut config = test_config();
    app.click_at(results_area.y + 2, results_area.x, &mut config);
    assert_eq!(app.selected, 0);

    app.click_at(results_area.y + 3, results_area.x, &mut config);
    assert_eq!(app.selected, 1);
}

#[test]
fn test_click_at_respects_filtered_indices_not_raw_results_order() {
    let mut app = make_app();
    app.results = make_results(5);
    // Simulate a filter that only kept results 3 and 4.
    app.filtered_indices = vec![3, 4];
    app.zones.update_areas(Rect::new(0, 0, 80, 24));

    let results_area = app.zones.get_area(ZoneId::Results);
    let mut config = test_config();
    app.click_at(results_area.y + 2, results_area.x, &mut config); // first visible (filtered) row
    assert_eq!(app.selected, 3);
}

// --- the Trackers panel ----------------------------------------------

/// The panel's rows are the registry plus the `all` switch, in the order
/// they are drawn -- so a new source lands in the list on its own, and
/// the cursor and the hit-test walk the same one.
#[test]
fn test_the_panel_lists_all_then_the_registry_in_order() {
    let rows = doris::ui::view::source_rows();
    assert_eq!(rows.first(), Some(&doris::ui::view::SourceRow::All));
    let ids: Vec<&str> = rows[1..].iter().map(|r| r.id()).collect();
    let registry: Vec<&str> = doris::sources::source::KNOWN_SOURCES
        .iter()
        .map(|info| info.id)
        .collect();
    assert_eq!(ids, registry, "one row per registered source, in order");
}

/// `all` is the master switch: checked when every implemented source is,
/// which is the default view -- so the panel opens saying "everything".
#[test]
fn test_all_is_checked_when_every_implemented_source_is() {
    let config = test_config();
    assert!(
        doris::ui::view::SourceRow::All.is_checked(&config),
        "a fresh config enables every implemented source"
    );

    let partial = Config {
        enabled_sources: vec!["rutracker".to_string()],
        ..Default::default()
    };
    assert!(!doris::ui::view::SourceRow::All.is_checked(&partial));
}

/// A source that is not implemented offers nothing: its row says so and Enter leaves the config
/// alone.
#[test]
fn test_an_unimplemented_source_cannot_be_switched_on() {
    let row = doris::ui::view::SourceRow::One("never-heard-of-it");
    assert!(!row.is_implemented(), "an unknown id is not a source");

    let mut app = make_app();
    let mut config = test_config();
    let before = config.enabled_sources.clone();

    // A cursor past the end of the panel is a no-op too: the row list
    app.sources_cursor = doris::ui::view::source_rows().len();
    app.toggle_source(&mut config);

    assert_eq!(config.enabled_sources, before);
    assert!(!app.source_changed, "nothing changed, so nothing is owed");
}

/// Enter on a source row switches exactly that source, and tells Enter
/// that the enabled set owes a search -- the same flag a category
/// switch sets, for the same reason.
#[test]
fn test_toggling_a_source_flips_only_it_and_owes_a_search() {
    let mut app = make_app();
    let mut config = test_config();
    let before = config.enabled_sources.clone();

    app.sources_cursor = 1; // the first registry entry
    app.toggle_source(&mut config);

    let id = doris::sources::source::KNOWN_SOURCES[0].id;
    assert!(
        !config.enabled_sources.iter().any(|s| s == id),
        "{} was on and is now off",
        id
    );
    assert_eq!(config.enabled_sources.len(), before.len() - 1);
    assert!(app.source_changed, "the next Enter must re-search");

    app.toggle_source(&mut config);

    // Compared as sets, not lists: switching a source off and on again
    let mut back = config.enabled_sources.clone();
    back.sort();
    let mut expected = before.clone();
    expected.sort();
    assert_eq!(back, expected, "and back on again");
}

/// The `all` row is the other way round: off means "check the whole
/// roster", on means "clear it" -- one keypress for either extreme.
#[test]
fn test_the_all_row_checks_or_clears_the_whole_roster() {
    let mut app = make_app();
    let mut config = test_config();
    let roster: Vec<String> = doris::sources::source::KNOWN_SOURCES
        .iter()
        .filter(|info| info.implemented)
        .map(|info| info.id.to_string())
        .collect();

    app.sources_cursor = 0;
    app.toggle_source(&mut config);
    assert!(config.enabled_sources.is_empty(), "all on -> clear");

    app.toggle_source(&mut config);
    assert_eq!(
        config.enabled_sources, roster,
        "all off -> check everything"
    );
}

/// Switching a source off can take a category with it, so the row the
/// view was showing has to fall back to one that still exists -- the
/// same repair `set_group_tabs` has always done.
#[test]
fn test_losing_the_last_source_of_a_category_falls_back_to_all() {
    let mut app = make_app();
    app.active_group = Some(Group::Games);

    let config = Config {
        enabled_sources: vec!["yts".to_string()],
        ..Default::default()
    };
    app.set_group_tabs(&config);

    assert_eq!(app.active_group, None, "falls back to all");
    assert!(app.group_tabs.contains(&Some(Group::Movies)));
    assert!(!app.group_tabs.contains(&Some(Group::Games)));
}

/// Re-deriving must not disturb a category that still has sources behind
/// it -- opening and closing the panel is not a switch.
#[test]
fn test_keeping_the_category_leaves_the_selection_alone() {
    let mut app = make_app();
    app.active_group = Some(Group::Movies);

    app.set_group_tabs(&test_config());

    assert_eq!(app.active_group, Some(Group::Movies));
    assert!(!app.group_changed, "re-deriving is not a switch");
}

/// The cursor wraps both ways, like every other list in the app -- a
/// panel that dead-ends at the last row is a trap.
#[test]
fn test_the_cursor_wraps_in_both_directions() {
    let mut app = make_app();
    let len = doris::ui::view::source_rows().len() as i64;

    app.navigate_trackers(-1);
    assert_eq!(
        app.sources_cursor,
        len as usize - 1,
        "up from the top wraps to the bottom"
    );

    app.navigate_trackers(1);
    assert_eq!(app.sources_cursor, 0, "and back to the top");
}

/// The drawn rows and the hit-test read the same list, so a row that is
/// drawn can be clicked: the click lands on the row under the cursor's
/// column and switches it.
#[test]
fn test_clicking_a_panel_row_switches_it() {
    let mut app = make_app();
    let mut config = test_config();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    let area = app.zones.get_area(ZoneId::Trackers);
    assert!(area.height > 3, "the panel is on screen");

    // The `all` row is the first line inside the border. The panel's
    let action = app.click_at(area.y + 1, area.x + 1, &mut config);
    assert_eq!(action, Some(doris::ui::view::UiAction::TrackersChanged));
    assert!(
        config.enabled_sources.is_empty(),
        "the default view has everything on, so the click cleared it"
    );
    assert_eq!(app.zones.focused, ZoneId::Trackers);
}

/// The same gate the keyboard has: while a query is being typed the
/// pointer can still be over the panel, and that click belongs to the
/// query -- it may move the cursor, it may not flip a checkbox behind
/// the user's back.
#[test]
fn test_clicking_a_panel_row_while_typing_does_not_switch_it() {
    let mut app = make_app();
    let mut config = test_config();
    app.enter_input_mode();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    let area = app.zones.get_area(ZoneId::Trackers);

    let action = app.click_at(area.y + 1, area.x + 1, &mut config);

    assert_eq!(action, None, "nothing changed, so nothing to persist");
    assert!(
        !config.enabled_sources.is_empty(),
        "the click must not have touched the selection"
    );
}

/// What is left on the Results frame after the source tabs moved to their own panel: the row
/// counter, zero-padded like the log's scroll position, and the filter while one is set.
#[test]
fn test_the_results_info_slot_counts_without_naming_sources() {
    let mut app = make_app();
    let mut config = test_config();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    let area = app.zones.get_area(ZoneId::Results);

    assert_eq!(
        app.frame_info(ZoneId::Results, area, &config),
        "(0/0)",
        "an empty table counts nothing, in as few digits as it takes"
    );

    config.enabled_sources = vec!["rutracker".to_string(), "yts".to_string()];
    assert_eq!(
        app.frame_info(ZoneId::Results, area, &config),
        "(0/0)",
        "the checked sources never come back onto this frame"
    );

    config.enabled_sources.clear();
    assert_eq!(
        app.frame_info(ZoneId::Results, area, &config),
        "(0/0)",
        "not even as `[none]`"
    );
}

/// The counter sits directly after the zone's name -- `¹ Results
/// (3/3)` -- and the filter trails it rather than pushing it off.
#[test]
fn test_the_results_counter_leads_the_info_slot() {
    let mut app = make_app();
    app.results = make_results(3);
    app.update_filter();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    let area = app.zones.get_area(ZoneId::Results);
    let config = test_config();

    assert_eq!(
        app.frame_info(ZoneId::Results, area, &config),
        "(3/3)",
        "filtered 3 of 3"
    );

    app.zones.filter_input = "rutor".to_string();
    assert_eq!(
        app.frame_info(ZoneId::Results, area, &config),
        "(3/3) [F: rutor]",
        "the count stays first, the filter trails it"
    );
}

#[test]
fn test_click_at_returns_none_on_the_zone_title() {
    let mut config = test_config();
    let mut app = make_app();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    let torrent_area = app.zones.get_area(ZoneId::Torrent);
    // The top-left corner carries the zone title, not a frame button.
    let action = app.click_at(torrent_area.y, torrent_area.x, &mut config);
    assert_eq!(action, None);
}

/// The frame legend is drawn from `frame_layout`, and `click_at` tests
/// those very rects -- so a click on the drawn `pause` word has to come
/// back as the pause action, not fall through to "clicked the panel".
#[test]
fn test_click_at_torrent_pause_button_returns_toggle_pause() {
    let mut config = test_config();
    let mut app = make_app();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    let torrent_area = app.zones.get_area(ZoneId::Torrent);
    let layout = app.frame_layout(ZoneId::Torrent, torrent_area, &config);
    let rect = layout
        .buttons
        .iter()
        .find(|(b, _)| b.key == 'p')
        .map(|(_, r)| *r)
        .expect("the Torrent frame has a pause button");

    assert_eq!(
        app.click_at(rect.y, rect.x, &mut config),
        Some(UiAction::TogglePause)
    );
    // And the last column of the word too, not just its first.
    assert_eq!(
        app.click_at(rect.y, rect.x + rect.width - 1, &mut config),
        Some(UiAction::TogglePause)
    );
}

#[test]
fn test_click_at_torrent_delete_button_returns_remove() {
    let mut config = test_config();
    let mut app = make_app();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    let torrent_area = app.zones.get_area(ZoneId::Torrent);
    let layout = app.frame_layout(ZoneId::Torrent, torrent_area, &config);
    let rect = layout
        .buttons
        .iter()
        .find(|(b, _)| b.key == 'd')
        .map(|(_, r)| *r)
        .expect("the Torrent frame has a delete button");

    assert_eq!(
        app.click_at(rect.y, rect.x, &mut config),
        Some(UiAction::Remove)
    );
}

/// Between the two buttons there is a gap: landing in it must do
/// nothing, or the legend would be a single imprecise hot zone.
#[test]
fn test_click_between_two_frame_buttons_does_nothing() {
    let mut app = make_app();
    let mut config = test_config();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    let results_area = app.zones.get_area(ZoneId::Results);
    let layout = app.frame_layout(ZoneId::Results, results_area, &config);
    // `filter` and `group` share the right cluster, drawn in table
    let filter = layout
        .buttons
        .iter()
        .find(|(b, _)| b.key == 'f')
        .map(|(_, r)| *r)
        .expect("filter button");
    let group = layout
        .buttons
        .iter()
        .find(|(b, _)| b.key == 'g')
        .map(|(_, r)| *r)
        .expect("group button");
    assert!(group.x > filter.x + filter.width);

    let gap_col = filter.x + filter.width;
    assert!(app.click_at(filter.y, gap_col, &mut config).is_none());
}

/// The buttons `ui::App` can act on itself happen right here rather than
/// being handed to the orchestrator.
#[test]
fn test_clicking_the_filter_button_enters_filter_mode() {
    let mut config = test_config();
    let mut app = make_app();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    assert!(!app.zones.filter_mode);

    let results_area = app.zones.get_area(ZoneId::Results);
    let layout = app.frame_layout(ZoneId::Results, results_area, &config);
    let rect = layout
        .buttons
        .iter()
        .find(|(b, _)| b.key == 'f')
        .map(|(_, r)| *r)
        .expect("the Results frame has a filter button");

    assert_eq!(app.click_at(rect.y, rect.x, &mut config), None);
    assert!(app.zones.filter_mode, "the click entered filter mode");
}

// --- the panel is derived from the registry, not written down ----------------

/// Decided with the user after live run: a source the user
/// switched off must not be asked -- the panel is the only place that
/// decides, so its rows come from the registry and nothing else.
#[test]
fn test_the_panel_offers_exactly_the_registry_in_order() {
    let rows = doris::ui::view::source_rows();
    assert_eq!(rows.len(), doris::sources::source::KNOWN_SOURCES.len() + 1);
    assert_eq!(rows[0].id(), "all");
    for (row, info) in rows[1..]
        .iter()
        .zip(doris::sources::source::KNOWN_SOURCES.iter())
    {
        assert_eq!(row.id(), info.id, "registry order, one row each");
    }
}

/// A source that is not implemented offers nothing: it is listed (so
/// the next one is visible where it will land) but reads as planned, and
/// the dispatch never asks it -- the same invariant the old tab bar had.
#[test]
fn test_unimplemented_sources_are_listed_but_never_asked() {
    let rows = doris::ui::view::source_rows();
    for info in doris::sources::source::KNOWN_SOURCES
        .iter()
        .filter(|s| !s.implemented)
    {
        let row = rows
            .iter()
            .find(|r| r.id() == info.id)
            .expect("a planned source has a row");
        assert!(!row.is_implemented(), "{} is planned", info.id);
    }

    let all: Vec<String> = doris::sources::source::KNOWN_SOURCES
        .iter()
        .filter(|s| s.implemented)
        .map(|s| s.id.to_string())
        .collect();
    let asked: Vec<&str> = doris::sources::orchestrator::selected_sources(&all, None, false)
        .iter()
        .map(|info| info.id)
        .collect();
    for info in doris::sources::source::KNOWN_SOURCES
        .iter()
        .filter(|s| !s.implemented)
    {
        assert!(
            !asked.contains(&info.id),
            "{} is planned and would be asked",
            info.id
        );
    }
}

/// Switching every source off leaves the panel saying so rather than
/// silently asking nothing -- the search's "selected nobody" line is
/// what explains it.
#[test]
fn test_switching_every_source_off_leaves_the_panel_empty() {
    let config = Config {
        enabled_sources: Vec::new(),
        ..Default::default()
    };

    assert!(!doris::ui::view::SourceRow::All.is_checked(&config));
    assert_eq!(doris::ui::view::sources_summary(&config), "none");
    assert!(
        doris::sources::orchestrator::selected_sources(&config.enabled_sources, None, false)
            .is_empty(),
        "nothing checked means nothing to dispatch"
    );
}

// --- category tab row ( second Results row) ---------------------------

/// Availability rather than a fixed four: with only single-group
/// sources enabled the row shrinks to what they can answer, which is
/// the same rule `source_tabs` applies to disabled sources.
#[test]
fn test_category_row_offers_only_groups_an_enabled_source_serves() {
    let yts_only = Config {
        enabled_sources: vec!["yts".to_string()],
        ..Default::default()
    };
    assert_eq!(
        doris::ui::modals::settings::group_tabs(&yts_only),
        vec![None, Some(Group::Movies)],
        "yts declares Movies and nothing else"
    );

    let subsplease_only = Config {
        enabled_sources: vec!["subsplease".to_string()],
        ..Default::default()
    };
    assert_eq!(
        doris::ui::modals::settings::group_tabs(&subsplease_only),
        vec![None, Some(Group::Anime)],
        "a single-group source still gets the unfiltered tab"
    );

    let all_off = Config {
        enabled_sources: Vec::new(),
        ..Default::default()
    };
    assert_eq!(
        doris::ui::modals::settings::group_tabs(&all_off),
        vec![None],
        "with everything off the row still has somewhere to be"
    );

    // The default config enables every implemented source, and between
    assert_eq!(
        doris::ui::modals::settings::group_tabs(&test_config()),
        vec![
            None,
            Some(Group::Movies),
            Some(Group::TV),
            Some(Group::Games),
            Some(Group::Anime),
        ]
    );
}

#[test]
fn test_cycle_group_walks_the_row_and_wraps() {
    let mut app = make_app();
    // Derived from the function the row is drawn from rather than
    let tabs = doris::ui::modals::settings::group_tabs(&test_config());
    assert_eq!(app.active_group, tabs[0], "starts on all");
    assert_eq!(app.active_group, None);

    for expected in tabs.iter().skip(1) {
        app.cycle_group(true);
        assert_eq!(&app.active_group, expected, "walks the whole row");
        assert!(app.group_changed, "every step is owed a re-search");
    }
    app.cycle_group(true);
    assert_eq!(app.active_group, tabs[0], "the cycle must wrap");

    // Backwards from "all" lands on the last tab, never one before it.
    app.cycle_group(false);
    assert_eq!(app.active_group, tabs[tabs.len() - 1]);
}

/// The category moved from a row inside the panel onto the frame, as a `◀ name ▶` sort
/// header: the current category is named on the border next to `group`, and the two arrows are
/// mouse targets for previous / next.
#[test]
fn test_the_category_button_names_the_category_and_its_arrows_switch() {
    let mut app = make_app();
    let mut config = test_config();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    let area = app.zones.get_area(ZoneId::Results);

    let (button, rect) = {
        let layout = app.frame_layout(ZoneId::Results, area, &config);
        layout
            .buttons
            .iter()
            .find(|(b, _)| b.is_category())
            .map(|(b, r)| (b.clone(), *r))
            .expect("the Results frame has a category button")
    };
    // The name is padded to the widest category, so the arrows stay in
    let text = button.text();
    assert!(text.starts_with("◀ "), "the left arrow: {text}");
    assert!(text.ends_with(" ▶"), "the right arrow: {text}");
    assert!(
        text.contains("all"),
        "a fresh app is on the all category: {text}"
    );

    // The right arrow steps forward through the category row and asks
    let right_arrow = rect.x + rect.width - 1;
    assert_eq!(
        app.click_at(rect.y, right_arrow, &mut config),
        Some(UiAction::ReaskCategory)
    );
    assert_eq!(
        app.active_group,
        Some(Group::Movies),
        "the right arrow steps forward"
    );
    assert!(app.group_changed);

    //...and the left arrow steps back.
    let rect = {
        let layout = app.frame_layout(ZoneId::Results, area, &config);
        layout
            .buttons
            .iter()
            .find(|(b, _)| b.is_category())
            .map(|(_, r)| *r)
            .expect("the category button")
    };
    assert_eq!(
        app.click_at(rect.y, rect.x, &mut config),
        Some(UiAction::ReaskCategory)
    );
    assert_eq!(app.active_group, None, "the left arrow steps back to all");

    // The name between the arrows is display-only, not a target.
    let rect = {
        let layout = app.frame_layout(ZoneId::Results, area, &config);
        layout
            .buttons
            .iter()
            .find(|(b, _)| b.is_category())
            .map(|(_, r)| *r)
            .expect("the category button")
    };
    assert_eq!(app.click_at(rect.y, rect.x + 2, &mut config), None);
    assert_eq!(app.active_group, None, "clicking the name does nothing");
}

/// The `group` frame button is still there and still cycles the category
/// forward -- the keyboard `g` and this click are the same action.
#[test]
fn test_the_group_button_still_cycles_the_category() {
    let mut app = make_app();
    let mut config = test_config();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    let area = app.zones.get_area(ZoneId::Results);

    let rect = {
        let layout = app.frame_layout(ZoneId::Results, area, &config);
        layout
            .buttons
            .iter()
            .find(|(b, _)| b.key == 'g' && !b.is_category())
            .map(|(_, r)| *r)
            .expect("the group button")
    };
    assert_eq!(
        app.click_at(rect.y, rect.x, &mut config),
        Some(UiAction::ReaskCategory)
    );
    assert_eq!(app.active_group, Some(Group::Movies));
    assert!(app.group_changed);
}

/// Switching off the source that was the last one serving a category
/// must not leave the row pointing at a tab that is no longer drawn.
#[test]
fn test_losing_the_category_moves_the_selection_to_all() {
    let mut app = make_app();
    app.active_group = Some(Group::Games);

    let config = Config {
        enabled_sources: vec!["yts".to_string()],
        ..Default::default()
    };
    app.set_group_tabs(&config);

    assert_eq!(app.active_group, None, "falls back to all");
    assert!(app.group_tabs.contains(&Some(Group::Movies)));
    assert!(!app.group_tabs.contains(&Some(Group::Games)));
}

// --- the frame legend's geometry ------------------------------------------

/// `frame_layout` is what both the renderer and `click_at` go through, so a button that escaped
/// its own zone would be drawn on top of somebody else's border and click through to it.
#[test]
fn test_every_frame_button_stays_on_its_own_border() {
    let config = test_config();
    for (w, h) in [(80u16, 24u16), (40, 12), (20, 8), (10, 4), (6, 3)] {
        let mut app = make_app();
        app.zones.update_areas(Rect::new(0, 0, w, h));
        app.zones.set_fullscreen(Some(ZoneId::Results));
        let area = app.zones.get_area(ZoneId::Results);
        let layout = app.frame_layout(ZoneId::Results, area, &config);

        for (button, rect) in &layout.buttons {
            assert!(
                rect.x > area.x,
                "{:?} at {} touches the left border",
                button,
                w
            );
            assert!(
                rect.x + rect.width < area.x + area.width,
                "{:?} at {} overruns the right border",
                button,
                w
            );
            assert!(
                rect.y == area.y || rect.y + 1 == area.y + area.height,
                "{:?} at {}x{} is not on a border row",
                button,
                w,
                h
            );
        }
        if layout.info.width > 0 {
            assert_eq!(layout.info.y, area.y, "info stays on the top border");
            assert!(layout.info.x + layout.info.width < area.x + area.width);
        }
    }
}

/// Two buttons must never share a column, or a click would be a guess
/// about which of them the user meant.
#[test]
fn test_frame_buttons_do_not_overlap() {
    let config = test_config();
    let mut app = make_app();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));
    for &id in ZoneId::all() {
        let area = app.zones.get_area(id);
        let layout = app.frame_layout(id, area, &config);
        for (i, (a, ra)) in layout.buttons.iter().enumerate() {
            for (b, rb) in layout.buttons.iter().skip(i + 1) {
                assert!(
                    ra.y != rb.y || ra.x + ra.width <= rb.x || rb.x + rb.width <= ra.x,
                    "{:?} and {:?} overlap on {:?}",
                    a,
                    b,
                    id
                );
            }
        }
        if layout.info.width > 0 {
            for (_, rect) in &layout.buttons {
                if rect.y != layout.info.y {
                    continue;
                }
                assert!(
                    rect.x + rect.width <= layout.info.x
                        || layout.info.x + layout.info.width <= rect.x,
                    "info text overlaps a button on {:?}",
                    id
                );
            }
        }
    }
}

// --- the filter matches every field, not just the title -------------------

/// The filter prompt answers to a size, a source, a word from the title
/// or the category -- the user should not have to know which column a
/// term lives in.
#[test]
fn test_the_filter_matches_size_source_word_and_category() {
    let mut app = make_app();
    app.results = vec![
        TorrentItem {
            title: "Dune".into(),
            size: "1.4 GB".into(),
            source: "rutracker".into(),
            group: Some(Group::Movies),
            ..Default::default()
        },
        TorrentItem {
            title: "Some Anime".into(),
            size: "350 MB".into(),
            source: "subsplease".into(),
            group: Some(Group::Anime),
            ..Default::default()
        },
    ];

    for (filter, expected) in [
        ("dune", 1),        // a word from the title
        ("1.4", 1),         // the size
        ("rutracker", 1),   // the source
        ("anime", 1),       // the category
        ("350", 1),         // the other row's size
        ("dune\nanime", 0), // AND is not the rule: no row has both
    ] {
        app.zones.filter_input = filter.to_string();
        app.update_filter();
        assert_eq!(
            app.filtered_indices.len(),
            expected,
            "filter '{filter}' should match {expected} row(s)"
        );
    }
}

// --- the category button keeps its arrows in the same columns --------------

/// The name is padded to the widest category, so `◀ TV ▶` and
/// `◀ Movies ▶` are the same width and the arrows never move.
#[test]
fn test_the_category_button_keeps_its_arrows_in_the_same_columns() {
    let mut app = make_app();
    let config = test_config();
    app.zones.update_areas(Rect::new(0, 0, 100, 30));
    let area = app.zones.get_area(ZoneId::Results);

    let mut arrow_positions = Vec::new();
    for group in [
        None,
        Some(Group::TV),
        Some(Group::Movies),
        Some(Group::Games),
    ] {
        app.active_group = group;
        let layout = app.frame_layout(ZoneId::Results, area, &config);
        let (_, rect) = layout
            .buttons
            .iter()
            .find(|(b, _)| b.is_category())
            .expect("the category button");
        arrow_positions.push((rect.x, rect.x + rect.width - 1));
    }

    let first = arrow_positions[0];
    for (i, pos) in arrow_positions.iter().enumerate() {
        assert_eq!(
            *pos, first,
            "the arrows must not move between categories (index {i})"
        );
    }
}

// --- `D` opens the detail modal (Shift+Enter's fallback) ------------------

/// Most terminals send Shift+Enter as a plain Enter with no modifier, so `D` is the same action
/// on a key every terminal sends distinctly.
#[test]
fn test_the_help_page_documents_d_as_the_detail_fallback() {
    // Every page, not one: the split moved this row onto `2:search`, and a
    // test still reading the first table would have passed only because the
    // first table happened to name some other D.
    let keys: Vec<&str> = sections()
        .iter()
        .flat_map(|(_, table)| table.iter().map(|(k, _)| *k))
        .collect();
    assert!(
        keys.iter().any(|k| k.contains('D')),
        "a help page should name 'D' as the detail fallback: {:?}",
        keys
    );
}

/// A click on a border that separates two zones arms the WM-style
/// resize; a click on the frame legend -- which also sits on a border --
/// stays the legend's own action and arms nothing.
#[test]
fn test_a_border_click_arms_the_resize_but_a_legend_click_does_not() {
    let mut app = make_app();
    let mut config = test_config();
    app.zones.update_areas(Rect::new(0, 0, 80, 24));

    // Trackers' top border separates it from Torrent, and Trackers has
    let border = app.zones.get_area(ZoneId::Trackers).y;
    assert_eq!(app.zone_at(border, 40), Some(ZoneId::Trackers));
    assert_eq!(app.click_at(border, 40, &mut config), None);
    assert!(app.zones.resize.is_some(), "the drag is armed");
    app.zones.resize_end();

    // Torrent's `pause` sits on the same kind of border: it pauses.
    let area = app.zones.get_area(ZoneId::Torrent);
    let rect = {
        let layout = app.frame_layout(ZoneId::Torrent, area, &config);
        layout
            .buttons
            .iter()
            .find(|(b, _)| b.key == 'p')
            .map(|(_, r)| *r)
            .expect("the pause button")
    };
    assert_eq!(
        app.click_at(rect.y, rect.x + rect.width - 1, &mut config),
        Some(UiAction::TogglePause)
    );
    assert!(app.zones.resize.is_none(), "and armed nothing");
}

/// What the hit-test answers is where the word is drawn.
#[test]
fn every_menu_item_rect_covers_a_glyph_the_renderer_draws() {
    use doris::ui::menu::menu_item_rects;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let mut app = UiApp::new("http://127.0.0.1:8090".into(), None);
    app.show_menu = true;
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
    let buf = terminal.backend().buffer();

    for rect in menu_item_rects(ratatui::layout::Rect::new(0, 0, 100, 30)) {
        let glyphs = (rect.y..rect.y + rect.height)
            .flat_map(|y| (rect.x..rect.x + rect.width).map(move |x| (x, y)))
            .filter(|&(x, y)| buf[(x, y)].symbol() != " ")
            .count();
        assert!(
            glyphs > 0,
            "{rect:?} is a click target with nothing drawn in it"
        );
    }
}
