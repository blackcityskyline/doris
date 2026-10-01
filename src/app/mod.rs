use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

mod config;
mod input;
mod search;
mod sources;
mod torrent;

#[cfg(test)]
mod tests;

use crate::bridge::handler::BridgeServer;
use crate::browser::cdp::{Browser, BrowserVisibility};
use crate::browser::detect;
use crate::cli::Args;
use crate::config::Config;
use crate::event::{Event, EventHandler};
use crate::search::{apply_source_done, finish_search, resolve_cookie_file, source_outcome_line};
use crate::sources::cache::{CacheKey, SearchCache};
use crate::sources::orchestrator::{self, SourceStatus};
use crate::sources::source::{self, AuthContext, LogFn, SearchRequest, Source, SourceEnv};
use crate::torrserver::api::TorrServer;
use crate::tui;
use crate::ui::app::{
    sources_summary, App as UiApp, AppState, DetailAction, Modal, TorrentDetailState,
    TorrentStatus, UiAction,
};
use crate::ui::menu::MenuItem;
use crate::ui::modals::settings::SettingsAction;
use crate::ui::theme::Theme;
use crate::ui::zones::ZoneId;

/// How often the event handler wakes up to poll the terminal for keys,
/// mouse and resize. 100 ms is the sweet spot: responsive enough that
/// typing never feels laggy, cheap enough that an idle app does not spin.
const EVENT_POLL_MS: u64 = 100;

/// Resolve the effective download directory from `download_dir_mode` and
/// the three custom slots (Options -> download), falling back to the OS
/// Downloads folder for "default" or an unset/empty custom slot. A free
/// function (rather than only an `App` method) so it can also be called
/// during `App::new()`, before `self` exists.
pub fn resolve_download_dir(config: &Config) -> String {
    let custom = match config.download_dir_mode.as_str() {
        "custom1" => Some(&config.download_dir_custom_1),
        "custom2" => Some(&config.download_dir_custom_2),
        "custom3" => Some(&config.download_dir_custom_3),
        _ => None,
    };
    match custom {
        Some(path) if !path.is_empty() => path.clone(),
        _ => dirs::download_dir()
            .map(|d| d.display().to_string())
            .unwrap_or_else(|| "/tmp".to_string()),
    }
}

/// Step `pos` by `direction` (+1/-1) within `0..len`, wrapping around --
/// shared by every Options cycle-type action so Left and Right actually
/// go opposite ways instead of both always stepping forward.
pub fn cycle_index(pos: usize, len: usize, direction: i8) -> usize {
    if len == 0 {
        return 0;
    }
    if direction >= 0 {
        (pos + 1) % len
    } else {
        (pos + len - 1) % len
    }
}

/// Whether fetching this source's `.torrent` files needs the browser-backed
/// rutracker searcher. Rutor is plain unauthenticated HTTP; everything else
/// -- including rows produced before the `source` field existed -- routes
/// through the browser, which is what the old hardcoded path did. Extracted
/// as a free function so tests can pin the routing choice (B0.1).
///
/// Since B2 the answer comes from the registry (`SourceInfo::
/// requires_browser`) instead of a literal `"rutor"` comparison, so a
/// newly registered source gets its routing from one place.
pub fn source_needs_browser(source: &str) -> bool {
    source::requires_browser(source)
}

/// The Options rows that flip one bool and nothing else.
///
/// Data, not code: fifteen `||`-chained macro calls spelled this out
/// before, and the chain read as fifteen statements where there is one
/// rule. It is a table because the *pairing* is the content -- a row that
/// names the wrong field is a button that flips the wrong setting -- and a
/// table is where a pairing can be read at a glance and checked.
/// An Options row and the field it inverts.
type BoolToggle = (SettingsAction, fn(&mut Config));

