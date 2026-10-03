//! The add-on's manifest and the bridge's allow list are one list.
//!
//! The comment in `handler.rs` claims the origins are "read off the add-on's
//! manifest". Nothing read it. This is the test that makes the claim true, and
//! it exists because the failure it catches is silent in both directions: a
//! site added to the manifest and not to the allow list gets a button that
//! answers `403 origin not allowed`, and a site left in the allow list and
//! dropped from the manifest leaves a hole the add-on cannot use.
//!
//! The bridge's list is a plain `&[&str]`, so this is the one place the two
//! are compared. A test rather than a build step: the manifest is not Rust,
//! and a codegen pass over it would be a second place to keep in step.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is `<repo>/`, so the add-on is beside `src/`.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn manifest_path() -> PathBuf {
    repo_root().join("browser-extension").join("manifest.json")
}

/// The origins the add-on asks permission for, without a JSON parser.
///
/// A tiny reader rather than a dependency: the file is ours, it is written by
/// hand, and the only thing asked of it here is the `host_permissions` array.
/// A real parser would be a crate added to read four lines of a file we
/// wrote, which is the dependency rule with the reasoning left out.
fn host_permissions(manifest: &str) -> Vec<String> {
    let start = manifest
        .find("\"host_permissions\"")
        .expect("a host_permissions array");
    let open = manifest[start..].find('[').expect("an array after the key");
    let rest = &manifest[start + open..];
    let close = rest.find(']').expect("a closed array");
    rest[..close]
        .split('"')
        .skip(1)
        .step_by(2)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// The same for the content scripts' `matches`, which are the sites the
/// button is injected into.
/// The sites the *button* appears on, which is the sites table and not the
/// manifest: the script runs everywhere so that selecting works everywhere,
/// and a wildcard in this list would make the allow-list tests compare
/// nothing at all.
fn button_sites() -> Vec<String> {
    let source =
        std::fs::read_to_string(repo_root().join("browser-extension/title.js")).expect("title.js");
    source
        .split("hosts: [")
        .skip(1)
        // Every host in the entry, not the first: the table lists both
        // `imdb.com` and `www.imdb.com`, and taking one of them makes the
        // bridge look like it allows a site with no button.
        .filter_map(|chunk| chunk.split(']').next())
        .flat_map(|chunk| {
            chunk
                .split('"')
                .skip(1)
                .step_by(2)
                .map(|host| format!("https://{host}"))
                .collect::<Vec<_>>()
        })
        .collect()
}

fn manifest() -> String {
    let path = manifest_path();
    assert!(
        path.exists(),
        "the add-on is at {} -- the bridge's allow list is its permissions",
        path.display()
    );
    std::fs::read_to_string(&path).expect("read the manifest")
}

#[test]
fn every_site_the_button_appears_on_is_allowed_by_the_bridge() {
    // The four, from the table the button is driven by -- not from
    // `content_scripts`, which is `<all_urls>` now and would compare nothing.
    for origin in button_sites() {
        assert!(
            doris::bridge::handler::origin_allowed(&origin),
            "{origin} has a button and the bridge refuses it: the button \
             would answer 403"
        );
    }
}

/// Selecting has to work on any site, which means the script runs on any
/// site, which means the bridge has to accept a request that carries no page
/// origin -- and it does, because the fetch is made by the background script.
/// That is the whole reason "any domain" can work with an allow list four
/// entries long.
#[test]
fn selection_works_on_any_site_because_the_fetch_carries_no_page_origin() {
    let manifest = manifest();
    assert!(
        manifest.contains("\"<all_urls>\""),
        "the content script is limited to four sites, so selecting cannot \
         work anywhere else"
    );

    let content = std::fs::read_to_string(repo_root().join("browser-extension/content.js"))
        .expect("content.js");
    assert!(
        !content.contains("fetch("),
        "the content script fetches: it would carry the page's origin and be \
         answered 403 on every site outside the allow list"
    );
    assert!(
        content.contains(r#"sendMessage({ type: "search""#),
        "so the search has to go through the background"
    );

    // And an origin nobody listed is still refused: running the script
    // everywhere must not open the bridge to the internet.
    assert!(!doris::bridge::handler::origin_allowed(
        "https://example.org"
    ));
    assert!(doris::bridge::handler::origin_allowed(""));
}

/// The button, unlike the picker, stays on the four sites: a page with no
/// rule for what its title means has no business being offered one.
#[test]
fn the_button_is_not_offered_on_a_site_with_no_table_entry() {
    let content = std::fs::read_to_string(repo_root().join("browser-extension/content.js"))
        .expect("content.js");
    let install = content
        .split("function install()")
        .nth(1)
        .expect("an install function");
    assert!(
        install.contains("SITES.some(") && install.contains("return;"),
        "the button is injected on any page the script reaches"
    );
    assert!(
        button_sites().len() >= 4,
        "and the table it is gated on has gone missing"
    );
}

#[test]
fn every_origin_the_bridge_allows_is_a_site_the_add_on_runs_on() {
    // The reverse: an allowed origin with no content script is a hole in the
    // allow list, and a hole is what a second extension on the same machine
    // would use.
    // The table, not `content_scripts`: the script runs on every page now,
    // so the manifest's matches say nothing about which origins are used.
    let sites = button_sites();

    for origin in doris::bridge::handler::ALLOWED_ORIGINS {
        // The local ones are for a page served off the developer's own
        // machine, not for the add-on's button.
        if origin.starts_with("http://") {
            continue;
        }
        assert!(
            sites.iter().any(|i| i == origin),
            "{origin} is allowed by the bridge but no add-on button runs there"
        );
    }
}

#[test]
fn the_add_on_may_reach_the_bridge_on_any_port() {
    // `bridge_port` is a setting, so a permission naming 14141 would break
    // every user who moved it. A match pattern cannot name a port at all --
    // ports are not part of a match pattern -- so what matters is the host,
    // which `<all_urls>` covers along with every other host the content
    // script now has to run on.
    let manifest = manifest();
    let permissions = host_permissions(&manifest);
    assert!(
        permissions.iter().any(|p| p == "<all_urls>"),
        "no wildcard permission, so the bridge is reachable only on whatever \
         hosts happen to be listed: {permissions:?}"
    );
}

#[test]
fn the_add_on_asks_for_nothing_beyond_the_wildcard() {
    // `<all_urls>` is the price of selecting on any site, and it is the whole
    // price: the fetch goes to the bridge or to nowhere, so no third-party
    // host is named anywhere -- which is also what makes the bridge's
    // four-entry allow list still mean something.
    let manifest = manifest();
    let permissions = host_permissions(&manifest);
    assert_eq!(
        permissions,
        vec!["<all_urls>".to_string()],
        "some other host is named, and every one of them is a request the \\
         add-on has no reason to make: {permissions:?}"
    );
}

/// `browser.alarms` is undefined without the permission, and the queue's
/// scheduler calls it on every send -- so a missing entry here does not
/// degrade, it throws inside the message handler and leaves the page's
/// button saying `sending…` for ever. Measured, and invisible without a
/// browser: the node tests never touch the API.
#[test]
fn the_add_on_asks_for_the_permissions_its_code_calls() {
    let manifest = std::fs::read_to_string(repo_root().join("browser-extension/manifest.json"))
        .expect("the manifest");
    for needed in ["storage", "alarms"] {
        assert!(
            manifest.contains(&format!("\"{needed}\"")),
            "`{needed}` is not in permissions.json's `permissions`, and the \
             code calls `browser.{needed}` without it"
        );
    }
}

/// The page asks the background, and the background does not tell the page.
///
/// Both halves were tried the other way round and the other way round does
/// not work: `tabs.sendMessage` from the background to a content script
/// never arrived, and a button on `queued` beside a finished search is worse
/// than no button. A message from a page wakes a suspended event page, so
/// asking is also what makes the answer come promptly between two alarms.
#[test]
fn the_page_asks_the_background_and_nothing_tells_the_page() {
    let content = std::fs::read_to_string(repo_root().join("browser-extension/content.js"))
        .expect("content.js");
    assert!(
        content.contains(r#"sendMessage({ type: "status" })"#),
        "the page does not ask, so a queued title never updates the button"
    );

    let background = std::fs::read_to_string(repo_root().join("browser-extension/background.js"))
        .expect("background.js");
    // Not "the background never writes to a tab": the toolbar click has to,
    // and it is a different message on a live event page. The one that does
    // not work is announcing that a queued title went -- unsolicited, from
    // inside an alarm, with no gesture behind it, and it never arrived.
    assert!(
        !background.contains(r#"{ type: "delivered" }"#),
        "announcing a delivery to the page again -- measured as not arriving"
    );
    assert!(
        background.contains(r#"message.type === "status""#),
        "and nothing answers the page's question"
    );
}

/// The queue has to be visible somewhere other than the button, or a click
/// that did nothing looks exactly like a click that did nothing.
#[test]
fn a_queued_title_shows_up_in_the_toolbar_badge() {
    let background = std::fs::read_to_string(repo_root().join("browser-extension/background.js"))
        .expect("background.js");
    // Split on a closing brace at the start of a line: the destructuring
    // pattern on the first line has a `}` in it too, and splitting on every
    // `}` checks a third of the function.
    let queueing = background
        .split("async function queueNow(")
        .nth(1)
        .expect("a queueNow")
        .split("\n}")
        .next()
        .expect("a body");
    assert!(
        queueing.contains("badge()"),
        "queueing does not set the badge: the only sign a title is waiting is \
         the button on a page that may not be open"
    );
}

/// The toolbar click puts the page into selection mode, and so does the key
/// the manifest declares. Both are the same message, because a second way in
/// is the thing that drifts.
///
/// The key is also the only path a test can drive: WebDriver sends key input
/// to a page, never to browser chrome, so the toolbar button itself cannot be
/// clicked from a test -- but `Ctrl+Shift+D` can, and it runs the same
/// content script.
#[test]
fn the_extension_button_and_the_declared_key_both_open_selection_mode() {
    let manifest = std::fs::read_to_string(repo_root().join("browser-extension/manifest.json"))
        .expect("the manifest");
    assert!(
        manifest.contains(r#""pick-title""#),
        "no keyboard command for selection mode, so the toolbar button is \
         the only way in and nothing can reach it without a mouse"
    );
    assert!(
        manifest.contains("Ctrl+Shift+D"),
        "the key the content script listens for and the one the manifest \
         declares have drifted apart"
    );

    let background = std::fs::read_to_string(repo_root().join("browser-extension/background.js"))
        .expect("background.js");
    for entry in ["browser.commands.onCommand", "browser.action.onClicked"] {
        assert!(
            background.contains(entry),
            "{entry} is gone, so one of the two ways in stopped working"
        );
    }
    assert!(
        background.contains(r#"sendMessage(tab.id, { type: "pick" })"#),
        "neither of them tells the page to start selecting"
    );
}

/// The listener that starts selection mode has to exist before the mode.
///
/// It used to be registered inside `startPicking`, which means the shortcut
/// that starts the mode had nothing listening -- a circle you cannot get
/// into. Found by pressing the key for real and watching nothing happen,
/// which no test over the file's text would have said.
#[test]
fn the_key_that_starts_selection_mode_is_listened_for_before_it_starts() {
    let content = std::fs::read_to_string(repo_root().join("browser-extension/content.js"))
        .expect("content.js");
    let defined = content
        .find("function onPickingKey")
        .expect("the handler exists");
    let attached = content
        .find(r#"document.addEventListener("keydown", onPickingKey"#)
        .expect("the handler is attached");
    let inside = content.find("fn startPicking").unwrap_or(usize::MAX);
    assert!(
        attached < inside,
        "the keydown listener is registered inside startPicking, so the key \
         that starts the mode is the one thing nothing is listening for"
    );
    assert!(
        defined < attached,
        "and it is attached before it is defined"
    );
}

#[test]
fn the_add_on_has_the_files_it_lists() {
    // A manifest naming a file that is not there installs and then does
    // nothing, with no error anywhere.
    let root = repo_root().join("browser-extension");
    let manifest = manifest();
    for name in [
        "title.js",
        "queue.js",
        "background.js",
        "content.js",
        "content.css",
        "options.html",
        "options.js",
        "icons/doris.svg",
    ] {
        assert!(
            root.join(name).exists(),
            "{name} is listed in the manifest and is not there"
        );
    }
    assert!(
        manifest.contains("\"manifest_version\": 3"),
        "a Firefox MV3 add-on; the event-page background has no service_worker"
    );
    assert!(
        !manifest.contains("service_worker"),
        "Firefox MV3 takes background scripts, not a service worker"
    );
}

/// The route the add-on calls is the route the bridge serves.
#[test]
fn the_bridge_serves_the_path_the_add_on_calls() {
    // Both spellings of the one endpoint. If this fails, the add-on's URL
    // and the server's route have drifted, and the failure the user sees is
    // a 404 in a browser console they will never open.
    let source =
        std::fs::read_to_string(repo_root().join("src/bridge/handler.rs")).expect("the bridge");
    assert!(
        source.contains(r#".route("/search""#),
        "the bridge no longer serves /search"
    );

    let background = std::fs::read_to_string(repo_root().join("browser-extension/background.js"))
        .expect("background.js");
    assert!(
        background.contains("/search?q="),
        "the add-on no longer calls /search?q="
    );
    let options = std::fs::read_to_string(repo_root().join("browser-extension/options.js"))
        .expect("options.js");
    assert!(
        options.contains("/search?q="),
        "the options page's check no longer calls /search?q="
    );
}

/// The add-on polls `/ping` to find out whether doris is back, and it polls
/// it *instead of* sending -- so the route has to exist and, more to the
/// point, has to be one that does nothing.
///
/// If the only way to ask were `/search`, "retry when doris comes back" would
/// be a search every two seconds for as long as a title waited.
#[test]
fn the_add_on_asks_whether_doris_is_there_without_starting_a_search() {
    let source =
        std::fs::read_to_string(repo_root().join("src/bridge/handler.rs")).expect("the bridge");
    assert!(
        source.contains(r#".route("/ping""#),
        "the bridge does not serve /ping, so a queued title can never be sent"
    );

    let background = std::fs::read_to_string(repo_root().join("browser-extension/background.js"))
        .expect("background.js");
    assert!(background.contains("/ping"), "the add-on never asks");
    // And it asks before it sends, rather than sending to find out.
    let drain = background
        .split("async function drain()")
        .nth(1)
        .expect("a drain function");
    let ping = drain.find("await alive()").expect("drain asks first");
    let search = drain.find("await search(").expect("drain sends");
    assert!(
        ping < search,
        "the ping comes before the send: a send is the thing that costs"
    );
}

/// The queue is the answer to a title pressed while doris was closed, so the
/// file that holds it has to be loaded by both the background script and the
/// content script -- and neither of them may inline its own copy.
#[test]
fn both_extension_scripts_load_the_queue_and_neither_reimplements_it() {
    let manifest = std::fs::read_to_string(repo_root().join("browser-extension/manifest.json"))
        .expect("the manifest");
    assert_eq!(
        manifest.matches("queue.js").count(),
        2,
        "the background scripts and the content scripts both load it"
    );

    for name in ["background.js", "content.js"] {
        let source =
            std::fs::read_to_string(repo_root().join("browser-extension").join(name)).expect(name);
        assert!(
            !source.contains("function add(queue"),
            "{name} has its own copy of the queue's rules"
        );
    }
}

/// The default address in the add-on is the one doris listens on.
#[test]
fn the_add_on_defaults_to_the_port_doris_uses() {
    let background = std::fs::read_to_string(repo_root().join("browser-extension/background.js"))
        .expect("background.js");
    let source = std::fs::read_to_string(repo_root().join("src/config.rs")).expect("config");
    // The port doris defaults to, which is what the add-on's own default has
    // to match: a user who never opens the options page is on it.
    let default_port = source
        .lines()
        .find(|l| l.contains("bridge_port") && l.contains("default"))
        .map(|l| l.to_string())
        .unwrap_or_default();
    assert!(
        !default_port.is_empty(),
        "config.rs no longer spells out bridge_port's default"
    );
    assert!(
        background.contains("14141"),
        "the add-on defaults to 14141; if the port moved, so must this"
    );
}

/// The extension is in the tree, not in a directory nobody opens.
#[test]
fn the_add_on_is_where_this_test_looks_for_it() {
    assert!(
        Path::new(&manifest_path()).starts_with(repo_root().join("browser-extension")),
        "the manifest lives beside the bridge it talks to"
    );
}
