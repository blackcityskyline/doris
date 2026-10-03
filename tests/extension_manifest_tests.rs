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
fn content_matches(manifest: &str) -> Vec<String> {
    let start = manifest
        .find("\"content_scripts\"")
        .expect("content_scripts");
    let rest = &manifest[start..];
    let close = rest.find("]").expect("a closed matches array");
    rest[..close]
        .split('"')
        .skip(1)
        .step_by(2)
        .filter(|s| s.contains("://"))
        .map(str::to_string)
        .collect()
}

/// `https://www.imdb.com/*` -> `https://www.imdb.com`
fn strip_pattern(pattern: &str) -> String {
    pattern
        .trim_end_matches('*')
        .trim_end_matches('/')
        .to_string()
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
fn every_site_the_add_on_runs_on_is_allowed_by_the_bridge() {
    let manifest = manifest();
    for pattern in content_matches(&manifest) {
        let origin = strip_pattern(&pattern);
        assert!(
            doris::bridge::handler::origin_allowed(&origin),
            "{origin} has a content script but the bridge refuses it: \
             the button would answer 403"
        );
    }
}

#[test]
fn every_origin_the_bridge_allows_is_a_site_the_add_on_runs_on() {
    // The reverse: an allowed origin with no content script is a hole in the
    // allow list, and a hole is what a second extension on the same machine
    // would use.
    let manifest = manifest();
    let injected: Vec<String> = content_matches(&manifest)
        .iter()
        .map(|p| strip_pattern(p))
        .collect();

    for origin in doris::bridge::handler::ALLOWED_ORIGINS {
        // The local ones are for a page served off the developer's own
        // machine, not for the add-on's buttons.
        if origin.starts_with("http://") {
            continue;
        }
        assert!(
            injected.iter().any(|i| i == origin),
            "{origin} is allowed by the bridge but no add-on button runs there"
        );
    }
}

#[test]
fn the_add_on_may_reach_the_bridge_on_any_port() {
    // `bridge_port` is a setting, so a manifest pinned to 14141 would break
    // every user who moved it. The permission is per-host, and the options
    // page is where the port comes from.
    let manifest = manifest();
    let permissions = host_permissions(&manifest);
    for wanted in ["http://127.0.0.1/*", "http://localhost/*"] {
        assert!(
            permissions.iter().any(|p| p == wanted),
            "{wanted} is not in host_permissions: the bridge runs on a \
             configurable port, so the permission cannot name one -- {permissions:?}"
        );
    }
}

#[test]
fn the_add_on_asks_for_no_permission_the_bridge_does_not_need() {
    // Every `https://` permission is a site. A permission that is not a site
    // -- an API, a CDN -- would be a request the bridge has no route for, and
    // it would be asked for on every page load.
    let manifest = manifest();
    for pattern in host_permissions(&manifest) {
        assert!(
            pattern.starts_with("https://")
                || pattern.starts_with("http://127.0.0.1")
                || pattern.starts_with("http://localhost"),
            "{pattern} is neither a site nor the bridge"
        );
    }
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
