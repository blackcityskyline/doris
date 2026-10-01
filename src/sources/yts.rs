//! YTS's JSON API ported from torio's `yts.ts`. Why it is the first wave-1 source to land: it
//! is pure JSON over plain HTTP -- no browser, no login, no HTML -- and it is the source that
//! needs [`first_ok`] ( deferred failover helper), because a list of mirror hosts is the only
//! way to stay up when one of them moves, dies or starts rate-limiting.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::Deserialize;

use super::format::{format_bytes, format_date};
use super::magnet::build_magnet;
use super::models::TorrentItem;
use super::net::{browser_client, fetch_resilient, first_ok, FetchOptions};
use super::source::{AuthContext, Group, LogFn, SearchPage, SearchRequest, Source};

/// Mirror hosts, first live-verified answer first (see the module doc).
pub const HOSTS: [&str; 4] = ["yts.gg", "movies-api.accel.li", "yts.am", "yts.mx"];

/// The API's `limit`: movies per page.
pub const PAGE_SIZE: usize = 50;

/// One YTS movie.
#[derive(Debug, Deserialize)]
struct YtsMovie {
    title: Option<String>,
    title_long: Option<String>,
    url: Option<String>,
    date_uploaded_unix: Option<i64>,
    torrents: Option<Vec<YtsTorrent>>,
}

#[derive(Debug, Deserialize)]
struct YtsTorrent {
    /// Live responses carry this UPPERCASE; we lowercase it ourselves
    /// because `TorrentItem::info_hash` promises hex lowercase.
    hash: Option<String>,
    quality: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    size_bytes: Option<u64>,
    seeds: Option<u32>,
    peers: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct YtsData {
    movie_count: Option<i64>,
    movies: Option<Vec<YtsMovie>>,
}

#[derive(Debug, Deserialize)]
struct YtsResponse {
    status: Option<String>,
    status_message: Option<String>,
    data: Option<YtsData>,
}

/// The search/browse URL for one host.
pub fn list_movies_url(base: &str, query: &str, offset: usize) -> String {
    let mut url = format!(
        "https://{}/api/v2/list_movies.json?limit={}&page_number={}",
        base,
        PAGE_SIZE,
        offset + 1
    );
    let trimmed = query.trim();
    if trimmed.is_empty() {
        url.push_str("&sort_by=date_added");
    } else {
        url.push_str(&format!("&query_term={}", urlencoding::encode(trimmed)));
    }
    url
}

/// One API page -> one [`SearchPage`].
pub fn parse_page(body: &str, offset: usize) -> Result<SearchPage> {
    let parsed: YtsResponse =
        serde_json::from_str(body).map_err(|e| anyhow!("YTS response did not parse: {}", e))?;
    if let Some(status) = parsed.status.as_deref() {
        if status != "ok" {
            // Surfacing it (instead of rendering an empty page) is what
            // lets `first_ok` move on to the next mirror.
            anyhow::bail!(
                "YTS: {}",
                parsed.status_message.unwrap_or_else(|| status.to_string())
            );
        }
    }

    let movie_count = parsed
        .data
        .as_ref()
        .and_then(|d| d.movie_count)
        .unwrap_or(0);
    let movies = parsed.data.and_then(|d| d.movies).unwrap_or_default();

    let mut items = Vec::new();
    for movie in &movies {
        let base = movie
            .title_long
            .as_deref()
            .or(movie.title.as_deref())
            .unwrap_or("Unknown");
        let added = movie.date_uploaded_unix.unwrap_or(0);
        let date = format_date(added);
        for torrent in movie.torrents.iter().flatten() {
            // A torrent without a hash is unusable: no magnet can be
            // built and no hash can be deduped (torio skips these too).
            let Some(hash) = torrent.hash.as_deref().filter(|h| !h.is_empty()) else {
                continue;
            };
            let info_hash = hash.to_lowercase();
            // "720p web" / "1080p bluray", exactly torio's join of the
            // two tags -- the same movie in two qualities is two rows
            // and must say which is which.
            let tag: Vec<&str> = [torrent.quality.as_deref(), torrent.kind.as_deref()]
                .into_iter()
                .flatten()
                .collect();
            let title = if tag.is_empty() {
                base.to_string()
            } else {
                format!("{} [{}]", base, tag.join(" "))
            };
            let size_bytes = torrent.size_bytes.unwrap_or(0);
            let seeds = torrent.seeds.unwrap_or(0);
            items.push(TorrentItem {
                magnet: Some(build_magnet(&info_hash, &title)),
                title: title.clone(),
                size: format_bytes(size_bytes),
                seeds: seeds.to_string(),
                size_bytes,
                seeds_n: seeds,
                leechers: torrent.peers.unwrap_or(0),
                added,
                date: date.clone(),
                info_hash,
                page_url: movie.url.clone().unwrap_or_default(),
                source: "yts".to_string(),
                group: Some(Group::Movies),
                ..Default::default()
            });
        }
    }

    // `movie_count` is the *total* for the query, so the verdict is
    // "are there movies left beyond this page?" -- in movies, not rows.
    let loaded = (offset + 1) as i64 * PAGE_SIZE as i64;
    let has_more = loaded < movie_count;
    Ok(SearchPage {
        items,
        has_more,
        // The API's own unit, which is the whole point: rows this page
        // yielded depend on how many qualities each movie has, so
        // `offset + rows` would skip or repeat movies (B8 part B).
        next_offset: Some(offset + 1),
    })
}

pub struct YtsSearcher {
    client: reqwest::Client,
}

impl Default for YtsSearcher {
    fn default() -> Self {
        Self::new()
    }
}

impl YtsSearcher {
    pub const HOME_URL: &str = "https://yts.gg";

