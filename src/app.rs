use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseButton, MouseEventKind};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{Mutex, mpsc};

use crate::browser::cdp::{Browser, BrowserVisibility};
use crate::browser::detect;
use crate::event::{Event, EventHandler};
use crate::search::cache::{CacheKey, SearchCache};
use crate::search::orchestrator::{self, SourceStatus};
use crate::search::ordering::{default_order, dedupe_by_hash};
use crate::search::source::{self, AuthContext, LogFn, SearchRequest, Source, SourceEnv};
use crate::torrserver::api::TorrServer;
use crate::bridge::handler::BridgeServer;
use crate::tui;
use crate::ui::app::{App as UiApp, AppState, Modal, SettingsAction, TorrentStatus, HeaderHint, TorrentClickAction};
use crate::ui::zones::ZoneId;
use crate::ui::menu::MenuItem;
use crate::ui::theme::Theme;
use crate::cli::Args;
use crate::config::Config;

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

/// The registered id to talk to for a result row. Rows carry their own
/// source id; rows from before that field existed (or with an id no
/// longer in the registry) hold rutracker-shaped URLs, so they fall back
/// to `"rutracker"` -- the same conservative default
/// [`source_needs_browser`] has had since B0.1.
pub fn source_id_for(item: &crate::search::models::TorrentItem) -> &'static str {
    source::get_source(&item.source).map(|s| s.id).unwrap_or("rutracker")
}

/// The single log line describing how one source's dispatch ended (B0.3):
/// `rutor: 42 results` / `rutracker: HTTP 503`.
///
/// Every source always reports one line, because `last_err` is only
/// surfaced to the user when *nothing* came back -- so before this, a
/// source failing next to a healthy one was completely silent.
pub fn source_outcome_line(source: &str, outcome: &Result<usize, String>) -> String {
    match outcome {
        Ok(count) => format!("{}: {} results", source, count),
        Err(err) => format!("{}: {}", source, err),
    }
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
    /// Source tab was switched and needs a re-search instead of playing.
    RestartSearch,
    /// Play the highlighted result.
    Play,
}

pub fn enter_action(
    input_mode: bool,
    has_query: bool,
    source_changed: bool,
    has_selection: bool,
) -> EnterAction {
    if input_mode {
        if has_query {
            EnterAction::SubmitQuery
        } else {
            EnterAction::DoNothing
        }
    } else if source_changed {
        EnterAction::RestartSearch
    } else if has_selection {
        EnterAction::Play
    } else {
        EnterAction::DoNothing
    }
}

/// Merge one source's page into the Results panel and log its B0.3
/// outcome line -- unless the event came from a dispatch that a newer
/// `start_search` has since superseded, in which case it is dropped and
/// `false` is returned (B0.2: a late answer from the previous query used
/// to land after the new one started and overwrite the fresh one's
/// results).
///
/// Since B3 rows arrive one source at a time, so this only ever appends:
/// the fresh search clears the table before dispatching, and appending
/// keeps the selection where the user put it while the other sources are
/// still answering.
///
/// A free function over `&mut UiApp` rather than a method on `App` so the
/// stale-vs-fresh decision is testable without a terminal, a browser or a
/// running event loop.
pub fn apply_source_done(
    ui: &mut UiApp,
    event_generation: u64,
    current_generation: u64,
    source: &str,
    items: Vec<crate::search::models::TorrentItem>,
    error: Option<&str>,
) -> bool {
    if event_generation != current_generation {
        ui.add_log("Dropped stale search results (superseded by a newer search)");
        return false;
    }
    let outcome: Result<usize, String> = match error {
        None => Ok(items.len()),
        Some(err) => Err(err.to_string()),
    };
    ui.add_log(&source_outcome_line(source, &outcome));
    ui.results.extend(items);
    ui.update_filter();
    true
}

/// Every source of `generation` reported in (or failed to): nothing more
/// is coming for it, so the UI goes idle. Whether "Load more" still has
/// anything to offer is read from the per-source `has_more` verdicts
/// (B2/B3) instead of the old `count < 50` guess.
///
/// Returns `false` when the completion belongs to a superseded dispatch
/// -- it must not flip a newer search back to idle (B0.2's rule,
/// applied to the new final event).
pub fn finish_search(
    ui: &mut UiApp,
    event_generation: u64,
    current_generation: u64,
    has_more: &HashMap<String, bool>,
) -> bool {
    if event_generation != current_generation {
        ui.add_log("Dropped stale search completion (superseded by a newer search)");
        return false;
    }
    present_results(ui);
    ui.all_loaded = !has_more.values().any(|&more| more);
    ui.state = AppState::Idle;
    true
}

/// Dedupe the merged multi-source list and put it into its default order
/// (B4). This runs exactly once per generation -- when every source has
/// answered -- because reordering while sources are still arriving would
/// move rows out from under the user's selection.
///
/// The row the selection pointed at is followed to its new position, so
/// what the user had highlighted stays highlighted; if dedup removed that
/// row, the selection clamps into the list instead of going stale.
fn present_results(ui: &mut UiApp) {
    let before = ui.results.len();
    let anchor = ui.results.get(ui.selected).cloned();

    ui.results = default_order(&dedupe_by_hash(&ui.results), false);

    let removed = before - ui.results.len();
    if removed > 0 {
        ui.add_log(&format!("Removed {} duplicate results", removed));
    }
    if let Some(anchor) = anchor {
        let keep = ui
            .results
            .iter()
            .position(|row| row.page_url == anchor.page_url && row.title == anchor.title);
        ui.selected = keep.unwrap_or_else(|| {
            ui.selected
                .min(ui.results.len().saturating_sub(1))
        });
    } else {
        ui.selected = 0;
    }
    ui.update_filter();
}

/// Resolve the cookie file path used for Rutracker login, or `None` if
/// "Save cookies" is off. `cli_override` is `Args.cookie_file` (the
/// `--cookie-file` flag) which takes priority when given; otherwise falls
/// back to `Config.cookie_file` (the `config.toml` setting, which used to
/// be completely dead -- see the instance method that calls this for the
/// full story).
pub fn resolve_cookie_file(config: &Config, cli_override: Option<&std::path::Path>) -> Option<std::path::PathBuf> {
    if !config.save_cookies {
        return None;
    }
    cli_override.map(|p| p.to_path_buf())
        .or_else(|| Some(std::path::PathBuf::from(&config.cookie_file)))
}

