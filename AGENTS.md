# AGENTS.md — Doris Development Constraints

## Code Style (MANDATORY)

- Rust 2021 edition
- `snake_case` for variables, functions, modules
- `PascalCase` for types, enums, traits
- `SCREAMING_SNAKE_CASE` for constants
- Max line length: 100 characters
- No trailing whitespace
- No unused imports or variables (use `_` prefix for intentionally unused)
- `#[allow(dead_code)]` only for struct fields, never for functions
- Always use `?` operator for error propagation, never `.unwrap()` in production code
- `match` must be exhaustive or have `_ =>` arm
- `pub` only for items that are part of the public API
- Module visibility: `pub mod` for top-level, private for internals

## Architecture

This tree reflects the current, refactored layout (see ROADMAP.md for the
full audit + phase history of how it got here from the original "FIXED"
version of this document). It's still the intended shape going forward,
just no longer frozen — Phase 3/10 of that refactor deliberately restructured
`sources/` toward a `Source`-trait model, split `ui/app.rs`'s widgets out, and
-- in Phase 10's last step -- split its three modals into `ui/modals/*`, so
`ui/app.rs` is now the non-modal UI only.

```
src/
├── main.rs          # Entry point, CLI parsing (run_cli asks the registry's sources)
├── lib.rs           # Module declarations
├── cli.rs           # CLI argument definitions (incl. --source)
├── config.rs        # Config file handling (~40 persisted Options fields)
├── event.rs         # Event enum + EventHandler
├── tui.rs           # Terminal init/restore
├── log.rs           # File logger
├── app.rs           # App orchestrator (event loop, spawn)
├── browser/
│   ├── mod.rs
│   ├── cdp.rs       # Browser automation (chromedriver, fantoccini)
│   ├── detect.rs    # BrowserKind (chrome/chromium/brave/helium) + priority-ordered detection
│   └── cloudflare.rs # Cloudflare bypass patches
├── sources/
│   ├── mod.rs
│   ├── source.rs    # Source trait + KNOWN_SOURCES registry (add new sources here)
│   ├── orchestrator.rs # Concurrent dispatch: selected_sources, per-source cursors, cache wiring
│   ├── net.rs       # fetch_resilient, first_ok multi-host failover
│   ├── cache.rs     # TTL cache
│   ├── ordering.rs  # dedupe_by_hash, default_order, sort cycle
│   ├── magnet.rs    # magnet build/parse, info-hash normalization
│   ├── format.rs    # size/date parsing shared by the HTML sources
│   ├── models.rs    # TorrentItem + resolve_url
│   ├── cookies.rs   # Cookie load/save/parse (Netscape)
│   ├── rutracker.rs # Search + auth logic; impl Source; f[] category filter
│   ├── rutor.rs     # Rubric-id fan-out category filter
│   ├── yts.rs       # Movies only, multi-host
│   ├── tpb.rs       # apibay Movies+TV via GROUP_CATS
│   ├── eztv.rs      # TV only; query refused (its API ignores `search`)
│   ├── subsplease.rs # Anime, one row per episode
│   ├── nyaa.rs      # Anime; per-row group from nyaa:categoryId
│   ├── nnmclub.rs   # windows-1251 tracker; f[] forum-id category filter
│   └── x1337x.rs    # Mirrors, category paths, client-side OR-filter fallback
├── torrserver/
│   ├── mod.rs
│   └── api.rs       # TorrServer HTTP API (list/get/pause/resume/remove)
├── torrent/
│   └── mod.rs       # Manager: background poller feeding live torrent status
├── bridge/
│   ├── mod.rs
│   └── handler.rs   # Extension bridge server
├── credentials/
│   └── mod.rs       # AES-128-GCM credential encryption, keyed by resource id
└── ui/
    ├── mod.rs       # UI module declarations
    ├── app.rs       # TUI state + non-modal rendering (zones, menu, main view, modal dispatcher)
    ├── modals/      # modal dialogs; split out in Phase 10 (see ROADMAP.md)
    │   ├── mod.rs
    │   ├── settings.rs # typed descriptor table: Options modal (pagination, keys, item builders)
    │   ├── login.rs   # login modal: resource tabs, Ctrl+S save, saved-indicator
    │   ├── health.rs  # health check modal: browser/TorrServer/credentials/cookies/sources
    │   └── help.rs    # help page (btop's `helpMenu`): Key:/Description: table + paging
    ├── menu.rs      # btop-style main menu
    ├── theme.rs     # Theme system (colors, gradients)
    ├── zones.rs     # Zone layout system (toggle, focus, presets)
    └── widgets/
        ├── mod.rs
        └── graph.rs # btop-style history sparkline (braille/block/dot)
```

The single list of sources is `KNOWN_SOURCES` in `sources/source.rs` (one
entry per source: implemented flag, groups, browser need, home URL) -- add
new sources there, not in a second hand-written list; the Options sources
list, the tab bar and the CLI all derive from it.

## UI Design (btop-inspired)

### Layout
- Search input: top bar (always visible, not a zone)
- Zones below search bar

### Zone System (4 zones)
- **Zone 1 (Results)**: Table with torrent results (seeds, size, date, title)
  - Navigation: j/k, PgUp/PgDn, Enter to play
  - F key: filter results by title (type filter text, Enter to apply, Esc to clear)
