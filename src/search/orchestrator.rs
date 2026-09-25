//! Registry-driven concurrent fan-out for one search (ROADMAP.md B3).
//!
//! `app.rs` decides *which* sources run (Results tab + Options) and owns
//! their instances; everything about *how* they run lives here so it can
//! be tested without a terminal, a browser or a network:
//!
//! - one task per selected source, each under a deadline, so a wedged
//!   source can't hold the whole fan-out hostage;
//! - every source reports on its own as soon as it answers
//!   ([`Event::SourceDone`], sent from inside its own task, so events
//!   arrive in completion order) -- rows render incrementally instead of
//!   waiting for the slowest source (today rutor's answer waits for
//!   rutracker's Cloudflare walk);
//! - a final [`Event::SearchComplete`] once the set is exhausted, which
//!   is what puts the UI back to idle.
//!
//! Which sources a dispatch includes is decided here too
//! (`selected_sources`) so the rule is testable as a plain function.

use std::fmt;
use std::future::Future;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::models::TorrentItem;
use super::source::{SearchPage, SourceInfo, KNOWN_SOURCES};
use crate::event::Event;

/// torio's `PER_SOURCE_TIMEOUT_MS`: one slow source must not hold the
/// whole fan-out hostage. Timeout is per source, not per dispatch, so a
/// hung rutracker still lets rutor's rows through.
pub const PER_SOURCE_TIMEOUT: Duration = Duration::from_secs(25);

/// Where one source of the current dispatch stands. Kept on `App` so a
/// future status row can render it; until then the log line is the
/// interim surface (B3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceStatus {
    Pending,
    Ok(usize),
    Error(String),
    Timeout,
}

impl SourceStatus {
    /// The status an [`Event::SourceDone`] carries implies: deadline
    /// first (its message is also the error text), then the source's own
    /// error, then a plain row count.
    pub fn from_event(count: usize, error: Option<&str>, timed_out: bool) -> Self {
        match (timed_out, error) {
            (true, _) => SourceStatus::Timeout,
            (false, Some(e)) => SourceStatus::Error(e.to_string()),
            (false, None) => SourceStatus::Ok(count),
        }
    }
}

impl fmt::Display for SourceStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceStatus::Pending => write!(f, "pending"),
            SourceStatus::Ok(n) => write!(f, "ok({})", n),
            SourceStatus::Error(e) => write!(f, "error: {}", e),
            SourceStatus::Timeout => write!(f, "timeout"),
        }
    }
}

/// What one source's page fetch came back as. Failures are values here,
/// not `Err`: every source must report in, because `Event::SearchComplete`
/// only fires once all of them did.
#[derive(Debug, Clone)]
pub struct SourceOutcome {
    pub items: Vec<TorrentItem>,
    pub has_more: bool,
    /// `None` on success; on failure the message already says whether it
    /// was a timeout (`timed_out` mirrors that for status reporting).
    pub error: Option<String>,
    pub timed_out: bool,
}

impl SourceOutcome {
    pub fn ok(page: SearchPage) -> Self {
        Self {
            items: page.items,
            has_more: page.has_more,
            error: None,
            timed_out: false,
        }
    }

    pub fn failed(message: impl fmt::Display) -> Self {
        Self {
            items: Vec::new(),
            has_more: false,
            error: Some(message.to_string()),
            timed_out: false,
        }
    }

    pub fn deadline(timeout: Duration) -> Self {
        Self {
            items: Vec::new(),
            has_more: false,
            error: Some(format!("timed out after {}s", timeout.as_secs())),
            timed_out: true,
        }
    }

    /// The status this outcome leaves behind (what `App` stores).
    pub fn status(&self) -> SourceStatus {
        SourceStatus::from_event(self.items.len(), self.error.as_deref(), self.timed_out)
    }
}

/// Run one source's whole page fetch -- login walk included, since that
/// is where rutracker's time goes -- under `timeout`, and report the
/// outcome as [`Event::SourceDone`] the moment it lands.
///
/// The send happens inside the source's own task, so events arrive in
/// *completion* order: a source stuck behind a Cloudflare walk cannot
/// delay a source that already answered (B3's whole point), and
/// `Event::SearchComplete` still comes last because [`coordinate`] only
/// emits it after every task has finished -- and every task sends its
/// `SourceDone` before finishing.
///
/// Never returns `Err`: a timeout, an error and a success are all
/// outcomes, because a source that fails has to say so rather than stay
/// silent (B0.3) and leave the UI waiting forever.
pub async fn run_source(
    source_id: &'static str,
    generation: u64,
    fetch: impl Future<Output = Result<SearchPage>>,
    timeout: Duration,
    tx: mpsc::UnboundedSender<Event>,
) {
    let outcome = match tokio::time::timeout(timeout, fetch).await {
        Ok(Ok(page)) => SourceOutcome::ok(page),
        Ok(Err(e)) => SourceOutcome::failed(e),
        Err(_) => SourceOutcome::deadline(timeout),
    };
    let _ = tx.send(Event::SourceDone {
        source: source_id.to_string(),
        generation,
        items: outcome.items,
        has_more: outcome.has_more,
        error: outcome.error,
        timed_out: outcome.timed_out,
    });
}

/// Wait out every per-source task, then close the generation with
/// [`Event::SearchComplete`].
///
/// A task that panicked is reported as a failed `SourceDone` rather than
/// dropped: it must neither swallow the healthy sources' results nor
/// leave the UI stuck in `Searching` (B0.3).
pub async fn coordinate(
    generation: u64,
    tasks: Vec<(&'static str, JoinHandle<()>)>,
    tx: mpsc::UnboundedSender<Event>,
) {
    for (source, task) in tasks {
        if let Err(join_err) = task.await {
            let _ = tx.send(Event::SourceDone {
                source: source.to_string(),
                generation,
                items: Vec::new(),
                has_more: false,
                error: Some(format!("task failed: {}", join_err)),
                timed_out: false,
            });
        }
    }
    let _ = tx.send(Event::SearchComplete { generation });
}

/// Which sources one dispatch runs: implemented, enabled in Options, and
/// on the active Results tab (`"all"` means every one of them). This is
/// the registry-driven replacement for `app.rs`'s two hardcoded
/// branches -- a source registered later needs no orchestrator change.
pub fn selected_sources(active_tab: &str, enabled: &[String]) -> Vec<&'static SourceInfo> {
    KNOWN_SOURCES
        .iter()
        .filter(|info| info.implemented)
        .filter(|info| enabled.iter().any(|e| e == info.id))
        .filter(|info| active_tab == "all" || active_tab == info.id)
        .collect()
}
