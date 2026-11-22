//! TTL cache for search pages, ported from torio's `sources/cache.ts`. What it is for:
//! re-running the same query (a fresh search, going back to a previous page, the extension
//! bridge asking again) should not pay for the network a second time while the answer is at
//! most minutes old.
//!
//! It lives on disk (`search-cache.json` in the state dir) because the cache that helps
//! anyone is the one that outlives the process that filled it: the extension is a short
//! process, so a memory-only cache answered nothing there.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::source::SearchPage;

/// torio's `TTL_MS = 5 * 60 * 1000`.
pub const TTL: Duration = Duration::from_secs(5 * 60);

/// What a cached page is: which source, what was asked, in which category, from which page
/// cursor.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    at: SystemTime,
    page: SearchPage,
}

/// One row of the on-disk file: a page and when it was fetched. Seconds since
/// the Unix epoch, so the file means the same thing read back after a minute
/// or from another process.
#[derive(Serialize, Deserialize)]
struct PersistedEntry {
    page: SearchPage,
    at: u64,
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

    /// The file every doris process reads and writes, so a repeated search
    /// answered by it costs nothing in the network, however it was started.
    pub fn default_path() -> PathBuf {
        crate::log::state_dir().join("search-cache.json")
    }

    /// The file the cache was written to, restored after this process started.
    /// Anything unreadable reads as an empty cache: a cache is worth more for
    /// being small than for being trusted.
    pub fn load_or_new() -> Self {
        Self::load_from(&Self::default_path())
    }

    pub fn load_from(path: &Path) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::new();
        };
        let Ok(file) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&text)
        else {
            return Self::new();
        };
        let mut entries = HashMap::new();
        for (key_text, value) in file {
            let (Ok(key), Ok(entry)) = (
                serde_json::from_str::<CacheKey>(&key_text),
                serde_json::from_value::<PersistedEntry>(value),
            ) else {
                continue;
            };
            entries.insert(
                key,
                Entry {
                    at: UNIX_EPOCH + Duration::from_secs(entry.at),
                    page: entry.page,
                },
            );
        }
        Self {
            entries: Mutex::new(entries),
            ttl: TTL,
        }
    }

    /// Write the whole cache: what is left after expiry prunes, so the file
    /// does not grow for a month of nightly searches.
    pub fn save_to(&self, path: &Path) {
        let Ok(mut map) = self.entries.lock() else {
            return;
        };
        map.retain(|_, entry| !entry.expired(self.ttl));
        let file: serde_json::Map<String, serde_json::Value> = map
            .iter()
            .map(|(key, entry)| {
                (
                    serde_json::to_string(key).unwrap_or_default(),
                    json!(PersistedEntry {
                        page: entry.page.clone(),
                        at: entry
                            .at
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs(),
                    }),
                )
            })
            .collect();
        let _ = std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")));
        let _ = std::fs::write(path, serde_json::to_string(&file).unwrap_or_default());
    }

    /// A copy of the page stored under `key`, or `None` when it is missing or older than the
    /// TTL (an expired entry is dropped rather than left to linger).
    pub fn get(&self, key: &CacheKey) -> Option<SearchPage> {
        let mut map = self.entries.lock().ok()?;
        let entry = map.get(key)?;
        if entry.expired(self.ttl) {
            map.remove(key);
            return None;
        }
        map.get(key).map(|entry| entry.page.clone())
    }

    pub fn put(&self, key: CacheKey, page: SearchPage) {
        if let Ok(mut map) = self.entries.lock() {
            map.retain(|_, entry| !entry.expired(self.ttl));
            map.insert(
                key,
                Entry {
                    at: SystemTime::now(),
                    page,
                },
            );
        }
    }
}

impl Entry {
    fn expired(&self, ttl: Duration) -> bool {
        // A clock moved forward (DST, an NTP step): the entry is not yet
        // old enough to expire, so keep it rather than answering an error.
        self.at.elapsed().map(|age| age >= ttl).unwrap_or(false)
    }
}
