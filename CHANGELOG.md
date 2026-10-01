# Changelog

## 0.1.0

The first tagged build. A terminal torrent client: search trackers, play
or download, watch the download live.

### Sources

Ten trackers behind one registry — the Trackers panel, the CLI's
`--source`, and the Options list all derive from it, so a new tracker is
one entry rather than three edits.

`rutracker`, `rutor`, `nnmclub` and `torentino` are DLE-family trackers
filtered server-side by forum, so asking for a category does not mean
asking for everything and dropping rows. `1337x` is asked per category
and filtered client-side. `yts`, `tpb`, `subsplease`, `eztv` and `nyaa`
are JSON and RSS APIs with no browser needed.

A source's `block_hosts` is what the browser session is told to refuse,
and the cookie host comes from the source that asked for the session —
so one tracker cannot decide whose cookies are read out of the real
profile, and a second tracker with an ad host needs no change to the
browser layer.

### Search

One query fans out to every checked source at once and each answer is
merged as it lands, so the table fills while the slow trackers are still
thinking. A generation number travels with every dispatch and comes back
with its rows: an answer to a query that has since been replaced is
dropped rather than landing on top of the new one.

Pages are cached per source, per query, per category and per offset.
A cache hit answers without a browser, without a request, and through
the same path a live answer takes — so the offsets, the paging verdict
and the log line all update whether the rows came from disk or the wire.

`b` browses: an empty query asking the browse-capable sources for their
freshest rows.

### Playback

TorrServer streams or downloads. A background poller feeds the Torrent
panel: speeds, downloaded and total, seeds, peers, and a history
sparkline drawn as braille, blocks or dots. `p` pauses and resumes,
`d` removes behind a confirmation. With "Stop download on exit" the
torrent is paused on the way out — never removed, which would delete a
file while its owner is still watching it.

### Interface

Four panels, focusable with `1`–`4` or Tab, each resizable by dragging
its border, each able to take over the frame (`L`, `T`, `R`). Keybinds
are written on the frame they belong to, with the letter that triggers
one highlighted. Layout presets are tiling specs written as rows and
columns (`"1,3|4"`), cycled with `Shift+P`.

`f` filters the results with a grep-shaped syntax: bare words, `-word`,
`src:`, `group:`, `title:`, `size:>1gb`, `seeds:>50`.

Forty bundled themes, all four of the optional accents falling back to
the field each replaced, so a theme file that names only the classic
fields draws exactly as it always did. Frames can be bracketed by
synchronized output (DEC 2026) so a slow redraw is never shown
half-drawn.

### Storage

Credentials are AES-128-GCM encrypted and written `0600`, as are the
saved tracker cookies. The key is derived from the machine identity, so
this protects the file from every *other* account and from being read
accidentally — not from a process already running as you.