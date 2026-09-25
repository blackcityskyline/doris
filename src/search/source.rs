//! `Source` is the seam the whole app is meant to depend on instead of
//! reaching into `rutracker.rs` by name (see ROADMAP.md, "Architecture
//! problems" A2). Adding a new content source is meant to be:
//!
//! 1. Write `src/search/<name>.rs` implementing [`Source`].
//! 2. Add one entry to [`KNOWN_SOURCES`].
//!
//! Nothing else in the orchestrator, browser layer, or Options UI should
//! need to change. This file is intentionally the *only* place that knows
//! the concrete list of sources.
//!
//! (Phase 3's original note about rewiring `app.rs`/`main.rs` onto this
//! trait is what ROADMAP.md phase B2 closes.)

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

use crate::browser::cdp::Browser;
use tokio::sync::Mutex;

use super::models::TorrentItem;
use super::rutracker::RutrackerSearcher;
use super::rutor::RutorSearcher;
use super::yts::YtsSearcher;

/// Content categories a source can attribute its results to. Declared
/// here, next to the registry it describes (and not in `models.rs`) so
/// `TorrentItem.group` is typed against the same enum the `Source` trait
/// hands out -- see ROADMAP.md B1/B6.
///
/// Serde renders variants as plain strings (`"Games"`), which is what
/// `TorrentItem`'s JSON needs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Group {
    #[default]
    Games,
    Movies,
    TV,
    Anime,
}

/// One run of a query against a source. Replaces the old
/// `search(query)` / `search_page(query, start)` trait pair: the page
/// cursor moved into the request, and a category slot was added for B6.
#[derive(Debug, Clone)]
pub struct SearchRequest {
    /// The words to look for.
    pub query: String,
    /// Page cursor. Its unit (rows vs. page index) is deliberately owned
    /// by the source -- rutor counts rows of 100, rutracker counts the
    /// forum's own `start=` step -- so callers treat it as opaque and
    /// just hand back what the previous [`SearchPage`] implied.
    pub offset: usize,
    /// `None` = all categories. B6 passes a real group down so sources
    /// can filter server-side (rutor's URL has a category slot).
    pub category: Option<Group>,
}

impl SearchRequest {
    /// An "all categories" request -- what the Results tabs issue today.
    pub fn new(query: impl Into<String>, offset: usize) -> Self {
        Self { query: query.into(), offset, category: None }
    }
}

/// One page of results plus an honest "was that the last page?".
///
/// `has_more` replaces app.rs's `count < 50` guess, which only worked by
/// accident (rutracker really does page by 50, while rutor pages by 100
/// and so could never trip it).
#[derive(Debug, Clone, Default)]
pub struct SearchPage {
    pub items: Vec<TorrentItem>,
    pub has_more: bool,
    /// The cursor the *next* dispatch should hand back, in this source's
    /// own unit -- rows for row-paged sources, a page number for an API
    /// that counts pages of its own (yts pages by *movie*, and how many
    /// rows a page yields depends on how many qualities each movie has,
    /// so any row-derived cursor would skip or repeat).
    ///
    /// `None` = "rows": `offset + items.len()`, which is exactly what
    /// rutor/rutracker want and what a *failed* page wants too (no rows
    /// -> cursor unchanged). See `orchestrator::advance_offset`.
    pub next_offset: Option<usize>,
}

/// Credentials + cookie path handed to [`Source::ensure_logged_in`].
/// Bundled into one struct so a source gaining an auth detail (a second
/// cookie jar, a token) doesn't change the trait's signature.
#[derive(Debug, Clone, Default)]
pub struct AuthContext {
    /// Where to load/save the browser cookie jar; `None` when the user
    /// turned "Save cookies" off (callers gate it, same as before).
    pub cookie_file: Option<PathBuf>,
    pub username: Option<String>,
    pub password: Option<String>,
}

/// Log sink shared by every source: messages land in the Log zone and in
/// the TUI's detailed log view.
pub type LogFn = Arc<dyn Fn(&str) + Send + Sync>;

/// One pluggable content source. Everything the orchestrator, the browser
/// layer, and the Options "Sources" checklist need from a source goes
/// through here.
#[async_trait]
pub trait Source: Send + Sync {
    /// Stable lowercase identifier, e.g. `"rutracker"`. Used as the
    /// credentials-store key and the Options "Sources" checklist key.
    fn id(&self) -> &'static str;

    /// Human-readable name shown in the UI ("Rutor").
    fn label(&self) -> &'static str;

    /// Groups this source can attribute results to -- the instance-side
    /// view of [`SourceInfo::groups`], so a live source and the
    /// metadata-only registry can never disagree. B6 passes a group down
    /// through [`SearchRequest::category`].
    fn groups(&self) -> &'static [Group];

