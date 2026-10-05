//! `Source` is the seam the whole app is meant to depend on instead of reaching into
//! `rutracker.rs` by name. Adding a new content source is meant to be: 1.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

use crate::browser::cdp::Browser;
use tokio::sync::Mutex;

use super::models::{FileEntry, TorrentItem};
use super::nnmclub::NnmclubSearcher;
use super::nyaa::NyaaSearcher;
use super::rutor::RutorSearcher;
use super::rutracker::RutrackerSearcher;
use super::subsplease::SubsPleaseSearcher;
use super::torentino::TorentinoSearcher;
use super::tpb::TpbSearcher;
use super::x1337x::X1337xSearcher;
use super::yts::YtsSearcher;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Group {
    #[default]
    Games,
    Movies,
    TV,
    Anime,
}

/// The `f[]` forum selector one DLE tracker's search form posts: `f%5B%5D=<id>` repeated for
/// every forum of the chosen group, or `all_forums` when the group has none (and rutracker
/// passes `""`, which is its own "no filter").
pub fn forum_params(
    table: &[(Group, &[i32])],
    category: Option<Group>,
    all_forums: &str,
) -> String {
    let ids = match category {
        Some(group) => table
            .iter()
            .find(|(g, _)| *g == group)
            .map_or(&[][..], |(_, ids)| ids),
        None => &[],
    };
    if ids.is_empty() {
        return all_forums.to_string();
    }
    ids.iter()
        .map(|id| format!("f%5B%5D={}", id))
        .collect::<Vec<_>>()
        .join("&")
}

impl Group {
    /// What this group is called where a user can read it: the category
    /// row under the source tabs, and the search log's category marker.
    pub fn label(self) -> &'static str {
        match self {
            Group::Movies => "Movies",
            Group::TV => "TV",
            Group::Games => "Games",
            Group::Anime => "Anime",
        }
    }
}

/// The order the category row offers the groups in, left to right after
/// the "all" tab: the same order tab row was decided in, kept here
/// so the visible order is one recorded decision instead of an accident
/// of where a variant happened to be typed above.
pub const GROUP_ORDER: [Group; 4] = [Group::Movies, Group::TV, Group::Games, Group::Anime];

#[derive(Debug, Clone)]
pub struct SearchRequest {
    pub query: String,
    pub offset: usize,
    /// `None` = all categories.
    pub category: Option<Group>,
}

