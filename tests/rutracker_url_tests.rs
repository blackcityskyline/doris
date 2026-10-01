use doris::sources::rutracker::{resolve_url, search_url, GROUP_FORUMS};
use doris::sources::source::Group;

// Moved here from `models_tests.rs` together with `resolve_url` itself:

#[test]
fn test_resolve_url_absolute() {
    assert_eq!(
        resolve_url("https://rutracker.org/forum/dl.php?t=123"),
        "https://rutracker.org/forum/dl.php?t=123"
    );
}

#[test]
fn test_resolve_url_root_relative() {
    assert_eq!(
        resolve_url("/forum/dl.php?t=123"),
        "https://rutracker.org/forum/dl.php?t=123"
    );
}

#[test]
fn test_resolve_url_bare_path() {
    assert_eq!(
        resolve_url("dl.php?t=123"),
        "https://rutracker.org/forum/dl.php?t=123"
    );
}

#[test]
fn test_resolve_url_bare_viewtopic() {
    assert_eq!(
        resolve_url("viewtopic.php?t=123&start=0"),
        "https://rutracker.org/forum/viewtopic.php?t=123&start=0"
    );
}

#[test]
fn test_resolve_url_with_query_params() {
    assert_eq!(
        resolve_url("dl.php?t=4682196"),
        "https://rutracker.org/forum/dl.php?t=4682196"
    );
}

/// B6: with no category selected the query is the unfiltered one -- no
/// `f[]` parameter at all, the way `cat=0` means "all" for rutor.
#[test]
fn test_search_url_without_category_has_no_forum_param() {
    assert_eq!(
        search_url("matrix", 0, None),
        "https://rutracker.org/forum/tracker.php?nm=matrix&o=10&s=2"
    );
}

/// acceptance in one direction: a selected category becomes exactly
/// one `f%5B%5D=<id>` per forum of that group's table -- no more (another
/// group's forums would answer rows the view must drop) and no fewer
/// (an unanswered forum is a hole in the category).
#[test]
fn test_search_url_with_category_asks_for_exactly_that_groups_forums() {
    for (group, ids) in GROUP_FORUMS {
        let url = search_url("matrix", 0, Some(group));
        assert_eq!(
            url.matches("f%5B%5D=").count(),
            ids.len(),
            "{group:?} must ask for each of its own forums: {url}"
        );
        for id in ids {
            assert!(
                url.contains(&format!("f%5B%5D={id}")),
                "{group:?} is missing forum {id}: {url}"
            );
        }
    }
}

/// `start=` only from the second page on; page one was verified without
/// it, and `start=0` was never seen.
#[test]
fn test_search_url_second_page_adds_start() {
    let unfiltered = search_url("matrix", 50, None);
    assert_eq!(
        unfiltered,
        "https://rutracker.org/forum/tracker.php?nm=matrix&o=10&s=2&start=50"
    );
    let filtered = search_url("matrix", 50, Some(Group::Movies));
    assert!(filtered.contains("&start=50"), "{filtered}");
    assert!(
        filtered.contains(&format!("f%5B%5D={}", GROUP_FORUMS[0].1[0])),
        "{filtered}"
    );
}

/// The four lists are pairwise disjoint and non-empty: a forum in two
/// groups could not be attributed to one, and an empty group would be a
/// tab that only ever shows an empty table.
#[test]
fn test_group_forums_are_disjoint_and_nonempty() {
    let all: Vec<i32> = GROUP_FORUMS
        .iter()
        .flat_map(|(_, ids)| ids.iter().copied())
        .collect();
    let unique: std::collections::BTreeSet<i32> = all.iter().copied().collect();
    assert!(!all.is_empty(), "the table spans no forums at all");
    assert_eq!(
        all.len(),
        unique.len(),
        "a forum in two groups could not be attributed to one"
    );
}
