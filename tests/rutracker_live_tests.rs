//! Live probe of one open question: does rutracker's
//! `tracker.php` honour a category the way its own search form offers
//! it, and which forums a category is made of -- the input B6 needs to
//! route `SearchRequest.category` down to this source at all.
//!
//! The live answer, 26.09.2026: the parameter is **`f[]`**, not the
//! `c[]` this file was written around -- the form's category control is
//! a multi-select of *forum ids* (1339 of them, the whole tree, nested
//! under a `|-` prefix), so a "category" here is a forum id the way it
//! is on nnmclub, and `tracker.php?f[]=<id>` answers with that forum's
//! topics alone (live: 50 topics for f[]=22, 8 for f[]=7, no id in
//! both). That is what licenses `category_filter: true` and the group
//! mapping on top of it.
//!
//! Ignored by default: it launches a real browser (a plain client gets
//! a Cloudflare challenge from rutracker -- live-confirmed 403
//! "Just a moment..." on 25.09.2026), waits it out, and needs the
//! rutracker session it cannot establish itself: a guest is redirected
//! to `login.php`, so the cookies the app saved in `cookies.txt` by its
//! own login are injected instead. Without them this probe answers
//! "0 categories" against a login wall, not against the site.
//!
//! Run with (one thread: two browsers at once have been seen to kill a
//! session mid-test):
//! `cargo test --test rutracker_live_tests -- --ignored --nocapture
//! --test-threads=1`

use std::collections::BTreeSet;
use std::path::Path;

use doris::browser::cdp::{Browser, BrowserVisibility};
use doris::browser::detect;
use doris::sources::source::Group;

const HOME: &str = "https://rutracker.org/forum/";

/// Ask the site for two of the categories its own form offers. Both must
/// answer with topics, and no topic may appear under both: a topic lives
/// in exactly one forum, so a filter letting one through twice is not
/// filtering -- it is ignoring the parameter.
#[tokio::test]
#[ignore = "launches a browser, waits out Cloudflare, needs rutracker cookies"]
async fn live_category_param_selects_disjoint_sections() {
    // The same priority the app would use: reading it from the config
    // keeps this probe on the browser (and chromedriver) the user's own
    // session runs on, instead of the built-in order's first installed
    // one -- which here is a Helium the cached patched driver does not
    // match, an unrelated defect this test walked straight into.
    let config = doris::config::load(None).unwrap_or_default();
    let priority = detect::parse_priority(&config.browser_priority);
    let (_, path) =
        detect::detect_browser_with_priority(None, &priority).expect("a browser to probe with");
    let mut browser = Browser::launch(&path, BrowserVisibility::Hidden, HOME, true)
        .await
        .expect("browser session");

    // The question itself needs a session: a guest is bounced straight to
    // login.php. The app's own login (Settings -> streaming -> Edit
    // credentials, then any search on the rutracker tab) leaves that
    // session in `cookies.txt`, so the probe reuses it the same way the
    // next run of the app does. Cookies can only be set once the tab is
    // on a real page (chrome://new-tab-page/ rejects every domain), so
    // the navigation comes first.
    fetch(&browser, HOME).await;
    inject_saved_cookies(&browser, Some(Path::new(&config.cookie_file))).await;

    // One results page serves both purposes: it carries the search form
    // (hence the categories) and proves the session is logged in well
    // enough to see results at all.
    fetch(&browser, &format!("{HOME}tracker.php?nm=gta&o=10&s=2")).await;
    let categories = listed_categories(&browser).await;
    println!(
        "the search form offers {} categories via {:?}:",
        categories.len(),
        categories.first().map(|(param, _, _)| param.as_str())
    );
    if categories.is_empty() {
        // Say what the page actually was instead of failing on a bare
        // count: "no categories" has three very different causes (a
        // challenge, a login wall, a form moved elsewhere) and only one
        // of them is about `f[]` at all.
        println!("page diagnostics: {}", describe(&browser).await);
    }
    assert!(!categories.is_empty(), "tracker.php must offer categories");

    // Top-level sections only: the form nests its tree under a "|-"
    // prefix, and a parent already carries its children's topics, so two
    // parents are the honest comparison. "-1" is the form's own
    // "every forum" value and is skipped the way rutor's `0` is.
    let top_level: Vec<&(String, String, String)> = categories
        .iter()
        .filter(|(_, id, label)| id != "-1" && !label.starts_with("|-"))
        .collect();

    let mut answered: Vec<(&str, String, BTreeSet<String>)> = Vec::new();
    for (param, id, label) in top_level.iter().take(15) {
        let url = format!("{}tracker.php?{}={}&o=10&s=2", HOME, param, id);
        fetch(&browser, &url).await;
        let topics = result_topic_ids(&browser).await;
        println!("{}={} ({}) -> {} topics", param, id, label, topics.len());
        if !topics.is_empty() {
            answered.push((param.as_str(), id.clone(), topics));
        }
        if answered.len() == 2 {
            break;
        }
    }
    browser.shutdown().await;

    assert_eq!(
        answered.len(),
        2,
        "at least two of the offered categories must answer with topics"
    );
    let overlap: Vec<&String> = answered[0].2.intersection(&answered[1].2).collect();
    assert!(
        overlap.is_empty(),
        "a topic lives in one forum, yet {:?} answered under both {}={} and {}={}",
        overlap,
        answered[0].0,
        answered[0].1,
        answered[1].0,
        answered[1].1
    );
}

