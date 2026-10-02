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

This is the current layout. The audit that reshaped it (the "FIXED" version
of this document -> a `Source` trait, `ui/app.rs`'s widgets split out, and
its three modals split into `ui/modals/*`, leaving `ui/app.rs` the
non-modal UI only) is a closed chapter: `ROADMAP.md`, `REFACTOR_PLAN.md`
and `UI_REFACTOR_PLAN.md` were retired from the repository, together with
their history. A copy of all three lives outside the tree at
`/home/black/dev/me/doris-plans-backup/`. Markers in the comments below
("bug B3", "Phase 7", "B8 wave 3") name sections of that retired document,
nothing else.

```
src/
├── main.rs          # Entry point, CLI parsing (run_cli asks the registry's sources)
├── lib.rs           # Module declarations
├── cli.rs           # CLI argument definitions (incl. --source)
├── config.rs        # Config file handling (33 persisted fields)
├── event.rs         # Event enum + EventHandler
├── tui.rs           # Terminal init/restore
├── log.rs           # File logger
├── player_log.rs    # MPV stderr keyword filter (should_log)
├── filter.rs        # the grep-shaped Results filter: Filter::parse / matches
├── search.rs        # search-domain free fns: source_outcome_line,
│                    #   apply_source_done, finish_search, resolve_cookie_file
├── app.rs           # App orchestrator (event loop, spawn)
├── browser/
│   ├── mod.rs
│   ├── cdp.rs       # Browser automation (chromedriver, fantoccini)
│   ├── detect.rs    # BrowserKind + priority-ordered detection; one BROWSER_ROWS
│   │                  #   entry per browser (key, aliases, binaries, label)
│   └── cloudflare.rs # Cloudflare bypass patches
├── sources/
│   ├── mod.rs
│   ├── source.rs    # Source trait + KNOWN_SOURCES registry (add new sources here)
│   ├── orchestrator.rs # Concurrent dispatch: selected_sources, per-source cursors, cache wiring
│   ├── net.rs       # fetch_resilient, first_ok multi-host failover
│   ├── cache.rs     # TTL cache
│   ├── ordering.rs  # dedupe_by_hash, default_order
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
│   ├── torentino.rs # Games only, DLE tracker: POST search, no browse feed
│   └── x1337x.rs    # Mirrors, category paths, client-side AND filter
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
    ├── modals/      # modal dialogs, split out of `ui/app.rs`
    │   ├── mod.rs
    │   ├── settings.rs # typed descriptor table: Options modal (pagination, keys, item builders)
    │   ├── login.rs   # login modal: resource tabs, Ctrl+S save, saved-indicator
    │   ├── health.rs  # health check modal: browser/TorrServer/credentials/cookies/sources
    │   ├── help.rs    # help page: Key:/Description: table + paging
    │   └── detail.rs  # torrent detail modal (П.7): row facts + the file list its source reads
    ├── menu.rs      # the main menu
    ├── theme.rs     # Theme system: the three optional accents, each with a fallback
    ├── zones.rs     # Zone layout system (toggle, focus, presets, focus marker);
    │                  #   key_char/label/from_key read one ZONE_ROWS entry
    └── widgets/
        ├── mod.rs
        └── graph.rs # history sparkline (braille/block/dot)
```

The single list of sources is `KNOWN_SOURCES` in `sources/source.rs` (one
entry per source: implemented flag, groups, browser need, home URL) -- add
new sources there, not in a second hand-written list; the Options sources
list, the tab bar and the CLI all derive from it.

## UI Design

### Layout
- Search input: top bar (always visible, not a zone)
- Zones below search bar

### Zone System (4 zones)

Keys `1`-`4` move the focus to that zone (showing it first if it was
hidden); a second press on the zone already focused hides it, and the
focus walks to the next zone still on screen. `5` is deliberately unused.

- **Zone 1 (Results)**: Table with torrent results (seeds, size, date, title)
  - Navigation: j/k, PgUp/PgDn (a page is half the terminal), Enter to play
  - f key: filter results -- a grep-shaped syntax (see below); type it, Enter
    to apply, Esc to clear
  - `R` takes the frame over with the detail view: the table full height plus
    a preview line naming the selected row's facts
  - `v` logs the selected row's details, `d` downloads it to disk, `Shift+Enter`/`D` opens the detail modal