impl SearchRequest {
    /// An "all categories" request -- what the Results tabs issue today.
    pub fn new(query: impl Into<String>, offset: usize) -> Self {
        Self {
            query: query.into(),
            offset,
            category: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SearchPage {
    pub items: Vec<TorrentItem>,
    pub has_more: bool,
    /// The cursor the *next* dispatch should hand back, in this source's own unit -- rows for
    /// row-paged sources, a page number for an API that counts pages of its own (yts pages by
    /// *movie*, and how many rows a page yields depends on how many qualities each movie has,
    /// so any row-derived cursor would skip or repeat).
    pub next_offset: Option<usize>,
}

/// Credentials + cookie path handed to [`Source::ensure_logged_in`].
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

#[async_trait]
pub trait Source: Send + Sync {
    /// Stable lowercase identifier, e.g.
    fn id(&self) -> &'static str;

    /// Human-readable name shown in the UI ("Rutor").
    fn label(&self) -> &'static str;

    /// Groups this source can attribute results to -- the instance-side view of
    /// [`SourceInfo::groups`], so a live source and the metadata-only registry can never
    /// disagree.
    fn groups(&self) -> &'static [Group];

    /// A page on this source's domain.
    fn home_url(&self) -> &'static str;

    /// Whether talking to this source requires a running browser session.
    fn requires_browser(&self) -> bool;

    /// Whether it can answer a `SearchRequest` with an empty `query` (browse mode -- the `b`
    /// key).
    fn supports_browse(&self) -> bool;

    /// Establish (or verify) a session, reusing cached state when the source already has one.
    async fn ensure_logged_in(&self, auth: &AuthContext, log: &LogFn) -> Result<bool>;

    async fn search(&self, req: &SearchRequest) -> Result<SearchPage>;
    async fn download_torrent(&self, url: &str) -> Result<Vec<u8>>;

    /// The magnet link that lives on the row's *own* page, fetched when a row arrives with
    /// neither a magnet nor a `.torrent` link.
    async fn resolve_magnet(&self, _page_url: &str) -> Result<Option<String>> {
        Ok(None)
    }

    /// The files inside a torrent, read from the row's own page.
    async fn details(&self, _page_url: &str) -> Result<Vec<FileEntry>> {
        Ok(Vec::new())
    }
}

/// Rutracker's search form posts a real category slot -- `f[]` with forum
/// ids, live-verified 26.09.2026 -- so it declares the four groups it can
/// filter, and `rutracker::GROUP_FORUMS` maps them onto forum ids.
const RUTRACKER_GROUPS: &[Group] = &[Group::Games, Group::Movies, Group::TV, Group::Anime];

const RUTRACKER_AD_CDN: &str = "rutrk.org";

/// Torentino is a games tracker, top to bottom, so it declares the
/// one group its rows can claim (B8 wave 3; playback is
/// `.torrent -> upload_torrent` fallback, no bencode crate).
const TRENTINO_GROUPS: &[Group] = &[Group::Games];

/// Rutor's search URL carries a real category slot (`0` = all), and B6
/// maps these groups onto the rubric ids live-verified in
/// `rutor::GROUP_IDS` -- that table feeds both the URL and the rows'
/// claim, so the two cannot drift apart.
const RUTOR_GROUPS: &[Group] = &[Group::Movies, Group::TV, Group::Games, Group::Anime];

/// YTS only ever has movies, so it declares that one group -- which is
/// also what makes its rows land in the Movies view without any
/// per-row category guessing (B8 wave 1; category *filtering* is B6).
const YTS_GROUPS: &[Group] = &[Group::Movies];

/// One apibay source covers torio's tpb-movies + tpb-tv pair: it
/// declares the two groups it can attribute rows to, and filtering
/// *within* a search is not its job.
const TPB_GROUPS: &[Group] = &[Group::Movies, Group::TV];

/// SubsPlease is anime-only by nature.
const SUBSPLEASE_GROUPS: &[Group] = &[Group::Anime];

/// Nyaa is anime's tracker by nature, which is what
/// `groups()` says about the *source*; an all-category query means each
/// row still carries its own group (`nyaa::group_from_category`), and
/// rows nyaa calls Audio/Literature claim none at all.
const NYAA_GROUPS: &[Group] = &[Group::Anime];

/// NNM-Club spans four forums -- the three torio splits (movies, TV, games) plus the anime
/// ones.
const NNMCLUB_GROUPS: &[Group] = &[Group::Movies, Group::TV, Group::Games, Group::Anime];

/// 1337x's site sections that map onto a `Group`, declared when wave 3 landed it as
/// implemented.
const X1337X_GROUPS: &[Group] = &[Group::Movies, Group::TV, Group::Games, Group::Anime];

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
        let items =
            RutrackerSearcher::search_page(self, &req.query, req.offset, req.category).await?;
        // The forum pages `tracker.php?start=` by 50, so a short page is
        let has_more = items.len() >= RutrackerSearcher::PAGE_SIZE;
        Ok(SearchPage {
            items,
            has_more,
            next_offset: None,
        })
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
        // The homepage index answers an empty query with the latest
        true
    }

    async fn ensure_logged_in(&self, _auth: &AuthContext, _log: &LogFn) -> Result<bool> {
        Ok(true)
    }

    async fn search(&self, req: &SearchRequest) -> Result<SearchPage> {
        // Fixed 100-row pages (see `RutorSearcher::PAGE_SIZE`), fanned
        RutorSearcher::search_page(self, &req.query, req.offset, req.category).await
    }

    async fn download_torrent(&self, url: &str) -> Result<Vec<u8>> {
        RutorSearcher::download_torrent(self, url).await
    }
}

/// Metadata-only description of a source, for listing in the Options "Sources" checklist
/// without needing a live, logged-in instance (which requires a running `Browser`).
#[derive(Debug, Clone, Copy)]
pub struct SourceInfo {
    pub id: &'static str,
    /// Human-readable name shown in the UI.
    pub label: &'static str,
    /// `false` for sources reserved for the future (e.g.
    pub implemented: bool,
    pub groups: &'static [Group],
    /// Whether a *selected category* may be asked of this source.
    pub category_filter: bool,
    /// Whether the source can answer an *empty query* -- browse mode the freshest rows it has,
    /// with no search terms.
    pub supports_browse: bool,
    /// Whether using this source needs a browser session launched first (see
    /// `Source::requires_browser`).
    pub requires_browser: bool,
    /// Domain home page, `""` while the source isn't implemented.
    pub home_url: &'static str,
    /// Hosts the browser should not resolve for this source's pages.
    pub block_hosts: &'static [&'static str],
}

