//! Registry-driven concurrent fan-out for one search. `app.rs` decides *which* sources run
//! (Results tab + Options) and owns their instances; everything about *how* they run lives here
//! so it can be tested without a terminal, a browser or a network: - one task per selected
//! source, each under a deadline, so a wedged source can't hold the whole fan-out hostage; -
//! every source reports on its own as soon as it answers ([`Event::SourceDone`], sent from
//! inside its own task, so events arrive in completion order) -- rows render incrementally
//! instead of waiting for the slowest source (today rutor's answer waits for rutracker's
//! Cloudflare walk); - a final [`Event::SearchComplete`] once the set is exhausted, which is
//! what puts the UI back to idle.

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

/// torio's `PER_SOURCE_TIMEOUT_MS`: one slow source must not hold the whole fan-out hostage.
pub const PER_SOURCE_TIMEOUT: Duration = Duration::from_secs(25);

/// What a browser-backed source gets on top of the deadline, for the
/// login walk. Live: establishing a rutracker session from cold --
/// launch, Cloudflare challenge, fill the form, submit -- took 26 s, so
/// a 25 s deadline fired *after* a successful login and before the
/// search could start. The Trackers panel then showed a timeout with
/// `LOGIN SUCCESSFUL` a line above it.
pub const LOGIN_GRACE: Duration = Duration::from_secs(60);

/// The deadline one source's fetch gets.
///
/// A source that needs a browser has to establish a session before it
/// can fetch anything, and that walk is inside the same future, so it
/// gets the extra on top. Everything else gets the plain deadline.
pub fn deadline_for(requires_browser: bool) -> Duration {
    if requires_browser {
        PER_SOURCE_TIMEOUT + LOGIN_GRACE
    } else {
        PER_SOURCE_TIMEOUT
    }
}

/// Where one source of the current dispatch stands.
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

/// What one source's page fetch came back as.
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

/// Run one source's whole page fetch -- login walk included, since that is where rutracker's
/// time goes -- under `timeout`, and report the outcome as [`Event::SourceDone`] the moment it
/// lands.
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

/// Wait out every per-source task, then close the generation with [`Event::SearchComplete`].
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

/// Which sources one dispatch runs: implemented, enabled in Options, and on the active Results
/// tab (`"all"` means every one of them).
pub fn selected_sources(
    enabled: &[String],
    group: Option<Group>,
    browse: bool,
) -> Vec<&'static SourceInfo> {
    KNOWN_SOURCES
        .iter()
        .filter(|info| info.implemented)
        .filter(|info| enabled.iter().any(|e| e == info.id))
        .filter(|info| match group {
            None => true,
            Some(group) => info.category_filter && info.groups.contains(&group),
        })
        .filter(|info| !browse || info.supports_browse)
        .collect()
}

/// The log line for a category search that selected nobody, phrased by what would actually
/// change the outcome.
pub fn nothing_to_ask_reason(enabled: &[String], group: Group) -> String {
    let blocked = KNOWN_SOURCES.iter().find(|info| {
        enabled.iter().any(|e| e == info.id)
            && !info.category_filter
            && info.groups.contains(&group)
    });
    match blocked {
        Some(info) => format!(
            "{} cannot filter by '{}' yet (its category slot is \
             unverified) -- uncheck it or check another source.",
            info.label,
            group.label()
        ),
        None => format!(
            "No checked source serves '{}' -- check one in the Trackers \
             panel (3).",
            group.label()
        ),
    }
}

/// The cursor a source gets after reporting a page.
pub fn advance_offset(current: usize, count: usize, next_offset: Option<usize>) -> usize {
    next_offset.unwrap_or(current + count)
}

/// The `(source, offset)` pairs a dispatch should run: every selected source on a fresh search,
/// and on a "load more" only the ones that said they have another page -- each at *its own*
/// cursor.
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

/// Wrap one source's page fetch so a *successful* page is stored under `key` before it is
/// reported.
pub async fn cached_fetch(
    fetch: impl Future<Output = Result<SearchPage>>,
    cache: Arc<SearchCache>,
    key: CacheKey,
) -> Result<SearchPage> {
    let page = fetch.await?;
    cache.put(key, page.clone());
    Ok(page)
}

/// The cache-first half of a dispatch: a fresh hit becomes the very same [`Event::SourceDone`]
/// a live fetch would have produced, so the UI, the per-source offsets and the paging verdict
/// all update through the normal path -- the only thing skipped is the network, and with it the
/// browser launch a browser-backed source would otherwise need.
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