- **Zone 2 (Torrent)**: Live status of the tracked torrent, polled from TorrServer by `torrent::Manager` (a background poller, never a static panel)
  - Hash, title, status (shows "(paused)" when client-side-paused)
  - braille/block/dot history sparkline (`ui/widgets/graph.rs`), not a plain fill bar
  - DL/UL speed, downloaded/total, seeds, peers
  - `p`: pause/resume (TorrServer `drop`/`get`), `d`: remove -- both keyboard and click (see the frame legend below)
  - `T` takes the frame over with the detail view: every fact on its own line,
    name included, bar at full terminal width (the panel caps it at 50)
- **Zone 3 (Trackers)**: the trackers (sources) checklist -- `[x] all` on top, then one row per
  registered source. j/k move the cursor (wrapping), Enter switches the row, clicking
  a row switches it. This is the only place sources are switched: the Results tab
  bar it replaced now just *displays* the selection on its frame (`[all]`,
  `[rutracker, yts]`, `[none]`)
- **Zone 4 (Log)**: Short log panel
  - Scroll with mouse/j/k; `L` flips to the full log
  - The three detail views (`L`/`T`/`R`) share one state,
    `ui::App::detail_view: Option<ZoneId>`: while one is up it owns the
    keyboard (no zone digits, no search box), Esc or its own key closes it,
    and the other two jump straight across.

### Frame legend

Keybinds for a zone are written **on its border**, not inside it (the
`filter`/`pause`/`kill`/`signals` row). The word is `title` colour and the
character that triggers it is `hi_fg` + bold -- the highlight marks the
hotkey, not the alphabet, so `pause` leads with `p` only because that key is
free here. Both tokens are mandatory in every theme file, so a theme
cannot name a keybind the colour of ordinary text; `hi_fg` is the whole
reason the keybind is readable. What is drawn today: `f filter` and `g group` on Results,
`p pause` and `d delete` on Torrent, and nothing on Log or Trackers (their
rows *are* the controls; Log's `L` is drawn in its title instead). The bottom action row
(`play ⏎` / `download d` / `info v`) is gone -- those three are keyboard
and help-page actions now, and a legend that repeats them would be a
second place documenting the same keys.

- Buttons come from `zone_buttons()` (`src/ui/layout.rs`), one table per
  zone, each drawn bracketed as `┌word┐` in `div_line` so it reads as a
  control and not as more of the panel title; the brackets go away with
  `show_boxes`, since there is no frame to bracket against then.
  `zone_title()` draws the label in `title` and both of its keybinds -- the
  superscript digit and the detail-view key (`L`/`T`/`R`, the label's first
  letter) -- in `hi_fg` + bold, the same mark `button_spans` puts on a
  frame button's hotkey. Trackers has no detail view, so its label stays
  one plain span.
- `App::frame_layout()` (`src/ui/app.rs`) is the single source of truth for
  where each button lands: the renderer draws into those rects and
  `click_at` tests them, so drawn == clickable. Anything that does not fit
  is dropped rather than clipped (narrow zones lose the right-hand cluster,
  exactly like the width guards around them).
- Buttons `ui::App` can act on (filter, group) happen inside
  `click_at`; the rest come back as a `UiAction` for the orchestrator.

### Colour distribution (theme tokens)

Two layers, and the split between them is what makes a label readable.

**Keybinds and words** come from the mandatory fields, which every theme
file has:

- `title` -- the word in a label: `Results`, `filter`, `group`, `Search`
- `hi_fg` -- every keybind glyph, always bold: the letter inside a frame
  word, the zone's superscript digit, the panel's full-view letter, the
  category arrows, help's and Options' key columns, the paging arrows,
  and a hovered button (which also underlines)

Both are mandatory, so no theme can name a keybind the colour of ordinary
text and lose it against its own word. That is the whole reason the
keybind is readable, and it is why the `on_hover` accent that used to hold
this role is gone rather than left unused: it was optional, so one theme
could set it to `main_fg` and every keybind on screen would read as body
text.

**Structure** comes from three **optional** accents -- `primary`,
`secondary`, `error` -- each falling back to the classic field it replaced
(`hi_fg`, `hi_fg`, red), so the bundled themes keep drawing without being
edited. Where each one lands:

- `primary` -- focused frame border, frame info, modal and menu titles,
  section headers, the ASCII banner, the warning colour of a log line
- `secondary` -- labels that name a value (torrent facts, login fields),
  the seed column, a success line
- `error` -- a refused source, an `ERROR`/`✘` log line
- `main_fg` / `graph_text` / `div_line` -- body text, informational
  metadata (date, source badge), the frame border, anything not under the
  cursor

No `Color::Yellow`/`Green`/`Red`/`Cyan` left in zone, table or modal
rendering; the theme decides. A test walks every bundled theme to say no
`primary` of theirs may come out near-white, and two themes whose `hi_fg`
is white (`gotham`, `orange`) name a `primary` outright.

### Menu System
- ASCII art banner "DORIS"
- 3 items: Options, Help, Quit
- Navigation: j/k/Tab, Enter to select
- `m` key toggles menu from any view
- Options opens Settings modal; Help opens the same modal `?` does, and the
  menu closes behind it
- The picked item's **ascii glyphs** are highlighted (`menu_selected_bg`/
  `_fg`), never the spaces between them -- a background behind the spaces
  would be a solid stripe across the menu

