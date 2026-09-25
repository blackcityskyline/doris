//! Offline tests for the B3 orchestrator: fake `Source`s stand in for a
//! fast source, a hanging one, a failing one and a panicking one, which
//! is what proves incremental delivery, the per-source deadline, and
//! "a failing source never suppresses a healthy one's results" without a
//! terminal, a browser or the network.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use doris::event::Event;
use doris::search::models::TorrentItem;
use doris::search::orchestrator::{self, SourceStatus};
use doris::search::source::{
    AuthContext, Group, LogFn, SearchPage, SearchRequest, Source,
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
            error,
            timed_out,
        } => {
            assert_eq!(source, "fast", "rows must not wait behind a slow source");
            assert_eq!(generation, 7);
            assert_eq!(items.len(), 2);
            assert!(has_more, "the source's paging verdict travels with its rows");
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
            Event::SourceDone {
                source, error, ..
            } if source == "bad" => {
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
        Event::SourceDone {
            source, error, ..
        } => {
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
    assert_eq!(SourceStatus::from_event(4, None, false), SourceStatus::Ok(4));
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

#[test]
fn selection_follows_the_results_tab_and_the_options_checklist() {
    let both = vec!["rutracker".to_string(), "rutor".to_string()];
    let ids = |tab: &str, enabled: &[String]| -> Vec<&'static str> {
        orchestrator::selected_sources(tab, enabled)
            .iter()
            .map(|info| info.id)
            .collect()
    };

    assert_eq!(ids("all", &both), vec!["rutracker", "rutor"]);
    assert_eq!(ids("rutor", &both), vec!["rutor"]);
    assert_eq!(ids("rutracker", &both), vec!["rutracker"]);
    assert_eq!(ids("all", &both[..1]), vec!["rutracker"], "a disabled source is skipped");
    assert!(ids("all", &[]).is_empty(), "nothing enabled means nothing to dispatch");
    // Planned sources are listed in the registry but never dispatched.
    assert!(!ids("all", &both).contains(&"nnmclub"));
}
