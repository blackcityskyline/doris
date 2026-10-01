//! `Source` is the seam the whole app is meant to depend on instead of
//! reaching into `rutracker.rs` by name. Adding a new content source is
//! meant to be:
//!
//! 1. Write `src/sources/<name>.rs` implementing [`Source`].
//! 2. Add one entry to [`KNOWN_SOURCES`].
//!
//! Nothing else in the orchestrator, browser layer, or Options UI should
//! need to change. This file is intentionally the *only* place that knows
//! the concrete list of sources.
//!
//! (Phase 3's original note about rewiring `app.rs`/`main.rs` onto this
//! trait is what the registry is for.)

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;

use crate::browser::cdp::Browser;
use tokio::sync::Mutex;

use super::eztv::EztvSearcher;
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

/// Content categories a source can attribute its results to. Declared
/// here, next to the registry it describes (and not in `models.rs`) so
/// `TorrentItem.group` is typed against the same enum the `Source` trait
/// hands out.
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

/// The `f[]` forum selector one DLE tracker's search form posts:
/// `f%5B%5D=<id>` repeated for every forum of the chosen group, or
/// `all_forums` when the group has none (and rutracker passes `""`,
/// which is its own "no filter").
///
/// It is one function because two trackers built it the same way, down
/// to the url-encoding, and a third that got it subtly different would
/// be invisible until its results came back filtered by the wrong forum.
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
/// the "all" tab: the same order B6's tab row was decided in, kept here
/// so the visible order is one recorded decision instead of an accident
/// of where a variant happened to be typed above.
pub const GROUP_ORDER: [Group; 4] = [Group::Movies, Group::TV, Group::Games, Group::Anime];

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
        Self {
            query: query.into(),
            offset,
            category: None,
        }
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
    /// (browse mode -- the `b` key). A source that has a fresh-releases
    /// page answers; one that only accepts search terms does not, and the
    /// Browse key is then answered by the sources that do.
    fn supports_browse(&self) -> bool;

    /// Establish (or verify) a session, reusing cached state when the
    /// source already has one. Takes `&self` because a registry hands
    /// out `Arc<dyn Source>` with no `&mut` to give; the mutable session
    /// flag lives behind interior mutability.
    async fn ensure_logged_in(&self, auth: &AuthContext, log: &LogFn) -> Result<bool>;

    async fn search(&self, req: &SearchRequest) -> Result<SearchPage>;
    async fn download_torrent(&self, url: &str) -> Result<Vec<u8>>;

    /// The magnet link that lives on the row's *own* page, fetched when
    /// a row arrives with neither a magnet nor a `.torrent` link.
    ///
    /// Most sources fill `magnet`/`download_url` while parsing the
    /// results page and never need this. An aggregator that links to
    /// torrent pages instead of serving files does (1337x, B8 wave 3):
    /// its rows carry the page URL, and the link is one request away.
    /// The default answers "no such link" without touching the network,
    /// so the other six sources keep their shape, and the caller pays
    /// the request only for a row it is about to play -- never per row
    /// of a search.
    async fn resolve_magnet(&self, _page_url: &str) -> Result<Option<String>> {
        Ok(None)
    }

    /// The files inside a torrent, read from the row's own page.
    ///
    /// The default answers "this source cannot list files" with an
    /// empty list rather than an error: the modal is opened on demand,
    /// so a source with nothing to add should leave the row's own facts
    /// on screen, not fail the modal. A source that can list files
    /// overrides this the same way `resolve_magnet` is overridden --
    /// one method with a default, so no source has to change and the
    /// orchestrator does not know the method exists.
    async fn details(&self, _page_url: &str) -> Result<Vec<FileEntry>> {
        Ok(Vec::new())
    }
}

/// Rutracker's search form posts a real category slot -- `f[]` with forum
/// ids, live-verified 26.09.2026 -- so it declares the four groups it can
/// filter, and `rutracker::GROUP_FORUMS` maps them onto forum ids.
const RUTRACKER_GROUPS: &[Group] = &[Group::Games, Group::Movies, Group::TV, Group::Anime];

/// Rutracker's ad CDN. It serves the looping `<video>`/GIF banners that
/// keep the compositor busy for as long as a page stays open -- measured
/// as the largest idle-CPU cost of a session, VizCompositor near 66% of a
/// core. No parsing depends on ad creatives, so the browser is told not to
/// resolve it; drop this entry if a page ever legitimately needs it.
const RUTRACKER_AD_CDN: &str = "rutrk.org";

/// Torentino is a games tracker, top to bottom, so it declares the
/// one group its rows can claim (B8 wave 3; playback is B7's
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

/// One apibay source covers torio's tpb-movies + tpb-tv pair (B8 wave 1
/// decision): it declares the two groups it can attribute rows to, and
/// filtering *within* a search is B6's job.
const TPB_GROUPS: &[Group] = &[Group::Movies, Group::TV];

/// SubsPlease is anime-only by nature (B8 wave 1).
const SUBSPLEASE_GROUPS: &[Group] = &[Group::Anime];

