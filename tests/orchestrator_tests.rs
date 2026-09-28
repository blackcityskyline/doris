//! Offline tests for the B3 orchestrator: fake `Source`s stand in for a
//! fast source, a hanging one, a failing one and a panicking one, which
//! is what proves incremental delivery, the per-source deadline, and
//! "a failing source never suppresses a healthy one's results" without a
//! terminal, a browser or the network.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use doris::event::Event;
use doris::sources::models::TorrentItem;
use doris::sources::orchestrator::{self, SourceStatus};
use doris::sources::source::{
    AuthContext, Group, LogFn, SearchPage, SearchRequest, Source, KNOWN_SOURCES,
};
use tokio::sync::mpsc;

const FAKE_GROUPS: &[Group] = &[Group::Games];

/// A source that sleeps `delay` and then either answers with `rows` rows
/// or fails with `fail`.
struct FakeSource {
    id: &'static str,
    delay: Duration,
    rows: usize,
    has_more: bool,
    fail: Option<&'static str>,
}

fn fast_source(id: &'static str, rows: usize) -> Arc<FakeSource> {
    Arc::new(FakeSource {
        id,
        delay: Duration::from_millis(10),
        rows,
        has_more: false,
        fail: None,
    })
}

fn hanging_source(id: &'static str) -> Arc<FakeSource> {
    Arc::new(FakeSource {
        id,
        delay: Duration::from_secs(600),
        rows: 5,
        has_more: false,
        fail: None,
    })
}

fn failing_source(id: &'static str, message: &'static str) -> Arc<FakeSource> {
    Arc::new(FakeSource {
        id,
        delay: Duration::from_millis(50),
        rows: 9,
        has_more: false,
        fail: Some(message),
    })
}

fn row(source: &str, i: usize) -> TorrentItem {
    TorrentItem {
        title: format!("{}-{}", source, i),
        source: source.to_string(),
        ..Default::default()
    }
}

/// The future `run_source` runs: one source's page fetch.
fn fetch(source: Arc<dyn Source>) -> impl std::future::Future<Output = anyhow::Result<SearchPage>> {
    let req = SearchRequest::new("query", 0);
    async move { source.search(&req).await }
}

async fn next_event(rx: &mut mpsc::UnboundedReceiver<Event>) -> Event {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("no event within 5s -- the dispatch never finished")
        .expect("event channel closed")
}

#[async_trait]
impl Source for FakeSource {
    fn id(&self) -> &'static str {
        self.id
    }

    fn label(&self) -> &'static str {
        self.id
    }

    fn groups(&self) -> &'static [Group] {
        FAKE_GROUPS
    }

    fn home_url(&self) -> &'static str {
        "http://localhost"
    }

    fn requires_browser(&self) -> bool {
        false
    }

    fn supports_browse(&self) -> bool {
        false
    }

    async fn ensure_logged_in(&self, _auth: &AuthContext, _log: &LogFn) -> anyhow::Result<bool> {
        Ok(true)
    }

    async fn search(&self, _req: &SearchRequest) -> anyhow::Result<SearchPage> {
        tokio::time::sleep(self.delay).await;
        if let Some(message) = self.fail {
            anyhow::bail!(message);
        }
        Ok(SearchPage {
            items: (0..self.rows).map(|i| row(self.id, i)).collect(),
            has_more: self.has_more,
            next_offset: None,
        })
    }

    async fn download_torrent(&self, _url: &str) -> anyhow::Result<Vec<u8>> {
        Ok(vec![0])
    }
}

