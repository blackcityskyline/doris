# Doris — Audit & Roadmap

This document is the working plan for the `refactor/audit-and-architecture` branch.
It records what was found wrong with the current codebase, the target architecture,
and the phase-by-phase plan to get there. Update the **Status** column as work lands
so anyone (human or agent) picking this up mid-flight knows exactly where things stand.

No `cargo build`/`cargo test` is run by the agent producing these commits (sandboxed
network has no path to a modern Rust toolchain). Every commit is reviewed carefully
by hand and kept small and mechanical where possible, but **build with your own
toolchain and run `cargo test` before merging each phase.** If something doesn't
compile, that's expected feedback, not a surprise — report it back and it'll get fixed.

**First thing to check if `cargo check` fails on this branch:** the `Source: Send +
Sync` bound added in Phase 3 (`src/search/source.rs`). `RutrackerSearcher` holds
`Arc<Mutex<Browser>>`, and `Browser` (`src/browser/cdp.rs`) holds a `fantoccini::Client`
plus a couple of `std::process::Child`s — these should all be `Send`, but it's the one
bound in this branch that wasn't hand-traced field-by-field with total confidence.
If the compiler disagrees, dropping the `Send + Sync` supertrait bound on `Source` (or
narrowing it to just `Send`) is a safe, local fix that doesn't ripple anywhere else.

---

## 1. Audit findings

Concrete, file-and-line issues found while reading the codebase (not guesses):

### Bugs
| # | File | Issue |
|---|------|-------|
| B1 | `src/ui/app.rs` (settings render) | Selected-item position indicator was hardcoded `format!("{} 3/{}", item.label, cat.items.len())` — always shows "3" regardless of actual position. This is the exact bug visible in the bug-report screenshot ("Color theme 3/8"). |
| B2 | `src/ui/app.rs` (`settings_key` vs settings render) | Page size used for j/k pagination is a hardcoded `let visible_items = 10;` in the input handler, while the renderer computes it dynamically from the real terminal height (`content_h / 2`). On any window where the real page size isn't 10, paging and the visible page desync — selection can scroll off-screen. |
| B3 | `src/app.rs` | `TorrentStatus` (hash/progress/speed/seeds/peers) is **never written to** anywhere after construction. The whole Torrent panel is permanently frozen at its zero-value defaults — there is no polling loop against TorrServer's status API at all. |
| B4 | `src/credentials/mod.rs` | Username/password are joined as `"{username}:{password}"` and split on `:` on load. A password containing `:` silently corrupts on read. |
| B5 | `src/ui/app.rs` (settings construction) | Half the Options items shown (`Theme background`, `Truecolor`, `Vim keys`, `Disable mouse`, `Update ms`, `Rounded corners`, `Terminal sync`, `Log level`, `Save on exit`) are **hardcoded literal strings** (`"True"`, `"1000"`, `"INFO"`...) with no backing field in `App` at all. Toggling them changes nothing — confirmed, these are decorative. |
| B6 | `src/app.rs` (`SettingsAction::ToggleBrowserVisibility` handler) | Toggling "Browser visible" only flipped `ui.browser_hidden` (the display label) and never touched `self.browser_visibility` (the field `get_browser()` actually launches with). The toggle looked like it worked but had zero effect on the next browser launch. Found and fixed in Phase 5. |

