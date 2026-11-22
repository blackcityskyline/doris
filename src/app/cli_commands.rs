//! What each subcommand actually does.
//!
//! This file lives inside `app` rather than beside it for one reason: the
//! actions it calls are `pub(super)`, which is the visibility that keeps
//! them out of the public API. A CLI that could not reach them would have
//! to reimplement them, and a second `search` is a second answer to "what
//! does a source's page do when it lands".
//!
//! Two families here, and they do not cost the same:
//!
//! - The search family (`search`, `play`, `download`, `info`, `login`)
//!   builds a [`App`] with no keyboard and no bridge port, then drives it
//!   through `start_search` and [`App::pump_until`].
//! - The rest (`torrent`, `config`, `sources`, `health`, `logs`) answer
//!   from the pieces the app is made of -- [`TorrServer`], [`Config`], the
//!   source registry -- without building an app at all. A `torrent list`
//!   that had to start a cache and a search cache to ask an HTTP API what
//!   it is holding would be a worse way of asking.

use anyhow::{anyhow, Result};
use std::path::Path;

use super::*;
use crate::cli::{
    Args, Command, ConfigCommand, LoginArgs, RowArgs, SearchArgs, SourcesCommand, TorrentCommand,
};
use crate::config::Config;
use crate::filter::Filter;
use crate::sources::models::TorrentItem;
use crate::sources::orchestrator::{self, SourceStatus};
use crate::sources::ordering::{order, SortBy};
use crate::sources::source::{Group, KNOWN_SOURCES};
use crate::torrserver::api::{StoragePolicy, TorrServer};
use crate::ui::view::Modal;

/// Run one `doris <subcommand>`.
///
/// The one door from `main` into this module. Everything below is private
/// on purpose: a subcommand is reached by name, not by importing a
/// function per command, so the CLI surface is exactly what `cli.rs`
/// parses.
pub async fn dispatch(args: &Args, command: &Command) -> Result<i32> {
    run(args, command).await
}

/// Run one command and return the process exit code.
///
/// A non-zero code is a real answer, not decoration: a script that asked
/// and got nothing has to be able to tell. Sources that fail while others
/// answer do not make the command fail -- a search across twelve trackers
/// where three are down is still a search -- but a search that found
/// nothing is `1`, so `doris search ... && play` does not try to play
/// nothing.
pub async fn run(args: &Args, command: &Command) -> Result<i32> {
    let json = args.json;
    match command {
        Command::Search(a) => search(args, a, json).await,
        Command::Play(a) => play(args, a, json).await,
        Command::Download(a) => download(args, a, json).await,
        Command::Info(a) => info(args, a, json).await,
        Command::Login(a) => login(args, a, json).await,
        Command::Torrent(c) => torrent(args, c, json).await,
        Command::Downloads(ask) => {
            let config = crate::config::load(args.config.as_deref())?;
            crate::transmission::downloads_cmd::run(args, &config, ask, json).await
        }
        Command::Logs => logs(json),
        Command::Sources { action } => match action {
            None | Some(SourcesCommand::List) => sources(args, json),
            Some(other) => sources_switch(args, other, json),
        },
        Command::Health => health(args, json).await,
        Command::Config(c) => config_command(args, c, json),
        Command::Cookies(c) => cookies(args, c, json),
        Command::Credentials(c) => credentials(c, json),
    }
}

// --- the search family -------------------------------------------------

/// A round of searching is over once the state has left `Searching`:
/// `SearchComplete` puts it back to `Idle`, and so does every path that
/// had nothing to ask.
fn searching(app: &App) -> bool {
    app.ui.state != crate::ui::view::AppState::Searching
}

/// Ask, wait, print. `--pages` is the End key: each extra page is one
/// `load_more` against the cursors the first page left behind.
async fn search(args: &Args, a: &SearchArgs, json: bool) -> Result<i32> {
    let mut app = headless_app(args).await?;
    if a.stream {
        return stream(&mut app, a).await;
    }
    let rows = fetch(&mut app, a).await?;
    let found = !rows.is_empty();
    print_rows(&rows, json);
    report_sources(&app, json);
    Ok(i32::from(!found))
}

