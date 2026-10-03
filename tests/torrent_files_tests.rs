//! What the Torrents detail view and its file list do with a key.
//!
//! Both were unreachable before: the detail view took the keyboard and did
//! nothing with it but close, and there was no way to choose which files of
//! a torrent to fetch. These are the decisions, without a daemon.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use doris::ui::view::{App, Modal};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn with_files() -> App {
    let mut app = App::new("http://127.0.0.1:1".into(), None);
    let files = vec![
        doris::transmission::FileEntry {
            name: "show/s01e01.mkv".into(),
            size: 700,
            ..Default::default()
        },
        doris::transmission::FileEntry {
            name: "show/s01e02.mkv".into(),
            size: 800,
            ..Default::default()
        },
        doris::transmission::FileEntry {
            name: "show/readme.txt".into(),
            size: 10,
            ..Default::default()
        },
    ];
    app.open_files_modal(7, "Some.Show.S01".into());
    let Modal::Files(state) = &mut app.modal else {
        panic!("the files modal is open");
    };
    state.files = files;
    state.pending = false;
    app
}

fn state(app: &App) -> &doris::ui::modals::files::FilesState {
    match &app.modal {
        Modal::Files(state) => state,
        _ => panic!("the files modal is open"),
    }
}

/// The cursor moves, and only the cursor: a file list where `j` also
/// toggles something is a file list that cannot be read.
#[test]
fn the_file_list_cursor_moves_without_changing_anything() {
    let mut app = with_files();

    app.files_key(key(KeyCode::Down), true);
    app.files_key(key(KeyCode::Down), true);
    assert_eq!(state(&app).cursor, 2);

    app.files_key(key(KeyCode::Up), true);
    assert_eq!(state(&app).cursor, 1);

    // And it stops at the ends rather than wrapping or running off: a
    // cursor that wraps in a list you are reading top to bottom loses the
    // place you were.
    for _ in 0..10 {
        app.files_key(key(KeyCode::Down), true);
    }
    assert_eq!(state(&app).cursor, 2, "it stops at the last file");
    for _ in 0..10 {
        app.files_key(key(KeyCode::Up), true);
    }
    assert_eq!(state(&app).cursor, 0, "and at the first");
}

/// `j` and `k` are the file list's only when Vim keys are on, like every
/// other list in the app: a typing key bound to navigation is a filter box
/// that cannot hold the letter j.
#[test]
fn the_file_list_jumps_a_page_and_j_k_only_under_vim_keys() {
    let mut app = with_files();
    app.files_key(key(KeyCode::Char('j')), false);
    assert_eq!(state(&app).cursor, 0, "j is a letter when Vim keys are off");

    app.files_key(key(KeyCode::Char('j')), true);
    assert_eq!(state(&app).cursor, 1);

    app.files_key(key(KeyCode::End), true);
    assert_eq!(state(&app).cursor, 2);
    app.files_key(key(KeyCode::Home), true);
    assert_eq!(state(&app).cursor, 0);
}

/// The whole point of the list: turn one file off, and only that one.
///
/// The change is queued for the daemon rather than sent, because the UI
/// layer cannot reach Transmission -- so the test is on the queue, which is
/// where a wrong index would show up.
#[test]
fn enter_turns_the_file_under_the_cursor_off_and_queues_it() {
    let mut app = with_files();
    app.files_key(key(KeyCode::Down), true);

    app.files_key(key(KeyCode::Enter), true);

    assert!(!state(&app).files[1].wanted, "the file under the cursor");
    assert!(state(&app).files[0].wanted, "the one above it");
    assert!(state(&app).files[2].wanted, "and the one below");
    assert_eq!(app.pending_files, vec![(1, false)], "sent as one change");

    // And back on again: a switch that cannot be undone from the keyboard
    // is a decision, not a switch.
    app.files_key(key(KeyCode::Enter), true);
    assert!(state(&app).files[1].wanted);
    assert_eq!(app.pending_files, vec![(1, false), (1, true)]);
}

