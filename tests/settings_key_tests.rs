use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use doris::config::Config;
use doris::ui::view::App as UiApp;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn make_app_in_settings() -> UiApp {
    let mut app = UiApp::new("http://127.0.0.1:8090".into(), None);
    app.open_settings(&Config::default(), false);
    app
}

#[test]
fn test_settings_has_three_categories_general_streaming_download() {
    let app = make_app_in_settings();
    let names: Vec<&str> = match &app.modal {
        doris::ui::view::Modal::Settings(state) => {
            state.categories.iter().map(|c| c.name.as_str()).collect()
        }
        _ => panic!("expected Settings modal"),
    };
    assert_eq!(names, vec!["general", "streaming", "download"]);
}

#[test]
fn test_digit_3_switches_to_the_third_category() {
    // Regression test: '3' (download) used to do nothing at all -- only
    // '1' and '2' were handled, hardcoded, from back when there were only
    // two categories.
    let mut app = make_app_in_settings();
    app.settings_key(key(KeyCode::Char('3')));
    match &app.modal {
        doris::ui::view::Modal::Settings(state) => assert_eq!(state.selected_category, 2),
        _ => panic!("expected Settings modal"),
    }
}

#[test]
fn test_digit_1_and_2_still_work() {
    let mut app = make_app_in_settings();
    app.settings_key(key(KeyCode::Char('2')));
    match &app.modal {
        doris::ui::view::Modal::Settings(state) => assert_eq!(state.selected_category, 1),
        _ => panic!("expected Settings modal"),
    }
    app.settings_key(key(KeyCode::Char('1')));
    match &app.modal {
        doris::ui::view::Modal::Settings(state) => assert_eq!(state.selected_category, 0),
        _ => panic!("expected Settings modal"),
    }
}

#[test]
fn test_digit_beyond_category_count_is_ignored() {
    let mut app = make_app_in_settings();
    app.settings_key(key(KeyCode::Char('9')));
    match &app.modal {
        // Still on the default category -- '9' doesn't exist, so it must
        // not panic or jump anywhere.
        doris::ui::view::Modal::Settings(state) => assert_eq!(state.selected_category, 0),
        _ => panic!("expected Settings modal"),
    }
}

#[test]
fn test_down_arrow_moves_selection() {
    let mut app = make_app_in_settings();
    let before = match &app.modal {
        doris::ui::view::Modal::Settings(state) => state.selected,
        _ => panic!("expected Settings modal"),
    };
    app.settings_key(key(KeyCode::Down));
    let after = match &app.modal {
        doris::ui::view::Modal::Settings(state) => state.selected,
        _ => panic!("expected Settings modal"),
    };
    assert_eq!(after, before + 1);
}

#[test]
fn test_left_sets_backward_direction_right_sets_forward() {
    // Regression test: Left and Right used to be indistinguishable --
    // both always meant "cycle forward" to whatever action handled them.
    let mut app = make_app_in_settings();
    app.settings_key(key(KeyCode::Right));
    assert_eq!(app.last_cycle_direction, 1);
    app.settings_key(key(KeyCode::Left));
    assert_eq!(app.last_cycle_direction, -1);
}

#[test]
fn test_enter_sets_forward_direction() {
    let mut app = make_app_in_settings();
    app.last_cycle_direction = -1;
    app.settings_key(key(KeyCode::Enter));
    assert_eq!(app.last_cycle_direction, 1);
}

#[test]
fn test_tab_and_backtab_cycle_all_three_categories() {
    let mut app = make_app_in_settings();
    let get_cat = |app: &UiApp| match &app.modal {
        doris::ui::view::Modal::Settings(state) => state.selected_category,
        _ => panic!("expected Settings modal"),
    };

    assert_eq!(get_cat(&app), 0);
    app.settings_key(key(KeyCode::Tab));
    assert_eq!(get_cat(&app), 1);
    app.settings_key(key(KeyCode::Tab));
    assert_eq!(get_cat(&app), 2);
    app.settings_key(key(KeyCode::Tab));
    assert_eq!(get_cat(&app), 0); // wraps

    app.settings_key(key(KeyCode::BackTab));
    assert_eq!(get_cat(&app), 2); // wraps the other way
}

/// The `n/m` in the Color theme row is supposed to be the *theme's*
/// position (`settings.rs:267` says so in as many words), but the
/// renderer printed `item_idx + 1 / cat.items.len()` -- the row's
/// position in the category. So every setup read "Color theme 1/15":
/// wrong on both ends, since the themes number dozens and the selected
/// one was almost never the first. The number belongs to the value, so
/// `open_settings` is where it is measured: that is the same place the
/// theme itself is read from disk.
#[test]
fn test_the_color_theme_row_counts_the_themes() {
    let app = make_app_in_settings();
    let themes = doris::ui::theme::Theme::load_themes();
    let current = app.theme.name.clone();
    let want = themes
        .iter()
        .position(|t| t.name == current)
        .map(|i| (i + 1, themes.len()))
        .unwrap_or_else(|| panic!("the running theme is one of the files: {current}"));

    match &app.modal {
        doris::ui::view::Modal::Settings(state) => {
            let cat = &state.categories[state.selected_category];
            assert_eq!(
                cat.items[state.selected].action,
                doris::ui::modals::settings::SettingsAction::CycleTheme,
                "the first row of general is the theme"
            );
            assert_eq!(
                state.theme_pos,
                Some(want),
                "the row counts themes: {:?}, got {:?}",
                want,
                state.theme_pos
            );
        }
        other => panic!("expected Settings modal, got {other:?}"),
    }

    let (_, total) = want;
    assert!(
        total > 15,
        "far more themes than settings rows: {total} is not the row count"
    );
}

/// And the rendered label is the one with the theme's number in it --
/// the number the user actually reads as "which theme am I on".
#[test]
fn test_the_drawn_theme_label_carries_the_theme_number() {
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    let mut app = make_app_in_settings();
    let label = match &app.modal {
        doris::ui::view::Modal::Settings(state) => {
            let (n, total) = state
                .theme_pos
                .expect("the theme row carries its own index");
            let cat = &state.categories[state.selected_category];
            format!("{} {}/{}", cat.items[state.selected].label, n, total)
        }
        other => panic!("expected Settings modal, got {other:?}"),
    };

    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| app.render(frame, &Config::default()))
        .unwrap();
    let buf = terminal.backend().buffer();
    let drawn: Vec<String> = (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().chars().next().unwrap_or(' '))
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .filter(|line| !line.is_empty())
        .collect();

    assert!(
        drawn.iter().any(|line| line.contains(&label)),
        "expected a drawn line containing {label:?}, got:\n{}",
        drawn.join("\n")
    );
}
