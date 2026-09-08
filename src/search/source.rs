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
//! Note: `app.rs`/`main.rs` still call `RutrackerSearcher` directly today
//! (that rewiring is a follow-up once this trait has been built by the
//! project's own toolchain and confirmed to compile) — this module is
//! additive and does not change any existing call path.

use anyhow::Result;
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;

use super::models::TorrentItem;
use super::rutracker::RutrackerSearcher;

/// One pluggable content source. Everything the orchestrator, the browser
/// layer, and the Options "Sources" checklist need from a source goes
/// through here.
#[async_trait]
pub trait Source: Send + Sync {
    /// Stable lowercase identifier, e.g. `"rutracker"`. Used as the
    /// credentials-store key and the Options "Sources" checklist key.
    fn id(&self) -> &'static str;

    /// Human-readable name shown in the UI.
    fn display_name(&self) -> &'static str;

    /// A page on this source's domain. Used as the navigation target for
    /// cookie injection when the browser runs hidden — see
    /// `browser::cdp::Browser::launch`.
    fn home_url(&self) -> &'static str;

    async fn ensure_logged_in(
        &mut self,
        cookie_file: Option<&Path>,
        username: Option<&str>,
        password: Option<&str>,
        log: Arc<dyn Fn(&str) + Send + Sync>,
    ) -> Result<bool>;

    async fn search(&self, query: &str) -> Result<Vec<TorrentItem>>;
    async fn search_page(&self, query: &str, start: usize) -> Result<Vec<TorrentItem>>;
    async fn download_torrent(&self, url: &str) -> Result<Vec<u8>>;
}

#[async_trait]
impl Source for RutrackerSearcher {
    fn id(&self) -> &'static str {
        "rutracker"
    }

    fn display_name(&self) -> &'static str {
        "Rutracker"
    }

    fn home_url(&self) -> &'static str {
        Self::HOME_URL
    }

    async fn ensure_logged_in(
        &mut self,
        cookie_file: Option<&Path>,
        username: Option<&str>,
        password: Option<&str>,
        log: Arc<dyn Fn(&str) + Send + Sync>,
    ) -> Result<bool> {
        RutrackerSearcher::ensure_logged_in(self, cookie_file, username, password, log).await
    }

    async fn search(&self, query: &str) -> Result<Vec<TorrentItem>> {
        RutrackerSearcher::search(self, query).await
    }

    async fn search_page(&self, query: &str, start: usize) -> Result<Vec<TorrentItem>> {
        RutrackerSearcher::search_page(self, query, start).await
    }

    async fn download_torrent(&self, url: &str) -> Result<Vec<u8>> {
        RutrackerSearcher::download_torrent(self, url).await
    }
}

/// Metadata-only description of a source, for listing in the Options
/// "Sources" checklist without needing a live, logged-in instance (which
/// requires a running `Browser`). Real `Source` instances are constructed
/// lazily by the orchestrator only when a source is actually used.
#[derive(Debug, Clone, Copy)]
pub struct SourceInfo {
    pub id: &'static str,
    pub display_name: &'static str,
    /// `false` for sources reserved for the future (e.g. rutor, nnm-club)
    /// so Options can list them as coming-soon rather than hide them.
    pub implemented: bool,
}

/// The full list of sources the app knows about, implemented or not. This
/// is the single place to touch when adding a new source's *listing*;
/// implementing [`Source`] for it is the separate step that makes
/// `implemented` become `true`.
pub const KNOWN_SOURCES: &[SourceInfo] = &[
    SourceInfo { id: "rutracker", display_name: "Rutracker", implemented: true },
    SourceInfo { id: "rutor", display_name: "Rutor", implemented: false },
    SourceInfo { id: "nnmclub", display_name: "NNM-Club", implemented: false },
];