#[tokio::test]
async fn rows_arrive_in_completion_order_and_a_deadline_does_not_block_the_rest() {
    let (tx, mut rx) = mpsc::unbounded_channel();

    // The hanging source is spawned first *and* listed first: if events
    // were batched or delivered in spawn order, the fast source's rows
    // could only show up after the 100ms deadline -- B3 exists so they
    // show up while the slow one is still waiting.
    let slow_task = tokio::spawn(orchestrator::run_source(
        "hanging",
        7,
        fetch(hanging_source("hanging")),
        Duration::from_millis(100),
        tx.clone(),
    ));
    let fast = Arc::new(FakeSource {
        id: "fast",
        delay: Duration::from_millis(10),
        rows: 2,
        has_more: true,
        fail: None,
    });
    let fast_task = tokio::spawn(orchestrator::run_source(
        "fast",
        7,
        fetch(fast),
        Duration::from_secs(5),
        tx.clone(),
    ));
    tokio::spawn(orchestrator::coordinate(
        7,
        vec![("hanging", slow_task), ("fast", fast_task)],
        tx,
    ));

    match next_event(&mut rx).await {
        Event::SourceDone {
            source,
            generation,
            items,
            has_more,
            next_offset,
            error,
            timed_out,
        } => {
            assert_eq!(source, "fast", "rows must not wait behind a slow source");
            assert_eq!(generation, 7);
            assert_eq!(items.len(), 2);
            assert!(
                has_more,
                "the source's paging verdict travels with its rows"
            );
            assert_eq!(next_offset, None, "a row-paged source hands back no cursor");
            assert!(error.is_none());
            assert!(!timed_out);
        }
        other => panic!("expected the fast source's SourceDone, got {:?}", other),
    }

    match next_event(&mut rx).await {
        Event::SourceDone {
            source,
            timed_out,
            error,
            items,
            has_more,
            ..
        } => {
            assert_eq!(source, "hanging");
            assert!(timed_out, "the deadline must cut a hung source off");
            assert!(items.is_empty());
            assert!(!has_more, "a source that never answered has no verdict");
            let message = error.expect("a timeout still reports an error message");
            assert!(message.contains("timed out"), "got: {}", message);
        }
        other => panic!("expected the hanging source's SourceDone, got {:?}", other),
    }

    match next_event(&mut rx).await {
        Event::SearchComplete { generation } => assert_eq!(generation, 7),
        other => panic!("expected SearchComplete, got {:?}", other),
    }
}

#[tokio::test]
async fn a_failing_source_does_not_suppress_a_healthy_one() {
    let (tx, mut rx) = mpsc::unbounded_channel();

    let bad_task = tokio::spawn(orchestrator::run_source(
        "bad",
        3,
        fetch(failing_source("bad", "HTTP 503")),
        Duration::from_secs(5),
        tx.clone(),
    ));
    let ok_task = tokio::spawn(orchestrator::run_source(
        "ok",
        3,
        fetch(fast_source("ok", 3)),
        Duration::from_secs(5),
        tx.clone(),
    ));
    tokio::spawn(orchestrator::coordinate(
        3,
        vec![("bad", bad_task), ("ok", ok_task)],
        tx,
    ));

    let mut got_ok_rows = false;
    let mut got_bad_error = false;
    for _ in 0..3 {
        match next_event(&mut rx).await {
            Event::SourceDone {
                source,
                items,
                error,
                timed_out,
                ..
            } if source == "ok" => {
                assert_eq!(items.len(), 3, "the healthy source's rows must arrive");
                assert!(error.is_none() && !timed_out);
                got_ok_rows = true;
            }
            Event::SourceDone { source, error, .. } if source == "bad" => {
                let message = error.expect("a failed source must report why");
                assert_eq!(message, "HTTP 503");
                got_bad_error = true;
            }
            Event::SearchComplete { generation } => {
                assert_eq!(generation, 3);
            }
            other => panic!("unexpected event: {:?}", other),
        }
    }
    assert!(got_ok_rows, "healthy results were suppressed by a failure");
    assert!(got_bad_error, "the failure never reported itself");
}