/// `search --stream`: one JSON array per source, on its own line, printed the
/// moment that source answers.
///
/// The merged answer is only complete when the slowest tracker is done, and
/// with rutracker behind Cloudflare that is half a minute -- of an empty
/// window, in a launcher. The other four answer in under a second; this
/// prints their rows then, and each later answer as it lands, so the cost of
/// the slowest tracker is rows arriving late rather than a table that is not
/// there at all.
///
/// The events are the same ones the TUI pumps, and rows land in `ui.results`
/// exactly as they do there: a streaming search is the normal search, read
/// out one answer at a time instead of at the end.
async fn stream(app: &mut App, a: &SearchArgs) -> Result<i32> {
    let query = a.query.clone().unwrap_or_default();
    let browsing = query.trim().is_empty();
    apply_selection(app, a)?;
    app.start_search(query.clone()).await;

    let filter = a.filter.as_ref().map(|f| Filter::parse(f));
    let limit = a.limit.unwrap_or(usize::MAX);
    // Zero waits forever, as the flag says; a stream that gave up at once
    // would print an empty answer for a search that never ran.
    let give_up = (a.deadline_ms > 0)
        .then(|| std::time::Instant::now() + std::time::Duration::from_millis(a.deadline_ms));
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut next = 0usize;
    let mut printed = 0usize;

    loop {
        if app.ui.state == crate::ui::view::AppState::Searching {
            let arrived = match give_up {
                None => app.event_handler.next().await.ok(),
                Some(at) => match at.checked_duration_since(std::time::Instant::now()) {
                    Some(remaining) => {
                        match tokio::time::timeout(remaining, app.event_handler.next()).await {
                            Ok(Ok(event)) => Some(event),
                            // Nothing before the deadline, or the handler
                            // is gone: what was printed is the answer.
                            Err(_) | Ok(Err(_)) => None,
                        }
                    }
                    None => None,
                },
            };
            match arrived {
                Some(event) => app.apply_event(event).await?,
                None => break,
            }
        }

        let mut fresh: Vec<TorrentItem> = unseen(&app.ui.results, next, &mut seen)
            .into_iter()
            .filter(|item| filter.as_ref().is_none_or(|f| f.matches(item)))
            .collect();
        next = app.ui.results.len();
        if printed < limit && !fresh.is_empty() {
            fresh.truncate(limit - printed);
            printed += fresh.len();
            let ordered = order(&fresh, SortBy::Seeds, browsing, Some(&query));
            println!("{}", serde_json::to_string(&ordered)?);
        }
        if app.ui.state != crate::ui::view::AppState::Searching {
            break;
        }
    }
    // A reader waits for a line to know the answer is in. An empty search
    // still prints its one `[]`, the same as the one-array form, so "found
    // nothing" and "failed before it could ask" are not the same silence.
    if printed == 0 {
        println!("[]");
    }
    app.save_cache();
    report_sources(app, true);
    Ok(i32::from(printed == 0))
}

/// The rows a streaming search has not printed yet: everything past `from`
/// that no earlier batch already carried.
///
/// Two indices are needed because the two lists are not the same list. A row
/// arrives once per source, and the same torrent can arrive twice; and the
/// final event dedupes and re-orders the whole table, so `from` can end up
/// past the end of it -- which is not an error and not a reason to print
/// anything twice, because everything in the table has already gone out.
fn unseen(
    results: &[TorrentItem],
    from: usize,
    seen: &mut std::collections::HashSet<String>,
) -> Vec<TorrentItem> {
    results
        .get(from..)
        .unwrap_or_default()
        .iter()
        .filter(|item| item.info_hash.is_empty() || seen.insert(item.info_hash.clone()))
        .cloned()
        .collect()
}

/// What each asked source actually did, for the ones that did not answer.
///
/// Without this the command's whole answer to "why is the list empty" is
/// "nothing found", and a source that refused and a source with nothing to say
/// are the same output. Measured 05.10.2026: `doris --json search --source ext`
/// printed `[]` while the reason (a Cloudflare checkbox nobody was there to
/// tick) went only to the log file -- and an empty array reads as an empty
/// index, which is a claim about the tracker rather than about the run.
///
/// On stderr, so a `--json` run still pipes a clean array and a table still
/// ends where it always did.
fn report_sources(app: &App, json: bool) {
    let mut lines: Vec<String> = Vec::new();
    for (id, status) in &app.ui.source_status {
        match status {
            SourceStatus::Error(why) => lines.push(format!("{id}: {why}")),
            SourceStatus::Timeout => lines.push(format!(
                "{id}: timed out ({})",
                orchestrator::PER_SOURCE_TIMEOUT.as_secs()
            )),
            _ => {}
        }
    }
    lines.sort();
    if lines.is_empty() {
        return;
    }
    if json {
        for line in lines {
            eprintln!("{line}");
        }
    } else {
        eprintln!();
        for line in lines {
            eprintln!("  {line}");
        }
    }
}

/// Run a search and hand back the rows that survived the filter and the
/// limit, which is the same list the Results table would be showing.
async fn fetch(app: &mut App, a: &SearchArgs) -> Result<Vec<TorrentItem>> {
    let query = a.query.clone().unwrap_or_default();
    apply_selection(app, a)?;
    app.start_search(query.clone()).await;
    let deadline = std::time::Duration::from_millis(a.deadline_ms);
    app.pump_until(deadline, searching).await?;
    for _ in 1..a.pages {
        if app.ui.all_loaded {
            break;
        }
        app.load_more(query.clone()).await;
        app.pump_until(deadline, searching).await?;
    }
    app.save_cache();
    Ok(select_rows(app, a))
}

/// `--source` and `--group` are the Trackers panel's checkboxes and the
/// category arrows, written down. Both go through the config and the UI
/// field the panel itself writes, so a CLI search and a TUI search take
/// the same route to the same source list.
fn apply_selection(app: &mut App, a: &SearchArgs) -> Result<()> {
    if a.source.is_empty() {
        // No `--source`: whatever the config has checked, which is what
        // the panel shows.
    } else {
        let known: Vec<&str> = KNOWN_SOURCES
            .iter()
            .filter(|info| info.implemented)
            .map(|info| info.id)
            .collect();
        let ids = check_ids(&a.source)?;
        let _ = known;
        app.config.enabled_sources = ids;
    }
    if let Some(group) = &a.group {
        app.ui.active_group = parse_group(group)?;
    }
    Ok(())
}