const BOOL_TOGGLES: &[BoolToggle] = &[
    (SettingsAction::ToggleCloseBrowserOnExit, |c| {
        c.close_browser_on_exit = !c.close_browser_on_exit
    }),
    (SettingsAction::ToggleSaveCookies, |c| {
        c.save_cookies = !c.save_cookies
    }),
    (SettingsAction::ToggleSaveCredentials, |c| {
        c.save_credentials = !c.save_credentials
    }),
    (SettingsAction::ToggleEnableTorrserver, |c| {
        c.enable_torrserver = !c.enable_torrserver
    }),
    (SettingsAction::ToggleThemeBackground, |c| {
        c.theme_background = !c.theme_background
    }),
    (SettingsAction::ToggleTruecolor, |c| {
        c.truecolor = !c.truecolor
    }),
    (SettingsAction::ToggleFalseTty, |c| {
        c.false_tty = !c.false_tty
    }),
    (SettingsAction::ToggleVimKeys, |c| c.vim_keys = !c.vim_keys),
    (SettingsAction::ToggleMouse, |c| {
        c.disable_mouse = !c.disable_mouse
    }),
    (SettingsAction::ToggleDisablePresets, |c| {
        c.disable_presets = !c.disable_presets
    }),
    (SettingsAction::ToggleShowBoxes, |c| {
        c.show_boxes = !c.show_boxes
    }),
    (SettingsAction::ToggleRoundedCorners, |c| {
        c.rounded_corners = !c.rounded_corners
    }),
    (SettingsAction::ToggleTerminalSync, |c| {
        c.terminal_sync = !c.terminal_sync
    }),
    (SettingsAction::ToggleDownloadEnabled, |c| {
        c.download_enabled = !c.download_enabled
    }),
    (SettingsAction::ToggleCloseTorrentCoreOnExit, |c| {
        c.close_torrent_core_on_exit = !c.close_torrent_core_on_exit
    }),
    (SettingsAction::ToggleSaveOnExit, |c| {
        c.save_config_on_exit = !c.save_config_on_exit
    }),
];

/// Flip the field `action` names, if it names one. Returns whether it
/// did, which is what the caller uses to decide the modal needs
/// rebuilding.
fn apply_bool_toggle(config: &mut Config, action: SettingsAction) -> bool {
    match BOOL_TOGGLES.iter().find(|(a, _)| *a == action) {
        Some((_, flip)) => {
            flip(config);
            true
        }
        None => false,
    }
}

/// The hash to stop on the way out, or `None` to leave it downloading.
///
/// A free function taking the config rather than `self`, so what decides
/// is testable without a terminal, a browser, or a running TorrServer --
/// the exit path has all three, and is the one path nobody exercises by
/// hand twice.
pub fn stop_download_on_exit<'a>(config: &Config, active_hash: Option<&'a str>) -> Option<&'a str> {
    if config.close_torrent_core_on_exit {
        active_hash
    } else {
        None
    }
}

/// Do the stop [`stop_download_on_exit`] asks for, and say what happened.
///
/// **`pause`, never `remove`.** Dropping the torrent is what "stop the
/// download" means: it stays on the server and on disk and resumes when
/// asked for again. Removing it would delete the user's file on the way
/// out of the program they were watching it with.
///
/// Returns a line to show, or `None` when there was nothing to stop, so
/// a run with the option off stays silent. Free function taking the
/// client, so the test can watch the bytes rather than re-type the call.
pub async fn stop_the_download(
    config: &Config,
    active_hash: Option<&str>,
    torrserver: &crate::torrserver::api::TorrServer,
) -> Option<String> {
    let hash = stop_download_on_exit(config, active_hash)?;
    Some(match torrserver.pause(hash).await {
        Ok(()) => format!("Stopped the download on exit ({hash})."),
        Err(e) => format!("Could not stop the download on exit: {e}"),
    })
}