#[tokio::test]
async fn a_panicking_task_is_reported_rather_than_hanging_the_dispatch() {
    let (tx, mut rx) = mpsc::unbounded_channel();

    let boom: tokio::task::JoinHandle<()> = tokio::spawn(async { panic!("boom") });
    tokio::spawn(orchestrator::coordinate(4, vec![("boom", boom)], tx));

    match next_event(&mut rx).await {
        Event::SourceDone { source, error, .. } => {
            assert_eq!(source, "boom");
            let message = error.expect("a dead task still owes a report");
            assert!(message.contains("task failed"), "got: {}", message);
        }
        other => panic!("expected a SourceDone for the dead task, got {:?}", other),
    }
    match next_event(&mut rx).await {
        Event::SearchComplete { generation } => assert_eq!(generation, 4),
        other => panic!("the dispatch must still finish, got {:?}", other),
    }
}

#[test]
fn status_mapping_separates_timeout_error_and_success() {
    assert_eq!(
        SourceStatus::from_event(4, None, false),
        SourceStatus::Ok(4)
    );
    assert_eq!(
        SourceStatus::from_event(0, Some("HTTP 503"), false),
        SourceStatus::Error("HTTP 503".to_string())
    );
    assert_eq!(
        SourceStatus::from_event(0, Some("timed out after 25s"), true),
        SourceStatus::Timeout
    );
    // The Display form is what a future status row would print.
    assert_eq!(SourceStatus::Pending.to_string(), "pending");
    assert_eq!(SourceStatus::Ok(12).to_string(), "ok(12)");
    assert_eq!(SourceStatus::Timeout.to_string(), "timeout");
}

#[test]
fn the_per_source_deadline_is_torios_25_seconds() {
    assert_eq!(orchestrator::PER_SOURCE_TIMEOUT, Duration::from_secs(25));
}

/// П.4 removed the Results tab bar: the panel's checkboxes *are* the
/// selection, so "ask everything" is simply every checked source and
/// "ask one source" means checking only it.
#[test]
fn selection_follows_the_panel_checklist() {
    let both = vec!["rutracker".to_string(), "rutor".to_string()];
    let ids = |enabled: &[String]| -> Vec<&'static str> {
        orchestrator::selected_sources(enabled, None, false)
            .iter()
            .map(|info| info.id)
            .collect()
    };

    assert_eq!(ids(&both), vec!["rutracker", "rutor"]);
    assert_eq!(
        ids(&both[..1]),
        vec!["rutracker"],
        "an unchecked source is skipped"
    );
    assert!(
        ids(&[]).is_empty(),
        "nothing checked means nothing to dispatch"
    );
    // An id that is not in the registry is not asked either: the panel
    // derives its rows from the registry, so a typo cannot become a
    // dispatch.
    let with_unknown = vec!["rutracker".to_string(), "never-heard-of-it".to_string()];
    assert_eq!(ids(&with_unknown), vec!["rutracker"]);
}

/// B6: the category decides who gets asked, and a source that does not
/// serve it is skipped outright -- not asked and filtered afterwards.
/// Its rows would arrive claiming no category, the view would drop all
/// of them, and the table would read as "this category found nothing"
/// while the sources able to filter it server-side were the only ones
/// really consulted.
#[test]
fn the_category_narrows_the_dispatch_to_sources_that_serve_it() {
    // yts declares only Movies and serves it by construction; tpb
    // declares Movies and TV; the rest declare all four.
    let both = vec!["rutracker".to_string(), "yts".to_string()];
    let ids = |group: Option<Group>| -> Vec<&'static str> {
        orchestrator::selected_sources(&both, group, false)
            .iter()
            .map(|info| info.id)
            .collect()
    };

    assert_eq!(ids(None), vec!["rutracker", "yts"]);
    assert_eq!(
        ids(Some(Group::Movies)),
        vec!["rutracker", "yts"],
        "both declare Movies, so both are asked"
    );
    assert_eq!(
        ids(Some(Group::TV)),
        vec!["rutracker"],
        "yts cannot answer TV, so it is not asked"
    );
    assert_eq!(ids(Some(Group::Anime)), vec!["rutracker"]);
    assert_eq!(ids(Some(Group::Games)), vec!["rutracker"]);

    // Checking only yts narrows the same way: one source, one group.
    let yts_only = vec!["yts".to_string()];
    assert_eq!(
        orchestrator::selected_sources(&yts_only, Some(Group::Movies), false).len(),
        1
    );
    assert!(
        orchestrator::selected_sources(&yts_only, Some(Group::TV), false).is_empty(),
        "yts is checked but serves nothing here"
    );
}