pub struct App {
    args: Args,
    #[allow(dead_code)]
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
    /// Where each source of the current dispatch stands (B3): `Pending`
    /// while its task runs, then `Ok`/`Error`/`Timeout` from its
    /// `SourceDone`. The log line is the interim surface; a status row
    /// can render this map later.
    source_status: HashMap<String, SourceStatus>,
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
    #[allow(dead_code)]
    search_tx: mpsc::UnboundedSender<String>,
    search_rx: mpsc::UnboundedReceiver<String>,
    /// Bumped by every `start_search`; each dispatch carries the value it
    /// was started with, and results from an older generation are dropped.
    /// Without this, a slow answer from the previous query landed after the
    /// new one started and overwrote its results (B0.2) -- torio's
    /// equivalent is the AbortController + `alive` flag on a search.
    search_generation: u64,
    bridge: Option<BridgeServer>,
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
        let torrserver_url = if args.torrserver == "http://127.0.0.1:8090" {
            config.torrserver_url.clone()
        } else {
            args.torrserver.clone()
        };

        let browser_visibility_str = args.browser_visibility.clone()
            .unwrap_or_else(|| config.browser_visibility.clone());
        let browser_visibility: BrowserVisibility = browser_visibility_str.parse()?;

        let browser_choice = args.browser.as_deref().or(config.browser.as_deref());
        let browser_priority = detect::parse_priority(&config.browser_priority);
        let browser_info = detect::detect_browser_with_priority(browser_choice, &browser_priority)
            .map(|(kind, path)| format!("{} [{}] ({})", kind, browser_visibility, path.display()))
            .unwrap_or_else(|e| format!("Error: {}", e));

        let (search_tx, search_rx) = mpsc::unbounded_channel();

        let bridge_port = config.bridge_port;
        let bridge = if bridge_port > 0 {
            let mut bridge = BridgeServer::new(search_tx.clone(), bridge_port);
            if bridge.start().await.is_ok() {
                Some(bridge)
            } else {
                None
            }
        } else {
            None
        };

        let event_handler = EventHandler::new(std::time::Duration::from_millis(100));
        let torrserver = TorrServer::new(&torrserver_url);
        crate::torrent::Manager::spawn(
            torrserver.clone(),
            config.update_ms,
            event_handler.sender(),
        );