/// The full list of sources the app knows about, implemented or not.
pub const KNOWN_SOURCES: &[SourceInfo] = &[
    SourceInfo {
        id: "rutracker",
        label: "Rutracker",
        implemented: true,
        groups: RUTRACKER_GROUPS,
        // Its search form's `f[]` multi-select is a real category slot
        category_filter: true,
        supports_browse: false,
        requires_browser: true,
        block_hosts: &[RUTRACKER_AD_CDN],
        home_url: RutrackerSearcher::HOME_URL,
    },
    SourceInfo {
        id: "rutor",
        label: "Rutor",
        implemented: true,
        groups: RUTOR_GROUPS,
        category_filter: true,
        supports_browse: true,
        requires_browser: false,
        block_hosts: &[],
        home_url: RutorSearcher::HOME_URL,
    },
    SourceInfo {
        id: "yts",
        label: "YTS",
        implemented: true,
        groups: YTS_GROUPS,
        category_filter: true,
        supports_browse: true,
        requires_browser: false,
        block_hosts: &[],
        home_url: YtsSearcher::HOME_URL,
    },
    SourceInfo {
        id: "tpb",
        label: "TPB",
        implemented: true,
        groups: TPB_GROUPS,
        category_filter: true,
        supports_browse: true,
        requires_browser: false,
        block_hosts: &[],
        home_url: TpbSearcher::HOME_URL,
    },
    SourceInfo {
        id: "subsplease",
        label: "SubsPlease",
        implemented: true,
        groups: SUBSPLEASE_GROUPS,
        category_filter: true,
        supports_browse: true,
        requires_browser: false,
        block_hosts: &[],
        home_url: SubsPleaseSearcher::HOME_URL,
    },
    SourceInfo {
        id: "nyaa",
        label: "Nyaa",
        implemented: true,
        groups: NYAA_GROUPS,
        category_filter: true,
        // The empty-query feed was never answered live (B8 wave 2
        supports_browse: false,
        requires_browser: false,
        block_hosts: &[],
        home_url: NyaaSearcher::HOME_URL,
    },
    SourceInfo {
        id: "nnmclub",
        label: "NNM-Club",
        implemented: true,
        groups: NNMCLUB_GROUPS,
        category_filter: true,
        supports_browse: true,
        requires_browser: false,
        block_hosts: &[],
        home_url: NnmclubSearcher::HOME_URL,
    },
    // 1337x's row has been in the registry since before it existed
    SourceInfo {
        id: "1337x",
        label: "1337x",
        implemented: true,
        groups: X1337X_GROUPS,
        category_filter: true,
        // `/home/` answers an empty query with the same row markup as
        supports_browse: true,
        requires_browser: false,
        block_hosts: &[],
        home_url: X1337xSearcher::HOME_URL,
    },
    // The last planned id, listed before it exists for the same reason
    SourceInfo {
        id: "torentino",
        label: "Torentino",
        implemented: true,
        groups: TRENTINO_GROUPS,
        category_filter: true,
        // Search is a POST, and the.torrent link lives on the item page
        supports_browse: false,
        requires_browser: false,
        block_hosts: &[],
        home_url: super::torentino::HOME_URL,
    },
];

pub struct SourceEnv {
    /// An already-launched browser session.
    pub browser: Option<Arc<Mutex<Browser>>>,
}

/// Build the live instance for `id`: the one place that maps ids to concrete types, so
/// `app.rs`/`main.rs` only ever handle `Arc<dyn Source>` (B2 closes Phase 3's "rewire app.rs"
/// note).
pub fn build_source(id: &str, env: SourceEnv) -> Result<Arc<dyn Source>> {
    match id {
        "rutracker" => {
            let browser = env
                .browser
                .ok_or_else(|| anyhow!("source '{}' needs a running browser session", id))?;
            Ok(Arc::new(RutrackerSearcher::new(browser)))
        }
        "rutor" => Ok(Arc::new(RutorSearcher::new())),
        "yts" => Ok(Arc::new(YtsSearcher::new())),
        "tpb" => Ok(Arc::new(TpbSearcher::new())),
        "subsplease" => Ok(Arc::new(SubsPleaseSearcher::new())),
        "nyaa" => Ok(Arc::new(NyaaSearcher::new())),
        "nnmclub" => Ok(Arc::new(NnmclubSearcher::new())),
        "1337x" => Ok(Arc::new(X1337xSearcher::new())),
        "torentino" => Ok(Arc::new(TorentinoSearcher::new())),
        other => Err(anyhow!("unknown source '{}'", other)),
    }
}

