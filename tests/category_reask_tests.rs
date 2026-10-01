//! A category switch re-asks the sources instead of quietly filtering what an earlier,
//! differently-categorised search happened to leave behind. Two sources can only tag a row with
//! the category they were *asked* for (`rutracker`, `rutor`, `x1337x` do `item.group =
//! category`), so rows fetched under `all` carry no category at all: switching to Movies
//! afterwards used to hide them and show nothing but the sources that read the category off the
//! row.

use doris::results::apply_source_done;
use doris::sources::models::TorrentItem;
use doris::ui::view::App as UiApp;

fn row(title: &str, group: Option<doris::sources::source::Group>) -> TorrentItem {
    TorrentItem {
        title: title.to_string(),
        group,
        ..Default::default()
    }
}

fn make_app() -> UiApp {
    let mut app = UiApp::new("http://127.0.0.1:8090".into(), None);
    // Three rows as an "all" search would have left them: no category.
    app.results = vec![
        row("old one", None),
        row("old two", None),
        row("old three", None),
    ];
    app.search_query = Some("world war z".into());
    app.update_filter();
    app
}

/// Re-asking the same query keeps the rows the user is looking at until
/// the first answer of the new round lands.
#[test]
fn a_reask_of_the_same_query_keeps_the_rows_on_screen() {
    let mut app = make_app();

    app.begin_search("world war z");

    assert_eq!(
        app.results.len(),
        3,
        "the old rows stay while the new ones travel"
    );
    assert!(
        app.pending_clear,
        "and they are marked for the drop when the first answer arrives"
    );
    assert_eq!(app.search_query.as_deref(), Some("world war z"));
    assert!(!app.group_changed, "the re-ask paid what Enter was owed");
}

/// A *different* query is a different table: it drops at once, the way
/// it always did.
#[test]
fn a_new_query_drops_the_rows_at_once() {
    let mut app = make_app();
    app.search_query = Some("dune".into());

    app.begin_search("world war z");

    assert!(app.results.is_empty(), "a new query starts from nothing");
    assert!(!app.pending_clear);
}

/// The drop happens when the first source of the new round answers --
/// before its rows go in, or the two generations would mix.
#[test]
fn the_first_answer_takes_the_old_rows_with_it() {
    let mut app = make_app();
    app.begin_search("world war z");

    let answered = apply_source_done(
        &mut app,
        1,
        1,
        "rutracker",
        vec![row(
            "new arrival",
            Some(doris::sources::source::Group::Movies),
        )],
        None,
    );

    assert!(answered, "the answer belongs to the running generation");
    assert_eq!(app.results.len(), 1, "old rows are gone, new one is in");
    assert_eq!(app.results[0].title, "new arrival");
    assert!(!app.pending_clear, "and the mark is spent");
}

/// A late answer from the generation the re-ask superseded still gets
/// dropped, mark or no mark: it is not the answer to this question.
#[test]
fn a_stale_answer_does_not_take_the_old_rows_with_it() {
    let mut app = make_app();
    app.begin_search("world war z");

    let answered = apply_source_done(&mut app, 1, 2, "rutracker", vec![row("late", None)], None);

    assert!(!answered, "superseded");
    assert_eq!(app.results.len(), 3, "and the rows are untouched");
    assert!(app.pending_clear, "still waiting for the real answer");
}

/// When no answer is coming at all -- no source checked for that
/// category -- the old rows belong to a question nobody is answering,
/// and holding on to them is the same lie with a delay.
#[test]
fn a_reask_no_source_can_answer_drops_the_rows_too() {
    let mut app = make_app();
    app.begin_search("world war z");

    app.take_pending_clear();

    assert!(app.results.is_empty());
    assert!(!app.pending_clear);
    assert!(app.filtered_indices.is_empty());
}
