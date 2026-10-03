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

## Keys

`?` opens the help page, which is the full list, in four pages split by
what a key is for: `1:keys`, `2:search`, `3:filter`, `4:torrents` — switch
with `←`/`→` or the digit. The short version:
`s` searches, `Enter` plays, `d` downloads, `1`–`4` focus a panel, `L`
`T` `R` take a panel over the frame, `f` filters, `g` changes category,
`m` or `Esc` opens the menu, `q` quits.

In the Torrents view `T` takes over the frame: `j`/`k` move the cursor,
`p` pauses, `d` removes, `v` verifies, `f` opens the file list (per file,
what to fetch and what not), `o` opens the folder in a file manager, and
`+`/`-`/`0` walk the download-rate limit.

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