- **Zone 2 (Torrent)**: Live status of the tracked torrent, polled from TorrServer by `torrent::Manager` (not static/decorative -- see ROADMAP.md bug B3)
  - Hash, title, status (shows "(paused)" when client-side-paused)
  - btop-style braille/block/dot history sparkline (`ui/widgets/graph.rs`), not a plain fill bar
  - DL/UL speed, downloaded/total, seeds, peers
  - `p`: pause/resume (TorrServer `drop`/`get`), `d`: remove -- both keyboard and click (see the frame legend below)
- **Zone 3 (Log)**: Short log panel
  - Scroll with mouse/j/k
- **Zone 4 (Extra)**: TBD

### Frame legend (btop-style)

Keybinds for a zone are written **on its border**, not inside it (btop's
`filter`/`pause`/`kill`/`signals` row). The word is `title` colour and the
character that triggers it is `hi_fg` + bold -- the highlight marks the
hotkey, not the alphabet, so `pause` leads with `p` only because that key is
free here, while `source`/`info`/`play` trail their `]`/`v`/`⏎`.

- Buttons come from `zone_buttons()` (`src/ui/zones.rs`), one table per zone;
  `zone_title()` draws the superscript number in `hi_fg` + bold and the label
  in `title`.
- `App::frame_layout()` (`src/ui/app.rs`) is the single source of truth for
  where each button lands: the renderer draws into those rects and
  `click_at` tests them, so drawn == clickable. Anything that does not fit
  is dropped rather than clipped (narrow zones lose the right-hand cluster,
  exactly like btop's `if (width > 60 + sort_len)`).
- Buttons `ui::App` can act on (filter, group, source) happen inside
  `click_at`; the rest come back as a `UiAction` for the orchestrator.

### Menu System
- ASCII art banner "T-HUNTER"
- 3 items: Options, Help, Quit
- Navigation: j/k/Tab, Enter to select
- `m` key toggles menu from any view
- Options opens Settings modal

### Theme System
- Dark theme by default
- Colors defined in `Theme` struct
- Gradient arrays for meters/graphs
- User themes in `~/.config/doris/themes/`

### Input Routing
- When menu shown: menu keys only
- When modal shown: modal keys only
- When filter_mode: typing goes to filter, Esc/Enter to exit
- When input_mode: typing goes to search
- Otherwise: zone-aware routing based on focused zone

### Zone Controls
- Tab/Shift+Tab: cycle focus between visible zones
- F: toggle fullscreen for focused zone
- 1-4: toggle zone visibility

## Key Bindings
- `s`/`i`: enter search input mode
- `Enter`: search (in input mode) or play (in results mode)
- `b`: browse mode -- an empty query asking the browse-capable sources for their freshest rows (takes the `all` tab and category with it)
- `S`: open settings modal (login is now here too: streaming -> Edit credentials -- there's no top-level login keybind anymore)
- `L`: toggle detailed log view
- `F`: enter filter mode (type to filter results)
- `f`: toggle fullscreen for focused zone
- `m`: open main menu
- `1-4`: toggle zone visibility
- `Tab`/`Shift+Tab`: cycle zone focus
- `j`/`k`/`Up`/`Down`: navigate within focused zone (`j`/`k` only when Options -> general -> Vim keys is on; arrows always work)
- `g`/`G`: cycle the category row (forward/back; an empty query is browse mode, not a category)
- `]`: cycle the source tab
- `p`/`d`: pause-or-resume / remove the tracked torrent, when the Torrent zone is focused
- `Esc`: close modal / exit input mode / exit filter mode
- `?`/`/`/`F1`: open the help page (`ui/modals/help.rs`, btop's `helpMenu`)
- Mouse: click any zone to focus it, click a frame button to
  trigger it, click the search box to start typing, scroll wheel over any
  zone to scroll/navigate it -- see ROADMAP.md Phase 9

## Dependencies

Kept stable unless a phase in ROADMAP.md has a specific, documented reason to
change the list (Phase 3 added `async-trait` for the `Source` trait object;
Phase 10 removed the unused `chromiumoxide`/`chromiumoxide_cdp`). Otherwise,
don't add or bump dependencies speculatively.

- ratatui 0.29
- crossterm 0.28
- fantoccini 0.22.1
- reqwest (rustls-tls)
- rusqlite (bundled)
- ring 0.17 (AES-128-GCM)
- tokio (full)
- serde + toml
- serial_test 3
- async-trait 0.1 (dyn-compatible async fns on the `Source` trait)

## Testing

- Run: `cargo test`
- Tests in `tests/` directory
- Use `#[serial]` for tests that modify global state
- Test file logger, cookies, credentials, models, TUI, config, TorrServer
  API parsing, and the sparkline widget
- Live probes of a real site are `#[ignore]`d tests (`*_live_tests.rs`) run
  with `cargo test --test <name> -- --ignored --nocapture`; give them
  `--test-threads=1` when two browsers at once have been seen to kill a
  session mid-test, and never let them run in CI

## Git

- Commit messages: imperative mood, explain *why* not just *what* for
  anything non-mechanical -- see the commit history on
  `refactor/audit-and-architecture` for the expected level of detail
- Never commit secrets or keys
- Binary at `target/release/doris`
