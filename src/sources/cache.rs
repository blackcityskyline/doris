//! TTL cache for search pages, ported from torio's `sources/cache.ts`. What it is for:
//! re-running the same query (a fresh search, going back to a previous page, the extension
//! bridge asking again) should not pay for the network a second time while the answer is at
//! most minutes old.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::source::SearchPage;

/// torio's `TTL_MS = 5 * 60 * 1000`.
pub const TTL: Duration = Duration::from_secs(5 * 60);

/// What a cached page is: which source, what was asked, in which category, from which page
/// cursor.
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

    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    /// A copy of the page stored under `key`, or `None` when it is missing or older than the
    /// TTL (an expired entry is dropped rather than left to linger).
    pub fn get(&self, key: &CacheKey) -> Option<SearchPage> {
        let mut map = self.entries.lock().ok()?;
        let expired = map.get(key).map(|entry| entry.at.elapsed() >= self.ttl)?;
        if expired {
            map.remove(key);
            return None;
        }
        map.get(key).map(|entry| entry.page.clone())
    }

    pub fn put(&self, key: CacheKey, page: SearchPage) {
        if let Ok(mut map) = self.entries.lock() {
            map.retain(|_, entry| entry.at.elapsed() < self.ttl);
            map.insert(
                key,
                Entry {
                    at: Instant::now(),
                    page,
                },
            );
        }
    }
}