/// The empty-dispatch message is the only hint a stuck user gets, so it
/// has to point at the fix that works. The blocked-source branch is
/// unreachable through the registry today (every implemented source
/// serves its categories, which `source_registry_tests` guards), so the
/// assertions cover the branch that can still fire.
#[test]
fn the_empty_dispatch_explains_which_fix_actually_applies() {
    // Nothing checked at all: the panel is the fix, and the line says so.
    let none: Vec<String> = Vec::new();
    let undeclared = orchestrator::nothing_to_ask_reason(&none, Group::TV);
    assert!(
        undeclared.contains("No checked source serves 'TV'"),
        "{}",
        undeclared
    );
    assert!(undeclared.contains("Sources panel"), "{}", undeclared);

    // Sources checked, but none serves the group: same advice, and it
    // names the panel rather than a tab that no longer exists.
    let yts_only = vec!["yts".to_string()];
    let wrong_group = orchestrator::nothing_to_ask_reason(&yts_only, Group::TV);
    assert!(
        wrong_group.contains("No checked source serves 'TV'"),
        "{}",
        wrong_group
    );
    assert!(!wrong_group.contains("tab"), "{}", wrong_group);
}

/// B9: an empty query is browse mode, and a source that cannot answer
/// one is not asked -- its "browse" would be a search for the empty
/// string, which reads as a broken page rather than as the freshest
/// rows the user asked for.
#[test]
fn a_browse_asks_only_the_sources_that_can_answer_an_empty_query() {
    let all: Vec<String> = KNOWN_SOURCES
        .iter()
        .filter(|s| s.implemented)
        .map(|s| s.id.to_string())
        .collect();
    let ids = |browse: bool| -> Vec<&'static str> {
        orchestrator::selected_sources(&all, None, browse)
            .iter()
            .map(|info| info.id)
            .collect()
    };

    let browse = ids(true);
    for id in [
        "rutor",
        "yts",
        "tpb",
        "eztv",
        "subsplease",
        "nnmclub",
        "1337x",
    ] {
        assert!(browse.contains(&id), "{} must be able to browse", id);
    }
    // The two that cannot: the browser-backed one, and the one whose
    // empty-query feed was never answered live.
    assert!(!browse.contains(&"rutracker"), "{:?}", browse);
    assert!(!browse.contains(&"nyaa"), "{:?}", browse);
    // A category search is unaffected by the browse flag: rutracker is
    // back when the query has terms again.
    assert!(ids(false).contains(&"rutracker"));
}

#[test]
fn a_fresh_search_asks_every_selected_source_from_zero() {
    let selected = orchestrator::selected_sources(&both_enabled(), None, false);
    let offsets = std::collections::HashMap::new();
    let has_more = std::collections::HashMap::new();

    let plan = orchestrator::dispatch_plan(&selected, &offsets, &has_more);

    assert_eq!(plan.len(), 2);
    for (info, offset) in &plan {
        assert_eq!(*offset, 0, "{} must start at its first page", info.id);
    }
}

#[test]
fn load_more_asks_only_sources_that_reported_another_page() {
    let selected = orchestrator::selected_sources(&both_enabled(), None, false);
    let mut offsets = std::collections::HashMap::new();
    offsets.insert("rutor".to_string(), 100);
    offsets.insert("rutracker".to_string(), 50);
    let mut has_more = std::collections::HashMap::new();
    has_more.insert("rutor".to_string(), false);
    has_more.insert("rutracker".to_string(), true);

    let plan = orchestrator::dispatch_plan(&selected, &offsets, &has_more);

    // Each source resumes at *its own* cursor: rutor's pages are 100
    // rows, rutracker's are 50, and a shared counter walks off rutor's
    // grid -- which it answers with nothing.
    assert_eq!(plan.len(), 1);
    let (info, offset) = plan[0];
    assert_eq!(info.id, "rutracker");
    assert_eq!(offset, 50);
}