/// The category arrows, written down. `all` is the no-category the `◀ all ▶`
/// button sets, which is why this returns an `Option` rather than a group.
pub fn parse_group(name: &str) -> Result<Option<Group>> {
    if name.eq_ignore_ascii_case("all") {
        return Ok(None);
    }
    for group in [Group::Games, Group::Movies, Group::TV, Group::Anime] {
        if group.label().eq_ignore_ascii_case(name) {
            return Ok(Some(group));
        }
    }
    Err(anyhow!(
        "unknown group '{name}'; try games, movies, tv, anime, or all"
    ))
}

/// The rows as the Results table would show them: merged, ordered,
/// filtered, and cut to `--limit`.
fn select_rows(app: &App, a: &SearchArgs) -> Vec<TorrentItem> {
    let filter = a.filter.as_ref().map(|f| Filter::parse(f));
    let mut rows: Vec<TorrentItem> = Vec::new();
    for &idx in &app.ui.filtered_indices {
        let Some(item) = app.ui.results.get(idx) else {
            continue;
        };
        if filter.as_ref().is_some_and(|f| !f.matches(item)) {
            continue;
        }
        rows.push(item.clone());
    }
    if let Some(limit) = a.limit {
        rows.truncate(limit);
    }
    rows
}

async fn play(args: &Args, a: &RowArgs, json: bool) -> Result<i32> {
    let (mut app, item) = resolve_row(args, a).await?;
    open_row_session(&mut app, &item).await?;
    // `spawn_stream` reads the selected row, which is the TUI's way of
    // saying the same thing; put this row under the cursor and call it.
    app.ui.selected = 0;
    app.ui.results = vec![item.clone()];
    app.spawn_stream().await;
    // The stream is handed to a task that answers over the event channel,
    // so the command waits the way a TUI would -- by pumping events --
    // until it reports back one way or the other.
    app.pump_until(
        std::time::Duration::from_millis(a.search.deadline_ms),
        |app| app.ui.state != crate::ui::view::AppState::Streaming,
    )
    .await?;
    let url = app.ui.last_stream_url.clone().unwrap_or_default();
    let failed = app.ui.last_stream_error.clone();
    if json {
        let payload = serde_json::json!({
            "title": item.title,
            "magnet": item.magnet,
            "url": url,
            "error": failed,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else if let Some(err) = &failed {
        println!("{err}");
    } else if url.is_empty() {
        // The same answer the download command gives: "it did not happen"
        // without saying why has thrown away what the action just logged.
        println!(
            "{}",
            app.ui
                .logs
                .back()
                .cloned()
                .unwrap_or_else(|| "TorrServer did not answer.".into())
        );
    } else {
        println!("{url}");
    }
    // No URL means nothing was started, whether a reason was reported or the
    // wait ran out; a script asking `if doris play` must not see success for
    // a row that did not play.
    Ok(i32::from(url.is_empty()))
}

async fn download(args: &Args, a: &RowArgs, json: bool) -> Result<i32> {
    let (mut app, item) = resolve_row(args, a).await?;
    open_row_session(&mut app, &item).await?;
    app.ui.selected = 0;
    app.ui.results = vec![item.clone()];
    let id = app.download_selected().await;
    // Why it did not happen is the last thing the app logged, and a
    // command that says only "nothing was added" has thrown away the
    // answer the action just gave it.
    let reason = app.ui.logs.back().cloned();
    if json {
        let payload = serde_json::json!({
            "title": item.title,
            "id": id,
            "reason": if id.is_none() { reason } else { None },
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else if let Some(id) = id {
        println!("{id}");
    } else {
        println!("{}", reason.unwrap_or_else(|| "nothing was added".into()));
    }
    Ok(i32::from(id.is_none()))
}

async fn info(args: &Args, a: &RowArgs, json: bool) -> Result<i32> {
    let (mut app, item) = resolve_row(args, a).await?;
    open_row_session(&mut app, &item).await?;
    app.ui.selected = 0;
    app.ui.results = vec![item.clone()];
    // The detail modal is what `Shift+Enter` opens; it asks the row's own
    // source for the file list, so a CLI `info` reads the same list the TUI
    // shows.
    app.open_detail_modal().await;
    app.pump_until(
        std::time::Duration::from_millis(a.search.deadline_ms),
        |app| !matches!(&app.ui.modal, Modal::TorrentDetail(state) if state.pending),
    )
    .await?;
    let detail = match &app.ui.modal {
        crate::ui::view::Modal::TorrentDetail(state) => Some((**state).clone()),
        _ => None,
    };
    if json {
        let payload = serde_json::json!({
            "title": item.title,
            "source": item.source,
            "size": item.size,
            "seeds": item.seeds,
            "date": item.date,
            "info_hash": item.info_hash,
            "magnet": item.magnet,
            "page_url": item.page_url,
            "download_url": item.download_url,
            "group": item.group.map(|g| g.label()),
            "uploader": item.uploader,
            "category": item.category,
            "files": detail.as_ref().map(|d| d.files.clone()).unwrap_or_default(),
            "pending": detail.as_ref().is_some_and(|d| d.pending),
            "error": detail.as_ref().and_then(|d| d.error.clone()),
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("title:   {}", item.title);
        println!("source:  {}", item.source);
        println!("size:    {}", item.size);
        println!("seeds:   {}", item.seeds);
        println!("date:    {}", item.date);
        println!(
            "group:   {}",
            item.group
                .map(|g| g.label().to_string())
                .unwrap_or_default()
        );
        println!("by:      {}", item.uploader);
        println!(
            "where:   {}",
            if item.category.is_empty() {
                item.group
                    .map(|g| g.label().to_string())
                    .unwrap_or_default()
            } else {
                item.category.clone()
            }
        );
        println!("hash:    {}", item.info_hash);
        println!("magnet:  {}", item.magnet.clone().unwrap_or_default());
        println!("page:    {}", item.page_url);
        if let Some(detail) = &detail {
            match (&detail.error, detail.files.is_empty()) {
                (Some(err), _) => println!("files:   {err}"),
                (_, true) if detail.pending => {
                    println!("files:   still loading (raise --deadline-ms)")
                }
                (_, true) => println!("files:   (none reported)"),
                (_, false) => {
                    println!("files:   {}", detail.files.len());
                    for file in &detail.files {
                        println!("           {}", file.name);
                    }
                }
            }
        }
    }
    Ok(0)
}

async fn login(args: &Args, a: &LoginArgs, json: bool) -> Result<i32> {
    let mut app = headless_app(args).await?;
    let username = a
        .username
        .clone()
        .or_else(|| args.username.clone())
        .ok_or_else(|| anyhow!("a username is required: --username or RUTRACKER_USER"))?;
    let password = a
        .password
        .clone()
        .or_else(|| args.password.clone())
        .ok_or_else(|| anyhow!("a password is required: --password or RUTRACKER_PASS"))?;
    app.do_login(&a.resource, &username, &password).await;
    // A login is three Cloudflare round trips (home, cookie-inject reload,
    // login.php) and each one measured tens of seconds on 15.11.2026, so the
    // 60 s that used to be here cut the attempt off before the form was posted.
    app.pump_until(std::time::Duration::from_millis(300_000), |app| {
        app.ui
            .logs
            .iter()
            .any(|l| l.contains("Login successful") || l.contains("Login failed"))
    })
    .await?;
    let ok = app.ui.logs.iter().any(|l| l.contains("Login successful"));
    if json {
        let payload = serde_json::json!({"resource": a.resource, "username": username, "ok": ok});
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("{}", if ok { "ok" } else { "failed" });
    }
    Ok(i32::from(!ok))
}

/// The row a command is about, from whichever of the two ways it was
/// named, together with the app that found it.
///
/// The app comes back because it is not finished with: the same session,
/// the same browser and the same source are what the next step needs.
/// Building a second one -- which is what returning only the row did --
/// meant launching a second browser to ask the same page the same
/// question.
async fn resolve_row(args: &Args, a: &RowArgs) -> Result<(App, TorrentItem)> {
    if let Some(magnet) = &a.magnet {
        return Ok((
            headless_app(args).await?,
            TorrentItem {
                title: String::new(),
                magnet: Some(magnet.clone()),
                source: "magnet".to_string(),
                ..TorrentItem::default()
            },
        ));
    }
    if let Some(row) = &a.row {
        // The row came from this CLI, so it already carries what the action
        // needs: the magnet, the .torrent link, the page and the source that
        // fetches it. Searching again would answer a question the caller did
        // not ask, and answer it slowly.
        let item = row_from_json(row)?;
        return Ok((headless_app(args).await?, item));
    }
    let mut app = headless_app(args).await?;
    let rows = fetch(&mut app, &a.search).await?;
    let item = rows.get(a.index).cloned().ok_or_else(|| {
        anyhow!(
            "no row {} among {} (use --index, or --magnet to name one directly)",
            a.index,
            rows.len()
        )
    })?;
    Ok((app, item))
}

async fn headless_app(args: &Args) -> Result<App> {
    let config = crate::config::load(args.config.as_deref())?;
    App::headless(args.clone(), config).await
}

/// Open the session a row's tracker needs, since this process never searched.
///
/// The TUI's browser is launched by the search, so a row on its table always
/// has a session by the time Enter reaches it. A CLI command starts cold, and
/// a browser-backed row acted on without this got "No browser session - search
/// first" -- from the command that is the search's only user.
async fn open_row_session(app: &mut App, item: &TorrentItem) -> Result<()> {
    let Some(info) = crate::sources::source::get_source(&item.source) else {
        return Ok(());
    };
    if !info.requires_browser {
        return Ok(());
    }
    app.open_source_session(info.id).await
}

/// The row a front end holds, read back. It came from `--json search`, so it
/// is a [`TorrentItem`] with the fields an action needs filled in; a row with
/// neither a magnet nor a `.torrent` link names nothing to play.
fn row_from_json(text: &str) -> Result<TorrentItem> {
    let item: TorrentItem = serde_json::from_str(text)
        .map_err(|e| anyhow!("--row is not a row from `doris --json search`: {e}"))?;
    if item.magnet.is_none() && item.download_url.is_empty() {
        anyhow::bail!("--row carries neither a magnet nor a .torrent link");
    }
    Ok(item)
}

// --- the pieces the app is made of -------------------------------------

async fn torrent(args: &Args, command: &TorrentCommand, json: bool) -> Result<i32> {
    let torrserver = TorrServer::new(&args.torrserver);
    let hash_arg = |h: &Option<String>| h.clone();
    match command {
        TorrentCommand::List => {
            let list = torrserver.list_torrents().await?;
            if json {
                println!("{}", serde_json::to_string_pretty(&list)?);
            } else if list.is_empty() {
                println!("TorrServer is holding nothing.");
            } else {
                println!("{:<40} {:>6} {:>10}  name", "hash", "seeds", "progress");
                for t in &list {
                    println!(
                        "{:<40} {:>6} {:>9.1}%  {}",
                        t.hash,
                        t.connected_seeders,
                        t.progress() * 100.0,
                        t.name
                    );
                }
            }
            Ok(0)
        }
        TorrentCommand::Files { hash } => {
            let Some(t) = one_torrent(&torrserver, hash_arg(hash)).await? else {
                println!("TorrServer is holding nothing by that hash.");
                return Ok(1);
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&t.files)?);
            } else if t.files.is_empty() {
                println!("the server reported no files for this torrent");
            } else {
                for file in &t.files {
                    println!("{:>3}  {:>14}  {}", file.id, file.length, file.path);
                }
            }
            Ok(0)
        }
        TorrentCommand::Watch { hash, deadline_ms } => {
            let Some(hash) = need_hash(&torrserver, hash_arg(hash)).await? else {
                println!("TorrServer is holding nothing.");
                return Ok(1);
            };
            if !json {
                eprintln!("watching {} (Ctrl-C to stop)", hash);
            }
            let last = torrserver
                .watch(
                    &hash,
                    std::time::Duration::from_millis(*deadline_ms),
                    |_| false,
                )
                .await?;
            let Some(last) = last else {
                println!("the torrent went away while it was being watched");
                return Ok(1);
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&last)?);
            } else {
                println!(
                    "{:.1}%  {}  {}",
                    last.progress() * 100.0,
                    human_bytes(last.loaded_size.max(0) as u64),
                    last.status_string
                );
            }
            Ok(i32::from(last.progress() < 1.0))
        }
        TorrentCommand::Storage(sub) => match sub {
            crate::cli::StorageCommand::Show => {
                say_storage(&torrserver.storage().await?, json)?;
                Ok(0)
            }
            crate::cli::StorageCommand::Set { path, keep_cache } => {
                // Both halves are written every time: a `--keep-cache` that
                // no later plain `set` could undo was a one-way door.
                torrserver
                    .set_settings(serde_json::json!({
                        "UseDisk": true,
                        "TorrentsSavePath": path.display().to_string(),
                        "RemoveCacheOnDrop": !keep_cache,
                    }))
                    .await?;
                say_storage(&torrserver.storage().await?, json)?;
                Ok(0)
            }
            crate::cli::StorageCommand::Off => {
                torrserver
                    .set_settings(serde_json::json!({
                        "UseDisk": false,
                        "RemoveCacheOnDrop": true,
                    }))
                    .await?;
                say_storage(&torrserver.storage().await?, json)?;
                Ok(0)
            }
        },
        TorrentCommand::Status { hash } => {
            let info = one_torrent(&torrserver, hash_arg(hash)).await?;
            match info {
                None => {
                    println!("TorrServer is holding nothing by that hash.");
                    Ok(1)
                }
                Some(t) => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&t)?);
                    } else {
                        println!("hash:       {}", t.hash);
                        println!("name:       {}", t.name);
                        println!("status:     {}", t.status_string);
                        println!("progress:   {:.1}%", t.progress() * 100.0);
                        println!("downloaded: {} bytes", t.loaded_size);
                        println!("total:      {} bytes", t.total_size);
                        println!("seeds:      {}", t.connected_seeders);
                        println!("peers:      {}", t.active_peers);
                    }
                    Ok(0)
                }
            }
        }
        TorrentCommand::Pause { hash } => {
            let Some(hash) = need_hash(&torrserver, hash_arg(hash)).await? else {
                return Ok(1);
            };
            torrserver.pause(&hash).await?;
            say(json, &hash, "paused");
            Ok(0)
        }
        TorrentCommand::Resume { hash } => {
            let Some(hash) = need_hash(&torrserver, hash_arg(hash)).await? else {
                return Ok(1);
            };
            torrserver.resume(&hash).await?;
            say(json, &hash, "resumed");
            Ok(0)
        }
        TorrentCommand::Remove { hash } => {
            let Some(hash) = need_hash(&torrserver, hash_arg(hash)).await? else {
                return Ok(1);
            };
            torrserver.remove(&hash).await?;
            say(json, &hash, "removed");
            Ok(0)
        }
        TorrentCommand::Add { link, title } => {
            let hash = torrserver.add_by_link(link, title).await?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({"hash": hash}))?
                );
            } else {
                println!("{hash}");
            }
            Ok(0)
        }
    }
}