### Architecture problems (why the above bugs exist / why the app is hard to extend)
| # | Area | Problem |
|---|------|---------|
| A1 | `src/browser/detect.rs`, `src/browser/cdp.rs` | `BrowserKind` only has `Chrome / Brave / Helium`; Chromium is silently folded into `Chrome`. Detection order is a hardcoded array, not user-configurable ("Prioritize browser" from the spec doesn't exist). `Browser::launch` is a ~120-line function mixing chromedriver-patch download, profile handling, Xvfb, port pick, and (worst) a **hardcoded `rutracker.org` URL** for cookie injection baked into the generic browser layer. This is the single biggest blocker to "any chromium-based browser" and "add more sources" from the spec. |
| A2 | `src/search/rutracker.rs`, `src/app.rs`, `src/main.rs` | There is no `Source` trait. Rutracker's search/login/parsing is called directly by name from the orchestrator and from the browser layer. Adding rutor/nnm-club means editing `app.rs`, `cdp.rs`, and `main.rs` again instead of adding one file and registering it. |
| A3 | `src/credentials/mod.rs` | Storage is a single hardcoded slot for exactly one username/password pair. The spec's "tabs per resource, each with its own saved login" needs a keyed store (`resource_id -> Credential`), not a single ciphertext blob. |
| A4 | `src/ui/app.rs` (1350 lines) | `App` (UI state) mixes: input routing, five different render functions, the entire Settings modal (construction + key handling + drawing, ~500 lines by itself), the Login modal, and the Health-check modal, all in one file/`impl` block. `SettingsItem.value` is a formatted **display string**, not a typed value — there's no way to read a setting back out programmatically, which is exactly why B5 exists: nothing forces every item to be backed by real state. |
| A5 | `Cargo.toml` | `chromiumoxide` + `chromiumoxide_cdp` are declared dependencies, fully unused (grep confirms zero references) — dead weight on every build. `axum` is used only for the extension bridge (fine, just noting it's single-purpose). |
| A6 | `src/ui/zones.rs` | Solid, actually — fixed-height zones (log/torrent = 8 rows) rather than proportional, but the toggle/focus/fullscreen model is clean and worth keeping as-is. |
| A7 | General | `AGENTS.md`'s own rule ("`.unwrap()` never in production code") is respected almost everywhere (4 hits total, in log/bridge init paths) — codebase discipline is actually fine here, the problems are structural/missing-feature, not sloppy Rust. |

### Naming
| # | Issue |
|---|-------|
| N1 | `headless`/`gui` terminology throughout (`BrowserMode`, `App.headless`, config key, CLI flag, temp-dir names, log lines) — **done, see Phase 1 below.** |

---

## 2. Target architecture

```
src/
├── sources/                     # was: search/ (rutracker-only)
│   ├── mod.rs                   # Source trait + SourceRegistry
│   ├── rutracker.rs             # existing logic, now impl Source
│   └── models.rs                # TorrentItem (source-agnostic; add `source_id`)
│
├── browser/
│   ├── mod.rs
│   ├── kind.rs                  # BrowserKind: Chrome, Chromium, Brave, Helium
│   ├── detect.rs                # detection + user-configurable priority order
│   ├── driver.rs                # BrowserDriver trait: launch/navigate/cookies/close
│   ├── chromium_driver.rs       # today's cdp.rs, generalized: no hardcoded URLs,
│   │                            # cookie-injection target comes from the Source
│   └── cloudflare.rs
│
├── credentials/
│   └── mod.rs                   # keyed store: HashMap<ResourceId, Credential>,
│                                 # same AES-128-GCM, JSON payload instead of "u:p"
│
├── torrent/                     # NEW: torrent lifecycle, separate from the UI
│   ├── mod.rs
│   ├── manager.rs                # poll loop against TorrServer, owns TorrentStatus
│   └── client.rs                 # was torrserver/api.rs — add pause/resume/drop/list
│
├── config/                      # was config.rs — split when it grows past ~150 lines
│   ├── mod.rs                   # Config struct + load/save (save was missing!)
│   └── schema.rs                # typed settings definitions (see Options design)
│
└── ui/
    ├── app.rs                   # orchestrator glue only — no modal bodies inline
    ├── modals/
    │   ├── settings.rs          # Options: state + render + key handling, one file
    │   ├── login.rs             # Login: tabbed per-resource, Save button
    │   └── health.rs
    ├── zones.rs                 # unchanged, it's fine
    ├── theme.rs
    ├── menu.rs
    └── widgets/
        └── dot_graph.rs         # NEW: btop-style braille/dot sparkline for progress
```

Key design decisions:

- **`Source` trait** (`search/*` → `sources/*`): `search()`, `login()`, `resolve_download_url()`,
  `id() -> &'static str`, `display_name()`. Rutracker becomes the first implementor.
  The orchestrator and browser layer talk only to `dyn Source` / a `SourceRegistry`,
  never to `rutracker.rs` by name. This is what makes "add rutor next" a
  one-file, zero-edit-elsewhere change, and what makes the Options "Sources"
  checklist (spec section 2) just iterate the registry.

- **`BrowserDriver` trait**: `launch`, `navigate`, `cookies`, `inject_cookies`, `close`.
  One implementation today (`chromium_driver.rs`, covers Chrome/Chromium/Brave/Helium
  since they're all Chromium-based and speak the same CDP/WebDriver protocol) —
  the trait boundary exists so a non-Chromium engine is a new file, not a rewrite.
  The cookie-injection target URL moves from a hardcoded string into a parameter
  supplied by the calling `Source`.

- **Typed settings, not display strings**: each setting is a real enum/bool/int field
  living on `Config` (already persisted) or on runtime `App` state, with a small
  descriptor table `{ label, description, kind: Bool|Enum|Int|Action, get, set }`
  used purely for rendering + pagination. This is what fixes B1/B2/B5 at the root
  instead of patching the symptom: the position indicator and page count are
  computed from `descriptor_table.len()` and the real content height, and every
  toggle round-trips through an actual field, so "add a new Option" can never again
  produce a decorative no-op.

- **`torrent::Manager`**: a background task polling TorrServer's `/torrents` list on
  the configured `update_ms` interval, owning the single source of truth for
  `TorrentStatus` (fixes B3), and exposing pause/resume/drop. The UI only ever reads
  a snapshot.

- **Credentials keyed by resource id**: `HashMap<String, Credential>` where key is
  the `Source::id()` (`"rutracker"`, later `"rutor"`, ...), JSON-serialized before
  encryption (fixes B4, and is exactly the storage shape the multi-tab Login panel
  needs).

---

## 3. UI/UX plan (btop++ reference)

### Options (Settings modal rewrite)
- Backed entirely by the typed descriptor table above — no more fake values.
- Categories: `general`, `streaming` (was "app" — renamed per spec, browser +
  torrserver + sources), `download` (new).
- Pagination: page size = `content_height / 2` computed once per frame and stored
  back on the state so key handling reads the same number the renderer used —
  kills B2. Automatically drops to 2 pages when the window/font makes one page not
  fit, automatically collapses back to 1 page when there's room, every frame.
- `general`: color theme, theme background, truecolor, false tty, vim keys,
  disable mouse, disable presets, presets, show boxes, update ms, rounded corners,
  terminal sync, graph symbol, health check, save config on exit.
- `streaming`: browser visible, prioritize browser (ordered pick of
  chrome/chromium/brave/helium), close on exit, save cookies, save credentials,
  edit credentials (opens the Login modal directly, no separate top-level login
  entry point per spec), torrserver enable/disable (prompts sudo inline if needed,
  polls service status after a short delay, reports up/down), sources checklist
  (rutracker today, layout ready for more).
- `download`: enable downloading, downloads directory (default/custom, 3 custom
  slots), sequential download toggle, speed/limits, close on exit (kills the
  background torrent core).
- Mouse: every row clickable (click = select + toggle/cycle in one action, matching
  btop), scroll wheel changes pages.

### Login modal
- Restyled to match the Settings chrome exactly (same border/tab treatment,
  xray-style highlighted-tab-in-brackets like today's Settings tabs).
- Tabs = one per resource (`Source::id()`), so adding rutor later adds a tab for
  free.
- `Save` button persists via the keyed credential store; a transient "Saved ✓"
  indicator (2–3s, theme-colored) confirms it — mirrors btop's save-confirmation
  pattern.
- Reachable only from Options → streaming → "Edit credentials" (removed from the
  top-level keybinding per spec).

### Torrent panel
- Replace the flat `[####    ] 0%` progress bar with a btop-style dot/braille
  sparkline (`ui/widgets/dot_graph.rs`) driven by a short rolling history of
  progress/speed samples from `torrent::Manager`, same visual language as btop's
  CPU/mem graphs.
- Add pause / resume / drop bound to keys + mouse clicks on the row, wired to the
  new `torrent::client` API methods.

### Mouse & scroll, everywhere
- Every bracket-hint in the top bar (`s: search`, `a: login`, `S: settings`,
  `L: log`, `F: filter`) becomes a real click target dispatching the same action
  as its key.
- Scroll wheel works in every zone (results, log, settings, torrent list), not
  just the log panel as today.

---

## 4. Phased plan

| Phase | Scope | Status |
|-------|-------|--------|
| 0 | Audit (this document) | ✅ done |
| 1 | Rename headless/gui → hidden/visible everywhere (enum, config key + legacy alias, CLI flag, App field, UI strings, tests). Default flipped to hidden. | ✅ done — commit `ca2bf4e` |
| 2 | Browser abstraction: add `BrowserKind::Chromium`, configurable priority list, extract `BrowserDriver` trait, remove hardcoded rutracker URL from `cdp.rs` | ✅ done — commit `b3bb94e` (Note: `BrowserDriver` *trait* extraction itself deferred to Phase 3, since it's cleanest to do alongside the `Source` trait — see below) |
| 3 | `Source` trait + `search/` → `sources/` rename, Rutracker as first impl, registry wired into orchestrator | 🔶 part 1 done — commit `fc03190`: trait + `impl Source for RutrackerSearcher` + `KNOWN_SOURCES` list wired into health check. **Not yet done:** the `search/` → `sources/` directory rename, and rewiring `app.rs`/`main.rs` to call through `dyn Source` instead of `RutrackerSearcher` directly (deferred deliberately — see commit message — until part 1 is confirmed to actually compile, since `async-trait` + `Send + Sync` bounds on a struct holding `fantoccini::Client` is the one part of this phase genuinely worth a compiler's opinion before building further on top). |
| 4 | Credentials: keyed store + JSON payload (fixes B4), multi-resource aware | ✅ done — commit `1fd5cc6` |
| 5 | Settings rewrite: typed descriptor table, dynamic pagination (fixes B1/B2/B5), general/streaming/download categories per spec, config save-on-exit actually implemented | ✅ done. Fixes B1 (hardcoded "3/8"), B2 (page-size desync), B5 (decorative values with no backing field), and a newly-found B6 (Browser visible toggle didn't affect the actual launch). `Config` gained 26 new persisted fields + `save()`/`load()` across this and the follow-up commit below. All three categories (`general`, `streaming`, `download`) now read/write real state; every toggle actually does something -- either a real behavioral effect (vim_keys, disable_mouse, save_config_on_exit, theme selection+persistence, presets via `ZoneLayout::apply_preset`, save_cookies/save_credentials gating do_login's actual save calls, enabled_sources gating whether `start_search` runs at all, Prioritize browser reordering the real probe order, TorrServer status check via the real `is_reachable()`) or an honestly-labeled "persisted but not yet consumed" state for things with no subsystem to hook into yet (`truecolor`/`false_tty`: no color-degradation renderer; `rounded_corners`/`show_boxes`/`terminal_sync`: needs the shared block-builder refactor in Phase 10; `update_ms`/`graph_symbol`: nothing consumes them until Phases 7/8 exist; TorrServer start/stop-with-sudo, download speed limits, sequential download: no `torrent::Manager` to send them to yet, Phase 7). |
| 6 | Login modal restyle: tabs per resource, Save button + saved-indicator, moved behind Options | ⏳ planned. (The "moved behind Options" half is already done as part of Phase 5 -- `Edit credentials` under Options → streaming opens the Login modal, and it's no longer reachable from the top-level keybind. The tabs/Save-button/saved-indicator restyle itself is still open.) |
| 7 | `torrent::Manager` polling loop (fixes B3) + pause/resume/drop + TorrServer enable/disable-with-sudo-prompt in Options | ⏳ planned |
| 8 | Torrent panel dot/braille progress graph (btop-style) | ⏳ planned |
| 9 | Mouse: clickable keybind hints, scroll in every zone | ⏳ planned |
| 10 | Cleanup: drop unused `chromiumoxide`/`chromiumoxide_cdp` deps, split `ui/app.rs` into `ui/modals/*`, update `AGENTS.md` to match the new module layout | ⏳ planned |

Each phase lands as its own commit (or short commit series) on this branch so it's
reviewable and bisectable. Build/test after every phase before starting the next.
