use doris::config::Config;
use doris::ui::app::App as UiApp;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn make_app_in_settings() -> UiApp {
    let mut app = UiApp::new(
        "http://127.0.0.1:8090".into(),
        "chrome".into(),
        true,
        None,
        "/tmp".into(),
        "braille".into(),
        true,
        true,
        true,
        false,
    );
    app.open_settings(&Config::default());
    app
}

#[test]
fn test_settings_has_three_categories_general_streaming_download() {
    let app = make_app_in_settings();
    let names: Vec<&str> = match &app.modal {
        doris::ui::app::Modal::Settings(state) => state.categories.iter().map(|c| c.name.as_str()).collect(),
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
        doris::ui::app::Modal::Settings(state) => assert_eq!(state.selected_category, 2),
        _ => panic!("expected Settings modal"),
    }
}

#[test]
fn test_digit_1_and_2_still_work() {
    let mut app = make_app_in_settings();
    app.settings_key(key(KeyCode::Char('2')));
    match &app.modal {
        doris::ui::app::Modal::Settings(state) => assert_eq!(state.selected_category, 1),
        _ => panic!("expected Settings modal"),
    }
    app.settings_key(key(KeyCode::Char('1')));
    match &app.modal {
        doris::ui::app::Modal::Settings(state) => assert_eq!(state.selected_category, 0),
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
        doris::ui::app::Modal::Settings(state) => assert_eq!(state.selected_category, 0),
        _ => panic!("expected Settings modal"),
    }
}

#[test]
fn test_down_arrow_moves_selection() {
    let mut app = make_app_in_settings();
    let before = match &app.modal {
        doris::ui::app::Modal::Settings(state) => state.selected,
        _ => panic!("expected Settings modal"),
    };
    app.settings_key(key(KeyCode::Down));
    let after = match &app.modal {
        doris::ui::app::Modal::Settings(state) => state.selected,
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
        doris::ui::app::Modal::Settings(state) => state.selected_category,
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
