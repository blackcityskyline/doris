# doris search bridge

A Firefox add-on that sends a title to [doris](../README.md), the torrent
search TUI, from the page you are reading the title on.

The search runs in doris's own window. This add-on hands it a title and says
whether the handoff happened — it does not search anything itself.

## Install

Firefox will not run an unsigned add-on permanently; two ways to try it:

**Temporarily** — `about:debugging#/runtime/this-firefox` → *Load Temporary
Add-on…* → pick `manifest.json`. It is gone when Firefox closes.

**Signed** — zip the directory and upload it through
[addons.mozilla.org](https://addons.mozilla.org/developers/). A signed add-on
installs permanently and updates on its own. Nothing here needs a build step:
it is five files and no bundler.

Then start doris with the bridge on:

```toml
# ~/.config/doris/config.toml
bridge_port = 14141
```

and press `o` on a download, or click the button on a page.

## What it does

On IMDb, Trakt, Kinopoisk and Lampa a small `doris` button appears next to the
title. It reads the title, sends it to `http://127.0.0.1:14141/search?q=`, and
reports on the button itself:

| the button says | it means |
|---|---|
| `sent` | doris took the title and is searching |
| `refused` | doris is running but does not answer this page |
| `doris is not listening` | nothing is on the bridge port |

The toolbar button searches the current tab's title, which is the way in from
any site without a button on it.

## The options page

Right-click the toolbar button → *Options*. One field: the bridge address.
The default is `http://127.0.0.1:14141`.

**127.0.0.1, not localhost.** On a machine where `localhost` resolves to `::1`
and doris listens on `127.0.0.1` only, the other address reaches nothing, and
the browser reports a connection failure for a daemon that is right there.

*Save and check* sends a probe query and reports what came back. The bridge has
one route and that route acts, so there is nothing to ask that does not start a
search — the probe is `doris-extension-check`, which matches no tracker.

## Permissions, and why each is there

| permission | why |
|---|---|
| IMDb, Trakt, Kinopoisk, Lampa | where the button goes. These are the same origins doris's bridge allows — `tests/extension_manifest_tests.rs` compares the two lists, so they cannot drift apart |
| `http://127.0.0.1/*`, `http://localhost/*` | reaching doris. Per-host, not per-port: `bridge_port` is a setting, and a permission naming one would break every user who moved it |
| `storage` | the bridge address |

The bridge answers an allow list, not `*`. It listens on loopback and it
*acts* — a request starts a search — so any page a browser happens to be on
must not be able to drive this machine. A page doris does not recognise is
answered `403 origin not allowed` and no search runs.

## Tests

```sh
node browser-extension/test/title_test.mjs    # or: make test-ext
```

18 checks over the title extraction, against a stub document. It is the part
with a bug in it: the button has to know the title, and every site spells that
differently. `title.js` is a plain function taking anything with
`querySelector` and a `location`, so it runs in a content script against the
real page and under node against a stub.

The Rust side has the other half in `tests/extension_manifest_tests.rs`:
every site the add-on runs on is allowed by the bridge, every origin the
bridge allows has a button, the permission covers any port, every file the
manifest lists exists, and the route the add-on calls is the route the bridge
serves.

`make gate` runs both.

## Layout

```
manifest.json    MV3. Firefox takes background scripts, not a service worker.
title.js         which title, per site. The table and the fallback.
content.js       the button, and what it says afterwards
content.css      three states, each with a word as well as a colour
background.js    the fetch, the toolbar button, the badge
options.html     the bridge address
options.js       and the check
icons/doris.svg  the menu's banner, reduced to a search and a magnet
test/            the node harness
```