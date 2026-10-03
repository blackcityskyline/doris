# doris search bridge

A Firefox add-on that sends a title to [doris](../README.md), the torrent
search TUI, from the page you are reading the title on.

The search runs in doris's own window. This add-on hands it a title and says
whether the handoff happened — it does not search anything itself.

## Install

```sh
make xpi      # → browser-extension/dist/doris-search-bridge-1.0.0.xpi
```

That file is not in the repository: it is a build product, and one packer
writes both the copy you install and the copy `test/live.py` loads.

**Now, without a signature** — Firefox and Zen will not install an unsigned
add-on permanently, and they do not say so nicely: measured here, the same
`.xpi` installs as *temporary* and is refused as *permanent* with
`ERROR_CORRUPT_FILE: The file appears to be corrupt`, which is a lie about a
perfectly good zip. A signed add-on from addons.mozilla.org installs
permanently through the same path in the same browser, which is how the two
were told apart.

So, in order of what works today:

**`about:debugging`** — `about:debugging#/runtime/this-firefox` → *Load
Temporary Add-on…* → pick `browser-extension/manifest.json`. Works
immediately, needs no signature, and is gone when the browser closes.

**Sign it, then install it permanently** — upload the `.xpi` at
[addons.mozilla.org](https://addons.mozilla.org/developers/) as an unlisted
add-on. Signing needs an AMO account and a click-through; after that the file
installs permanently and updates on its own through *Install Add-on From
File…* in `about:addons`.

There is no third way on a release build. `xpinstall.signatures.required =
false` is honoured only by Nightly, Developer Edition and ESR, and an
enterprise policy with `install_path` installs a *signed* extension too.

Then start doris with the bridge on:

```toml
# ~/.config/doris/config.toml
bridge_port = 14141
```

and press `o` on a download, or click the button on a page.

## What it does

**Click the toolbar button (or press `Ctrl+Shift+D`) and select a title.**
Whatever you circle is what gets searched -- the page's own title is right
when the page is about one film and useless when it is not, and even when
it is right it can be `East of Eden (TV Mini Series 2026) - IMDb`, which no
tracker has. `Esc` cancels, which matters because in this mode the mouse
belongs to you.

A small `doris` button also appears next to the title on the four sites, for
the case where the page is obviously about one film and there is nothing to
select.

The toolbar button falls back to the tab's title on a page with no add-on in
it, which is every site outside the four: there is no content script to tell,
and the tab's title beats sending nothing.

On IMDb, Trakt, Kinopoisk and Lampa a small `doris` button appears next to the
title. It reads the title, sends it to `http://127.0.0.1:14141/search?q=`, and
reports on the button itself:

| the button says | it means |
|---|---|
| `sent` | doris took the title and is searching |
| `queued` | doris was not running; the title is kept and sent when it starts |
| `refused` | doris is running but does not answer this page |
| `not a title` | this page is a section or a dashboard, not about one film |

`queued` is the one to know about: the bridge is a server **inside** the app,
so with doris closed there is nobody listening and a title pressed then has
nowhere to go. It is kept -- in the add-on's own storage, oldest first, twenty
deep, duplicates collapsed -- and sent as soon as a `/ping` says doris is
there. So closing doris, clicking on a film, and starting doris afterwards
does search for that film, and the button turns itself to `sent` about two
seconds after doris answers.

The toolbar badge carries the number waiting, because the button belongs to
one page and a click that is waiting on nothing looks exactly like a click
that did nothing.

There are two clocks, because one of them cannot go fast. The alarm is
Firefox's floor: 30 seconds is the smallest period it accepts and it clamps
anything smaller. The fast one is a 15-second timeout chain, which a page's
messages keep alive and which is gone the moment the browser suspends the
event page. So a queued title goes out in about fifteen seconds with a page
open and thirty with every tab closed -- and the second case is the one the
alarm exists for.

The page also asks the background every two seconds while a title waits,
which is a third path to the same answer and the fastest one, because a
message from a page wakes a suspended event page. Telling the page instead
does not work at all (measured), which is why the direction is the way it is.

## Titles the sites actually give

Measured on a real IMDb title page, the add-on sent:

```
East of Eden (TV Mini Series 2026) - IMDb
```

and searched for all of it, which finds nothing. Three separate things wrong
with one string: the site's own name, a bracketed qualifier that is IMDb's
description of the entry rather than part of the name, and the year inside
that qualifier. All three go now, and a number that is part of a name stays --
"Ocean's 8" is not a film called "Ocean's".

`h1` is preferred over `og:title` on a title page for the same reason: the
heading is the film and the metadata carries the decoration. Which is only
safe because the address already established that the page is about one film
-- on IMDb's front page `h1` is a section, and that page is refused before a
title is ever read.

I could not verify IMDb's current markup: an automated browser gets
`Human Verification` from imdb.com, so the selectors are the documented ones
with a bare `h1` behind them rather than the ones a real page was measured
to have. If a site changes its heading, the bare `h1` is what saves it, and
the fallback still gets the name -- cleaned.

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

## How the queue finds out doris is back

`GET /ping`, answered without starting a search. It has to be free: the
add-on asks it every 30 seconds for as long as a title is waiting, so if the
only way to ask were `/search`, "send it when doris comes back" would be a
search every 30 seconds.

Thirty seconds, not two, and through `alarms` rather than `setTimeout`: an
MV3 background is an *event page* which Firefox suspends after about half a
minute of quiet, and a suspended page runs no timers. Measured — with a plain
timer, a title queued and then delivered forty seconds later never was.

## Permissions, and why each is there

| permission | why |
|---|---|
| IMDb, Trakt, Kinopoisk, Lampa | where the button goes. These are the same origins doris's bridge allows — `tests/extension_manifest_tests.rs` compares the two lists, so they cannot drift apart |
| `http://127.0.0.1/*`, `http://localhost/*` | reaching doris. Per-host, not per-port: `bridge_port` is a setting, and a permission naming one would break every user who moved it |
| `storage` | the bridge address, and the queue of titles waiting for doris |
| `alarms` | the 30-second poll. Without the permission `browser.alarms` is undefined, and the scheduler throws inside the message handler — which leaves the button on `sending…` for ever |

The bridge answers an allow list, not `*`. It listens on loopback and it
*acts* — a request starts a search — so any page a browser happens to be on
must not be able to drive this machine. A page doris does not recognise is
answered `403 origin not allowed` and no search runs.

## Tests

Three layers, because three different things can be wrong.

**The title** — `node browser-extension/test/title_test.mjs`, or `make
test-ext`, or the gate. 18 checks against a stub document: the four site
tables, the `og:title` fallback, and the cleaning. It is the part with a bug
in it: the button has to know the title, and every site spells that
differently. `title.js` is a plain function over anything with
`querySelector` and a `location`, so it runs in a content script against the
real page and under node against a stub.

**The contract** — `cargo test --test extension_manifest_tests`. Every site
the add-on runs on is allowed by the bridge, every origin the bridge allows
has a button, the permission covers any port, every file the manifest lists
exists, and the route the add-on calls is the route the bridge serves.

**The browser** — `python3 browser-extension/test/live.py [browser]`. This
one needs three things the repository does not carry:

```sh
npm install --no-save geckodriver          # from the repository root
python3 browser-extension/test/live.py /usr/bin/firefox
```

with doris running and the bridge on. It installs the add-on temporarily
through WebDriver, serves `test/fixtures/page.html` — a page shaped like the
four sites, on `127.0.0.1`, which the bridge already allows for exactly this
— finds the button, clicks it, and waits for it to stop saying `sending…`.
It prints what it saw at every step and the last line is only reached when
they all passed.

This is the only layer that can catch the button not appearing, the click not
reaching the background script, or the fetch dying on CORS. It is also the
only one that found anything when it was written: `dataset.dorisTitle` was
read by nothing and set by nothing, and the harness asked for it and found it
missing.

`make gate` runs the first two. The third needs a browser and is not in it.

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