### Theme System
- Dark theme by default
- Colors defined in `Theme` struct
- Gradient arrays for meters/graphs
- User themes in `~/.config/doris/themes/`

### Filter syntax (`src/filter.rs`)

The Results box after `f` is a grep-shaped mini-language, parsed once by
`Filter::parse` and matched per row by `Filter::matches` -- pure functions
over `TorrentItem`, no UI. Tokens are whitespace-split and ANDed:

| token          | matches                                     |
|----------------|---------------------------------------------|
| `word`         | substring of title+size+source+group        |
| `-word`        | NOT that                                     |
| `src:id`       | the tracker id (`tracker:` is the alias)     |
| `group:name`   | the category (`cat:` is the alias)           |
| `title:word`   | the title alone                              |
| `size:>1gb`    | `size_bytes`; units b/kb/mb/gb/tb, via `parse_size` |
| `seeds:>50`    | `seeds_n`; operators `> < >= <= =`           |

An unknown `word:` and an unparsable number both fall back to being plain
words -- a token that silently matches nothing reads as a broken app.

### Input Routing
- When menu shown: menu keys only
- When modal shown: modal keys only
- When filter_mode: typing goes to filter, Esc/Enter to exit
- When input_mode: typing goes to search
- Otherwise: zone-aware routing based on focused zone

### Zone Controls
- Tab/Shift+Tab: cycle focus between visible zones
- F: toggle fullscreen for focused zone
- 1-4: focus that zone; press it again on the focused zone to hide it

## Key Bindings
- `s`/`i`/`S`: enter search input mode (all three; `S` used to be Settings)
- `Enter`: search (in input mode), play (in results mode), or switch the row under the cursor (in the trackers panel)
- `Shift+Enter`/`D`: open the selected row's details (title, source, size, hash, magnet, page, and the file list its source can read)
- `b`: browse mode -- an empty query asking the browse-capable sources for their freshest rows (takes the all-category with it)
- `m`: open main menu (Options/Settings lives there now; there is no top-level settings keybind)
- `L`/`T`/`R`: toggle the Log / Torrent / Results detail view (a full-frame
  takeover; Esc or the same key closes it, the other two jump across)
- `f`: enter filter mode -- see "Filter syntax" above
- `F`: toggle fullscreen for focused zone
- `1-4`: focus that zone; press it again on the focused zone to hide it
- `Shift+P`: cycle the layout presets -- `config.presets`, a list of tiling
  specs written **rows via `,`**, **columns via `|`**, each zone named by its
  1/2/3/4 digit (`"1,3|4"` = Results across the top, Trackers beside Log
  under it). A cell may hold several digits (`"34"` == `"3|4"`), unknown
  characters are ignored, and the old flat `"1,2,3,4"` still means four
  single-cell rows -- so an existing config.toml keeps meaning what it meant.
  `App::new` applies `presets[preset_index]` at startup unless
  `disable_presets`; that first default preset is what hides Torrent.
- `Tab`/`Shift+Tab`: cycle zone focus
- `j`/`k`/`Up`/`Down`: navigate within focused zone (`j`/`k` only when Options -> general -> Vim keys is on; arrows always work)
- `g`/`G`: cycle the category (forward/back; an empty query is browse mode, not a category) --
  the `◀ name ▶` button on the Results frame does the same by mouse -- and **re-ask the
  checked sources for it**: rutracker/rutor/x1337x can only tag a row with the category they
  were *asked* for, so an `all`-searched table filtered to Movies would hide them. The re-ask
  keeps the old rows on screen until the first answer of the new round lands
- `p`/`d`: with the Torrent zone focused, pause-or-resume / remove the tracked torrent; with Results focused, `d` downloads the row's `.torrent` to disk
- `v`: with Results focused, log the selected row's details to the Log zone
- `Esc`: close a modal; leaves input / filter mode; **in the main view it
  opens the menu** (so `m` and `Esc` are the same key there)
