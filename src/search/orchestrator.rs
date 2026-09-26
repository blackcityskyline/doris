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

use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use super::cache::{CacheKey, SearchCache};
use super::models::TorrentItem;
use super::source::{Group, SearchPage, SourceInfo, KNOWN_SOURCES};
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
    /// The page's own cursor for its next dispatch (`None` on failure --
    /// a page that never happened owes no cursor).
    pub next_offset: Option<usize>,
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
            next_offset: page.next_offset,
            error: None,
            timed_out: false,
        }
    }

    pub fn failed(message: impl fmt::Display) -> Self {
        Self {
            items: Vec::new(),
            has_more: false,
            next_offset: None,
            error: Some(message.to_string()),
            timed_out: false,
        }
    }

    pub fn deadline(timeout: Duration) -> Self {
        Self {
            items: Vec::new(),
            has_more: false,
            next_offset: None,
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
        next_offset: outcome.next_offset,
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
                next_offset: None,
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
/// Which sources a dispatch should ask: the Results tab, the Options
/// checklist, and -- since B6 -- the selected category.
///
/// A source that does not serve the category is *not asked* rather than
/// asked and filtered afterwards: it would answer with rows that claim
/// no category, the view would drop every one of them, and the table
/// would read as "this category is empty" while sources able to filter
/// it server-side were the only ones consulted. Two ways a source fails
/// that test, both handled here: it does not declare the group at all
/// (yts cannot answer TV), or it declares it but cannot filter by it
/// (`SourceInfo::category_filter` -- today only rutracker, whose `c[]`
/// slot has never been verified live, so a category search skips the
/// slow browser round-trip instead of discarding its rows).
pub fn selected_sources(
    active_tab: &str,
    enabled: &[String],
    group: Option<Group>,
) -> Vec<&'static SourceInfo> {
    KNOWN_SOURCES
        .iter()
        .filter(|info| info.implemented)
        .filter(|info| enabled.iter().any(|e| e == info.id))
        .filter(|info| active_tab == "all" || active_tab == info.id)
        .filter(|info| match group {
            None => true,
            Some(group) => info.category_filter && info.groups.contains(&group),
        })
        .collect()
}

/// The cursor a source gets after reporting a page.
///
/// `next_offset` wins when the source gave one: that is how a source
/// whose API counts pages of its own (yts counts *movies*, and rows per
/// page vary with how many qualities each has) stays aligned, instead of
/// deriving a cursor from a row count that does not match its unit.
///
/// Otherwise the row-paged default applies: `current + count`, which is
/// also the right answer for a *failed* page -- failures carry no rows
/// and no cursor, so `current + 0` leaves the cursor where it was and
/// the source is asked again from there.
pub fn advance_offset(current: usize, count: usize, next_offset: Option<usize>) -> usize {
    next_offset.unwrap_or(current + count)
}

/// The `(source, offset)` pairs a dispatch should run: every selected
/// source on a fresh search, and on a "load more" only the ones that
/// said they have another page -- each at *its own* cursor.
///
/// Own cursors are the fix for a real bug: offsets used to be one shared
/// row count, which drifts off rutor's 100-row page grid the moment two
/// sources with different page sizes are merged, and rutor silently
/// answers a misaligned offset with nothing (it guards `offset %
/// PAGE_SIZE`, see `rutor.rs`).
///
/// Skipping exactly `Some(false)` rather than requiring `Some(true)` is
/// deliberate: a source that failed last time has no verdict, so it gets
/// another chance -- which is what the old always-dispatch-both behavior
/// did.
pub fn dispatch_plan(
    selected: &[&'static SourceInfo],
    offsets: &HashMap<String, usize>,
    has_more: &HashMap<String, bool>,
) -> Vec<(&'static SourceInfo, usize)> {
    selected
        .iter()
        .copied()
        .filter(|info| has_more.get(info.id) != Some(&false))
        .map(|info| (info, offsets.get(info.id).copied().unwrap_or(0)))
        .collect()
}

/// Wrap one source's page fetch so a *successful* page is stored under
/// `key` before it is reported (B5).
///
/// Failures pass through untouched: caching an error would pin "no
/// results" for the whole TTL, turning one bad request into five minutes
/// of empty output.
pub async fn cached_fetch(
    fetch: impl Future<Output = Result<SearchPage>>,
    cache: Arc<SearchCache>,
    key: CacheKey,
) -> Result<SearchPage> {
    let page = fetch.await?;
    cache.put(key, page.clone());
    Ok(page)
}

/// The cache-first half of a dispatch (B5): a fresh hit becomes the very
/// same [`Event::SourceDone`] a live fetch would have produced, so the
/// UI, the per-source offsets and the paging verdict all update through
/// the normal path -- the only thing skipped is the network, and with it
/// the browser launch a browser-backed source would otherwise need.
///
/// `None` means miss or expired: spawn the source.
pub fn cached_source_done(cache: &SearchCache, key: &CacheKey, generation: u64) -> Option<Event> {
    let page = cache.get(key)?;
    Some(Event::SourceDone {
        source: key.source.clone(),
        generation,
        items: page.items,
        has_more: page.has_more,
        next_offset: page.next_offset,
        error: None,
        timed_out: false,
    })
}
