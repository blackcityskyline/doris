//! TTL cache tests (ROADMAP.md B5): hit within the TTL, miss after it,
//! per-key isolation, and the orchestrator's two cache entry points --
//! the write path (`cached_fetch`) and the read path
//! (`cached_source_done`) that lets a hit skip the spawn entirely.

use std::sync::Arc;
use std::time::Duration;

use doris::event::Event;
use doris::sources::cache::{CacheKey, SearchCache, TTL};
use doris::sources::models::TorrentItem;
use doris::sources::orchestrator::{cached_fetch, cached_source_done};
use doris::sources::source::SearchPage;

fn page(rows: usize, has_more: bool) -> SearchPage {
    SearchPage {
        items: (0..rows)
            .map(|i| TorrentItem {
                title: format!("row {}", i),
                ..Default::default()
            })
            .collect(),
        has_more,
        next_offset: None,
    }
}

fn key(query: &str) -> CacheKey {
    CacheKey::new("rutor", query, None, 0)
}

#[test]
fn test_a_fresh_entry_is_served_from_memory() {
    let cache = SearchCache::new();
    cache.put(key("matrix"), page(3, true));

    let hit = cache.get(&key("matrix")).expect("within the TTL");

    assert_eq!(hit.items.len(), 3);
    assert!(hit.has_more, "the paging verdict is cached with the rows");
}

#[tokio::test]
async fn test_an_entry_expires_after_the_ttl() {
    let cache = SearchCache::with_ttl(Duration::from_millis(50));
    cache.put(key("matrix"), page(3, false));

    tokio::time::sleep(Duration::from_millis(80)).await;

    assert!(cache.get(&key("matrix")).is_none(), "expired = a miss");
}

#[test]
fn test_the_production_ttl_is_five_minutes() {
    // torio's `TTL_MS = 5 * 60 * 1000`.
    assert_eq!(TTL, Duration::from_secs(300));
    assert!(SearchCache::new().get(&key("nothing")).is_none());
}

#[test]
fn test_keys_are_isolated_per_source_page_and_category() {
    let cache = SearchCache::new();
    cache.put(key("matrix"), page(1, false));

    assert!(
        cache.get(&CacheKey::new("rutor", "matrix", None, 100)).is_none(),
        "another page of the same query must not share the entry"
    );
    assert!(
        cache.get(&CacheKey::new("rutracker", "matrix", None, 0)).is_none(),
        "another source must not share the entry"
    );
    assert!(
        cache.get(&CacheKey::new("rutor", "matrix", Some("Games"), 0)).is_none(),
        "another category must not share the entry"
    );
    assert!(cache.get(&key("other")).is_none());
}

#[test]
fn test_the_query_is_normalized_like_torio() {
    // torio's `key()` trims and lowercases, so these are one search.
    assert_eq!(key("  MaTrIx "), key("matrix"));

    let cache = SearchCache::new();
    cache.put(key("  MaTrIx "), page(2, false));

    assert!(cache.get(&key("matrix")).is_some(), "same search, hit");
}

#[tokio::test]
async fn test_put_sweeps_expired_entries() {
    // torio leaves expired entries in the map forever; we drop them
    // while the lock is held anyway, so the cache cannot grow without
    // bound over a long session.
    let cache = Arc::new(SearchCache::with_ttl(Duration::from_millis(50)));
    cache.put(key("old"), page(1, false));
    tokio::time::sleep(Duration::from_millis(80)).await;

    cache.put(key("new"), page(1, false));

    assert!(cache.get(&key("old")).is_none());
    assert!(cache.get(&key("new")).is_some());
}

// --- orchestrator wiring -----------------------------------------------------

#[tokio::test]
async fn test_a_successful_fetch_lands_in_the_cache() {
    let cache = Arc::new(SearchCache::new());
    let k = key("matrix");

    let got = cached_fetch(
        async move { Ok::<_, anyhow::Error>(page(2, true)) },
        Arc::clone(&cache),
        k.clone(),
    )
    .await
    .expect("the fetch succeeds");

    assert_eq!(got.items.len(), 2);
    assert!(cache.get(&k).is_some(), "the page was stored for next time");
}

#[tokio::test]
async fn test_a_failed_fetch_is_never_cached() {
    // Caching an error would pin "no results" for the whole TTL.
    let cache = Arc::new(SearchCache::new());
    let k = key("matrix");

    let err = cached_fetch(
        async move { Err::<SearchPage, _>(anyhow::anyhow!("HTTP 503")) },
        Arc::clone(&cache),
        k.clone(),
    )
    .await
    .expect_err("the failure passes through");

    assert!(err.to_string().contains("503"));
    assert!(cache.get(&k).is_none(), "failures must not be remembered");
}

#[test]
fn test_a_cache_hit_becomes_the_same_source_done_a_live_fetch_would_send() {
    let cache = SearchCache::new();
    let k = key("matrix");
    cache.put(k.clone(), page(4, true));

    let event = cached_source_done(&cache, &k, 7).expect("a hit");

    match event {
        Event::SourceDone {
            source,
            generation,
            items,
            has_more,
            next_offset,
            error,
            timed_out,
        } => {
            assert_eq!(source, "rutor", "the hit speaks for its own source");
            assert_eq!(generation, 7, "tagged with the dispatch it answers");
            assert_eq!(items.len(), 4);
            assert!(has_more, "the cached paging verdict survives");
            assert_eq!(next_offset, None, "the cached cursor survives too");
            assert!(error.is_none() && !timed_out, "a hit is not a failure");
        }
        other => panic!("expected SourceDone, got {:?}", other),
    }
}

#[test]
fn test_a_cache_miss_says_to_spawn_the_source() {
    let cache = SearchCache::new();

    assert!(cached_source_done(&cache, &key("cold"), 7).is_none());
}