/// The category control the form actually posts: parameter name, value
/// and label, read off the page rather than hardcoded -- both the
/// checkbox and the multi-select shapes are covered, because which one
/// the site serves (and it is `f[]` here, not `c[]`) is exactly what is
/// being asked. A name starting `c` or `f` is the form's own mark.
async fn listed_categories(browser: &Browser) -> Vec<(String, String, String)> {
    let script = r#"(() => {
        const out = [];
        const seen = new Set();
        const push = (name, id, label) => {
            const clean = (label || "").replace(/\s+/g, " ").trim();
            if (!name || !id || seen.has(name + "=" + id)) return;
            seen.add(name + "=" + id);
            out.push([name, id, clean]);
        };
        for (const opt of document.querySelectorAll("select option")) {
            // `closest`, not `parentElement`: the tree is grouped into
            // optgroups, so an option's parent is often a group, and
            // only the select around it carries the parameter name.
            const select = opt.closest("select");
            if (select && select.name &&
                (select.name.indexOf("c") === 0 || select.name.indexOf("f") === 0)) {
                push(select.name, opt.value, opt.textContent);
            }
        }
        for (const el of document.querySelectorAll('input[type="checkbox"]')) {
            if (!el.name || (el.name.indexOf("c") !== 0 && el.name.indexOf("f") !== 0)) {
                continue;
            }
            const label = el.id
                ? document.querySelector('label[for="' + el.id + '"]')
                : null;
            push(el.name, el.value, label ? label.textContent : el.parentElement.textContent);
        }
        return JSON.stringify(out);
    })()"#;
    let value = browser.eval_js(script).await.expect("read the form");
    let json = value.as_str().expect("the script returns JSON");
    serde_json::from_str(json).expect("the script returns an array of triples")
}

/// Topic ids of the results table's rows. Every rutracker page carries
/// other topic links (nav, sidebar, banners), and the results table is
/// the one whose class starts with "forumline" -- its tablesorter hash
/// suffix differs per page, so the class prefix is the only stable
/// handle. A row links its topic (`viewtopic.php?t=<id>`) and nothing
/// else identifying, so the filter is read off the ids themselves.
async fn result_topic_ids(browser: &Browser) -> BTreeSet<String> {
    let script = r#"(() => {
        const table = document.querySelector('table[class*="forumline"]');
        if (!table) return JSON.stringify([]);
        const ids = [];
        for (const a of table.querySelectorAll('a[href*="viewtopic.php"]')) {
            const m = a.getAttribute("href").match(/t=(\d+)/);
            if (m) ids.push(m[1]);
        }
        return JSON.stringify(ids);
    })()"#;
    let value = browser.eval_js(script).await.expect("read the results");
    let json = value.as_str().expect("the script returns JSON");
    let ids: Vec<String> = serde_json::from_str(json).expect("the script returns id strings");
    ids.into_iter().collect()
}

