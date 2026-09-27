//! TTL cache for search pages (ROADMAP.md B5), ported from torio's
//! `sources/cache.ts`.
//!
//! What it is for: re-running the same query (a fresh search, going back
//! to a previous page, the extension bridge asking again) should not pay
//! for the network a second time while the answer is at most minutes
//! old. What it deliberately does *not* do: cache failures. A source
//! that errored has nothing to store, and pinning its emptiness for
//! five minutes would turn one bad request into five minutes of "no
//! results".
//!
//! Concurrency is a plain `std::sync::Mutex` held only for map access --
//! the work between lock and unlock is a `Vec` clone, so there is no
//! async context to keep the guard across and nothing to poison a
//! runtime with.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::source::SearchPage;

/// torio's `TTL_MS = 5 * 60 * 1000`.
pub const TTL: Duration = Duration::from_secs(5 * 60);

/// What a cached page is: which source, what was asked, in which
/// category, from which page cursor.
///
/// The query is normalized (trimmed + lowercased) exactly like torio's
/// `key()` -- `"Matrix "` and `"matrix"` are the same search. `offset`
/// is in the key by decision: caching only the first page made
/// "search again, then Load more" asymmetric, with page one served from
/// memory and page two always going to the network.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub source: String,
    pub query: String,
    pub category: Option<String>,
    pub offset: usize,
}

impl CacheKey {
    pub fn new(source: &str, query: &str, category: Option<&str>, offset: usize) -> Self {
        Self {
            source: source.to_string(),
            query: query.trim().to_lowercase(),
            category: category.map(str::to_string),
            offset,
        }
    }
}

struct Entry {
    at: Instant,
    page: SearchPage,
}

/// The cache itself. `Default` gives it the production [`TTL`];
/// `with_ttl` exists so tests can watch an entry expire without waiting
/// five minutes.
#[derive(Default)]
pub struct SearchCache {
    entries: Mutex<HashMap<CacheKey, Entry>>,
    ttl: Duration,
}

impl SearchCache {
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            ttl: TTL,
        }
    }

    /// Cache with a custom expiry, for tests that need an entry to age.
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    /// A copy of the page stored under `key`, or `None` when it is
    /// missing or older than the TTL (an expired entry is dropped rather
    /// than left to linger). A poisoned lock is treated as a miss: the
    /// cache is an optimization, never a reason to fail a search.
    pub fn get(&self, key: &CacheKey) -> Option<SearchPage> {
        let mut map = self.entries.lock().ok()?;
        let expired = map.get(key).map(|entry| entry.at.elapsed() >= self.ttl)?;
        if expired {
            map.remove(key);
            return None;
        }
        map.get(key).map(|entry| entry.page.clone())
    }

    /// Store a successfully fetched page. Expired entries are swept out
    /// while we hold the lock anyway -- torio leaves them forever, which
    /// grows its map for the lifetime of the tab.
    pub fn put(&self, key: CacheKey, page: SearchPage) {
        if let Ok(mut map) = self.entries.lock() {
            map.retain(|_, entry| entry.at.elapsed() < self.ttl);
            map.insert(key, Entry { at: Instant::now(), page });
        }
    }
}