/// Nyaa is anime's tracker by nature (B8 wave 2), which is what
/// `groups()` says about the *source*; an all-category query means each
/// row still carries its own group (`nyaa::group_from_category`), and
/// rows nyaa calls Audio/Literature claim none at all.
const NYAA_GROUPS: &[Group] = &[Group::Anime];

/// NNM-Club spans four forums -- the three torio splits (movies, TV,
/// games) plus the anime ones (B8 wave 3). Since B6 each row claims the
/// group of its own forum (`nnmclub::group_for_forum`), and the same
/// four groups are what `nnmclub::GROUP_FORUMS` asks the tracker for;
/// a test keeps the two declarations equal.
const NNMCLUB_GROUPS: &[Group] = &[Group::Movies, Group::TV, Group::Games, Group::Anime];

/// EZTV is TV-only, and its rows say `Group::TV` to match (B8 wave 1).
const EZTV_GROUPS: &[Group] = &[Group::TV];

/// 1337x's site sections that map onto a `Group`, declared when wave 3
/// landed it as implemented (B8 wave 3). Music, Documentaries,
/// Applications, Other and XXX map onto none and are queried without a
/// group; rows claim none of them either -- see `x1337x`'s module doc.
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
        // the last one and a full one may have more behind it.
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
        // releases (B9) -- see `rutor::BROWSE_URL`.
        true
    }

    async fn ensure_logged_in(&self, _auth: &AuthContext, _log: &LogFn) -> Result<bool> {
        Ok(true)
    }

    async fn search(&self, req: &SearchRequest) -> Result<SearchPage> {
        // Fixed 100-row pages (see `RutorSearcher::PAGE_SIZE`), fanned
        // out over the selected category's rubric ids when B6's
        // `category` says so -- `rutor::to_page` reads `has_more` off
        // each id's own page and steps the cursor by one page.
        RutorSearcher::search_page(self, &req.query, req.offset, req.category).await
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
    /// Whether a *selected category* may be asked of this source. `true`
    /// means every row it would return belongs to that category -- by
    /// filtering server-side (`SearchRequest.category`), or because the
    /// source only has that one group to begin with. `false` means it
    /// would answer with rows the view has to drop, so
    /// `orchestrator::selected_sources` leaves it out of a category
    /// search instead of asking and discarding (B6, decided with the
    /// user: a source whose category slot is unverified is not asked).
    /// A `false` on an implemented source needs its reason next to it
    /// in the entry below -- `source_registry_tests` checks that.
    pub category_filter: bool,
    /// Whether the source can answer an *empty query* -- browse mode
    /// (B9): the freshest rows it has, with no search terms. `false`
    /// means an empty query would come back as a broken page rather
    /// than as a list, so `selected_sources` leaves it out of a browse.
    pub supports_browse: bool,
    /// Whether using this source needs a browser session launched first
    /// (see `Source::requires_browser`). `false` for planned sources:
    /// nothing constructs them yet, so nothing may promise a browser.
    pub requires_browser: bool,
    /// Domain home page, `""` while the source isn't implemented. The
    /// orchestrator passes it to `Browser::launch` before the `Source`
    /// instance itself exists (the instance is what *needs* the browser,
    /// so it can't supply its own home page).
    pub home_url: &'static str,
    /// Hosts the browser should not resolve for this source's pages.
    ///
    /// A fact about the site, so it lives with the site: an ad CDN serving
    /// looping video keeps the compositor producing frames at full speed
    /// for as long as a page stays open, which was measured as the largest
    /// idle-CPU cost of a session. It used to be one hardcoded host in
    /// `browser::cdp`, which put one tracker's ad server in the module
    /// that drives a browser for every source.
    pub block_hosts: &'static [&'static str],
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
        // Its search form's `f[]` multi-select is a real category slot
        // (verified live 26.09.2026: two different forum ids answer
        // disjoint topic sets), so a selected category reaches it and the
        // rows claim it back -- see `rutracker::GROUP_FORUMS`.
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
        // decision), so browse is not claimed until it is.
        supports_browse: false,
        requires_browser: false,
        block_hosts: &[],
        home_url: NyaaSearcher::HOME_URL,
    },
    SourceInfo {
        id: "eztv",
        label: "EZTV",
        implemented: true,
        groups: EZTV_GROUPS,
        category_filter: true,
        supports_browse: true,
        requires_browser: false,
        block_hosts: &[],
        home_url: EztvSearcher::HOME_URL,
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
    // (an id the registry knows about is an id the migration, the
    // Options rows and the tab bar all agree on) and wave 3 filled it
    // in: `implemented`, four groups, and `requires_browser: false`.
    //
    // That flag is a claim about the *host*, and this one was wrong
    // for a while because only half the mirrors had been probed. The
    // probes of 25.09.2026, same browser UA, came back split: three
    // of torio's four hosts answer 403 with a Cloudflare JS challenge
    // to a plain client, while `1337xx.to` 301s to
    // `www.1337xx.to` and answers 200 on every path checked. The
    // challenge is those mirrors' business, not a session this source
    // is missing -- `x1337x`'s module doc keeps the evidence, and its
    // `HOSTS` const keeps the answering mirror first.
    SourceInfo {
        id: "1337x",
        label: "1337x",
        implemented: true,
        groups: X1337X_GROUPS,
        category_filter: true,
        // `/home/` answers an empty query with the same row markup as
        // search (live: 78 rows, one page).
        supports_browse: true,
        requires_browser: false,
        block_hosts: &[],
        home_url: X1337xSearcher::HOME_URL,
    },
    // The last planned id, listed before it exists for the same reason
    // the others were: `groups` stays empty -- a group is a claim
    // about rows nobody has parsed yet -- and its `requires_browser`
    // is the honest `false` the probe of 25.09.2026 showed (200 with
    // its front page), which is also what the fallback returns for an
    // unknown id on the *other* side of the question.
    SourceInfo {
        id: "torentino",
        label: "Torentino",
        implemented: true,
        groups: TRENTINO_GROUPS,
        category_filter: true,
        // Search is a POST, and the .torrent link lives on the item page
        // (live 26.09.2026), so `download_torrent` fetches it there --
        // no bencode crate needed for playback. Browse is not claimed:
        // no freshest-first feed has ever been verified on this host.
        supports_browse: false,
        requires_browser: false,
        block_hosts: &[],
        home_url: super::torentino::HOME_URL,
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
        "eztv" => Ok(Arc::new(EztvSearcher::new())),
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
/// Unknown ids fall back to `true`: the conservative answer, because
/// guessing "no browser" for a source we don't know about would send its
/// login through a path that can't reach a browser (same fallback
/// `app.rs::source_needs_browser` has always had).
pub fn requires_browser(id: &str) -> bool {
    get_source(id).map(|s| s.requires_browser).unwrap_or(true)
}

/// The sources a CLI run asks (B9): `--source <id>` names exactly one --
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
        // No `all` tab any more (П.4): the panel's checkboxes are the
        // selection, so "everything" is simply every checked source.
        None => Ok(crate::sources::orchestrator::selected_sources(
            enabled, None, false,
        )),
    }
}