/// The file name a result title may safely have on disk: everything
/// outside alphanumerics, spaces and the usual punctuation becomes `_`.
/// Shared by the `.torrent` and `.magnet` paths so the two spell the
/// same title the same way -- they are siblings in one download
/// directory, and a mismatch would show up as two names for one row.
pub fn safe_filename(title: &str) -> String {
    title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

/// What the download key owes a row that has no `.torrent` to fetch
/// (B8 wave 1: YTS publishes magnets, not files): `(file name, contents)`
/// for a `<title>.magnet` file, or `None` when the row *does* have a
/// download URL and must go through its Source exactly as before.
///
/// A row with neither URL nor magnet also returns `None`: it then fails
/// in the normal path with a message, which is the honest outcome --
/// the alternative is writing an empty file that looks like a result.
pub fn magnet_only_download(
    item: &crate::sources::models::TorrentItem,
) -> Option<(String, String)> {
    if !item.download_url.is_empty() {
        return None;
    }
    let magnet = item.magnet.as_deref()?;
    Some((
        format!("{}.magnet", safe_filename(&item.title)),
        format!("{}\n", magnet),
    ))
}

/// The registered id to talk to for a result row. A row whose id is not
/// in the registry (misspelled, or from a source since removed) holds
/// rutracker-shaped URLs, so it falls back to `"rutracker"`.
///
/// That fallback is logged rather than silent: this is the one place a
/// wrong id turns into a request to somebody else's server, and the log
/// line costs nothing when the id is right.
pub fn source_id_for(item: &crate::sources::models::TorrentItem) -> &'static str {
    match source::get_source(&item.source) {
        Some(s) => s.id,
        None => {
            crate::log::log(
                "app",
                &format!(
                    "row '{}' has no registered source (id '{}'); talking to rutracker instead",
                    truncate_for_log(&item.title),
                    item.source
                ),
            );
            "rutracker"
        }
    }
}