/// What the server does with what it fetches, in one place: `show`, `set` and
/// `off` all answer with the state that is now true, not with what was asked
/// for.
fn say_storage(policy: &StoragePolicy, json: bool) -> Result<()> {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "path": policy.path,
                "use_disk": policy.use_disk,
                "keep_cache": policy.keep_cache(),
            }))?
        );
    } else {
        println!(
            "path:       {}",
            if policy.path.is_empty() {
                "(unset)"
            } else {
                &policy.path
            }
        );
        println!("use_disk:   {}", policy.use_disk);
        println!("keep_cache: {}", policy.keep_cache());
    }
    Ok(())
}

async fn one_torrent(
    torrserver: &TorrServer,
    hash: Option<String>,
) -> Result<Option<crate::torrserver::api::TorrentInfo>> {
    match hash {
        Some(hash) => Ok(torrserver.get_torrent(&hash).await?),
        None => Ok(torrserver.list_torrents().await?.into_iter().next()),
    }
}

/// The hash to act on: the one named, or the only one there is. With
/// several and none named it is an error rather than a coin toss.
async fn need_hash(torrserver: &TorrServer, hash: Option<String>) -> Result<Option<String>> {
    if let Some(hash) = hash {
        return Ok(Some(hash));
    }
    let list = torrserver.list_torrents().await?;
    match list.len() {
        0 => Ok(None),
        1 => Ok(Some(list[0].hash.clone())),
        n => Err(anyhow!(
            "{n} torrents are held and no hash was given; name one with the hash argument"
        )),
    }
}