    pub fn new() -> Self {
        Self {
            // The shared browser-like client: these hosts sit
            // behind Cloudflare like rutor's do.
            client: browser_client(),
        }
    }
}

#[async_trait]
impl Source for YtsSearcher {
    fn id(&self) -> &'static str {
        "yts"
    }

    fn label(&self) -> &'static str {
        "YTS"
    }

    fn groups(&self) -> &'static [Group] {
        &[Group::Movies]
    }

    fn home_url(&self) -> &'static str {
        Self::HOME_URL
    }

    fn requires_browser(&self) -> bool {
        false
    }

    fn supports_browse(&self) -> bool {
        true
    }

    async fn ensure_logged_in(&self, _auth: &AuthContext, _log: &LogFn) -> Result<bool> {
        Ok(true)
    }

    async fn search(&self, req: &SearchRequest) -> Result<SearchPage> {
        let query = req.query.clone();
        let offset = req.offset;
        let client = self.client.clone();
        first_ok(&HOSTS, move |host| {
            let host = host.to_string();
            let client = client.clone();
            let query = query.clone();
            async move {
                // torio retries each host once before moving on; so do
                // we, because with three more mirrors behind it a slow
                // retry is worse than the next host.
                let options = FetchOptions {
                    retries: 1,
                    ..FetchOptions::default()
                };
                let url = list_movies_url(&host, &query, offset);
                let response = fetch_resilient(&url, || client.get(&url), &options).await?;
                // `fetch_resilient` hands back non-retryable statuses as-is
                // (a 404 is not worth retrying) -- so "did this host work"
                // is decided here, and a "no" moves to the next one.
                anyhow::ensure!(
                    response.status().is_success(),
                    "YTS at {} returned {}",
                    host,
                    response.status()
                );
                let body = response.text().await?;
                parse_page(&body, offset)
            }
        })
        .await
    }

    async fn download_torrent(&self, _url: &str) -> Result<Vec<u8>> {
        // YTS publishes magnets, not files: the download key writes a
        // `.magnet` file for these rows, and streaming goes through
        // `add_by_link`. What can land here is that path's
        // fallback, so say what happened instead of returning junk.
        anyhow::bail!(
            "YTS rows carry a magnet, not a .torrent file -- stream it, \
             or save the link as a .magnet file"
        )
    }
}