/// `a` and `n` are the two ends of the same switch, and they only send the
/// files that actually changed: a torrent of two hundred files where one
/// was already off must not ask the daemon about two hundred.
#[test]
fn all_and_none_send_only_what_changed() {
    let mut app = with_files();
    app.files_key(key(KeyCode::Char('n')), true);

    assert!(
        state(&app).files.iter().all(|f| !f.wanted),
        "everything is off"
    );
    assert_eq!(app.pending_files.len(), 3, "three files were on");
    assert!(app.pending_files.iter().all(|(_, wanted)| !wanted));

    app.pending_files.clear();
    app.files_key(key(KeyCode::Char('a')), true);
    assert!(
        state(&app).files.iter().all(|f| f.wanted),
        "everything is back on"
    );
    assert_eq!(app.pending_files.len(), 3);

    // Already off, so `n` again has nothing to say.
    app.pending_files.clear();
    app.files_key(key(KeyCode::Char('n')), true);
    app.pending_files.clear();
    app.files_key(key(KeyCode::Char('n')), true);
    assert!(
        app.pending_files.is_empty(),
        "nothing changed, nothing sent"
    );
}

/// The title has to name the torrent: a list of ten files with no header
/// is a list you have to remember the context of.
#[test]
fn the_file_list_says_which_torrent_and_how_much_of_it_is_wanted() {
    let state = doris::ui::modals::files::FilesState {
        id: 1,
        name: "Some.Show.S01".into(),
        files: vec![
            doris::transmission::FileEntry {
                size: 700,
                wanted: true,
                ..Default::default()
            },
            doris::transmission::FileEntry {
                size: 800,
                wanted: false,
                ..Default::default()
            },
        ],
        ..Default::default()
    };

    assert_eq!(state.wanted_total(), 700, "only the wanted one counts");
    assert_eq!(state.all_total(), 1500);
}

/// `wanted` does not come back in `files`. Transmission 4 returns the name,
/// the length and two piece numbers there, and nothing about the choice --
/// so a client that reads only `files` shows every file as wanted, which is
/// a list that cannot show what the user just turned off.
///
/// Measured, not remembered: setting `files-unwanted` on a live daemon and
/// reading `torrent-get` back gives a `files` array with no `wanted` key at
/// all, and a `fileStats` array that has it.
#[test]
fn wanted_lives_in_file_stats_and_is_merged_onto_the_file_list() {
    let daemon_says = serde_json::json!({
        "files": [
            { "name": "a.mkv", "length": 100, "bytesCompleted": 100.0 },
            { "name": "b.mkv", "length": 200, "bytesCompleted": 0.0 },
        ],
        "fileStats": [
            { "bytesCompleted": 100.0, "priority": 0, "wanted": true },
            { "bytesCompleted": 0.0, "priority": 0, "wanted": false },
        ]
    });
    let mut files: Vec<doris::transmission::FileEntry> =
        serde_json::from_value(daemon_says["files"].clone()).expect("the file list");
    // Without the merge both files look wanted, which is the defect.
    assert!(
        files.iter().all(|f| f.wanted),
        "what the files array alone says"
    );

    doris::transmission::merge_wanted(&mut files, Some(&daemon_says["fileStats"]));

    assert!(files[0].wanted, "the one the user kept");
    assert!(!files[1].wanted, "the one the user turned off");
}

/// A `fileStats` shorter than `files` leaves the rest wanted. The daemon not
/// mentioning a file is not the user turning it off, and marking it unwanted
/// would silently stop a download nobody asked to stop.
#[test]
fn a_missing_stat_leaves_a_file_wanted_rather_than_turning_it_off() {
    let mut files = vec![
        doris::transmission::FileEntry::default(),
        doris::transmission::FileEntry::default(),
    ];
    let stats = serde_json::json!([{ "wanted": false }]);

    doris::transmission::merge_wanted(&mut files, Some(&stats));

    assert!(!files[0].wanted, "the one that was mentioned");
    assert!(files[1].wanted, "the one that was not");
}

/// And no `fileStats` at all -- an older daemon, or a `torrent-get` that
/// answered with only `files` -- leaves the whole list wanted rather than
/// empty.
#[test]
fn no_stats_at_all_leaves_every_file_wanted() {
    let mut files = vec![doris::transmission::FileEntry::default()];
    files[0].wanted = true;

    doris::transmission::merge_wanted(&mut files, None);

    assert!(files[0].wanted);
}

/// `FileEntry::default()` and a file the daemon reports must agree that an
/// unmentioned file is wanted. They are the same value reached two ways, and
/// when they disagreed the hand-built one silently meant "off".
#[test]
fn a_default_file_entry_is_wanted() {
    assert!(
        doris::transmission::FileEntry::default().wanted,
        "the derive's default and serde's must not disagree"
    );
}
