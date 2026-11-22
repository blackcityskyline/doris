# doris

A terminal torrent client: search trackers, play or download what they
return, and watch the download live. The interface is four
panels, keybinds written on the frames they belong to, a theme system,
no mouse required.

## What it does

- Searches ten trackers at once and merges the results into one table
- Plays through a TorrServer instance, or downloads the `.torrent`
- Shows the tracked torrent live: speeds, progress, seeds, history
- Has a login flow for the trackers that need one, with credentials
  stored encrypted at `0600`
- Reads its sources through a browser session when a tracker needs one,
  and asks it to ignore its own ad hosts

## Build

```
make            # fmt-check, clippy, test, build
make release    # target/release/doris
make gate       # what CI runs
```

Rust 2021, no system dependencies beyond a browser and (for playback)
TorrServer.

## Run

```
doris                      # open the menu
doris "dune 2021"          # search straight away
doris --source rutor       # one tracker instead of the checked ones
```

Options live in `~/.config/doris/config.toml`; `config.toml` in this
repository is the annotated example.

## Command line

The TUI's keys have commands behind them, and a front end drives those
instead of reimplementing a search. `--json` is global, and so are the
session flags (`--browser`, `--browser-visibility`, `--cookie-file`,
`--username`, `--password`), so they read the same before or after the
subcommand.

```
doris --json search "the matrix 1999"
doris --json search --stream "the matrix 1999"
doris --json play --row '<a row as search printed it>'
doris --json download --row '<the same row>'
```

`--stream` prints one JSON array per line as each source answers, instead of
one array when the last one is through. A front end draws the fast trackers'
rows in the first second instead of an empty window until rutracker's
Cloudflare challenge clears — measured 22.11.2026: four answers in 0.6 s, the
fifth at 35 s. A batch is ordered among itself; the merged order is what the
one-array form sorts. `--pages` beyond the first means nothing here.

`--row` is the contract: the row goes back exactly as it was printed, and
the command acts on that row — its magnet, its `.torrent` link, its page,
its source — with no second search and no index into a list whose order
may have moved. A tracker that needs a browser gets its session opened by
the command; on rutracker the first `.torrent` fetch answers 403 until the
topic page has been opened, so the command opens it and retries once.

The rest is in `doris --help`: `sources`, `torrent` (list, status, pause,
resume, remove, files, storage), `health`, `logs`, `config`, `login`.

## Keys

`?` opens the help page, which is the full list, in five pages split by
what a key is for: `1:keys`, `2:search`, `3:filter`, `4:downloads`,
`5:server` — switch with `←`/`→` or the digit. The short version:
`s` searches, `Enter` plays, `d` downloads, `1`–`5` focus a panel, `L`
`D` `T` `R` take a panel over the frame, `f` filters, `g` changes category,
`m` or `Esc` opens the menu, `q` quits.

In the Download view `D` takes over the frame: `j`/`k` move the cursor,
`p` pauses, `d` removes, `v` verifies, `f` opens the file list (per file,
what to fetch and what not), `o` opens the folder in a file manager, and
`+`/`-`/`0` walk the download-rate limit.

## Browser add-on

`browser-extension/` is a Firefox add-on that sends a title to doris from the
page you are reading it on: a `doris` button next to the title on IMDb, Trakt,
Kinopoisk and Lampa, and a toolbar button for anywhere else. The bridge it
talks to is doris's own (`bridge_port` in `~/.config/doris/config.toml`).

Load it from `about:debugging` → *Load Temporary Add-on…* → pick
`manifest.json`. See `browser-extension/README.md` for the rest, and
`make test-ext` for its tests.

## Layout

```
src/           the application
  app.rs       the orchestrator: the event loop and what answers an event
  app/         its five decisions: input, search, playback, session, settings
  ui/          drawing and the state it draws
  sources/     the tracker registry, the fan-out, and each tracker
  browser/     the browser session the trackers that need one are driven through
  torrserver/  the playback API
themes/        the bundled themes (override with ~/.config/doris/themes/)
tests/         integration tests
```

## Licence

MIT.