    /// A page on this source's domain. Used as the navigation target for
    /// cookie injection when the browser runs hidden -- see
    /// `browser::cdp::Browser::launch`.
    fn home_url(&self) -> &'static str;

    /// Whether talking to this source requires a running browser
    /// session. Only rutracker does; plain-HTTP sources are skipped by
    /// the orchestrator instead of being handed a no-op login.
    fn requires_browser(&self) -> bool;

    /// Whether it can answer a `SearchRequest` with an empty `query`
    /// (browse mode). Both current sources need real search terms, so
    /// both return `false` until B9 builds browsing on top.
    fn supports_browse(&self) -> bool;

    /// Establish (or verify) a session, reusing cached state when the
    /// source already has one. Takes `&self` because a registry hands
    /// out `Arc<dyn Source>` with no `&mut` to give; the mutable session
    /// flag lives behind interior mutability.
    async fn ensure_logged_in(&self, auth: &AuthContext, log: &LogFn) -> Result<bool>;

    async fn search(&self, req: &SearchRequest) -> Result<SearchPage>;
    async fn download_torrent(&self, url: &str) -> Result<Vec<u8>>;
}

/// Rutracker exposes no server-side category filter we've verified live,
/// so it lists the four groups for browsing/registry purposes and B6
/// decides (after checking `tracker.php?c[]=`) whether
/// [`SearchRequest::category`] actually narrows anything for it.
const RUTRACKER_GROUPS: &[Group] = &[Group::Games, Group::Movies, Group::TV, Group::Anime];

/// Rutor's search URL carries a real category slot (`0` = all); B6 maps
/// these groups onto its ids once verified.
const RUTOR_GROUPS: &[Group] = &[Group::Movies, Group::TV, Group::Games, Group::Anime];

/// YTS only ever has movies, so it declares that one group -- which is
/// also what makes its rows land in the Movies view without any
/// per-row category guessing (B8 wave 1; category *filtering* is B6).
const YTS_GROUPS: &[Group] = &[Group::Movies];

#[async_trait]
impl Source for RutrackerSearcher {
    fn id(&self) -> &'static str {
        "rutracker"
    }

    fn label(&self) -> &'static str {
        "Rutracker"
    }

    fn groups(&self) -> &'static [Group] {
        RUTRACKER_GROUPS
    }

    fn home_url(&self) -> &'static str {
        Self::HOME_URL
    }

    fn requires_browser(&self) -> bool {
        true
    }

    fn supports_browse(&self) -> bool {
        false
    }

    async fn ensure_logged_in(&self, auth: &AuthContext, log: &LogFn) -> Result<bool> {
        RutrackerSearcher::ensure_logged_in(
            self,
            auth.cookie_file.as_deref(),
            auth.username.as_deref(),
            auth.password.as_deref(),
            log.clone(),
        )
        .await
    }

    async fn search(&self, req: &SearchRequest) -> Result<SearchPage> {
        let items = RutrackerSearcher::search_page(self, &req.query, req.offset).await?;
        // The forum pages `tracker.php?start=` by 50, so a short page is
        // the last one and a full one may have more behind it.
        let has_more = items.len() >= RutrackerSearcher::PAGE_SIZE;
        Ok(SearchPage { items, has_more, next_offset: None })
    }

    async fn download_torrent(&self, url: &str) -> Result<Vec<u8>> {
        RutrackerSearcher::download_torrent(self, url).await
    }
}

/// Rutor needs no browser, no login, and no cookies -- ensure_logged_in
/// is a trivial always-true no-op purely to satisfy the trait's shape.
#[async_trait]
impl Source for RutorSearcher {
    fn id(&self) -> &'static str {
        "rutor"
    }

    fn label(&self) -> &'static str {
        "Rutor"
    }

    fn groups(&self) -> &'static [Group] {
        RUTOR_GROUPS
    }

    fn home_url(&self) -> &'static str {
        Self::HOME_URL
    }

    fn requires_browser(&self) -> bool {
        false
    }

    fn supports_browse(&self) -> bool {
        false
    }

    async fn ensure_logged_in(&self, _auth: &AuthContext, _log: &LogFn) -> Result<bool> {
        Ok(true)
    }

    async fn search(&self, req: &SearchRequest) -> Result<SearchPage> {
        let items = RutorSearcher::search_page(self, &req.query, req.offset).await?;
        // Fixed 100-row pages (see `RutorSearcher::PAGE_SIZE`). The
        // category slot is intentionally not honored yet: B6 verifies
        // rutor's category ids against the live site before claiming
        // server-side filtering.
        let has_more = items.len() >= RutorSearcher::PAGE_SIZE;
        Ok(SearchPage { items, has_more, next_offset: None })
    }

    async fn download_torrent(&self, url: &str) -> Result<Vec<u8>> {
        RutorSearcher::download_torrent(self, url).await
    }
}