#[test]
fn a_source_that_failed_gets_retried_from_where_it_stopped() {
    let selected = orchestrator::selected_sources(&both_enabled(), None, false);
    let mut offsets = std::collections::HashMap::new();
    offsets.insert("rutracker".to_string(), 50);
    let mut has_more = std::collections::HashMap::new();
    // rutor timed out on the previous page: no verdict, so it must be
    // asked again rather than silently dropped from later pages.
    has_more.insert("rutracker".to_string(), false);

    let plan = orchestrator::dispatch_plan(&selected, &offsets, &has_more);

    assert_eq!(plan.len(), 1);
    let (info, offset) = plan[0];
    assert_eq!(info.id, "rutor");
    assert_eq!(
        offset, 0,
        "a source with no verdict restarts its own cursor"
    );
}

fn both_enabled() -> Vec<String> {
    vec!["rutracker".to_string(), "rutor".to_string()]
}

#[tokio::test]
async fn an_empty_dispatch_still_closes_the_generation() {
    // Nothing was spawned -- every source answered from cache, or none
    // could start -- and the UI still has to leave `Searching`. The
    // SearchComplete must travel through the same channel, so it queues
    // *behind* the cache hits already sent and the paging verdicts they
    // carry are read in the right order.
    let (tx, mut rx) = mpsc::unbounded_channel();

    tokio::spawn(orchestrator::coordinate(9, vec![], tx));

    match next_event(&mut rx).await {
        Event::SearchComplete { generation } => assert_eq!(generation, 9),
        other => panic!("expected SearchComplete, got {:?}", other),
    }
}

// --- cursor ownership (SearchPage::next_offset) -----------------------------

/// The row-paged default: rutor/rutracker hand back nothing, so their
/// cursor is exactly the row count delivered so far.
#[test]
fn a_source_without_a_cursor_advances_by_the_rows_it_delivered() {
    assert_eq!(orchestrator::advance_offset(0, 100, None), 100);
    assert_eq!(orchestrator::advance_offset(100, 100, None), 200);
}

/// Failures carry neither rows nor a cursor: `current + 0` is what
/// leaves the cursor alone so the failed page is asked again.
#[test]
fn a_failed_page_leaves_the_cursor_where_it_was() {
    assert_eq!(orchestrator::advance_offset(100, 0, None), 100);
    assert_eq!(orchestrator::advance_offset(7, 0, None), 7);
}

/// yts pages by *movie*, so its cursor is a page number that has nothing
/// to do with how many rows came back: the source's own number must win
/// over the row count, or "Load more" would skip movies (or repeat them).
#[test]
fn a_source_supplied_cursor_beats_the_row_count() {
    assert_eq!(orchestrator::advance_offset(0, 137, Some(2)), 2);
    assert_eq!(orchestrator::advance_offset(137, 0, Some(3)), 3);
    assert_eq!(orchestrator::advance_offset(50, 12, Some(6)), 6);
}

/// The cursor has to survive the trip through the event, not just exist
/// on `SearchPage`: `app.rs` only ever sees `Event::SourceDone`.
#[tokio::test]
async fn a_source_supplied_cursor_travels_with_its_rows() {
    let (tx, mut rx) = mpsc::unbounded_channel();

    let page = SearchPage {
        items: vec![row("yts", 0), row("yts", 1)],
        has_more: true,
        next_offset: Some(2),
    };
    tokio::spawn(orchestrator::run_source(
        "yts",
        9,
        async { Ok(page) },
        Duration::from_secs(1),
        tx,
    ));

    match next_event(&mut rx).await {
        Event::SourceDone {
            next_offset,
            has_more,
            ..
        } => {
            assert_eq!(next_offset, Some(2), "the source's own cursor must arrive");
            assert!(has_more);
        }
        other => panic!("expected a SourceDone, got {:?}", other),
    }
}