/// Reuse the session the app saved at `cookie_file` (Netscape format,
/// written by `RutrackerSearcher::ensure_logged_in` after a successful
/// login). Names and counts only -- this probe reads the category
/// parameter off the search form, and the login that produced the file
/// is the app's own.
async fn inject_saved_cookies(browser: &Browser, cookie_file: Option<&Path>) {
    let Some(path) = cookie_file else {
        return;
    };
    let cookies = match doris::sources::cookies::load_from_file(path) {
        Ok(cookies) => cookies,
        Err(e) => {
            println!("could not read cookies from {}: {}", path.display(), e);
            return;
        }
    };
    if cookies.is_empty() {
        println!("no cookies in {}", path.display());
        return;
    }
    let json: Vec<serde_json::Value> = cookies.iter().map(|c| c.to_json()).collect();
    match browser.add_cookies(&json).await {
        Ok(()) => println!("injected {} cookies from {}", cookies.len(), path.display()),
        Err(e) => println!("injecting cookies failed: {}", e),
    }
}

/// The table itself, end to end: one GET carrying every forum id of a
/// group must answer with rows -- this is the request the slot makes
/// when a category is selected, so a hole in `GROUP_FORUMS` or a
/// parameter the site stops honouring shows up here and not in the
/// category tab's empty table.
#[tokio::test]
#[ignore = "launches a browser, waits out Cloudflare, needs rutracker cookies"]
async fn live_a_group_search_asks_for_its_forums_in_one_request() {
    let config = doris::config::load(None).unwrap_or_default();
    let priority = detect::parse_priority(&config.browser_priority);
    let (_, path) =
        detect::detect_browser_with_priority(None, &priority).expect("a browser to probe with");
    let mut browser = Browser::launch(&path, BrowserVisibility::Hidden, HOME, true)
        .await
        .expect("browser session");

    fetch(&browser, HOME).await;
    inject_saved_cookies(&browser, Some(Path::new(&config.cookie_file))).await;

    let url = doris::sources::rutracker::search_url("gta", 0, Some(Group::Games));
    println!("asking: {} ({} bytes)", url, url.len());
    fetch(&browser, &url).await;
    let topics = result_topic_ids(&browser).await;
    println!("games search -> {} topics", topics.len());
    assert!(
        !topics.is_empty(),
        "the group's own forums must answer a search filtered to them"
    );

    browser.shutdown().await;
}

/// Navigate, then wait out a Cloudflare challenge if the page brought
/// one -- the same patience `RutrackerSearcher` applies before it reads
/// a search page, mirrored here so this probe survives on its own.
async fn fetch(browser: &Browser, url: &str) {
    browser.navigate(url).await.expect("navigate");
    for _ in 0..30 {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let title = browser.eval_js("document.title").await;
        if let Ok(serde_json::Value::String(s)) = &title {
            if !s.is_empty() && s != "Just a moment..." {
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                return;
            }
        }
    }
    panic!("{} stayed behind a challenge for 30s", url);
}

/// What the page turned out to be: title, URL, form count, how many
/// result links it holds and which controls it exposes -- names, not
/// values, because the question is what the site calls the parameter.
async fn describe(browser: &Browser) -> String {
    let script = r#"(() => {
        const names = [];
        for (const el of document.querySelectorAll("input, select")) {
            if (el.name && names.indexOf(el.name) === -1) names.push(el.name);
        }
        return JSON.stringify({
            title: document.title,
            href: location.href,
            forms: document.forms.length,
            topicLinks: document.querySelectorAll('a[href*="viewtopic.php"]').length,
            controls: names.slice(0, 60),
        });
    })()"#;
    let value = browser.eval_js(script).await.expect("describe the page");
    value.as_str().expect("the script returns JSON").to_string()
}