/// Registry lookup by id -- the metadata-only twin of building a live
/// [`Source`] (`build_source`, used by the orchestrator).
pub fn get_source(id: &str) -> Option<&'static SourceInfo> {
    KNOWN_SOURCES.iter().find(|s| s.id == id)
}

/// Whether orchestrating `id` needs a browser session launched first.
pub fn requires_browser(id: &str) -> bool {
    get_source(id).map(|s| s.requires_browser).unwrap_or(true)
}

/// The sources a CLI run asks: `--source <id>` names exactly one --
/// refusing an unknown or still-planned id rather than silently falling
/// back to a default -- and otherwise every enabled implemented source,
/// which is the same list `orchestrator::selected_sources` builds for
/// the `all` tab, so the CLI and the TUI cannot disagree about what
/// "all sources" means.
pub fn cli_sources(
    requested: Option<&str>,
    enabled: &[String],
) -> Result<Vec<&'static SourceInfo>> {
    match requested {
        Some(id) => {
            let info = get_source(id).ok_or_else(|| anyhow::anyhow!("unknown source '{}'", id))?;
            if !info.implemented {
                anyhow::bail!("source '{}' is not implemented yet", id);
            }
            Ok(vec![info])
        }
        // No `all` tab any more: the panel's checkboxes are the
        None => Ok(crate::sources::orchestrator::selected_sources(
            enabled, None, false,
        )),
    }
}

/// Source ids a config written before B8 wave 1 could possibly mention: exactly what
/// [`KNOWN_SOURCES`] held at `9d5ae14`, the last commit before wave 1 added yts.
const LEGACY_SOURCES: &[&str] = &["rutracker", "rutor", "nnmclub"];

/// "Every implemented source ships turned on" -- stated against the
/// registry rather than as a second handwritten list, so a source can
/// only be left out of the defaults by not being implemented.
fn default_enabled_sources() -> Vec<String> {
    KNOWN_SOURCES
        .iter()
        .filter(|s| s.implemented)
        .map(|s| s.id.to_string())
        .collect()
}

/// Give a config the source ids it has never heard of.
pub fn migrate_config(config: &mut crate::config::Config) {
    let seen: Vec<String> = if config.known_sources.is_empty() {
        LEGACY_SOURCES.iter().map(|s| s.to_string()).collect()
    } else {
        config.known_sources.clone()
    };
    // Drop the ids the registry lists but has not built: today
    let known: Vec<String> = seen
        .iter()
        .filter(|id| {
            !KNOWN_SOURCES
                .iter()
                .any(|info| info.id == **id && !info.implemented)
        })
        .cloned()
        .collect();

    // A source this build no longer has leaves the list it was checked in.
    //
    // EZTV is the first: its API has no search, so every query against it was
    // an error, and a config that still listed it kept listing it -- the
    // checkbox list filters by the registry, so the id sat there invisible and
    // stayed there across every run. Dropping it here is the same place that
    // already drops ids the registry has but has not built.
    let registered = |id: &String| KNOWN_SOURCES.iter().any(|info| info.id == *id);
    config.enabled_sources.retain(|id| registered(id));

    for id in default_enabled_sources() {
        let known_before = known.iter().any(|k| k == &id);
        let already_on = config.enabled_sources.iter().any(|e| e == &id);
        if !known_before && !already_on {
            config.enabled_sources.push(id);
        }
    }

    // Record every id this build knows *as something that could be
    let mut all: Vec<String> = KNOWN_SOURCES
        .iter()
        .filter(|info| info.implemented)
        .map(|info| info.id.to_string())
        .collect();
    for id in known {
        if !all.iter().any(|a| a == &id) {
            all.push(id);
        }
    }
    config.known_sources = all;
}

/// Every id this build implements, in registry order.
pub fn first_run_config(config: &mut crate::config::Config) {
    let ids = implemented_source_ids();
    config.known_sources = ids.clone();
    config.enabled_sources = ids;
}

/// The config a machine with no config file starts with.
pub fn first_run() -> crate::config::Config {
    let mut config = crate::config::Config::default();
    first_run_config(&mut config);
    config
}

pub fn implemented_source_ids() -> Vec<String> {
    KNOWN_SOURCES
        .iter()
        .filter(|s| s.implemented)
        .map(|s| s.id.to_string())
        .collect()
}