fn cookies(args: &Args, command: &crate::cli::CookieCommand, json: bool) -> Result<i32> {
    let config = crate::config::load(args.config.as_deref())?;
    let path = Path::new(&config.cookie_file);
    match command {
        crate::cli::CookieCommand::Show => {
            let jar = crate::sources::cookies::load_from_file(path).unwrap_or_default();
            // Domains and counts, never the values: a cookie jar is a
            // logged-in session, and a session is what this command is
            // for finding out about, not for reading out loud.
            let mut domains: std::collections::BTreeMap<String, usize> = Default::default();
            for cookie in &jar {
                *domains.entry(cookie.domain.clone()).or_default() += 1;
            }
            if json {
                let rows: Vec<serde_json::Value> = domains
                    .iter()
                    .map(|(domain, count)| serde_json::json!({"domain": domain, "cookies": count}))
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "path": path.display().to_string(),
                        "exists": path.exists(),
                        "domains": rows,
                    }))?
                );
            } else {
                println!("path: {}", path.display());
                if domains.is_empty() {
                    println!("empty");
                } else {
                    for (domain, count) in &domains {
                        println!("{domain:<40} {count}");
                    }
                }
            }
            Ok(0)
        }
        crate::cli::CookieCommand::Clear => {
            if path.exists() {
                std::fs::remove_file(path)?;
            }
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({"cleared": true}))?
                );
            } else {
                println!("removed {}", path.display());
            }
            Ok(0)
        }
    }
}

