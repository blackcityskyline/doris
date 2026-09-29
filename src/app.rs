use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

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

pub fn source_id_for(item: &crate::sources::models::TorrentItem) -> &'static str {
    source::get_source(&item.source)
        .map(|s| s.id)
        .unwrap_or("rutracker")
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
    /// A tab row was switched and needs a re-search instead of playing.
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
        let torrserver_url = if args.torrserver == "http://127.0.0.1:8090" {
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

        let event_handler = EventHandler::new(std::time::Duration::from_millis(100));
        let torrserver = TorrServer::new(&torrserver_url);
        crate::torrent::Manager::spawn(
            torrserver.clone(),
            config.update_ms,
            event_handler.sender(),
        );

        Ok(Self {
            ui: UiApp::new(torrserver_url.clone(), config.theme_name.as_deref())
                .with_group_tabs(&config),
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
        let mut terminal = tui::init()?;
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
            terminal.draw(|frame| {
                self.terminal_size = (frame.area().width, frame.area().height);
                self.ui.render(frame, &self.config);
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
                            // Torrent is a single status readout and
                            // Trackers scrolls its cursor rather than a
                            // list -- focusing them on hover is still
                            // correct, there's just no list to move
                            // within.
                            ZoneId::Torrent | ZoneId::Trackers => {}
                        }
                    }
                }
            }
            MouseEventKind::ScrollDown => {
                if self.ui.detail_log_mode {
                    self.ui.detail_log_scroll =
                        (self.ui.detail_log_scroll + 3).min(self.ui.detail_logs.len());
                } else if self.ui.modal == Modal::None {
                    if let Some(id) = self.ui.zone_at(mouse.row, mouse.column) {
                        self.ui.zones.focused = id;
                        match id {
                            ZoneId::Log => self.ui.scroll_logs_down(),
                            ZoneId::Results => self.handle_nav_down().await,
                            // Torrent is a single status readout and
                            // Trackers scrolls its cursor rather than a
                            // list -- focusing them on hover is still
                            // correct.
                            ZoneId::Torrent | ZoneId::Trackers => {}
                        }
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if self.ui.detail_log_mode {
                    self.ui.detail_log_scroll = self.ui.detail_logs.len();
                } else if self.ui.modal == Modal::None && self.ui.search_box_at(mouse.row) {
                    // The input box is the only thing left to hit on
                    // those rows: the header hints ("s: search | S:
                    // settings | ...") went with П.3, and clicking the
                    // field does what `s`/`i` do.
                    self.ui.enter_input_mode();
                } else if self.ui.modal == Modal::None {
                    match self.ui.click_at(mouse.row, mouse.column, &mut self.config) {
                        Some(UiAction::TrackersChanged) => self.persist_config(),
                        Some(UiAction::TogglePause) => self.toggle_pause_active_torrent().await,
                        Some(UiAction::Remove) => self.remove_active_torrent().await,
                        Some(UiAction::Download) => self.download_selected_to_disk().await,
                        Some(UiAction::Info) => self.show_selected_info(),
                        Some(UiAction::Play) => {
                            // The `play` frame button is Enter on the
                            // Results panel: same decision tree as the
                            // key, minus `input_mode` (a click can't have
                            // been typed into the search box).
                            match enter_action(
                                false,
                                !self.ui.search_input.is_empty(),
                                self.ui.source_changed,
                                self.ui.group_changed,
                                self.ui.submit_selection().is_some(),
                            ) {
                                EnterAction::RestartSearch => {
                                    self.ui.source_changed = false;
                                    self.ui.group_changed = false;
                                    if let Some(ref q) = self.ui.search_query.clone() {
                                        let query = q.clone();
                                        self.start_search(query).await;
                                    }
                                }
                                EnterAction::Play => self.spawn_stream().await,
                                EnterAction::SubmitQuery | EnterAction::DoNothing => {}
                            }
                        }
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
        item: &crate::sources::models::TorrentItem,
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
            self.ui
                .add_log("Downloading is disabled in Options -> download -> Enable downloading.");
            return;
        }

        self.ui.add_log(&format!("Downloading '{}'...", item.title));

        // A row with neither link nor file (1337x, B8 wave 3) reads its
        // magnet off its own page -- the one request this download
        // already costs anyway, and never one per search.
        let mut item = item;
        if item.magnet.is_none() && item.download_url.is_empty() {
            let resolved = match self.get_source(source_id_for(&item)).await {
                Ok(source) => fill_missing_magnet(&mut item, source.as_ref()).await,
                Err(e) => Err(e),
            };
            if let Err(e) = resolved {
                self.ui
                    .add_log(&format!("Could not read the magnet link: {}", e));
                return;
            }
            if item.magnet.is_none() {
                self.ui
                    .add_log("This row carries no magnet and no .torrent link.");
                return;
            }
        }

        // A row with no `.torrent` to fetch (YTS and friends, B8 wave 1)
        // pays its magnet as the file itself: nothing to download, so
        // the Source is never asked -- which also means no browser can
        // be launched for what is a local write.
        if let Some((name, payload)) = magnet_only_download(&item) {
            let dir = self.resolve_download_dir();
            let path = std::path::Path::new(&dir).join(&name);
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match std::fs::write(&path, payload.as_bytes()) {
                Ok(_) => self
                    .ui
                    .add_log(&format!("Saved magnet link to {}", path.display())),
                Err(e) => self.ui.add_log(&format!("Failed to save file: {}", e)),
            }
            return;
        }

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
                let path = std::path::Path::new(&dir)
                    .join(format!("{}.torrent", safe_filename(&item.title)));
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
    /// action from the Results panel's bottom action row.
    fn show_selected_info(&mut self) {
        let Some(item) = self.ui.results.get(self.ui.selected) else {
            self.ui.add_log("No result selected.");
            return;
        };
        let source = if item.source.is_empty() {
            "rutracker"
        } else {
            item.source.as_str()
        };
        self.ui.add_log(&format!(
            "INFO: {}  |  size={}  seeds={}  date={}  source={}  url={}",
            item.title, item.size, item.seeds, item.date, source, item.page_url,
        ));
    }

    /// Open the detail modal for the selected row (П.7, Shift+Enter) and
    /// ask its source for the file list.
    ///
    /// The row's own facts go on screen at once -- the modal is never an
    /// empty box waiting on the network. The file list is one request
    /// away and arrives as [`Event::DetailLoaded`], tagged with the page
    /// it was asked about so an answer for a row the user has already
    /// left is dropped rather than shown in the next row's modal.
    async fn open_detail_modal(&mut self) {
        let Some(item) = self.ui.results.get(self.ui.selected).cloned() else {
            self.ui.add_log("No result selected.");
            return;
        };
        self.ui.modal = Modal::TorrentDetail(Box::new(TorrentDetailState::new(item.clone())));

        let source = match self.get_source(source_id_for(&item)).await {
            Ok(source) => source,
            Err(e) => {
                self.ui.add_log(&e.to_string());
                return;
            }
        };
        let page_url = item.page_url.clone();
        let tx = self.event_handler.sender();
        tokio::spawn(async move {
            let outcome = source.details(&page_url).await;
            let (files, error) = match outcome {
                Ok(files) => (files, None),
                Err(e) => (Vec::new(), Some(e.to_string())),
            };
            let _ = tx.send(Event::DetailLoaded {
                page_url,
                files,
                error,
            });
        });
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

    /// Write the config back now rather than at exit.
    ///
    /// Every in-app edit of `self.config` goes through here: the
    /// Settings modal always did this, and the Trackers checkboxes only
    /// flipped a field in memory -- with "Save config on exit" defaulting
    /// to off, a checked source simply evaporated when the app closed.
    /// The path is `--config`, the one `config::load` read from, so a
    /// run pointed at another file cannot be overwritten by (or overwrite
    /// the contents of) the default one. A failure is worth saying out
    /// loud in the Log zone: silently losing a setting is what this
    /// function exists to stop.
    fn persist_config(&mut self) {
        if let Err(e) = crate::config::save(&self.config, self.args.config.as_deref()) {
            self.ui.add_log(&format!("Failed to save config: {e}"));
        }
    }

    /// A line the user must not miss: the Log zone, the full log `L`
    /// opens, and the file on disk. TorrServer's failures used to reach
    /// only the last of the three, so the panel said nothing while the
    /// app already knew the answer.
    fn report(&mut self, module: &str, msg: &str) {
        self.ui.add_log(msg);
        self.ui.add_detail(msg);
        crate::log::log(module, msg);
    }

    /// Turning TorrServer on says out loud whether it is actually up.
    ///
    /// The switch used to invert a bool and stop there: with the systemd
    /// unit stopped nothing happened until a stream was started, and the
    /// reason lived in the file log. So the moment it is enabled the
    /// server is pinged; if it does not answer, the `systemctl` that
    /// would start it gets one unprivileged chance -- never with a
    /// password, which cannot be answered from here and would only hang
    /// or fail silently -- and whatever it said is reported verbatim.
    async fn check_torrserver_on_enable(&mut self) {
        let url = self.torrserver.base_url().to_string();
        let started = if self.torrserver.is_reachable().await {
            None
        } else {
            Some(
                match std::process::Command::new("systemctl")
                    .args(["start", "torrserver.service"])
                    .output()
                {
                    Ok(out) if out.status.success() => Ok(String::new()),
                    Ok(out) => {
                        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
                        // An empty stderr would read as "no reason given";
                        // the exit status still says how it failed.
                        Err(if err.is_empty() {
                            out.status.to_string()
                        } else {
                            err
                        })
                    }
                    Err(e) => Err(e.to_string()),
                },
            )
        };
        let msg = torrserver_enable_message(&url, started);
        self.report("torrserver", &msg);
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
            // The Trackers panel is a list like the others, so the same
            // keys move its cursor -- the one piece of state it has.
            ZoneId::Trackers => self.ui.navigate_trackers(1),
            _ => {}
        }
    }

    /// Counterpart to [`handle_nav_down`](Self::handle_nav_down) for the Up
    /// arrow / vim-style 'k'.
    fn handle_nav_up(&mut self) {
        match self.ui.zones.focused {
            ZoneId::Results => {
                self.ui.navigate_up();
            }
            ZoneId::Log => self.ui.scroll_logs_up(),
            ZoneId::Trackers => self.ui.navigate_trackers(-1),
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
                    self.ui.detail_log_scroll =
                        (self.ui.detail_log_scroll + 1).min(self.ui.detail_logs.len());
                }
                KeyCode::Down => {
                    self.ui.detail_log_scroll =
                        (self.ui.detail_log_scroll + 1).min(self.ui.detail_logs.len());
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
                    self.ui.detail_log_scroll =
                        (self.ui.detail_log_scroll + 20).min(self.ui.detail_logs.len());
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

        if let Modal::Help(_) = self.ui.modal {
            // The help page owns the keyboard while it is up, exactly
            // like btop's `helpMenu` -- every key lands here.
            self.ui.help_key(key);
            return Ok(());
        }

        if let Modal::TorrentDetail(_) = self.ui.modal {
            // The detail modal owns the keyboard too: j/k move the file
            // cursor, Enter plays, `d` downloads, Esc/q close. The two
            // actions that belong to the orchestrator come back.
            if let Some(action) = self.ui.detail_key(key, self.config.vim_keys) {
                match action {
                    DetailAction::Play => {
                        // Playing leaves the modal: the user is going
                        // to watch the torrent, not read about it.
                        self.ui.modal = Modal::None;
                        self.spawn_stream().await;
                    }
                    DetailAction::Download => self.download_selected_to_disk().await,
                }
            }
            return Ok(());
        }

        if let Modal::Settings(_) = self.ui.modal {
            if let Some(action) = self.ui.settings_key(key) {
                // 17 toggle-actions all do the same: invert a bool field,
                // reopen the settings modal. Each is one macro call; `||`
                // short-circuits so only the first match flips a field.
                macro_rules! toggle {
                    ($action:ident, $field:ident) => {
                        if matches!(action, SettingsAction::$action) {
                            self.config.$field = !self.config.$field;
                            true
                        } else {
                            false
                        }
                    };
                }
                // Captured before the chain flips it: turning TorrServer
                // *on* is the one toggle that owes the user an answer.
                let torrserver_was_on = self.config.enable_torrserver;
                let toggled = toggle!(ToggleCloseBrowserOnExit, close_browser_on_exit)
                    || toggle!(ToggleSaveCookies, save_cookies)
                    || toggle!(ToggleSaveCredentials, save_credentials)
                    || toggle!(ToggleEnableTorrserver, enable_torrserver)
                    || toggle!(ToggleThemeBackground, theme_background)
                    || toggle!(ToggleTruecolor, truecolor)
                    || toggle!(ToggleFalseTty, false_tty)
                    || toggle!(ToggleVimKeys, vim_keys)
                    || toggle!(ToggleMouse, disable_mouse)
                    || toggle!(ToggleDisablePresets, disable_presets)
                    || toggle!(ToggleShowBoxes, show_boxes)
                    || toggle!(ToggleRoundedCorners, rounded_corners)
                    || toggle!(ToggleTerminalSync, terminal_sync)
                    || toggle!(ToggleDownloadEnabled, download_enabled)
                    || toggle!(ToggleDownloadSequential, download_sequential)
                    || toggle!(ToggleCloseTorrentCoreOnExit, close_torrent_core_on_exit)
                    || toggle!(ToggleSaveOnExit, save_config_on_exit);
                if toggled {
                    self.ui.open_settings(
                        &self.config,
                        self.browser_visibility == BrowserVisibility::Hidden,
                    );
                    if self.config.enable_torrserver && !torrserver_was_on {
                        self.check_torrserver_on_enable().await;
                    }
                }
                match action {
                    SettingsAction::ToggleBrowserVisibility => {
                        // `App::browser_visibility` is the single runtime
                        // owner; the modal reads a session copy on open.
                        // Previously this toggle only updated the display
                        // label and had zero effect on the next launch
                        // (ROADMAP.md bug B6).
                        self.browser_visibility = match self.browser_visibility {
                            BrowserVisibility::Hidden => BrowserVisibility::Visible,
                            BrowserVisibility::Visible => BrowserVisibility::Hidden,
                        };
                        self.ui.open_settings(
                            &self.config,
                            self.browser_visibility == BrowserVisibility::Hidden,
                        );
                    }
                    SettingsAction::ToggleMode => {
                        self.ui.stream_mode = !self.ui.stream_mode;
                        self.ui.open_settings(
                            &self.config,
                            self.browser_visibility == BrowserVisibility::Hidden,
                        );
                    }
                    SettingsAction::CyclePrioritizeBrowser => {
                        const ORDER: &[&str] = &["helium", "brave", "chrome", "chromium"];
                        let current = self
                            .config
                            .browser_priority
                            .first()
                            .cloned()
                            .unwrap_or_default();
                        let next_first = match ORDER.iter().position(|&k| k == current) {
                            Some(i) => {
                                ORDER[cycle_index(i, ORDER.len(), self.ui.last_cycle_direction)]
                            }
                            None => ORDER[0],
                        };
                        // Move next_first to the front, keep the rest in
                        // their existing relative order.
                        let mut rest: Vec<String> = self
                            .config
                            .browser_priority
                            .iter()
                            .filter(|k| k.as_str() != next_first)
                            .cloned()
                            .collect();
                        let mut new_priority = vec![next_first.to_string()];
                        new_priority.append(&mut rest);
                        self.config.browser_priority = new_priority;
                        self.ui.open_settings(
                            &self.config,
                            self.browser_visibility == BrowserVisibility::Hidden,
                        );
                    }
                    SettingsAction::EditCredentials => {
                        self.ui.open_login_modal();
                    }
                    SettingsAction::CheckTorrserverStatus => {
                        let reachable = self.torrserver.is_reachable().await;
                        let url = self.torrserver.base_url().to_string();
                        let msg = if reachable {
                            format!("TorrServer: reachable at {url}")
                        } else {
                            format!("TorrServer: not answering at {url}")
                        };
                        self.report("torrserver", &msg);
                        self.ui.open_settings(
                            &self.config,
                            self.browser_visibility == BrowserVisibility::Hidden,
                        );
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
                        if let Some(pos) = themes.iter().position(|t| t.name == self.ui.theme.name)
                        {
                            let next = cycle_index(pos, themes.len(), self.ui.last_cycle_direction);
                            self.ui.theme = themes[next].clone();
                        } else if !themes.is_empty() {
                            self.ui.theme = themes[0].clone();
                        }
                        self.config.theme_name = Some(self.ui.theme.name.clone());
                        self.ui.open_settings(
                            &self.config,
                            self.browser_visibility == BrowserVisibility::Hidden,
                        );
                    }
                    SettingsAction::CyclePreset => {
                        if !self.config.disable_presets && !self.config.presets.is_empty() {
                            self.config.preset_index = cycle_index(
                                self.config.preset_index,
                                self.config.presets.len(),
                                self.ui.last_cycle_direction,
                            );
                            let spec = self.config.presets[self.config.preset_index].clone();
                            self.ui.zones.apply_preset(&spec);
                        }
                        self.ui.open_settings(
                            &self.config,
                            self.browser_visibility == BrowserVisibility::Hidden,
                        );
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
                            Some(i) => {
                                STEPS[cycle_index(i, STEPS.len(), self.ui.last_cycle_direction)]
                            }
                            None => STEPS[0],
                        };
                        self.config.update_ms = next;
                        self.ui.open_settings(
                            &self.config,
                            self.browser_visibility == BrowserVisibility::Hidden,
                        );
                    }
                    SettingsAction::CycleGraphSymbol => {
                        const SYMBOLS: &[&str] = &["braille", "block", "dot"];
                        let next = match SYMBOLS.iter().position(|&s| s == self.config.graph_symbol)
                        {
                            Some(i) => {
                                SYMBOLS[cycle_index(i, SYMBOLS.len(), self.ui.last_cycle_direction)]
                            }
                            None => SYMBOLS[0],
                        };
                        self.config.graph_symbol = next.to_string();
                        self.ui.open_settings(
                            &self.config,
                            self.browser_visibility == BrowserVisibility::Hidden,
                        );
                    }
                    SettingsAction::CycleDownloadDirMode => {
                        const MODES: &[&str] = &["default", "custom1", "custom2", "custom3"];
                        let next = match MODES
                            .iter()
                            .position(|&m| m == self.config.download_dir_mode)
                        {
                            Some(i) => {
                                MODES[cycle_index(i, MODES.len(), self.ui.last_cycle_direction)]
                            }
                            None => MODES[0],
                        };
                        self.config.download_dir_mode = next.to_string();
                        self.ui.open_settings(
                            &self.config,
                            self.browser_visibility == BrowserVisibility::Hidden,
                        );
                    }
                    SettingsAction::CycleDownloadSpeedLimit => {
                        const STEPS: &[u32] = &[0, 128, 256, 512, 1024, 2048, 5120, 10240];
                        let next = match STEPS
                            .iter()
                            .position(|&v| v == self.config.download_speed_limit_kbps)
                        {
                            Some(i) => {
                                STEPS[cycle_index(i, STEPS.len(), self.ui.last_cycle_direction)]
                            }
                            None => STEPS[0],
                        };
                        self.config.download_speed_limit_kbps = next;
                        self.ui.open_settings(
                            &self.config,
                            self.browser_visibility == BrowserVisibility::Hidden,
                        );
                    }
                    SettingsAction::CycleUploadSpeedLimit => {
                        const STEPS: &[u32] = &[0, 64, 128, 256, 512, 1024, 2048, 5120];
                        let next = match STEPS
                            .iter()
                            .position(|&v| v == self.config.upload_speed_limit_kbps)
                        {
                            Some(i) => {
                                STEPS[cycle_index(i, STEPS.len(), self.ui.last_cycle_direction)]
                            }
                            None => STEPS[0],
                        };
                        self.config.upload_speed_limit_kbps = next;
                        self.ui.open_settings(
                            &self.config,
                            self.browser_visibility == BrowserVisibility::Hidden,
                        );
                    }
                    SettingsAction::Close => {}
                    // The 17 bool toggles are handled above by the
                    // `toggle!` chain; nothing else to do here.
                    _ => {}
                }
                // Persist every settings change immediately rather than
                // only on a clean exit ("Save config on exit" governs a
                // final flush, not whether changes are remembered at all
                // -- a crash between now and exit shouldn't lose them,
                // and it previously did).
                self.persist_config();
            }
            return Ok(());
        }

        if self.ui.modal != Modal::None {
            if let Some((resource, username, password)) = self.ui.login_modal_key(key) {
                self.do_login(resource, &username, &password).await;
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

        // The search box has no zone of its own, so "the Trackers panel is
        // focused" and "a query is being typed" are not mutually
        // exclusive. Every arm below therefore needs `!input_mode` to stay
        // out of the user's way -- and that is exactly the guard that kept
        // being left off (`j`/`k` on the panel, `j`/`k` and Up/Down
        // everywhere). Typing owns the key here instead, so one check
        // replaces a guard every future arm would have to remember.
        if self.ui.input_mode {
            return self.handle_input_key(key).await;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Char('c')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.ui.quit();
            }
            KeyCode::Char('m') => {
                self.ui.show_menu = !self.ui.show_menu;
            }
            KeyCode::Char('f') => {
                self.ui.zones.filter_mode = true;
            }
            KeyCode::Char('F') => {
                if self.ui.zones.fullscreen.is_some() {
                    self.ui.zones.set_fullscreen(None);
                } else {
                    self.ui.zones.set_fullscreen(Some(self.ui.zones.focused));
                }
            }
            KeyCode::Char('1') => {
                self.ui.zones.toggle(ZoneId::Results);
            }
            KeyCode::Char('2') => {
                self.ui.zones.toggle(ZoneId::Torrent);
            }
            KeyCode::Char('3') => {
                self.ui.zones.toggle(ZoneId::Trackers);
            }
            KeyCode::Char('4') => {
                self.ui.zones.toggle(ZoneId::Log);
            }
            // Shift+P: cycle the layout preset (П.8). Lowercase `p` is
            // pause/resume on the Torrent panel, so the capital is the
            // free one -- the same reasoning as `F` for filter.
            KeyCode::Char('P') => {
                self.ui.zones.cycle_preset();
            }
            KeyCode::Char('p') if self.ui.zones.focused == ZoneId::Torrent => {
                self.toggle_pause_active_torrent().await;
            }
            KeyCode::Char('d') if self.ui.zones.focused == ZoneId::Torrent => {
                self.remove_active_torrent().await;
            }
            KeyCode::Char('d') if self.ui.zones.focused == ZoneId::Results => {
                self.download_selected_to_disk().await;
            }
            KeyCode::Char('v') if self.ui.zones.focused == ZoneId::Results => {
                self.show_selected_info();
            }
            // The category row's keys, next to `g`/`G` and gated the same
            // way: `g` steps forward, `G` (shift) back. Both only ever
            // move the row and re-derive the view -- the search still
            // waits for Enter, exactly as it does after a category switch.
            KeyCode::Char('g') if self.ui.zones.focused == ZoneId::Results => {
                self.ui.cycle_group(true);
            }
            KeyCode::Char('G') if self.ui.zones.focused == ZoneId::Results => {
                self.ui.cycle_group(false);
            }
            // The Trackers panel's own keys: `j`/`k` move the cursor
            // (wrapping, like every other list in the app), Enter switches
            // the row under it. Both are gated on the panel being focused
            // for the same reason `g`/`G` are gated on Results -- a key
            // that moved a cursor somewhere the user is not looking would
            // be a surprise. Typing is already off the table by the time
            // this match runs (see the `input_mode` hand-over above), so
            // none of the three needs its own half of that guard.
            KeyCode::Char('j')
                if self.config.vim_keys && self.ui.zones.focused == ZoneId::Trackers =>
            {
                self.ui.navigate_trackers(1);
            }
            KeyCode::Char('k')
                if self.config.vim_keys && self.ui.zones.focused == ZoneId::Trackers =>
            {
                self.ui.navigate_trackers(-1);
            }
            KeyCode::Enter if self.ui.zones.focused == ZoneId::Trackers => {
                self.ui.toggle_source(&mut self.config);
                self.persist_config();
            }
            // Shift+Enter: the selected row's details (П.7). Separate
            // from the plain Enter below on purpose -- that one plays
            // or re-searches, and a modifier is the only thing that can
            // tell the two apart.
            //
            // `D` is the fallback: most terminals send Shift+Enter as a
            // plain Enter with no modifier (crossterm only reports the
            // shift when the terminal opts into the kitty keyboard
            // protocol, which 0.28 has no API to request), so on those
            // Shift+Enter falls through to play and the modal never
            // opens. `D` is the same action on a key every terminal
            // sends distinctly.
            KeyCode::Char('D') => {
                self.open_detail_modal().await;
            }
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.open_detail_modal().await;
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
            KeyCode::Tab => {
                self.ui.zones.focus_next();
            }
            KeyCode::BackTab => {
                self.ui.zones.focus_prev();
            }
            KeyCode::PageUp => {
                if self.ui.zones.focused == ZoneId::Log {
                    self.ui.scroll_logs_page_up()
                }
            }
            KeyCode::PageDown => {
                if self.ui.zones.focused == ZoneId::Log {
                    self.ui.scroll_logs_page_down()
                }
            }
            KeyCode::Char('s') | KeyCode::Char('i') => {
                self.ui.enter_input_mode();
            }
            KeyCode::Char('b') => {
                // Browse (B9): an empty query asks the browse-capable
                // sources for their freshest rows. Browse is cross-source
                // by nature, so it takes the "all" category with it -- a
                // mixed list of rows claiming no group must stay visible.
                self.ui.source_changed = true;
                self.ui.set_group(None);
                self.start_search(String::new()).await;
            }
            KeyCode::Char('L') => {
                self.ui.toggle_detail_log();
            }
            KeyCode::Char('S') => {
                self.ui.open_settings(
                    &self.config,
                    self.browser_visibility == BrowserVisibility::Hidden,
                );
            }
            // The help page (btop binds `F1`/`?`/`h`); `h` stays free
            // for future vim navigation, so the three triggers are `?`,
            // `/` and F1.
            KeyCode::Char('?') | KeyCode::Char('/') | KeyCode::F(1) => {
                self.ui.open_help_modal();
            }
            // detail log mode and input mode both returned above, so the
            // only Esc left to answer for is the main view's.
            KeyCode::Esc => {
                self.ui.show_menu = !self.ui.show_menu;
            }
            KeyCode::Enter => self.handle_enter().await,
            _ => {}
        }
        Ok(())
    }

    /// What Enter means on the main view (btop's `enter`/`play`): submit
    /// the typed query, re-search a selection that just changed, or play
    /// the highlighted row. Also the key input mode passes it to --
    /// [`handle_input_key`] calls it with `input_mode` still on, where
    /// `enter_action` can only answer SubmitQuery or DoNothing.
    async fn handle_enter(&mut self) {
        let action = enter_action(
            self.ui.input_mode,
            !self.ui.search_input.is_empty(),
            self.ui.source_changed,
            self.ui.group_changed,
            self.ui.submit_selection().is_some(),
        );
        match action {
            EnterAction::SubmitQuery => {
                if let Some(query) = self.ui.submit_search() {
                    self.start_search(query).await;
                }
            }
            EnterAction::RestartSearch => {
                // A tab row was just switched (`]`/click or
                // `g`/`G`/click): re-search with the new selection
                // instead of playing a torrent. Both flags clear here
                // as well as in `start_search`, for the only path where
                // no search follows -- nothing has ever been searched,
                // so there is no query to restart and no row to play
                // either.
                self.ui.source_changed = false;
                self.ui.group_changed = false;
                if let Some(ref q) = self.ui.search_query.clone() {
                    let query = q.clone();
                    self.start_search(query).await;
                }
            }
            EnterAction::Play => self.spawn_stream().await,
            EnterAction::DoNothing => {}
        }
    }

    /// Every key typed into the search box, and nothing else: the box
    /// has no zone of its own, so `input_mode` can overlap any focused
    /// zone, and this is the single place that overlap is decided --
    /// `handle_key` hands the key over before its own match runs.
    ///
    /// Esc and Enter go through the same paths they have on any other
    /// view (leave input mode / the Enter decision tree); quitting stays
    /// here so Ctrl+C works while a query is being typed.
    async fn handle_input_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('c')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.ui.quit();
            }
            KeyCode::Esc => self.ui.exit_input_mode(),
            KeyCode::Enter => self.handle_enter().await,
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.ui.clear_input();
            }
            KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.ui.delete_word();
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.ui.type_char(c);
            }
            KeyCode::Backspace => self.ui.backspace(),
            _ => {}
        }
        Ok(())
    }

    async fn handle_menu_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('c')
                if key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
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
                        self.ui.open_settings(
                            &self.config,
                            self.browser_visibility == BrowserVisibility::Hidden,
                        );
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

    /// Log in with the credentials the modal collected, for the resource
    /// its tab had selected. The login itself still targets rutracker --
    /// it is the only source with a session to establish (rutor's
    /// `ensure_logged_in` is a no-op) -- so the target id is fixed
    /// rather than read off the tab.
    async fn do_login(&mut self, resource: &str, username: &str, password: &str) {
        self.ui.add_log(&format!("Logging in as '{}'...", username));

        if self.config.save_credentials {
            let _ = crate::credentials::save_credential(resource, username, password);
        }

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
        // Whatever a tab switch owed this point is now paid: the search
        // below runs against the selection as it stands, so Enter must
        // go back to meaning "play" instead of restarting (B6's
        // `group_changed` and the older `source_changed` clear here for
        // the same reason).
        self.ui.source_changed = false;
        self.ui.group_changed = false;
        // Rows now arrive one source at a time (B3), so there is no
        // single moment where the old list gets replaced by the new one:
        // the table empties here, and each source appends into it. The
        // per-source records belong to the old query and go with it.
        self.ui.results.clear();
        self.ui.selected = 0;
        self.ui.update_filter();
        self.ui.source_status.clear();
        self.source_has_more.clear();
        self.source_offsets.clear();
        let category = match self.ui.active_group {
            Some(group) => format!(" [{}]", group.label()),
            None => String::new(),
        };
        self.ui.add_log(&format!(
            "Searching '{}'{} across {}...",
            query,
            category,
            sources_summary(&self.config)
        ));
        // New generation: anything still in flight for a previous query is
        // now stale and gets dropped when it lands (B0.2).
        self.search_generation += 1;
        let generation = self.search_generation;
        self.dispatch_search(query, generation).await;
    }

    /// Kick off the search for `query`: one task per source
    /// `orchestrator::selected_sources` picks (the Trackers panel's
    /// checkboxes, narrowed by the selected category) and `orchestrator::dispatch_plan` says is worth asking (a fresh
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
        // An empty query is browse mode (B9): only sources that can
        // answer one are asked, and the merged list is ordered
        // freshest-first rather than by seeds.
        let browsing = query.trim().is_empty();
        self.ui.browsing = browsing;
        let selected = orchestrator::selected_sources(
            &self.config.enabled_sources,
            self.ui.active_group,
            browsing,
        );
        if selected.is_empty() {
            // Two ways to get here, and they have different fixes: no
            // source is checked at all, or nothing the panel reaches
            // serves the selected category -- the orchestrator words the
            // second one by what would actually change it.
            let reason = match self.ui.active_group {
                Some(group) => {
                    orchestrator::nothing_to_ask_reason(&self.config.enabled_sources, group)
                }
                None => {
                    "No source is checked -- the Trackers panel (3) is where they are switched on."
                        .to_string()
                }
            };
            self.ui.add_log(&reason);
            self.ui.state = AppState::Idle;
            return;
        }

        let plan =
            orchestrator::dispatch_plan(&selected, &self.source_offsets, &self.source_has_more);
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
            // path. The category is part of the key (B6): the same words
            // at the same offset under a different category are
            // different pages, so an "all" hit must never answer a
            // "Movies" request.
            let key = CacheKey::new(
                info.id,
                &query,
                self.ui.active_group.map(source::Group::label),
                offset,
            );
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
                    self.ui
                        .add_log(&source_outcome_line(info.id, &Err(e.to_string())));
                    self.ui
                        .source_status
                        .insert(info.id.to_string(), SourceStatus::Error(e.to_string()));
                    continue;
                }
            };

            self.ui
                .source_status
                .insert(info.id.to_string(), SourceStatus::Pending);
            let mut req = SearchRequest::new(query.clone(), offset);
            // The selection rides along (B6): a source that can filter
            // server-side will, one that cannot returns what it has --
            // and the view keeps only the rows claiming this category,
            // so an unhonoured category reads as fewer rows rather than
            // as a category nobody actually applied.
            req.category = self.ui.active_group;
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
        let mut item = self.ui.results[self.ui.selected].clone();
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
        let torrserver_enabled = self.config.enable_torrserver;

        tokio::spawn(async move {
            let log = |msg: &str| {
                let _ = event_tx.send(Event::StreamLog(msg.to_string()));
            };

            if !torrserver_enabled {
                // The user switched TorrServer off in Options: say so
                // rather than reaching for a server they have decided
                // not to use (the app-side gate that replaces a guessed-at
                // `systemctl` flow).
                log("TorrServer is disabled in Options -> streaming -> Enable TorrServer.");
                let _ = event_tx.send(Event::StreamError(
                    "TorrServer is disabled in Options".into(),
                ));
                return;
            }

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
            // A row with neither link nor file (1337x, B8 wave 3) reads
            // its magnet off its own page, here, on the way to playing
            // it -- one request for the one row being played.
            if let Err(e) = fill_missing_magnet(&mut item, source.as_ref()).await {
                log(&format!("Reading the magnet link failed: {}", e));
            }
            if item.magnet.is_none() && item.download_url.is_empty() {
                let msg = "This row carries no magnet and no .torrent link.";
                log(msg);
                let _ = event_tx.send(Event::StreamError(msg.into()));
                return;
            }

            let mut linked: Option<String> = None;
            if let Some(magnet) = item.magnet.as_deref() {
                log(&format!("Adding by magnet link: {}", item.title));
                match torrserver.add_by_link(magnet, &item.title).await {
                    Ok(hash) => {
                        log(&format!("Added by link, hash: {}", hash));
                        linked = Some(hash);
                    }
                    Err(e) if item.download_url.is_empty() => {
                        // Nothing to fall back to: this row's only path
                        // was the link, and this source has no file.
                        let msg = format!(
                            "Magnet add failed ({}), and this row has no .torrent \
                             to fall back to",
                            e
                        );
                        log(&msg);
                        let _ = event_tx.send(Event::StreamError(msg));
                        return;
                    }
                    Err(e) => {
                        log(&format!(
                            "Magnet add failed ({}); fetching .torrent instead",
                            e
                        ));
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
                            if l.is_empty() {
                                continue;
                            }
                            if crate::player_log::should_log(l) {
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

        let browser_choice = self
            .args
            .browser
            .as_deref()
            .or(self.config.browser.as_deref());
        let browser_priority = detect::parse_priority(&self.config.browser_priority);
        let (kind, path) = detect::detect_browser_with_priority(browser_choice, &browser_priority)?;
        self.ui.add_log(&format!(
            "Launching {} ({})...",
            kind, self.browser_visibility
        ));

        let browser = Browser::launch(
            &path,
            self.browser_visibility,
            home_url,
            self.config.close_browser_on_exit,
        )
        .await?;
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
        let info =
            source::get_source(id).ok_or_else(|| anyhow::anyhow!("unknown source '{}'", id))?;
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

// --- key routing: who owns Enter while the search box is being typed ------
//
// The search input has no zone of its own, so `input_mode` and "the
// Trackers panel is focused" are not mutually exclusive -- a query typed
// after clicking the panel used to have its Enter (and its `j`/`k`)
// swallowed by the panel. These run against a real `App` because the bug
// lives in the *order* of the match arms, which no pure helper sees.

#[cfg(test)]
mod key_routing_tests {
    use super::*;
    use crate::ui::app::source_rows;
    use clap::Parser;
    use std::path::PathBuf;

    /// An `App` with the Trackers panel focused, no bridge listener, and
    /// no sources checked, so a key that submits a search starts no
    /// network task and the routing decision is all there is to observe.
    async fn app_focused_on_sources(config_path: Option<PathBuf>) -> App {
        let config = Config {
            bridge_port: 0,
            enabled_sources: Vec::new(),
            vim_keys: true,
            ..Config::default()
        };
        let mut argv = vec!["doris"];
        if let Some(path) = &config_path {
            argv.push("--config");
            argv.push(path.to_str().expect("utf-8 test path"));
        }
        let mut app = App::new(Args::parse_from(argv), config)
            .await
            .expect("an App for a key-routing test");
        app.ui.show_menu = false;
        app.ui.zones.focused = ZoneId::Trackers;
        app
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[tokio::test]
    async fn enter_submits_the_query_with_the_sources_panel_focused() {
        let mut app = app_focused_on_sources(None).await;
        app.ui.enter_input_mode();
        app.ui.type_char('m');
        let before = app.config.enabled_sources.clone();

        app.handle_key(press(KeyCode::Enter)).await.expect("Enter");

        assert_eq!(
            app.config.enabled_sources, before,
            "Enter was stolen by the Trackers panel while the query was typed"
        );
        assert!(!app.ui.input_mode, "the typed query was submitted");
        assert!(app.search_generation > 0, "and it started a search");
    }

    /// The zone digits follow the zone table, not a stale copy of it:
    /// `3` toggles Trackers and `4` toggles Log, the renumbered way.
    #[tokio::test]
    async fn zone_digit_keys_toggle_the_zone_they_name() {
        let mut app = app_focused_on_sources(None).await;
        assert!(app.ui.zones.is_visible(ZoneId::Trackers));
        assert!(app.ui.zones.is_visible(ZoneId::Log));

        app.handle_key(press(KeyCode::Char('3'))).await.expect("3");
        assert!(
            !app.ui.zones.is_visible(ZoneId::Trackers),
            "`3` must toggle Trackers"
        );

        app.handle_key(press(KeyCode::Char('4'))).await.expect("4");
        assert!(!app.ui.zones.is_visible(ZoneId::Log), "`4` must toggle Log");
    }

    /// A query is typed while the panel is focused: every one of its
    /// letters -- including the `j`/`k` that are nav keys everywhere
    /// else, and the arrow keys after them -- belongs to the box.
    #[tokio::test]
    async fn every_letter_of_a_query_reaches_the_box() {
        let mut app = app_focused_on_sources(None).await;
        app.ui.enter_input_mode();

        for c in "john wick".chars() {
            app.handle_key(press(KeyCode::Char(c)))
                .await
                .expect("a letter");
        }
        app.handle_key(press(KeyCode::Down)).await.expect("Down");
        app.handle_key(press(KeyCode::Up)).await.expect("Up");

        assert_eq!(
            app.ui.search_input, "john wick",
            "the query was eaten by the panel's (or Results') navigation keys"
        );
        assert_eq!(app.ui.sources_cursor, 0, "the panel cursor must not move");
    }

    /// The other half of the same guard: outside input mode the panel's
    /// keys must still do exactly what they did.
    #[tokio::test]
    async fn enter_still_switches_the_focused_source_row() {
        let mut app = app_focused_on_sources(None).await;
        app.ui.sources_cursor = 1; // the first real source, after `all`
        let id = source_rows()[1].id();

        app.handle_key(press(KeyCode::Enter)).await.expect("Enter");

        assert_eq!(app.config.enabled_sources, vec![id.to_string()]);
    }

    /// A checkbox the app forgets on quit is a checkbox that never
    /// happened: "Save config on exit" defaults to off, so the toggle
    /// has to reach the disk itself -- through the same `--config` path
    /// the config was loaded from.
    #[tokio::test]
    async fn switching_a_source_row_persists_the_config() {
        let dir = std::env::temp_dir().join(format!("doris-cfg-{}", std::process::id()));
        let path = dir.join("config.toml");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::remove_file(&path);

        let mut app = app_focused_on_sources(Some(path.clone())).await;
        app.ui.sources_cursor = 1;
        let id = source_rows()[1].id();

        app.handle_key(press(KeyCode::Enter)).await.expect("Enter");

        let text =
            std::fs::read_to_string(&path).expect("the checkbox change must reach the config file");
        assert!(
            text.contains(id),
            "`{id}` must be in the saved config:\n{text}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The path every "you should know this" line takes: the Log zone,
    /// the full log `L` opens, and the file. TorrServer's answer used to
    /// reach none of the first two, so a switch that could not work
    /// showed nothing at all.
    #[tokio::test]
    async fn report_reaches_both_logs() {
        let mut app = app_focused_on_sources(None).await;

        app.report("torrserver", "a line worth seeing");

        assert!(
            app.ui
                .logs
                .iter()
                .any(|l| l.contains("a line worth seeing")),
            "short log: {:?}",
            app.ui.logs
        );
        assert!(
            app.ui
                .detail_logs
                .iter()
                .any(|l| l.contains("a line worth seeing")),
            "full log: {:?}",
            app.ui.detail_logs
        );
    }

    /// A local server answering 200 on whatever port it is given, so the
    /// "TorrServer is up" branch can be tested without the real one --
    /// and without ever reaching the `systemctl` fallback.
    async fn fake_torrserver() -> u16 {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("addr").port();
        tokio::spawn(async move {
            while let Ok((mut sock, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    let _ = sock.read(&mut buf).await;
                    let _ = sock
                        .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n")
                        .await;
                });
            }
        });
        port
    }

    /// Put the Settings cursor on the row whose action is `action`, so a
    /// test can press Enter on it without walking the modal's layout.
    fn focus_setting(app: &mut App, action: SettingsAction) {
        let Modal::Settings(state) = &mut app.ui.modal else {
            panic!("settings modal is not open");
        };
        let found = state.categories.iter().enumerate().find_map(|(c, cat)| {
            cat.items
                .iter()
                .position(|i| i.action == action)
                .map(|r| (c, r))
        });
        match found {
            Some((c, r)) => {
                state.selected_category = c;
                state.selected = r;
            }
            None => panic!("no settings row for {action:?}"),
        }
    }

    /// Switching TorrServer on has to say whether it is actually there:
    /// the toggle used to invert a bool in silence, and with the unit
    /// stopped nothing happened until a stream was started -- with no
    /// reason attached there either.
    #[tokio::test]
    async fn enabling_torrserver_reports_where_it_answers() {
        let port = fake_torrserver().await;
        let dir = std::env::temp_dir().join(format!("doris-ts-{}", std::process::id()));
        let path = dir.join("config.toml");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::remove_file(&path);

        let config = Config {
            bridge_port: 0,
            enabled_sources: Vec::new(),
            vim_keys: true,
            enable_torrserver: false, // off, so Enter turns it on
            torrserver_url: format!("http://127.0.0.1:{port}"),
            ..Config::default()
        };
        let mut app = App::new(
            Args::parse_from(["doris", "--config", path.to_str().expect("utf-8")]),
            config,
        )
        .await
        .expect("app");
        app.ui.show_menu = false;
        app.ui.open_settings(&app.config, true);
        focus_setting(&mut app, SettingsAction::ToggleEnableTorrserver);

        app.handle_key(press(KeyCode::Enter)).await.expect("Enter");

        assert!(app.config.enable_torrserver, "the switch flipped on");
        let url = format!("http://127.0.0.1:{port}");
        let logs = app.ui.logs.iter().cloned().collect::<Vec<_>>().join("\n");
        assert!(
            logs.contains(&format!("reachable at {url}")),
            "the Log zone must name the answer:\n{logs}"
        );
        let full = app.ui.detail_logs.join("\n");
        assert!(
            full.contains(&format!("reachable at {url}")),
            "the full log must have it too:\n{full}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `g`/`G` are Results keys: they cycle the category when the panel
    /// has focus, and they are letters of the query when the box is
    /// being typed into -- the same hand-over every other key gets.
    #[tokio::test]
    async fn g_cycles_the_category_and_types_when_the_box_is_open() {
        let mut app = app_focused_on_sources(None).await;
        app.ui.zones.focused = ZoneId::Results;
        // The category row is derived from what is checked, and this app
        // starts with nothing checked.
        app.config.enabled_sources = vec!["rutracker".into(), "tpb".into(), "yts".into()];
        app.ui.set_group_tabs(&app.config);
        let all = app.ui.active_group;

        app.handle_key(press(KeyCode::Char('g'))).await.expect("g");
        assert_ne!(app.ui.active_group, all, "the category moved forward");
        assert!(app.ui.group_changed, "Enter owes the server-side search");

        app.handle_key(press(KeyCode::Char('G'))).await.expect("G");
        assert_eq!(app.ui.active_group, all, "and back again");

        app.ui.enter_input_mode();
        let parked = app.ui.active_group;
        app.handle_key(press(KeyCode::Char('g'))).await.expect("g");
        assert_eq!(app.ui.active_group, parked, "typing must not move it");
        assert_eq!(app.ui.search_input, "g", "the letter belongs to the query");
    }

    /// The category travels with the search: it is named in the line the
    /// user reads, and a category no checked source can serve says so --
    /// the honest "Anime 0/0" answer, rather than a table that looks
    /// like nothing was found.
    #[tokio::test]
    async fn a_search_names_its_category_and_says_when_nothing_can_answer_it() {
        let mut app = app_focused_on_sources(None).await;
        app.config.enabled_sources = vec!["yts".to_string()]; // Movies only
        app.ui.active_group = Some(source::Group::TV);

        app.start_search("matrix".to_string()).await;

        let logs = app.ui.logs.iter().cloned().collect::<Vec<_>>().join("\n");
        assert!(
            logs.contains("Searching 'matrix' [TV]"),
            "the category is named in the log:\n{logs}"
        );
        assert!(
            logs.contains("No checked source serves 'TV'"),
            "and the reason for the empty table names it:\n{logs}"
        );
        assert!(
            app.ui.state == AppState::Idle,
            "nothing was dispatched, so nothing is searching"
        );
    }

    /// The other empty-dispatch: nothing checked at all. The line has to
    /// name the key the Trackers panel is on, which moved to `3` when the
    /// placeholder zone went away -- a stale key points at a zone that
    /// does not exist.
    #[tokio::test]
    async fn nothing_checked_points_at_the_panel_that_switches_them() {
        let mut app = app_focused_on_sources(None).await;
        assert!(app.config.enabled_sources.is_empty(), "starts unchecked");

        app.start_search("matrix".to_string()).await;

        let logs = app.ui.logs.iter().cloned().collect::<Vec<_>>().join("\n");
        assert!(
            logs.contains("Trackers panel (3)"),
            "the message must name the panel's current key:\n{logs}"
        );
    }
}
