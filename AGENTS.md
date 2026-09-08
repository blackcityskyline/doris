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

## Architecture (FIXED)

```
src/
├── main.rs          # Entry point, CLI parsing
├── lib.rs           # Module declarations
├── cli.rs           # CLI argument definitions
├── config.rs        # Config file handling
├── event.rs         # Event enum + EventHandler
├── tui.rs           # Terminal init/restore
├── log.rs           # File logger
├── app.rs           # App orchestrator (event loop, spawn)
├── browser/
│   ├── cdp.rs       # Browser automation (chromedriver, fantoccini)
│   ├── detect.rs    # Browser detection
│   └── cloudflare.rs # Cloudflare bypass patches
├── search/
│   ├── rutracker.rs # Search + auth logic
│   ├── cookies.rs   # Cookie load/save/parse
│   └── models.rs    # TorrentItem + resolve_url
├── torrserver/
│   └── api.rs       # TorrServer HTTP API
├── bridge/
│   └── handler.rs   # Extension bridge server
├── credentials/
│   └── mod.rs       # AES-128-GCM credential encryption
└── ui/
    ├── mod.rs       # UI module declarations
    ├── app.rs       # TUI state + rendering (zones, modals)
    ├── draw.rs      # Draw primitives (box, banner, meter)
    ├── menu.rs      # btop-style main menu
    ├── theme.rs     # Theme system (colors, gradients)
    └── zones.rs     # Zone layout system (toggle, focus)
```

## UI Design (btop-inspired)

### Layout
- Search input: top bar (always visible, not a zone)
- Zones below search bar

### Zone System (4 zones)
- **Zone 1 (Results)**: Table with torrent results (seeds, size, date, title)
  - Navigation: j/k, PgUp/PgDn, Enter to play
  - F key: filter results by title (type filter text, Enter to apply, Esc to clear)
- **Zone 2 (Torrent)**: Current torrent status
  - Hash, title, status
  - Progress bar with percentage
  - DL/UL speed, downloaded/total
  - Seeds, peers count
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
- `a`: open login modal
- `S`: open settings modal
- `L`: toggle detailed log view
- `F`: enter filter mode (type to filter results)
- `f`: toggle fullscreen for focused zone
- `m`: open main menu
- `1-4`: toggle zone visibility
- `Tab`/`Shift+Tab`: cycle zone focus
- `j`/`k`/`Up`/`Down`: navigate within focused zone
- `Esc`: close modal / exit input mode / exit filter mode

## Dependencies (DO NOT CHANGE)

- ratatui 0.29
- crossterm 0.28
- fantoccini 0.22.1
- reqwest (rustls-tls)
- rusqlite (bundled)
- ring 0.17 (AES-128-GCM)
- tokio (full)
- serde + toml
- serial_test 3

## Testing

- Run: `cargo test`
- Tests in `tests/` directory
- Use `#[serial]` for tests that modify global state
- Test file logger, cookies, credentials, models, TUI

## Git

- Commit messages: imperative mood, < 72 chars
- Never commit secrets or keys
- Binary at `target/release/doris`