fn credentials(command: &crate::cli::CredentialCommand, json: bool) -> Result<i32> {
    let path = crate::credentials::credentials_path();
    match command {
        crate::cli::CredentialCommand::List => {
            // The file is an encrypted blob keyed by resource id, so there
            // is nothing to list from it but whether it is there and
            // whether anything decrypts. `load_credentials` answers for the
            // default resource; that is what it is for.
            let has = path.exists();
            let default = crate::credentials::load_credentials();
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "path": path.display().to_string(),
                        "exists": has,
                        "default_username": default.as_ref().map(|(u, _)| u.clone()),
                    }))?
                );
            } else {
                println!("path: {}", path.display());
                match &default {
                    Some((user, _)) => println!("default: {user}"),
                    None => println!("default: (none)"),
                }
            }
            Ok(0)
        }
        crate::cli::CredentialCommand::Clear { resource } => {
            // Only one shape of store exists, so "forget a resource" means
            // removing the file rather than editing an encrypted blob. The
            // answer says so instead of pretending it did a partial job.
            if path.exists() {
                std::fs::remove_file(&path)?;
            }
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "removed": true, "resource": resource
                    }))?
                );
            } else {
                println!("removed {}", path.display());
            }
            Ok(0)
        }
    }
}

/// A byte count the way a person reads it. TorrServer reports raw sizes
/// and the whole point of printing them is that they are legible.
fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn say(json: bool, hash: &str, what: &str) {
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({"hash": hash, "action": what}))
                .unwrap_or_default()
        );
    } else {
        println!("{what} {hash}");
    }
}

/// The config to write into: the named file if it is there, a fresh one if
/// it is not.
///
/// `config set --config /somewhere/new.toml` is how a config file gets
/// created, so that one command treats a missing file as "start from the
/// defaults". Every other command wants the error, because a typo'd path
/// there means it would quietly read somebody else's settings.
fn config_to_write(args: &Args) -> Result<Config> {
    match args.config {
        Some(ref path) if !path.exists() => {
            let mut fresh = crate::sources::source::first_run();
            crate::sources::source::migrate_config(&mut fresh);
            Ok(fresh)
        }
        _ => crate::config::load(args.config.as_deref()),
    }
}