/// Source ids a config written before B8 wave 1 could possibly mention:
/// exactly what [`KNOWN_SOURCES`] held at `9d5ae14`, the last commit
/// before wave 1 added yts. Seeds `known_sources` for a config that
/// predates the field.
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
///
/// It lives here and not on `Config` because the source list is a fact
/// about the build, not a setting; `config.rs` knowing it meant a source
/// had to be added to two files and forgetting the second left it
/// implemented but switched off.
///
/// "Never heard of" is `Config::known_sources`: an id the config has
/// already seen is not re-added, and one predating the field is
/// recognised by being empty and seeded with `LEGACY_SOURCES` -- so
/// somebody who disabled `rutor` back then keeps it off while `tpb`,
/// which they have never seen, arrives enabled. A *planned* source
/// counts as never heard of: its Options row is a caption, not a toggle,
/// so it was never something to accept or reject.
pub fn migrate_config(config: &mut crate::config::Config) {
    let seen: Vec<String> = if config.known_sources.is_empty() {
        LEGACY_SOURCES.iter().map(|s| s.to_string()).collect()
    } else {
        config.known_sources.clone()
    };
    // Drop the ids the registry lists but has not built: today
    // those rows cannot be toggled, so nothing was ever decided
    // about them. What is left -- implemented ids plus ids this
    // registry does not list at all -- is what counts as known.
    let known: Vec<String> = seen
        .iter()
        .filter(|id| {
            !KNOWN_SOURCES
                .iter()
                .any(|info| info.id == **id && !info.implemented)
        })
        .cloned()
        .collect();

    for id in default_enabled_sources() {
        let known_before = known.iter().any(|k| k == &id);
        let already_on = config.enabled_sources.iter().any(|e| e == &id);
        if !known_before && !already_on {
            config.enabled_sources.push(id);
        }
    }

    // Record every id this build knows *as something that could be
    // decided on* -- implemented ids only, by the same rule as
    // above, so a placeholder row never counts as the user having
    // seen it. A source added to the registry later is then unknown
    // again, which is what makes the *next* migration happen
    // without anyone extending a baseline; ids the registry no
    // longer lists are kept, since a config that knew them did not
    // stop knowing them.
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
///
/// The first-run defaults. Exposed because "no config file" and "an
/// empty config file" are different situations that happen to look alike
/// from inside `Config`: one is a machine that has decided nothing, the
/// other is a file from before the field existed.
pub fn first_run_config(config: &mut crate::config::Config) {
    let ids = implemented_source_ids();
    config.known_sources = ids.clone();
    config.enabled_sources = ids;
}

/// The config a machine with no config file starts with.
///
/// Used by `load` and by the UI's own construction, which both need the
/// same thing: a config that has the source list filled in. `Config::default()`
/// on its own is not that -- it is the raw struct, which deliberately
/// carries no opinion about which sources exist.
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