/// Enough of a title to identify a row in the log, without a multi-
/// kilobyte line: track titles run to several hundred characters.
fn truncate_for_log(s: &str) -> &str {
    const LIMIT: usize = 60;
    match s.char_indices().nth(LIMIT) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

/// Fill a row's magnet in from the row's own page, for rows that carry
/// neither a magnet nor a `.torrent` link (1337x, B8 wave 3).
///
/// Deliberately at play/download time, not at search time: the link is
/// only worth a request for a row somebody actually picks, so a search
/// of 20 rows stays one request instead of torio's fan-out of up to 8
/// detail pages -- at the price that every one of those 20 rows is
/// playable, where a fan-out leaves the rest unplayable. Rows that
/// already carry a magnet or a file never reach the `Source` call, so
/// the other six sources are not touched by this at all.
pub async fn fill_missing_magnet(
    item: &mut crate::sources::models::TorrentItem,
    source: &dyn Source,
) -> Result<()> {
    if item.magnet.is_some() || !item.download_url.is_empty() {
        return Ok(());
    }
    if let Some(magnet) = source.resolve_magnet(&item.page_url).await? {
        item.magnet = Some(magnet);
    }
    Ok(())
}

/// What pressing Enter in the results view means (B0.4).
///
/// The old chain of `if let Some(..)` calls let an empty search query fall
/// through: `submit_search()` turned `input_mode` off and returned `None`,
/// so the very same key reached `submit_selection()` and started a stream.
/// Making the decision a value -- with "typed but empty" spelled out as its
/// own outcome -- is what makes that impossible to reintroduce silently.
#[derive(Debug, PartialEq, Eq)]
pub enum EnterAction {
    /// Search input is focused and holds a non-empty query.
    SubmitQuery,
    /// Input focused but the query is empty: Enter only leaves input mode.
    DoNothing,
    /// The source selection or the category changed and needs a
    /// re-search instead of playing.
    RestartSearch,
    /// Play the highlighted result.
    Play,
}

/// `source_changed` and `group_changed` are separate flags because they
/// are set by separate switches, but they mean the same thing here: the
/// table no longer answers for the selection above it, so Enter owes a
/// search before it may start a stream.
pub fn enter_action(
    input_mode: bool,
    has_query: bool,
    source_changed: bool,
    group_changed: bool,
    has_selection: bool,
) -> EnterAction {
    if input_mode {
        if has_query {
            EnterAction::SubmitQuery
        } else {
            EnterAction::DoNothing
        }
    } else if source_changed || group_changed {
        EnterAction::RestartSearch
    } else if has_selection {
        EnterAction::Play
    } else {
        EnterAction::DoNothing
    }
}

/// What the Log zone says after "Enable TorrServer" was switched on.
///
/// `start` is `None` when the server already answered the ping, `Some(Ok)`
/// when `systemctl start` ran it, and `Some(Err(reason))` when the unit
/// could not be started at all. doris has no password to give, so the
/// refusal is the whole point: the user needs to see `Access denied`
/// here, next to the switch that did nothing, rather than infer it from
/// a stream that fails later with no reason attached.
pub fn torrserver_enable_message(url: &str, start: Option<Result<String, String>>) -> String {
    match start {
        None => format!("TorrServer: reachable at {url}"),
        Some(Ok(_)) => format!(
            "TorrServer is not answering at {url}; `systemctl start torrserver` ran, \
             it may take a moment to bind"
        ),
        Some(Err(reason)) => format!(
            "TorrServer is not answering at {url} and could not be started: {reason} \
             (run `sudo systemctl start torrserver`)"
        ),
    }
}

/// Merge a detail modal's file list into the modal (П.7).
///
/// A free function over `&mut UiApp` for the same reason as
/// [`apply_source_done`]: the "is this answer still wanted?" decision is
/// the interesting part, and it should be testable without a terminal or
/// a running event loop.
///
/// The answer is tagged with the page it was asked about, so opening
/// another row's details drops the previous row's file list instead of
/// showing it in the wrong modal. A closed modal drops it too.
pub fn apply_detail_loaded(
    ui: &mut UiApp,
    page_url: &str,
    files: Vec<crate::sources::models::FileEntry>,
    error: Option<&str>,
) {
    if let Modal::TorrentDetail(ref mut state) = ui.modal {
        if state.item.page_url == page_url {
            state.files = files;
            state.pending = false;
            state.error = error.map(|e| e.to_string());
            // The cursor was clamped to the old list; a shorter answer
            // must not leave it past the end.
            if state.cursor >= state.files.len() {
                state.cursor = state.files.len().saturating_sub(1);
            }
        }
    }
}

pub struct App {
    args: Args,
    config: Config,
    ui: UiApp,
    event_handler: EventHandler,
    torrserver: TorrServer,
    browser: Option<Arc<Mutex<Browser>>>,
    /// Live sources keyed by id, built once via `source::build_source`
    /// and reused across start_search/load_more/do_login (all run in
    /// spawned tasks). The session state that used to live in a cached
    /// `RutrackerSearcher` -- its `logged_in` flag, so pagination didn't
    /// re-login every call -- now lives in the `Arc<dyn Source>` itself;
    /// the browser behind it was always reused via `get_browser()` and
    /// still is.
    sources: HashMap<&'static str, Arc<dyn Source>>,
    /// Each source's last "has another page" verdict (B2/B3): consulted
    /// by "Load more" and turned into `ui.all_loaded` by
    /// [`finish_search`].
    source_has_more: HashMap<String, bool>,
    /// How many rows each source has delivered for the current query (B3):
    /// the cursor "Load more" resumes it at. One per source, because
    /// rutor pages by 100 and rutracker by 50 -- a shared counter walks
    /// off rutor's page grid and it answers with nothing.
    source_offsets: HashMap<String, usize>,
    /// Recently fetched pages, consulted before any source is spawned
    /// (B5): a fresh hit answers immediately, browser and all.
    cache: Arc<SearchCache>,
    browser_visibility: BrowserVisibility,
    /// Kept alive (never read): dropping the last sender would close the
    /// channel the extension bridge searches through.
    #[allow(dead_code)]
    search_tx: mpsc::UnboundedSender<String>,
    search_rx: mpsc::UnboundedReceiver<String>,
    /// Bumped by every `start_search`; each dispatch carries the value it
    /// was started with, and results from an older generation are dropped.
    /// Without this, a slow answer from the previous query landed after the
    /// new one started and overwrote its results (B0.2) -- torio's
    /// equivalent is the AbortController + `alive` flag on a search.
    search_generation: u64,
    terminal_size: (u16, u16),
    /// Set by the SIGHUP/SIGTERM/SIGINT listener spawned in [`App::run`] so
    /// a terminal being closed (or a plain `kill`) goes through the normal
    /// exit path -- browser shutdown and temp-profile cleanup included --
    /// instead of dropping the process mid-flight and leaving the browser
    /// stack behind.
    exit_signal: Arc<AtomicBool>,
}

impl App {
    pub async fn new(args: Args, config: Config) -> Result<Self> {
        let torrserver_url = if args.torrserver == crate::torrserver::api::DEFAULT_URL {
            config.torrserver_url.clone()
        } else {
            args.torrserver.clone()
        };

        let browser_visibility_str = args
            .browser_visibility
            .clone()
            .unwrap_or_else(|| config.browser_visibility.clone());
        let browser_visibility: BrowserVisibility = browser_visibility_str.parse()?;

        let (search_tx, search_rx) = mpsc::unbounded_channel();

        let bridge_port = config.bridge_port;
        if bridge_port > 0 {
            // The server task owns its own handle (listener + router with a
            // cloned sender), so nothing needs to keep this struct alive.
            let mut bridge = BridgeServer::new(search_tx.clone(), bridge_port);
            if let Err(e) = bridge.start().await {
                crate::log::log("bridge", &format!("bridge server failed to start: {}", e));
            }
        }

        let event_handler = EventHandler::new(std::time::Duration::from_millis(EVENT_POLL_MS));
        let torrserver = TorrServer::new(&torrserver_url);
        crate::torrent::Manager::spawn(
            torrserver.clone(),
            config.update_ms,
            event_handler.sender(),
        );

        let mut ui = UiApp::new(torrserver_url.clone(), config.theme_name.as_deref())
            .with_group_tabs(&config);
        // The zones know nothing about Config, so the saved tiling is
        // applied here rather than in `ZoneLayout::new`: the default
        // first preset (`1,3|4`) is what a fresh config.toml means.
        if !config.disable_presets {
            if let Some(spec) = config.presets.get(config.preset_index) {
                let spec = spec.clone();
                ui.zones.apply_preset(&spec);
            }
        }

        Ok(Self {
            ui,
            event_handler,
            torrserver,
            browser: None,
            sources: HashMap::new(),
            source_has_more: HashMap::new(),
            source_offsets: HashMap::new(),
            cache: Arc::new(SearchCache::new()),
            browser_visibility,
            search_tx,
            search_rx,
            args,
            config,
            terminal_size: (0, 0),
            search_generation: 0,
            exit_signal: Arc::new(AtomicBool::new(false)),
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        // The keyboard protocol is what makes Shift+Enter arrive as
        // Shift+Enter; `false_tty` asks for a terminal that may not
        // know the sequence, so it stays off there.
        let mut terminal = tui::init(!self.config.false_tty, !self.config.disable_mouse)?;
        self.terminal_size = terminal
            .size()
            .map(|s| (s.width, s.height))
            .unwrap_or((80, 24));

        Self::spawn_termination_watch(Arc::clone(&self.exit_signal));

        if let Some(query) = self.args.query.clone() {
            self.ui.search_input = query.clone();
            self.ui.show_menu = false;
            self.start_search(query).await;
        } else {
            self.ui.show_menu = false;
        }

        loop {
            self.draw_frame(&mut terminal)?;

            tokio::select! {
                event = self.event_handler.next() => {
                    match event? {
                        Event::Key(key) => self.handle_key(key).await?,
                        Event::Mouse(mouse) => self.handle_mouse(mouse).await,
                        Event::Tick => {},
                        Event::Resize(w, h) => {
                            self.terminal_size = (w, h);
                        },
                        Event::SourceDone {
                            source,
                            generation,
                            items,
                            has_more,
                            next_offset,
                            error,
                            timed_out,
                        } => {
                            let count = items.len();
                            let applied = apply_source_done(
                                &mut self.ui,
                                generation,
                                self.search_generation,
                                &source,
                                items,
                                error.as_deref(),
                            );
                            // A superseded dispatch may not touch the
                            // newer search's status or its paging verdict
                            // (B0.2), which is why the bookkeeping sits
                            // behind the merge's result.
                            if applied {
                                let status = SourceStatus::from_event(
                                    count,
                                    error.as_deref(),
                                    timed_out,
                                );
                                self.ui.source_status.insert(source.clone(), status);
                                self.source_has_more.insert(source.clone(), has_more);
                                // Failures deliver no rows and no cursor,
                                // so a failed page leaves the cursor where
                                // it was and the source gets asked again
                                // from there; a source that counts pages
                                // of its own hands over its own cursor.
                                let offset = self.source_offsets.entry(source).or_insert(0);
                                *offset = orchestrator::advance_offset(*offset, count, next_offset);
                            }
                        }
                        Event::SearchComplete { generation } => {
                            finish_search(
                                &mut self.ui,
                                generation,
                                self.search_generation,
                                &self.source_has_more,
                            );
                        }
                        Event::StreamComplete(url) => {
                            self.ui.state = AppState::Idle;
                            self.ui.add_log(&format!("Stream launched: {}", url));
                        }
                        Event::StreamError(err) => {
                            self.ui.state = AppState::Idle;
                            self.ui.add_log(&format!("Stream error: {}", err));
                        }
                        Event::StreamLog(msg) => {
                            self.ui.add_log(&msg);
                            self.ui.add_detail(&msg);
                        }
                        Event::LoginResult(success) => {
                            if success {
                                self.ui.add_log("Login successful!");
                                self.ui.add_detail("LOGIN: SUCCESS");
                            } else {
                                self.ui.add_log("Login failed.");
                                self.ui.add_detail("LOGIN: FAILED");
                            }
                        }
                        Event::ExtensionQuery(query) => {
                            self.ui.search_input = query.clone();
                            self.ui.show_menu = false;
                            self.start_search(query).await;
                        }
                        Event::TorrentListUpdate(list) => {
                            // Prefer the torrent we're actively
                            // managing/streaming; fall back to whatever
                            // TorrServer reports first so the panel shows
                            // something useful even before a stream has
                            // been started from this session (e.g. a
                            // torrent added in a previous run).
                            let chosen = match &self.ui.active_torrent_hash {
                                Some(hash) => list.iter().find(|t| &t.hash == hash).or_else(|| list.first()),
                                None => list.first(),
                            };
                            if let Some(t) = chosen {
                                self.ui.torrent_status = TorrentStatus {
                                    hash: t.hash.clone(),
                                    title: if t.name.is_empty() { self.ui.torrent_status.title.clone() } else { t.name.clone() },
                                    progress: t.progress(),
                                    download_speed: t.download_speed.max(0.0) as u64,
                                    upload_speed: t.upload_speed.max(0.0) as u64,
                                    seeds: t.connected_seeders.max(0) as u32,
                                    peers: t.active_peers.max(0) as u32,
                                    downloaded: t.loaded_size.max(0) as u64,
                                    total_size: t.total_size.max(0) as u64,
                                    status: t.status_string.clone(),
                                };
                                // Cap history length -- a very wide terminal
                                // in braille mode needs at most 2 samples
                                // per column, so this comfortably covers
                                // any realistic panel width.
                                const MAX_HISTORY: usize = 600;
                                self.ui.progress_history.push_back(self.ui.torrent_status.progress);
                                while self.ui.progress_history.len() > MAX_HISTORY {
                                    self.ui.progress_history.pop_front();
                                }
                            }
                        }
                        Event::TorrentActive(hash) => {
                            if self.ui.active_torrent_hash.as_deref() != Some(hash.as_str()) {
                                self.ui.progress_history.clear();
                            }
                            self.ui.active_torrent_hash = Some(hash);
                        }
                        Event::DetailLoaded { page_url, files, error } => {
                            apply_detail_loaded(
                                &mut self.ui,
                                &page_url,
                                files,
                                error.as_deref(),
                            );
                        }
                    }
                }
                query = self.search_rx.recv() => {
                    if let Some(query) = query {
                        self.ui.search_input = query.clone();
                        self.ui.show_menu = false;
                        self.start_search(query).await;
                    }
                }
            }

            if !self.ui.running || self.exit_signal.load(Ordering::Relaxed) {
                break;
            }
        }

        tui::restore(&mut terminal)?;

        // "Stop the download when doris exits" -- see `stop_the_download`
        // for why it is a pause and not a removal.
        if let Some(message) = stop_the_download(
            &self.config,
            self.ui.active_torrent_hash.as_deref(),
            &self.torrserver,
        )
        .await
        {
            eprintln!("{message}");
        }

        // End the WebDriver session while the runtime is still alive: the
        // session DELETE is what makes chromedriver take the browser down
        // with it, and Drop alone cannot await it (SIGKILLing chromedriver
        // first leaves the browser orphaned on a loaded page).
        if let Some(browser) = self.browser.take() {
            let mut browser = browser.lock().await;
            browser.shutdown().await;
        }

        if self.config.save_config_on_exit {
            // Same path `config::load` read from (see `persist_config`).
            if let Err(e) = crate::config::save(&self.config, self.args.config.as_deref()) {
                eprintln!("Failed to save config on exit: {}", e);
            }
        }

        Ok(())
    }

    /// Watch for a termination signal and expose it as a flag instead of
    /// letting the default handler kill the process: dying mid-flight skips
    /// `Browser::shutdown`, so chromedriver's session is never deleted, the
    /// temp profile (a ~100MB directory) and the run's Xvfb are left in /tmp
    /// forever, and any browser that did survive keeps rendering a page
    /// nobody will ever navigate away again.
    #[cfg(unix)]
    pub(super) fn spawn_termination_watch(flag: Arc<AtomicBool>) {
        use tokio::signal::unix::{signal, SignalKind};
        tokio::spawn(async move {
            let mut hup = match signal(SignalKind::hangup()) {
                Ok(s) => s,
                Err(e) => {
                    crate::log::log("app", &format!("could not install SIGHUP handler: {}", e));
                    return;
                }
            };
            let mut term = match signal(SignalKind::terminate()) {
                Ok(s) => s,
                Err(e) => {
                    crate::log::log("app", &format!("could not install SIGTERM handler: {}", e));
                    return;
                }
            };
            let mut interrupt = match signal(SignalKind::interrupt()) {
                Ok(s) => s,
                Err(e) => {
                    crate::log::log("app", &format!("could not install SIGINT handler: {}", e));
                    return;
                }
            };
            tokio::select! {
                _ = hup.recv() => crate::log::log("app", "SIGHUP received, exiting cleanly"),
                _ = term.recv() => crate::log::log("app", "SIGTERM received, exiting cleanly"),
                _ = interrupt.recv() => crate::log::log("app", "SIGINT received, exiting cleanly"),
            }
            flag.store(true, Ordering::Relaxed);
        });
    }

    #[cfg(not(unix))]
    pub(super) fn spawn_termination_watch(_flag: Arc<AtomicBool>) {}
}

// --- key routing: who owns Enter while the search box is being typed ------
//
// The search input has no zone of its own, so `input_mode` and "the
// Trackers panel is focused" are not mutually exclusive -- a query typed
// after clicking the panel used to have its Enter (and its `j`/`k`)
// swallowed by the panel. These run against a real `App` because the bug
// lives in the *order* of the match arms, which no pure helper sees.