fn logs(json: bool) -> Result<i32> {
    // The log file is the whole history; the Log zone only ever holds the
    // tail. A one-shot command has no tail to hold.
    let path = crate::log::log_path();
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    if json {
        let lines: Vec<&str> = text.lines().collect();
        println!("{}", serde_json::to_string_pretty(&lines)?);
    } else {
        print!("{text}");
    }
    Ok(0)
}

fn sources(args: &Args, json: bool) -> Result<i32> {
    let config = crate::config::load(args.config.as_deref()).ok();
    let rows: Vec<serde_json::Value> = KNOWN_SOURCES
        .iter()
        .map(|info| {
            serde_json::json!({
                "id": info.id,
                "label": info.label,
                "implemented": info.implemented,
                "browser": info.requires_browser,
                "groups": info.groups.iter().map(|g| g.label()).collect::<Vec<_>>(),
                "enabled": config
                    .as_ref()
                    .is_some_and(|c| c.enabled_sources.iter().any(|e| e == info.id)),
                "home": info.home_url,
            })
        })
        .collect();
    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else {
        println!(
            "{:<12} {:<11} {:<6} {:<7} groups",
            "id", "label", "browser", "enabled"
        );
        for row in &rows {
            println!(
                "{:<12} {:<11} {:<6} {:<7} {}",
                row["id"].as_str().unwrap_or_default(),
                row["label"].as_str().unwrap_or_default(),
                row["browser"].as_bool().unwrap_or_default(),
                row["enabled"].as_bool().unwrap_or_default(),
                row["groups"]
                    .as_array()
                    .map(|g| g
                        .iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>()
                        .join(","))
                    .unwrap_or_default()
            );
        }
    }
    Ok(0)
}

/// Switch sources on and off, and order the browser probe.
///
/// These write `config.toml` rather than doing anything to a running app:
/// a CLI command is a separate process, so the thing it can change is the
/// file the next launch reads. That is the same contract `config set` has,
/// and it is why the checkboxes in the panel persist the same way.
fn sources_switch(args: &Args, command: &SourcesCommand, json: bool) -> Result<i32> {
    let mut config = config_to_write(args)?;
    match command {
        SourcesCommand::List => {
            drop(config);
            return sources(args, json);
        }
        SourcesCommand::On { ids } => {
            let ids = check_ids(ids)?;
            for id in ids {
                if !config.enabled_sources.contains(&id) {
                    config.enabled_sources.push(id);
                }
            }
        }
        SourcesCommand::Off { ids } => {
            let ids = check_ids(ids)?;
            config.enabled_sources.retain(|e| !ids.contains(e));
        }
        SourcesCommand::Priority { ids } => {
            config.browser_priority = ids.clone();
        }
    }
    crate::config::save(&config, args.config.as_deref())?;
    if json {
        let payload = serde_json::json!({
            "enabled_sources": config.enabled_sources,
            "browser_priority": config.browser_priority,
        });
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else {
        println!("enabled: {}", config.enabled_sources.join(", "));
        println!("priority: {}", config.browser_priority.join(", "));
    }
    Ok(0)
}

/// Split `--source a,b` into `a` and `b`, and check them against the
/// registry. A source that is not there is named in the error rather than
/// written into the config, because a config with a typo in it fails
/// silently on the next launch.
fn check_ids(ids: &[String]) -> Result<Vec<String>> {
    let known: Vec<&str> = KNOWN_SOURCES
        .iter()
        .filter(|info| info.implemented)
        .map(|info| info.id)
        .collect();
    let mut out = Vec::new();
    for id in ids {
        for part in id.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            if !known.contains(&part) {
                anyhow::bail!("unknown source '{part}'; try one of: {}", known.join(", "));
            }
            out.push(part.to_string());
        }
    }
    Ok(out)
}

async fn health(args: &Args, json: bool) -> Result<i32> {
    let config = crate::config::load(args.config.as_deref())?;
    let browser = crate::browser::detect::detect_browser(args.browser.as_deref())
        .map(|(kind, path)| format!("{} ({})", kind.label(), path.display()))
        .unwrap_or_else(|e| format!("not found: {e}"));
    let torrserver = TorrServer::new(&args.torrserver);
    let reachable = torrserver.is_reachable().await;
    let cookie = Path::new(&config.cookie_file);
    let cookies = cookie.exists();
    let credentials = crate::credentials::load_credentials().is_some();
    let rows = vec![
        ("browser", browser),
        (
            "torrserver",
            if reachable {
                args.torrserver.clone()
            } else {
                format!("unreachable at {}", args.torrserver)
            },
        ),
        (
            "cookies",
            if cookies {
                cookie.display().to_string()
            } else {
                format!("none at {}", cookie.display())
            },
        ),
        (
            "credentials",
            if credentials {
                "saved".to_string()
            } else {
                "none".to_string()
            },
        ),
    ];
    if json {
        let payload: serde_json::Map<String, serde_json::Value> = rows
            .iter()
            .map(|(k, v)| ((*k).to_string(), serde_json::json!(v)))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::Value::Object(payload))?
        );
    } else {
        for (name, value) in rows {
            println!("{name:<12} {value}");
        }
    }
    Ok(i32::from(!reachable))
}