        Ok(Self {
            ui: UiApp::new(
                torrserver_url.clone(),
                browser_info,
                browser_visibility == BrowserVisibility::Hidden,
                config.theme_name.as_deref(),
                resolve_download_dir(&config),
                config.graph_symbol.clone(),
                config.rounded_corners,
                config.theme_background,
                config.truecolor,
                config.false_tty,
            ),
            event_handler,
            torrserver,
            browser: None,
            sources: HashMap::new(),
            source_status: HashMap::new(),
            source_has_more: HashMap::new(),
            source_offsets: HashMap::new(),
            cache: Arc::new(SearchCache::new()),
            browser_visibility,
            search_tx,
            search_rx,
            bridge,
            args,
            config,
            terminal_size: (0, 0),
            search_generation: 0,
            exit_signal: Arc::new(AtomicBool::new(false)),
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut terminal = tui::init()?;
        self.terminal_size = terminal.size().map(|s| (s.width, s.height)).unwrap_or((80, 24));

        Self::spawn_termination_watch(Arc::clone(&self.exit_signal));

        if let Some(query) = self.args.query.clone() {
            self.ui.search_input = query.clone();
            self.ui.show_menu = false;
            self.start_search(query).await;
        } else {
            self.ui.show_menu = false;
        }

        loop {
            terminal.draw(|frame| {
                self.terminal_size = (frame.area().width, frame.area().height);
                self.ui.render(frame);
            })?;

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
                                self.source_status.insert(source.clone(), status);
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

        // End the WebDriver session while the runtime is still alive: the
        // session DELETE is what makes chromedriver take the browser down
        // with it, and Drop alone cannot await it (SIGKILLing chromedriver
        // first leaves the browser orphaned on a loaded page).
        if let Some(browser) = self.browser.take() {
            let mut browser = browser.lock().await;
            browser.shutdown().await;
        }

        if self.config.save_config_on_exit {
            if let Err(e) = crate::config::save(&self.config, None) {
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
    fn spawn_termination_watch(flag: Arc<AtomicBool>) {
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
    fn spawn_termination_watch(_flag: Arc<AtomicBool>) {}

    async fn handle_mouse(&mut self, mouse: MouseEvent) {
        if self.config.disable_mouse {
            return;
        }
        if self.ui.show_menu {
            return;
        }

        match mouse.kind {
            MouseEventKind::ScrollUp => {
                if self.ui.detail_log_mode {
                    self.ui.detail_log_scroll = self.ui.detail_log_scroll.saturating_sub(3);
                } else if self.ui.modal == Modal::None {
                    if let Some(id) = self.ui.zone_at(mouse.row, mouse.column) {
                        self.ui.zones.focused = id;
                        match id {
                            ZoneId::Log => self.ui.scroll_logs_up(),
                            ZoneId::Results => self.handle_nav_up(),
                            // Torrent and Extra have nothing scrollable yet
                            // (a single status readout, and an unbuilt
                            // placeholder respectively) -- focusing them on
                            // hover is still correct, there's just no list
                            // to move within.
                            ZoneId::Torrent | ZoneId::Extra => {}
                        }
                    }
                }
            }
            MouseEventKind::ScrollDown => {
                if self.ui.detail_log_mode {
                    self.ui.detail_log_scroll = (self.ui.detail_log_scroll + 3).min(self.ui.detail_logs.len());
                } else if self.ui.modal == Modal::None {
                    if let Some(id) = self.ui.zone_at(mouse.row, mouse.column) {
                        self.ui.zones.focused = id;
                        match id {
                            ZoneId::Log => self.ui.scroll_logs_down(),
                            ZoneId::Results => self.handle_nav_down().await,
                            ZoneId::Torrent | ZoneId::Extra => {}
                        }
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if self.ui.detail_log_mode {
                    self.ui.detail_log_scroll = self.ui.detail_logs.len();
                } else if mouse.row == 0 && self.ui.modal == Modal::None {
                    match self.ui.hint_at_column(mouse.column) {
                        Some(HeaderHint::Search) => self.ui.enter_input_mode(),
                        Some(HeaderHint::Settings) => self.ui.open_settings(&self.config),
                        Some(HeaderHint::Log) => self.ui.toggle_detail_log(),
                        Some(HeaderHint::Filter) => self.ui.zones.filter_mode = true,
                        None => {}
                    }
                } else if self.ui.modal == Modal::None {
                    match self.ui.click_at(mouse.row, mouse.column) {
                        Some(TorrentClickAction::TogglePause) => self.toggle_pause_active_torrent().await,
                        Some(TorrentClickAction::Remove) => self.remove_active_torrent().await,
                        None => {}
                    }
                }
            }
            _ => {}
        }
    }

    /// Pause (drop) or resume (re-get) the torrent the panel is currently
    /// showing. See the doc comment on `ui::App::torrent_paused` for why
    /// this is tracked client-side rather than read back from TorrServer.
    async fn toggle_pause_active_torrent(&mut self) {
        let Some(hash) = self.ui.active_torrent_hash.clone() else {
            self.ui.add_log("No active torrent to pause/resume.");
            return;
        };
        if self.ui.torrent_paused {
            match self.torrserver.resume(&hash).await {
                Ok(_) => {
                    self.ui.torrent_paused = false;
                    self.ui.add_log("Torrent resumed.");
                }
                Err(e) => self.ui.add_log(&format!("Resume failed: {}", e)),
            }
        } else {
            match self.torrserver.pause(&hash).await {
                Ok(_) => {
                    self.ui.torrent_paused = true;
                    self.ui.add_log("Torrent paused.");
                }
                Err(e) => self.ui.add_log(&format!("Pause failed: {}", e)),
            }
        }
    }

    /// Remove the active torrent from TorrServer entirely.
    async fn remove_active_torrent(&mut self) {
        let Some(hash) = self.ui.active_torrent_hash.take() else {
            self.ui.add_log("No active torrent to remove.");
            return;
        };
        match self.torrserver.remove(&hash).await {
            Ok(_) => {
                self.ui.torrent_status = TorrentStatus::default();
                self.ui.torrent_paused = false;
                self.ui.progress_history.clear();
                self.ui.add_log("Torrent removed.");
            }
            Err(e) => self.ui.add_log(&format!("Remove failed: {}", e)),
        }
    }

    /// Fetch a result's `.torrent` bytes through the `Source` that
    /// actually owns it, instead of always going through rutracker's
    /// browser session -- which for a rutor row either failed ("No
    /// browser session") or fetched `rutor.org/download/...`
    /// cross-origin from a rutracker page (B0.1). Since B2 the client is
    /// just `&dyn Source`: which one to hand in is decided by
    /// [`source_id_for`] + [`source_needs_browser`] at the call site.
    /// Shared by `spawn_stream` and `download_selected_to_disk`.
    async fn download_bytes_for(
        item: &crate::search::models::TorrentItem,
        source: &dyn Source,
    ) -> Result<Vec<u8>> {
        source.download_torrent(&item.download_url).await
    }

    /// Download the selected result's .torrent file to disk (Options ->
    /// download's resolved directory), dispatching to whichever Source
    /// actually produced it -- `TorrentItem.source` matters here because
    /// the "all" Results tab can mix rows from more than one source at
    /// once, each needing a different download client.
    async fn download_selected_to_disk(&mut self) {
        let Some(item) = self.ui.results.get(self.ui.selected).cloned() else {
            self.ui.add_log("No result selected to download.");
            return;
        };

        if !self.config.download_enabled {
            self.ui.add_log("Downloading is disabled in Options -> download -> Enable downloading.");
            return;
        }

        self.ui.add_log(&format!("Downloading '{}'...", item.title));

        // `get_source` launches the browser only for sources whose
        // registry entry says they need one -- a rutor download must
        // never start Chrome (B0.1), and `requires_browser` is what says
        // so (B2).
        let bytes_result: Result<Vec<u8>> = match self.get_source(source_id_for(&item)).await {
            Ok(source) => Self::download_bytes_for(&item, source.as_ref()).await,
            Err(e) => Err(e),
        };

        match bytes_result {
            Ok(bytes) => {
                let dir = self.resolve_download_dir();
                let safe_title: String = item.title.chars()
                    .map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') { c } else { '_' })
                    .collect();
                let path = std::path::Path::new(&dir).join(format!("{}.torrent", safe_title.trim()));
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                match std::fs::write(&path, &bytes) {
                    Ok(_) => self.ui.add_log(&format!("Saved to {}", path.display())),
                    Err(e) => self.ui.add_log(&format!("Failed to save file: {}", e)),
                }
            }
            Err(e) => self.ui.add_log(&format!("Download failed: {}", e)),
        }
    }

    /// Show the selected result's full details in the log -- the 'v'
    /// action from the Results panel's bottom action bar.
    fn show_selected_info(&mut self) {
        let Some(item) = self.ui.results.get(self.ui.selected) else {
            self.ui.add_log("No result selected.");
            return;
        };
        let source = if item.source.is_empty() { "rutracker" } else { item.source.as_str() };
        self.ui.add_log(&format!(
            "INFO: {}  |  size={}  seeds={}  date={}  source={}  url={}",
            item.title, item.size, item.seeds, item.date, source, item.page_url,
        ));
    }

    /// See the free function of the same name for the resolution logic;
    /// this just supplies `&self.config`.
    fn resolve_download_dir(&self) -> String {
        resolve_download_dir(&self.config)
    }

    /// See the free function of the same name for the resolution logic
    /// and why this exists; this just supplies `&self.config`/`self.args`.
    fn resolve_cookie_file(&self) -> Option<std::path::PathBuf> {
        resolve_cookie_file(&self.config, self.args.cookie_file.as_deref())
    }

    /// Move the selection down in the focused zone, loading the next page
    /// of results if the Results zone just scrolled near its end. Shared
    /// by the Down arrow (always active) and the vim-style 'j' (only when
    /// `config.vim_keys` is on) -- see `handle_key`.
    async fn handle_nav_down(&mut self) {
        match self.ui.zones.focused {
            ZoneId::Results => {
                self.ui.navigate_down();
                if self.ui.needs_more() {
                    if let Some(q) = self.ui.search_query.clone() {
                        self.load_more(q).await;
                    }
                }
            }
            ZoneId::Log => self.ui.scroll_logs_down(),
            _ => {}
        }
    }

    /// Counterpart to [`handle_nav_down`](Self::handle_nav_down) for the Up
    /// arrow / vim-style 'k'.
    fn handle_nav_up(&mut self) {
        match self.ui.zones.focused {
            ZoneId::Results => { self.ui.navigate_up(); }
            ZoneId::Log => self.ui.scroll_logs_up(),
            _ => {}
        }
    }

    async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if self.ui.show_menu {
            return self.handle_menu_key(key).await;
        }

        if self.ui.detail_log_mode {
            match key.code {
                KeyCode::Char('L') | KeyCode::Esc => {
                    self.ui.detail_log_mode = false;
                }
                KeyCode::Char('j') if self.config.vim_keys => {
                    self.ui.detail_log_scroll = (self.ui.detail_log_scroll + 1).min(self.ui.detail_logs.len());
                }
                KeyCode::Down => {
                    self.ui.detail_log_scroll = (self.ui.detail_log_scroll + 1).min(self.ui.detail_logs.len());
                }
                KeyCode::Char('k') if self.config.vim_keys => {
                    self.ui.detail_log_scroll = self.ui.detail_log_scroll.saturating_sub(1);
                }
                KeyCode::Up => {
                    self.ui.detail_log_scroll = self.ui.detail_log_scroll.saturating_sub(1);
                }
                KeyCode::PageUp => {
                    self.ui.detail_log_scroll = self.ui.detail_log_scroll.saturating_sub(20);
                }
                KeyCode::PageDown => {
                    self.ui.detail_log_scroll = (self.ui.detail_log_scroll + 20).min(self.ui.detail_logs.len());
                }
                _ => {}
            }
            return Ok(());
        }

        if let Modal::HealthCheck(_) = self.ui.modal {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                self.ui.modal = Modal::None;
            }
            return Ok(());
        }

        if let Modal::Settings(_) = self.ui.modal {
            if let Some(action) = self.ui.settings_key(key) {
                match action {
                    SettingsAction::ToggleBrowserVisibility => {
                        self.ui.browser_hidden = !self.ui.browser_hidden;
                        // Keep the value actually used to launch the
                        // browser in sync. Previously this toggle only
                        // updated the display label and had zero effect on
                        // the next launch (ROADMAP.md bug B6).
                        self.browser_visibility = if self.ui.browser_hidden {
                            BrowserVisibility::Hidden
                        } else {
                            BrowserVisibility::Visible
                        };
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::ToggleMode => {
                        self.ui.stream_mode = !self.ui.stream_mode;
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::CyclePrioritizeBrowser => {
                        const ORDER: &[&str] = &["helium", "brave", "chrome", "chromium"];
                        let current = self.config.browser_priority.first().cloned().unwrap_or_default();
                        let next_first = match ORDER.iter().position(|&k| k == current) {
                            Some(i) => ORDER[cycle_index(i, ORDER.len(), self.ui.last_cycle_direction)],
                            None => ORDER[0],
                        };
                        // Move next_first to the front, keep the rest in
                        // their existing relative order.
                        let mut rest: Vec<String> = self.config.browser_priority.iter()
                            .filter(|k| k.as_str() != next_first)
                            .cloned()
                            .collect();
                        let mut new_priority = vec![next_first.to_string()];
                        new_priority.append(&mut rest);
                        self.config.browser_priority = new_priority;
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::ToggleCloseBrowserOnExit => {
                        self.config.close_browser_on_exit = !self.config.close_browser_on_exit;
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::ToggleSaveCookies => {
                        self.config.save_cookies = !self.config.save_cookies;
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::ToggleSaveCredentials => {
                        self.config.save_credentials = !self.config.save_credentials;
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::EditCredentials => {
                        self.ui.open_login_modal();
                    }
                    SettingsAction::CheckTorrserverStatus => {
                        let reachable = self.torrserver.is_reachable().await;
                        self.ui.add_log(if reachable {
                            "TorrServer: reachable"
                        } else {
                            "TorrServer: not reachable"
                        });
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::ToggleSourceRutracker => {
                        let id = "rutracker";
                        if self.config.enabled_sources.iter().any(|s| s == id) {
                            self.config.enabled_sources.retain(|s| s != id);
                        } else {
                            self.config.enabled_sources.push(id.to_string());
                        }
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::ToggleSourceRutor => {
                        let id = "rutor";
                        if self.config.enabled_sources.iter().any(|s| s == id) {
                            self.config.enabled_sources.retain(|s| s != id);
                        } else {
                            self.config.enabled_sources.push(id.to_string());
                        }
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::OpenLog => {
                        self.ui.modal = Modal::None;
                        self.ui.detail_log_mode = true;
                    }
                    SettingsAction::RunHealthCheck => {
                        let results = self.ui.health_check().await;
                        self.ui.modal = Modal::HealthCheck(results);
                    }
                    SettingsAction::CycleTheme => {
                        let themes = Theme::load_themes();
                        if let Some(pos) = themes.iter().position(|t| t.name == self.ui.theme.name) {
                            let next = cycle_index(pos, themes.len(), self.ui.last_cycle_direction);
                            self.ui.theme = themes[next].clone();
                        } else if !themes.is_empty() {
                            self.ui.theme = themes[0].clone();
                        }
                        self.config.theme_name = Some(self.ui.theme.name.clone());
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::ToggleThemeBackground => {
                        self.config.theme_background = !self.config.theme_background;
                        self.ui.theme_background = self.config.theme_background;
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::ToggleTruecolor => {
                        self.config.truecolor = !self.config.truecolor;
                        self.ui.truecolor = self.config.truecolor;
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::ToggleFalseTty => {
                        self.config.false_tty = !self.config.false_tty;
                        self.ui.false_tty = self.config.false_tty;
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::ToggleVimKeys => {
                        self.config.vim_keys = !self.config.vim_keys;
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::ToggleMouse => {
                        self.config.disable_mouse = !self.config.disable_mouse;
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::ToggleDisablePresets => {
                        self.config.disable_presets = !self.config.disable_presets;
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::CyclePreset => {
                        if !self.config.disable_presets && !self.config.presets.is_empty() {
                            self.config.preset_index =
                                cycle_index(self.config.preset_index, self.config.presets.len(), self.ui.last_cycle_direction);
                            let spec = self.config.presets[self.config.preset_index].clone();
                            self.ui.zones.apply_preset(&spec);
                        }
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::ToggleShowBoxes => {
                        self.config.show_boxes = !self.config.show_boxes;
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::SetUpdateMs => {
                        // No numeric text-entry widget exists in the
                        // Settings modal yet, so this cycles through a
                        // fixed set of sensible intervals -- same
                        // interaction pattern as Color theme/Presets/Graph
                        // symbol above. A free-form numeric input is a
                        // reasonable follow-up once the modal supports one.
                        const STEPS: &[u64] = &[250, 500, 1000, 2000, 5000, 10000, 30000, 60000];
                        let next = match STEPS.iter().position(|&v| v == self.config.update_ms) {
                            Some(i) => STEPS[cycle_index(i, STEPS.len(), self.ui.last_cycle_direction)],
                            None => STEPS[0],
                        };
                        self.config.update_ms = next;
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::ToggleRoundedCorners => {
                        self.config.rounded_corners = !self.config.rounded_corners;
                        self.ui.rounded_corners = self.config.rounded_corners;
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::ToggleTerminalSync => {
                        self.config.terminal_sync = !self.config.terminal_sync;
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::CycleGraphSymbol => {
                        const SYMBOLS: &[&str] = &["braille", "block", "dot"];
                        let next = match SYMBOLS.iter().position(|&s| s == self.config.graph_symbol) {
                            Some(i) => SYMBOLS[cycle_index(i, SYMBOLS.len(), self.ui.last_cycle_direction)],
                            None => SYMBOLS[0],
                        };
                        self.config.graph_symbol = next.to_string();
                        self.ui.graph_symbol = next.to_string();
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::ToggleDownloadEnabled => {
                        self.config.download_enabled = !self.config.download_enabled;
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::CycleDownloadDirMode => {
                        const MODES: &[&str] = &["default", "custom1", "custom2", "custom3"];
                        let next = match MODES.iter().position(|&m| m == self.config.download_dir_mode) {
                            Some(i) => MODES[cycle_index(i, MODES.len(), self.ui.last_cycle_direction)],
                            None => MODES[0],
                        };
                        self.config.download_dir_mode = next.to_string();
                        // Keep the display field ui.download_dir (used
                        // wherever a "current download directory" is shown)
                        // in sync with the resolved effective directory.
                        self.ui.download_dir = self.resolve_download_dir();
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::ToggleDownloadSequential => {
                        self.config.download_sequential = !self.config.download_sequential;
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::CycleDownloadSpeedLimit => {
                        const STEPS: &[u32] = &[0, 128, 256, 512, 1024, 2048, 5120, 10240];
                        let next = match STEPS.iter().position(|&v| v == self.config.download_speed_limit_kbps) {
                            Some(i) => STEPS[cycle_index(i, STEPS.len(), self.ui.last_cycle_direction)],
                            None => STEPS[0],
                        };
                        self.config.download_speed_limit_kbps = next;
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::CycleUploadSpeedLimit => {
                        const STEPS: &[u32] = &[0, 64, 128, 256, 512, 1024, 2048, 5120];
                        let next = match STEPS.iter().position(|&v| v == self.config.upload_speed_limit_kbps) {
                            Some(i) => STEPS[cycle_index(i, STEPS.len(), self.ui.last_cycle_direction)],
                            None => STEPS[0],
                        };
                        self.config.upload_speed_limit_kbps = next;
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::ToggleCloseTorrentCoreOnExit => {
                        self.config.close_torrent_core_on_exit = !self.config.close_torrent_core_on_exit;
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::ToggleSaveOnExit => {
                        self.config.save_config_on_exit = !self.config.save_config_on_exit;
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::Close => {}
                }
                // Persist every settings change immediately rather than
                // only on a clean exit ("Save config on exit" governs a
                // final flush, not whether changes are remembered at all
                // -- a crash between now and exit shouldn't lose them,
                // and it previously did).
                let _ = crate::config::save(&self.config, None);
            }
            return Ok(());
        }

        if self.ui.modal != Modal::None {
            if let Some((username, password)) = self.ui.login_modal_key(key) {
                self.do_login(&username, &password).await;
            }
            return Ok(());
        }

        if self.ui.zones.filter_mode {
            match key.code {
                KeyCode::Esc => {
                    self.ui.zones.filter_mode = false;
                    self.ui.zones.filter_input.clear();
                    self.ui.update_filter();
                }
                KeyCode::Enter => {
                    self.ui.zones.filter_mode = false;
                    self.ui.update_filter();
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.ui.zones.filter_input.push(c);
                    self.ui.update_filter();
                }
                KeyCode::Backspace => {
                    self.ui.zones.filter_input.pop();
                    self.ui.update_filter();
                }
                _ => {}
            }
            return Ok(());
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.ui.quit();
            }
            KeyCode::Char('m') if !self.ui.input_mode => {
                self.ui.show_menu = !self.ui.show_menu;
            }
            KeyCode::Char('F') if !self.ui.input_mode => {
                self.ui.zones.filter_mode = true;
            }
            KeyCode::Char('f') if !self.ui.input_mode => {
                if self.ui.zones.fullscreen.is_some() {
                    self.ui.zones.set_fullscreen(None);
                } else {
                    self.ui.zones.set_fullscreen(Some(self.ui.zones.focused));
                }
            }
            KeyCode::Char('1') if !self.ui.input_mode => {
                self.ui.zones.toggle(ZoneId::Results);
            }
            KeyCode::Char('2') if !self.ui.input_mode => {
                self.ui.zones.toggle(ZoneId::Torrent);
            }
            KeyCode::Char('3') if !self.ui.input_mode => {
                self.ui.zones.toggle(ZoneId::Log);
            }
            KeyCode::Char('4') if !self.ui.input_mode => {
                self.ui.zones.toggle(ZoneId::Extra);
            }
            KeyCode::Char('p') if !self.ui.input_mode && self.ui.zones.focused == ZoneId::Torrent => {
                self.toggle_pause_active_torrent().await;
            }
            KeyCode::Char('d') if !self.ui.input_mode && self.ui.zones.focused == ZoneId::Torrent => {
                self.remove_active_torrent().await;
            }
            KeyCode::Char('d') if !self.ui.input_mode && self.ui.zones.focused == ZoneId::Results => {
                self.download_selected_to_disk().await;
            }
            KeyCode::Char('v') if !self.ui.input_mode && self.ui.zones.focused == ZoneId::Results => {
                self.show_selected_info();
            }
            KeyCode::Char(']') if !self.ui.input_mode && self.ui.zones.focused == ZoneId::Results => {
                self.ui.cycle_source();
            }
            KeyCode::Char('j') if self.config.vim_keys => {
                self.handle_nav_down().await;
            }
            KeyCode::Down => {
                self.handle_nav_down().await;
            }
            KeyCode::Char('k') if self.config.vim_keys => {
                self.handle_nav_up();
            }
            KeyCode::Up => {
                self.handle_nav_up();
            }
            KeyCode::Tab if !self.ui.input_mode => {
                self.ui.zones.focus_next();
            }
            KeyCode::BackTab if !self.ui.input_mode => {
                self.ui.zones.focus_prev();
            }
            KeyCode::PageUp if !self.ui.input_mode => {
                match self.ui.zones.focused {
                    ZoneId::Log => self.ui.scroll_logs_page_up(),
                    _ => {}
                }
            }
            KeyCode::PageDown if !self.ui.input_mode => {
                match self.ui.zones.focused {
                    ZoneId::Log => self.ui.scroll_logs_page_down(),
                    _ => {}
                }
            }
            KeyCode::Char('s') | KeyCode::Char('i') if !self.ui.input_mode => {
                self.ui.enter_input_mode();
            }
            KeyCode::Char('L') if !self.ui.input_mode => {
                self.ui.toggle_detail_log();
            }
            KeyCode::Char('S') if !self.ui.input_mode => {
                self.ui.open_settings(&self.config);
            }
            KeyCode::Esc => {
                if self.ui.detail_log_mode {
                    self.ui.detail_log_mode = false;
                } else if self.ui.input_mode {
                    self.ui.exit_input_mode();
                } else {
                    self.ui.show_menu = !self.ui.show_menu;
                }
            }
            KeyCode::Enter => {
                let action = enter_action(
                    self.ui.input_mode,
                    !self.ui.search_input.is_empty(),
                    self.ui.source_changed,
                    self.ui.submit_selection().is_some(),
                );
                match action {
                    EnterAction::SubmitQuery => {
                        if let Some(query) = self.ui.submit_search() {
                            self.start_search(query).await;
                        }
                    }
                    EnterAction::RestartSearch => {
                        // Source was just switched via `]` or click — re-search
                        // with the new source instead of playing a torrent.
                        self.ui.source_changed = false;
                        if let Some(ref q) = self.ui.search_query.clone() {
                            let query = q.clone();
                            self.start_search(query).await;
                        }
                    }
                    EnterAction::Play => self.spawn_stream().await,
                    EnterAction::DoNothing => {}
                }
            }
            KeyCode::Char('u') if self.ui.input_mode && key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.ui.clear_input();
            }
            KeyCode::Char('w') if self.ui.input_mode && key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.ui.delete_word();
            }
            KeyCode::Char(c) if self.ui.input_mode && !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.ui.type_char(c);
            }
            KeyCode::Backspace if self.ui.input_mode => {
                self.ui.backspace();
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_menu_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.ui.quit();
            }
            KeyCode::Char('m') | KeyCode::Esc => {
                self.ui.show_menu = false;
            }
            KeyCode::Char('j') if self.config.vim_keys => {
                self.ui.menu.next();
            }
            KeyCode::Down => {
                self.ui.menu.next();
            }
            KeyCode::Char('k') if self.config.vim_keys => {
                self.ui.menu.prev();
            }
            KeyCode::Up => {
                self.ui.menu.prev();
            }
            KeyCode::Tab => {
                self.ui.menu.next();
            }
            KeyCode::BackTab => {
                self.ui.menu.prev();
            }
            KeyCode::Enter => {
                let item = self.ui.menu.select();
                match item {
                    MenuItem::Options => {
                        self.ui.show_menu = false;
                        self.ui.open_settings(&self.config);
                    }
                    MenuItem::Help => {
                        self.ui.menu.show_help = !self.ui.menu.show_help;
                    }
                    MenuItem::Quit => {
                        self.ui.quit();
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn do_login(&mut self, username: &str, password: &str) {
        self.ui.add_log(&format!("Logging in as '{}'...", username));

        if self.config.save_credentials {
            let _ = crate::credentials::save_credentials(username, password);
        }

        // The login modal is rutracker's -- it's the only source with a
        // session to establish (rutor's `ensure_logged_in` is a no-op) --
        // so the target id is fixed rather than read off the tab.
        let source = match self.get_source("rutracker").await {
            Ok(s) => s,
            Err(e) => {
                self.ui.add_log(&format!("Browser error: {}", e));
                return;
            }
        };

        let auth = AuthContext {
            cookie_file: self.resolve_cookie_file(),
            username: Some(username.to_string()),
            password: Some(password.to_string()),
        };
        let event_tx_login = self.event_handler.sender();
        let event_tx_result = self.event_handler.sender();
        let log: LogFn = Arc::new(move |msg: &str| {
            let _ = event_tx_login.send(Event::StreamLog(msg.to_string()));
        });

        tokio::spawn(async move {
            match source.ensure_logged_in(&auth, &log).await {
                Ok(true) => {
                    let _ = event_tx_result.send(Event::LoginResult(true));
                }
                Ok(false) => {
                    let _ = event_tx_result.send(Event::LoginResult(false));
                }
                Err(e) => {
                    log(&format!("LOGIN ERROR: {}", e));
                    let _ = event_tx_result.send(Event::LoginResult(false));
                }
            }
        });
    }

    async fn start_search(&mut self, query: String) {
        self.ui.state = AppState::Searching;
        self.ui.search_query = Some(query.clone());
        self.ui.all_loaded = false;
        // Rows now arrive one source at a time (B3), so there is no
        // single moment where the old list gets replaced by the new one:
        // the table empties here, and each source appends into it. The
        // per-source records belong to the old query and go with it.
        self.ui.results.clear();
        self.ui.selected = 0;
        self.ui.update_filter();
        self.source_status.clear();
        self.source_has_more.clear();
        self.source_offsets.clear();
        self.ui.add_log(&format!("Searching '{}' for '{}'...", self.ui.active_source, query));
        // New generation: anything still in flight for a previous query is
        // now stale and gets dropped when it lands (B0.2).
        self.search_generation += 1;
        let generation = self.search_generation;
        self.dispatch_search(query, generation).await;
    }

    /// Kick off the search for `query`: one task per source
    /// `orchestrator::selected_sources` picks (Results tab + Options)
    /// and `orchestrator::dispatch_plan` says is worth asking (a fresh
    /// search asks everyone, a "load more" asks only the sources that
    /// reported another page, each at its own cursor), each task under
    /// the per-source deadline. Every task reports in on its
    /// own through `Event::SourceDone`, so rows render as sources answer
    /// instead of after the slowest one, and `Event::SearchComplete`
    /// closes the generation -- both stamped with it, so an answer
    /// arriving after a newer search started is dropped instead of
    /// merged into it (B0.2).
    ///
    /// Rutor needs no browser/login at all; a source that does (rutracker)
    /// walks login *inside* its task, so the deadline covers that walk
    /// too rather than timing only the page fetch.
    async fn dispatch_search(&mut self, query: String, generation: u64) {
        let selected =
            orchestrator::selected_sources(&self.ui.active_source, &self.config.enabled_sources);
        if selected.is_empty() {
            self.ui.add_log("Selected source is disabled in Options -> streaming -> Sources.");
            self.ui.state = AppState::Idle;
            return;
        }

        let plan = orchestrator::dispatch_plan(
            &selected,
            &self.source_offsets,
            &self.source_has_more,
        );
        if plan.is_empty() {
            // Every selected source already reported its last page, so no
            // SourceDone is coming: close the generation here instead of
            // leaving the UI Searching.
            finish_search(
                &mut self.ui,
                generation,
                self.search_generation,
                &self.source_has_more,
            );
            return;
        }

        let tx = self.event_handler.sender();
        let mut tasks: Vec<(&'static str, tokio::task::JoinHandle<()>)> = Vec::new();

        for (info, offset) in plan {
            // Cache lookup before anything else, browser launch included
            // (B5): a fresh hit needs no task at all -- it just has to
            // arrive like the normal answer would, so the offsets,
            // paging verdict and log line all update through the same
            // path. Category is `None` until B6 gives dispatch one.
            let key = CacheKey::new(info.id, &query, None, offset);
            if let Some(done) = orchestrator::cached_source_done(&self.cache, &key, generation) {
                let _ = tx.send(done);
                continue;
            }

            let source = match self.get_source(info.id).await {
                Ok(s) => s,
                Err(e) => {
                    // This source can't run at all, and with no task
                    // spawned nothing else will ever speak for it: it
                    // still owes the user a line (B0.3).
                    self.ui.add_log(&source_outcome_line(info.id, &Err(e.to_string())));
                    self.source_status
                        .insert(info.id.to_string(), SourceStatus::Error(e.to_string()));
                    continue;
                }
            };

            self.source_status.insert(info.id.to_string(), SourceStatus::Pending);
            let req = SearchRequest::new(query.clone(), offset);
            let task = if info.requires_browser {
                // If "Save cookies" is off, don't pass a cookie file
                // path through at all -- see do_login for the same
                // gating.
                let event_tx_log = self.event_handler.sender();
                let cookie_file = self.resolve_cookie_file();
                let username = self.args.username.clone();
                let password = self.args.password.clone();
                let saved_creds = crate::credentials::load_credentials();
                tokio::spawn(orchestrator::run_source(
                    info.id,
                    generation,
                    orchestrator::cached_fetch(
                        async move {
                            let log: LogFn = Arc::new(move |msg: &str| {
                                let _ = event_tx_log.send(Event::StreamLog(msg.to_string()));
                            });
                            let (cred_user, cred_pass) = match (username, password) {
                                (Some(u), Some(p)) => (Some(u), Some(p)),
                                _ => match saved_creds {
                                    Some((u, p)) => {
                                        log("Using saved credentials");
                                        (Some(u), Some(p))
                                    }
                                    None => (None, None),
                                },
                            };
                            let auth = AuthContext {
                                cookie_file,
                                username: cred_user,
                                password: cred_pass,
                            };
                            match source.ensure_logged_in(&auth, &log).await {
                                Ok(true) => log("SEARCH: logged in, proceeding with search"),
                                Ok(false) => log("SEARCH: not logged in, proceeding anyway"),
                                Err(e) => log(&format!("SEARCH: login error: {}", e)),
                            }
                            source.search(&req).await
                        },
                        Arc::clone(&self.cache),
                        key,
                    ),
                    orchestrator::PER_SOURCE_TIMEOUT,
                    tx.clone(),
                ))
            } else {
                tokio::spawn(orchestrator::run_source(
                    info.id,
                    generation,
                    orchestrator::cached_fetch(
                        async move { source.search(&req).await },
                        Arc::clone(&self.cache),
                        key,
                    ),
                    orchestrator::PER_SOURCE_TIMEOUT,
                    tx.clone(),
                ))
            };
            tasks.push((info.id, task));
        }

        // Always coordinate, even with an empty task list: with nothing
        // spawned there is nothing to wait for, and sending
        // SearchComplete through the same channel is what keeps it
        // *behind* any SourceDone events already queued from cache hits
        // (finishing here instead would close the generation before its
        // own rows arrived and read paging verdicts nobody had recorded
        // yet).
        tokio::spawn(orchestrator::coordinate(generation, tasks, tx));
    }

    async fn spawn_stream(&mut self) {
        if self.ui.selected >= self.ui.results.len() {
            self.ui.add_log("No result selected");
            return;
        }
        let item = self.ui.results[self.ui.selected].clone();
        self.ui.state = AppState::Streaming;

        // Only browser-backed rows require a session that's already up;
        // a rutor row plays straight over plain HTTP and is built on the
        // spot (B0.1). Streaming never launches a browser itself: if the
        // session a search should have created isn't there, say so.
        let source = match self.source_for_row(source_id_for(&item)).await {
            Ok(s) => s,
            Err(e) => {
                self.ui.add_log(&e.to_string());
                return;
            }
        };

        let torrserver = self.torrserver.clone();
        let event_tx = self.event_handler.sender();

        tokio::spawn(async move {
            let log = |msg: &str| { let _ = event_tx.send(Event::StreamLog(msg.to_string())); };

            if !torrserver.is_reachable().await {
                log("TorrServer is not reachable! Start TorrServer on localhost:8090");
                let _ = event_tx.send(Event::StreamError("TorrServer unreachable".into()));
                return;
            }

            // How the torrent reaches TorrServer (B7): a row carrying a
            // magnet goes over as a *link* -- no .torrent round trip, and
            // the fetch starts from the DHT plus the link's trackers
            // instead of waiting on one host to hand over a file. Per
            // decision, any problem with the link (missing, malformed,
            // rejected) falls back to the old path rather than failing
            // the stream: those rows almost always play one way or the
            // other.
            let mut linked: Option<String> = None;
            if let Some(magnet) = item.magnet.as_deref() {
                log(&format!("Adding by magnet link: {}", item.title));
                match torrserver.add_by_link(magnet, &item.title).await {
                    Ok(hash) => {
                        log(&format!("Added by link, hash: {}", hash));
                        linked = Some(hash);
                    }
                    Err(e) => {
                        log(&format!("Magnet add failed ({}); fetching .torrent instead", e));
                    }
                }
            }

            let hash = match linked {
                Some(hash) => hash,
                None => {
                    log(&format!("Fetching .torrent file: {}", item.title));
                    let bytes = match Self::download_bytes_for(&item, source.as_ref()).await {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            log(&format!("Download error: {}", e));
                            let _ = event_tx.send(Event::StreamError(e.to_string()));
                            return;
                        }
                    };
                    log(&format!("Downloaded {} bytes", bytes.len()));
                    match torrserver.upload_torrent(&bytes, &item.title).await {
                        Ok(hash) => {
                            log(&format!("Uploaded, hash: {}", hash));
                            hash
                        }
                        Err(e) => {
                            log(&format!("Upload error: {}", e));
                            let _ = event_tx.send(Event::StreamError(e.to_string()));
                            return;
                        }
                    }
                }
            };

            let _ = event_tx.send(Event::TorrentActive(hash.clone()));
            match torrserver.play(&hash, &item.title, None).await {
                Ok(mut child) => {
                    let stream_url = format!("http://127.0.0.1:8090/stream/{}", hash);
                    let _ = event_tx.send(Event::StreamComplete(stream_url));

                    if let Some(stderr) = child.stderr.take() {
                        use tokio::io::{AsyncBufReadExt, BufReader};
                        let mut reader = BufReader::new(stderr).lines();
                        while let Ok(Some(line)) = reader.next_line().await {
                            let l = line.trim();
                            if l.is_empty() { continue; }
                            let low = l.to_lowercase();
                            if low.contains("vo:")
                                || low.contains("ao:")
                                || low.contains("av:")
                                || low.contains("video:")
                                || low.contains("audio:")
                                || low.contains("cache")
                                || low.contains("hwdec")
                                || low.contains("vaapi")
                                || low.contains("vdpau")
                                || low.contains("nvdec")
                                || low.contains("cuda")
                                || low.contains("drm")
                                || low.contains("duration:")
                                || low.contains("playing:")
                                || low.contains("exiting")
                                || low.contains("resume")
                                || low.contains("track")
                                || low.contains("tag:")
                                || low.contains("kbps")
                                || low.contains("fps")
                                || low.contains("h264")
                                || low.contains("h265")
                                || low.contains("hevc")
                                || low.contains("av1")
                                || low.contains("vp9")
                                || low.contains("aac")
                                || low.contains("ac3")
                                || low.contains("opus")
                                || low.contains("flac")
                                || low.contains("passthrough")
                                || low.contains("format")
                                || low.contains("video output")
                                || low.contains("audio output")
                                || low.contains("pix_fmt")
                                || low.contains("backend")
                                || low.contains("1056")
                                || low.contains("1920")
                                || low.contains("1280")
                            {
                                log(&format!("MPV: {}", l));
                            }
                        }
                    }
                }
                Err(e) => {
                    log(&format!("Player error: {}", e));
                    let _ = event_tx.send(Event::StreamError(e.to_string()));
                }
            }
        });
    }

    /// `home_url` comes from the registry entry of the source asking for
    /// the browser: the `Source` instance can't supply it, because
    /// building that instance is exactly what needs the browser.
    async fn get_browser(&mut self, home_url: &str) -> Result<Arc<Mutex<Browser>>> {
        if let Some(ref b) = self.browser {
            return Ok(Arc::clone(b));
        }

        let browser_choice = self.args.browser.as_deref().or(self.config.browser.as_deref());
        let browser_priority = detect::parse_priority(&self.config.browser_priority);
        let (kind, path) = detect::detect_browser_with_priority(browser_choice, &browser_priority)?;
        self.ui.add_log(&format!("Launching {} ({})...", kind, self.browser_visibility));

        let browser = Browser::launch(&path, self.browser_visibility, home_url, self.config.close_browser_on_exit).await?;
        let browser = Arc::new(Mutex::new(browser));
        self.browser = Some(Arc::clone(&browser));

        Ok(browser)
    }

    /// Build (once) and reuse the live `Source` for `id` -- the one
    /// path from `app.rs` onto a concrete source type, via the registry
    /// and `source::build_source` (B2). Browser-backed sources get their
    /// session launched here with the registry's `home_url`.
    async fn get_source(&mut self, id: &str) -> Result<Arc<dyn Source>> {
        if let Some(existing) = self.sources.get(id) {
            return Ok(Arc::clone(existing));
        }
        let info = source::get_source(id)
            .ok_or_else(|| anyhow::anyhow!("unknown source '{}'", id))?;
        let browser = if info.requires_browser {
            Some(self.get_browser(info.home_url).await?)
        } else {
            None
        };
        let built = source::build_source(info.id, SourceEnv { browser })?;
        self.sources.insert(info.id, Arc::clone(&built));
        Ok(built)
    }

    /// The instance a result row is played through: plain-HTTP sources
    /// are built on the spot, browser-backed ones must already be in the
    /// cache -- streaming never launches a browser itself (B0.1), it
    /// reports "No browser session - search first" instead.
    async fn source_for_row(&mut self, id: &'static str) -> Result<Arc<dyn Source>> {
        if source_needs_browser(id) {
            self.sources
                .get(id)
                .map(Arc::clone)
                .ok_or_else(|| anyhow::anyhow!("No browser session - search first"))
        } else {
            self.get_source(id).await
        }
    }

    async fn load_more(&mut self, query: String) {
        self.ui.state = AppState::Searching;
        // Same generation as the results already on screen: the next page
        // appends to them instead of being treated as a superseded search.
        // Which sources get asked, and from which cursor, is decided per
        // source inside `dispatch_search`.
        let generation = self.search_generation;
        self.dispatch_search(query, generation).await;
    }
}