/// Metadata-only description of a source, for listing in the Options
/// "Sources" checklist without needing a live, logged-in instance (which
/// requires a running `Browser`). Real `Source` instances are constructed
/// lazily by the orchestrator only when a source is actually used.
///
/// The `groups`/`requires_browser`/`home_url` values mirror what the
/// corresponding `Source` impl returns -- `source_registry_tests.rs`
/// pins that correspondence where it can be checked offline.
#[derive(Debug, Clone, Copy)]
pub struct SourceInfo {
    pub id: &'static str,
    /// Human-readable name shown in the UI. Was called `display_name`
    /// until B2 renamed it to match `Source::label()`.
    pub label: &'static str,
    /// `false` for sources reserved for the future (e.g. nnm-club)
    /// so Options can list them as coming-soon rather than hide them.
    pub implemented: bool,
    pub groups: &'static [Group],
    /// Whether using this source needs a browser session launched first
    /// (see `Source::requires_browser`). `false` for planned sources:
    /// nothing constructs them yet, so nothing may promise a browser.
    pub requires_browser: bool,
    /// Domain home page, `""` while the source isn't implemented. The
    /// orchestrator passes it to `Browser::launch` before the `Source`
    /// instance itself exists (the instance is what *needs* the browser,
    /// so it can't supply its own home page).
    pub home_url: &'static str,
}

/// The full list of sources the app knows about, implemented or not. This
/// is the single place to touch when adding a new source's *listing*;
/// implementing [`Source`] for it is the separate step that makes
/// `implemented` become `true`.
pub const KNOWN_SOURCES: &[SourceInfo] = &[
    SourceInfo {
        id: "rutracker",
        label: "Rutracker",
        implemented: true,
        groups: RUTRACKER_GROUPS,
        requires_browser: true,
        home_url: RutrackerSearcher::HOME_URL,
    },
    SourceInfo {
        id: "rutor",
        label: "Rutor",
        implemented: true,
        groups: RUTOR_GROUPS,
        requires_browser: false,
        home_url: RutorSearcher::HOME_URL,
    },
    SourceInfo {
        id: "yts",
        label: "YTS",
        implemented: true,
        groups: YTS_GROUPS,
        requires_browser: false,
        home_url: YtsSearcher::HOME_URL,
    },
    SourceInfo {
        id: "nnmclub",
        label: "NNM-Club",
        implemented: false,
        groups: &[],
        requires_browser: false,
        home_url: "",
    },
];

/// What a source needs from the app in order to be *built*. Browser
/// lifecycle (detect, visibility, close-on-exit) stays in `app.rs`,
/// where that config lives; this struct is the hand-off point.
pub struct SourceEnv {
    /// An already-launched browser session. Required by sources with
    /// `requires_browser() == true`, ignored by plain-HTTP ones.
    pub browser: Option<Arc<Mutex<Browser>>>,
}

/// Build the live instance for `id`: the one place that maps ids to
/// concrete types, so `app.rs`/`main.rs` only ever handle
/// `Arc<dyn Source>` (B2 closes Phase 3's "rewire app.rs" note).
///
/// Browser-backed sources need `env.browser` handed in rather than
/// launching one themselves -- they are exactly the thing that *needs*
/// the browser, so they cannot exist before it.
pub fn build_source(id: &str, env: SourceEnv) -> Result<Arc<dyn Source>> {
    match id {
        "rutracker" => {
            let browser = env.browser.ok_or_else(|| {
                anyhow!("source '{}' needs a running browser session", id)
            })?;
            Ok(Arc::new(RutrackerSearcher::new(browser)))
        }
        "rutor" => Ok(Arc::new(RutorSearcher::new())),
        "yts" => Ok(Arc::new(YtsSearcher::new())),
        other => Err(anyhow!("unknown source '{}'", other)),
    }
}

/// Registry lookup by id -- the metadata-only twin of building a live
/// [`Source`] (`build_source`, used by the orchestrator).
pub fn get_source(id: &str) -> Option<&'static SourceInfo> {
    KNOWN_SOURCES.iter().find(|s| s.id == id)
}

/// Which sources belong to a group, for B6's category -> source mapping
/// and for the Options UI grouping rows by what they can filter.
pub fn sources_by_group(group: Group) -> Vec<&'static SourceInfo> {
    KNOWN_SOURCES.iter().filter(|s| s.groups.contains(&group)).collect()
}

/// Whether orchestrating `id` needs a browser session launched first.
/// Unknown ids fall back to `true`: the conservative answer, because
/// guessing "no browser" for a source we don't know about would send its
/// login through a path that can't reach a browser (same fallback
/// `app.rs::source_needs_browser` has always had).
pub fn requires_browser(id: &str) -> bool {
    get_source(id).map(|s| s.requires_browser).unwrap_or(true)
}
