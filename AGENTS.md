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
`search/` toward a `Source`-trait model and split `ui/app.rs`'s widgets out,
and more of that kind of evolution is expected as sources/features are added.

```
src/
├── main.rs          # Entry point, CLI parsing
├── lib.rs           # Module declarations
├── cli.rs           # CLI argument definitions
├── config.rs        # Config file handling (~40 persisted Options fields)
├── event.rs         # Event enum + EventHandler
├── tui.rs           # Terminal init/restore
├── log.rs           # File logger
├── app.rs           # App orchestrator (event loop, spawn)
├── browser/
│   ├── cdp.rs       # Browser automation (chromedriver, fantoccini)
│   ├── detect.rs    # BrowserKind (chrome/chromium/brave/helium) + priority-ordered detection
│   └── cloudflare.rs # Cloudflare bypass patches
├── search/
│   ├── source.rs    # Source trait + KNOWN_SOURCES registry (add new sources here)
│   ├── rutracker.rs # Search + auth logic; impl Source
│   ├── cookies.rs   # Cookie load/save/parse
│   └── models.rs    # TorrentItem + resolve_url
├── torrserver/
│   └── api.rs       # TorrServer HTTP API (list/get/pause/resume/remove)
├── torrent/
│   └── mod.rs       # Manager: background poller feeding live torrent status
├── bridge/
│   └── handler.rs   # Extension bridge server
├── credentials/
│   └── mod.rs       # AES-128-GCM credential encryption, keyed by resource id
└── ui/
    ├── mod.rs       # UI module declarations
    ├── app.rs       # TUI state + rendering (zones, modals) -- large; see ROADMAP.md Phase 10 for the planned ui/modals/* split, not yet done
    ├── menu.rs      # btop-style main menu
    ├── theme.rs     # Theme system (colors, gradients)
    ├── zones.rs     # Zone layout system (toggle, focus, presets)
    └── widgets/
        └── graph.rs # btop-style history sparkline (braille/block/dot)
```

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
  - `p`: pause/resume (TorrServer `drop`/`get`), `d`: remove -- both keyboard and click (the hint line itself is a click target)
- **Zone 3 (Log)**: Short log panel
  - Scroll with mouse/j/k
- **Zone 4 (Extra)**: TBD

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
- `S`: open settings modal (login is now here too: streaming -> Edit credentials -- there's no top-level login keybind anymore)
- `L`: toggle detailed log view
- `F`: enter filter mode (type to filter results)
- `f`: toggle fullscreen for focused zone
- `m`: open main menu
- `1-4`: toggle zone visibility
- `Tab`/`Shift+Tab`: cycle zone focus
- `j`/`k`/`Up`/`Down`: navigate within focused zone (`j`/`k` only when Options -> general -> Vim keys is on; arrows always work)
- `p`/`d`: pause-or-resume / remove the tracked torrent, when the Torrent zone is focused
- `Esc`: close modal / exit input mode / exit filter mode
- Mouse: click any zone to focus it, click any header hint (s/S/L/F) to trigger it, click the Torrent panel's pause/remove hint, scroll wheel over any zone to scroll/navigate it -- see ROADMAP.md Phase 9

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

## Git

- Commit messages: imperative mood, explain *why* not just *what* for
  anything non-mechanical -- see the commit history on
  `refactor/audit-and-architecture` for the expected level of detail
- Never commit secrets or keys
- Binary at `target/release/doris`