- `?`/`/`/`F1`: open the help page (`ui/modals/help.rs`) --
  two tables, `keys` and `filter & grouping`, picked with `←`/`→`;
  `j`/`k`/`Tab` page whichever is showing
- Mouse: click any zone to focus it, click a frame button to
  trigger it, click a Trackers checkbox to switch that source, click the
  search box to start typing, scroll wheel over any zone to
  scroll/navigate it, drag the border between two zones to
  resize them like a WM (rows and cells alike; the weights are
  transient -- a `Shift+P` preset resets them)

## Dependencies

`Cargo.toml` is the full and current list -- 29 packages. Don't add or
bump one speculatively: `async-trait` (for the `Source` trait object) and
the removal of the unused `chromiumoxide`/`chromiumoxide_cdp` were each
made against a stated reason, and that is the bar. Three more came out the
other way -- `tracing`, `tracing-subscriber` and `futures-lite` had zero
references in `src/` and `tests/`.

The ones worth knowing what they are *for*, because that is not guessable
from their names:

| Package | Why it is here |
|---|---|
| `ratatui`, `crossterm` | the TUI and the terminal |
| `tokio` (full) | every async call in the app |
| `reqwest` (rustls-tls, no native-tls), `rustls` | HTTP; also multipart for the `.torrent` upload |
| `fantoccini` | the WebDriver side of the browser session |
| `rusqlite` (bundled) | reading cookies out of a Chrome profile (`cdp.rs`) -- no system SQLite |
| `ring` | AES-128-GCM for the credential store, SHA-256 for its key |
| `hostname`, `whoami`, `getrandom` | the parts of key derivation and nonce generation that are not in `ring` |
| `base64` | the credential file is base64 on disk |
| `scraper`, `regex`, `encoding_rs` | HTML sources; `encoding_rs` decodes windows-1251 (nnmclub) |
| `axum` | the extension bridge server |
| `clap`, `serde`, `toml`, `serde_json` | CLI, config, JSON payloads |
| `anyhow` | the error type every fallible call returns |
| `dirs` | `~/.config/doris` |
| `url`, `urlencoding`, `chrono`, `which` | URL building, query encoding, log timestamps, locating a browser |
| `unicode-width` | the ASCII-art banner in the menu has to be measured, not counted |
| `async-trait` | `dyn Source` needs boxed futures; native async-fn-in-traits is not object-safe |
| `serial_test` | tests that touch process-wide state (`HOME`, the logger) |

## Testing

- Run: `cargo test` -- 731 run, 40 `#[ignore]`d
- 66 test files in `tests/`, plus unit tests inside `src/` where a private
  item is what has to be tested (`app.rs` key routing, `tui.rs` byte
  sequences, `zones.rs` layout math). The split is by reachability, not by
  size: `src/app.rs::handle_key` is private, so the tests that pin what a
  key does live next to it.
- Use `#[serial]` for tests that touch process-wide state. A `#[serial]`
  only orders against other `#[serial]` tests, so anything that sets
  `HOME` or initialises the file logger wants a test file of its own --
  one process, one `HOME` to change. This is not theoretical: a pair of
  such tests shared a file with 36 others and flaked under a full
  parallel run.
- A test pins a *place*, not a function. Every claim here that was checked
  by mutation passed at least once with the fix reverted, and three times
  out of four the first version of a test did not: it called the function
  the fix lives in, while the decision is made one level up. When the
  claim is about routing, the test belongs where the routing is.
- Covered: the file logger, cookies, the credential store, `TorrentItem`,
  the TUI (rendered through `TestBackend`, so assertions are on the drawn
  buffer), config and its migrations, TorrServer API parsing over a real
  socket, theme contrast for all 40 bundled themes, and the sparkline.
- Live probes of a real site are `#[ignore]`d (`11` files,
  `*_live_tests.rs`), run with `cargo test --test <name> -- --ignored
  --nocapture`; give them `--test-threads=1` when two browsers at once have
  been seen to kill a session mid-test, and never let them run in CI.
- Gate before every commit: `cargo test`, `cargo clippy --all-targets`
  (0 warnings), `cargo fmt --check`, and `cargo doc --no-deps`
  (0 warnings -- it catches doc links to methods that no longer exist,
  which is worse than a broken link because it claims an API is there).

## Git

- Commit messages: imperative mood, explain *why* not just *what* for
  anything non-mechanical -- see the commit history on
  `refactor/audit-and-architecture` for the expected level of detail
- Never commit secrets or keys
- Binary at `target/release/doris`