fn config_command(args: &Args, command: &ConfigCommand, json: bool) -> Result<i32> {
    match command {
        ConfigCommand::List => {
            let config = crate::config::load(args.config.as_deref())?;
            println!("{}", serde_json::to_string_pretty(&config)?);
            Ok(0)
        }
        ConfigCommand::Get { key } => {
            let config = crate::config::load(args.config.as_deref())?;
            let value = config.get(key)?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({ key: value }))?
                );
            } else {
                println!("{value}");
            }
            Ok(0)
        }
        ConfigCommand::Set { key, value } => {
            let mut config = config_to_write(args)?;
            config.set(key, value)?;
            crate::config::save(&config, args.config.as_deref())?;
            if json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({"key": key, "value": value}))?
                );
            } else {
                println!("{key} = {value}");
            }
            Ok(0)
        }
    }
}

fn print_rows(rows: &[TorrentItem], json: bool) {
    if json {
        let payload = serde_json::to_string_pretty(rows).unwrap_or_default();
        println!("{payload}");
        return;
    }
    if rows.is_empty() {
        println!("nothing found");
        return;
    }
    println!(
        "{:>4}  {:>10}  {:>6}  {:<10}  title",
        "seeds", "size", "date", "source"
    );
    for (i, row) in rows.iter().enumerate() {
        println!(
            "{:>4}  {:>10}  {:>6}  {:<10}  {}",
            i, row.size, row.seeds, row.source, row.title
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{row_from_json, unseen};
    use crate::sources::models::TorrentItem;

    /// A row with the one field the streaming reader keys on.
    fn hashed(hash: &str, title: &str) -> TorrentItem {
        TorrentItem {
            title: title.to_string(),
            info_hash: hash.to_string(),
            ..Default::default()
        }
    }

    fn titles(items: &[TorrentItem]) -> Vec<&str> {
        items.iter().map(|i| i.title.as_str()).collect()
    }

    #[test]
    fn test_each_batch_is_printed_once() {
        let mut seen = Default::default();
        let results = vec![hashed("a", "first"), hashed("b", "second")];

        let first = unseen(&results, 0, &mut seen);
        assert_eq!(titles(&first), ["first", "second"]);
        assert!(unseen(&results, results.len(), &mut seen).is_empty());
    }

    /// The same torrent can land twice: one hash announced by two sources.
    #[test]
    fn test_a_hash_that_arrives_twice_is_printed_once() {
        let mut seen = Default::default();
        let first = vec![hashed("a", "first")];
        unseen(&first, 0, &mut seen);

        let later = vec![hashed("a", "first"), hashed("c", "third")];
        assert_eq!(titles(&unseen(&later, 0, &mut seen)), ["third"]);
    }

    /// The final event dedupes and reorders the whole table, so a read
    /// position can end up past its end. That is not an error, and it is not
    /// a reason to print anything a second time.
    #[test]
    fn test_a_shrunken_table_past_the_read_position_prints_nothing() {
        let mut seen = Default::default();
        let full = vec![hashed("a", "a"), hashed("b", "b")];
        unseen(&full, 0, &mut seen);

        let shrunk = vec![hashed("b", "b")];
        assert!(unseen(&shrunk, 2, &mut seen).is_empty());
    }

    /// A row without a hash cannot be deduped, so it is never swallowed:
    /// dropping it would hide a result rather than repeat one.
    #[test]
    fn test_a_row_without_a_hash_is_still_printed() {
        let mut seen = Default::default();
        let rows = vec![hashed("", "hashless"), hashed("", "hashless")];
        assert_eq!(
            titles(&unseen(&rows, 0, &mut seen)),
            ["hashless", "hashless"]
        );
    }

    /// A row `doris --json search --source rutracker matrix` printed,
    /// captured 22.11.2026. The hardest case: no magnet, a relative
    /// `.torrent` path only the browser session can fetch.
    const RUTRACKER_ROW: &str = r#"{"title":"Матрица / The Matrix (1999) [2160p]","size":"18.45 GB","seeds":"311","download_url":"dl.php?t=6306464","query":"","date":"7-Янв-23","page_url":"viewtopic.php?t=6306464","source":"rutracker","uploader":"","category":"","group":null,"info_hash":"","magnet":null,"size_bytes":18450000000,"seeds_n":311,"leechers":0,"added":0}"#;

    #[test]
    fn test_a_row_search_printed_comes_back_as_the_same_row() {
        let row = row_from_json(RUTRACKER_ROW).expect("the shape search prints");
        assert_eq!(row.source, "rutracker");
        assert_eq!(row.download_url, "dl.php?t=6306464");
        assert_eq!(row.page_url, "viewtopic.php?t=6306464");
        assert!(row.magnet.is_none(), "no magnet is invented on the way");
        assert_eq!(row.title, "Матрица / The Matrix (1999) [2160p]");
    }

    #[test]
    fn test_a_row_that_names_nothing_to_play_is_refused() {
        let err = row_from_json(r#"{"title":"nothing","source":"rutor"}"#)
            .unwrap_err()
            .to_string();
        assert!(err.contains("neither a magnet"), "{err}");
        assert!(row_from_json("not json").is_err());
    }
}
