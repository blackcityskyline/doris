use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

pub mod cli_commands;
pub mod files;
mod input;
pub mod open;
mod search;
mod session;
mod settings;
mod stream;

#[cfg(test)]
mod tests;

use crate::bridge::handler::BridgeServer;
use crate::browser::cdp::{Browser, BrowserVisibility};
use crate::browser::detect;
use crate::cli::Args;
use crate::config::Config;
use crate::event::{Event, EventHandler};
use crate::results::{apply_source_done, finish_search, resolve_cookie_file, source_outcome_line};
use crate::sources::cache::{CacheKey, SearchCache};
use crate::sources::orchestrator::{self, SourceStatus};
use crate::sources::source::{self, AuthContext, LogFn, SearchRequest, Source, SourceEnv};
use crate::torrserver::api::TorrServer;
use crate::tui;
use crate::ui::layout::ZoneId;
use crate::ui::menu::MenuItem;
use crate::ui::modals::settings::SettingsAction;
use crate::ui::theme::Theme;
use crate::ui::view::{
    sources_summary, App as UiApp, AppState, DetailAction, Modal, TorrentDetailState,
    TorrentStatus, UiAction,
};

/// How often the event handler wakes up to poll the terminal for keys, mouse and resize.
const EVENT_POLL_MS: u64 = 100;

/// Resolve the effective download directory from `download_dir_mode` and the three custom slots
/// (Options -> download), falling back to the OS Downloads folder for "default" or an
/// unset/empty custom slot.
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

/// Cycle a string-valued setting through a fixed list, the way every "cycle this option" row
/// works: forward on Right, backward on Left, and back to the first entry when the current
/// value is not in the list.
pub fn cycle_str(current: &str, choices: &[&str], direction: i8) -> String {
    let next = match choices.iter().position(|&c| c == current) {
        Some(i) => choices[cycle_index(i, choices.len(), direction)],
        None => choices.first().copied().unwrap_or_default(),
    };
    next.to_string()
}

/// The same walk as [`cycle_str`], for the numeric settings that pick from
/// a fixed set of steps rather than from a list of names.
pub fn cycle_u64(current: u64, choices: &[u64], direction: i8) -> u64 {
    match choices.iter().position(|&c| c == current) {
        Some(i) => choices[cycle_index(i, choices.len(), direction)],
        None => choices.first().copied().unwrap_or_default(),
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

/// Whether fetching this source's `.torrent` files needs the browser-backed rutracker searcher.
pub fn source_needs_browser(source: &str) -> bool {
    source::requires_browser(source)
}

/// The Options rows that flip one bool and nothing else.
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

/// Flip the field `action` names, if it names one.
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
pub fn stop_download_on_exit<'a>(config: &Config, active_hash: Option<&'a str>) -> Option<&'a str> {
    if config.close_torrent_core_on_exit {
        active_hash
    } else {
        None
    }
}

/// Do the stop [`stop_download_on_exit`] asks for, and say what happened.
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

/// The file name a result title may safely have on disk: everything outside alphanumerics,
/// spaces and the usual punctuation becomes `_`.
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

/// What the download key owes a row that has no `.torrent` to fetch YTS publishes magnets, not
/// files `(file name, contents)` for a `<title>.magnet` file, or `None` when the row *does*
/// have a download URL and must go through its Source exactly as before.
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

/// The registered id to talk to for a result row.
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

/// Fill a row's magnet in from the row's own page, for rows that carry neither a magnet nor a
/// `.torrent` link (1337x, B8 wave 3).
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

/// What pressing Enter in the results view means.
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

/// Merge a detail modal's file list into the modal.
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
            if state.cursor >= state.files.len() {
                state.cursor = state.files.len().saturating_sub(1);
            }
        }
    }
}

/// The resource name Transmission's own login is stored under in the
/// encrypted credential store.
pub const TRANSMISSION_RESOURCE: &str = "transmission";

/// How long startup waits for the daemon's torrent list.
///
/// Short on purpose. A daemon that is not running answers at once; one
/// that is running answers in milliseconds; one that hangs must not hold
/// the launch, because the panel showing nothing is a far smaller problem
/// than the program not starting.
const ADOPT_DEADLINE_MS: u64 = 1500;

pub struct App {
    args: Args,
    config: Config,
    ui: UiApp,
    event_handler: EventHandler,
    torrserver: TorrServer,
    /// The downloading daemon. Separate from `torrserver` because it is a
    /// different job: TorrServer streams, this writes to a directory.
    transmission: crate::transmission::Transmission,
    browser: Option<Arc<Mutex<Browser>>>,
    /// Live sources keyed by id, built once via `source::build_source` and reused across
    /// start_search/load_more/do_login (all run in spawned tasks).
    sources: HashMap<&'static str, Arc<dyn Source>>,
    /// Each source's last "has another page" verdict: consulted
    /// by "Load more" and turned into `ui.all_loaded` by
    /// [`finish_search`].
    source_has_more: HashMap<String, bool>,
    /// How many rows each source has delivered for the current query: the cursor "Load more"
    /// resumes it at.
    source_offsets: HashMap<String, usize>,
    /// Recently fetched pages, consulted before any source is spawned
    /// a fresh hit answers immediately, browser and all.
    cache: Arc<SearchCache>,
    browser_visibility: BrowserVisibility,
    /// Kept alive (never read): dropping the last sender would close the
    /// channel the extension bridge searches through.
    #[allow(dead_code)]
    search_tx: mpsc::UnboundedSender<String>,
    search_rx: mpsc::UnboundedReceiver<String>,
    /// Bumped by every `start_search`; each dispatch carries the value it was started with, and
    /// results from an older generation are dropped.
    search_generation: u64,
    terminal_size: (u16, u16),
    /// Set by the SIGHUP/SIGTERM/SIGINT listener spawned in [`App::run`] so
    /// a terminal being closed (or a plain `kill`) goes through the normal
    /// exit path -- browser shutdown and temp-profile cleanup included --
    /// instead of dropping the process mid-flight and leaving the browser
    /// stack behind.
    exit_signal: Arc<AtomicBool>,
    /// The greeting being typed into the Options modal, `Some` while the
    /// editor is open.
    ///
    /// It lives here rather than in the modal because the modal cannot
    /// reach `config`: committing a value is the orchestrator's job, the
    /// same split every other setting has. `Esc` drops the buffer without
    /// committing, so a half-typed greeting costs nothing.
    pub editing_welcome_text: Option<String>,
    /// Set when the bridge port was already taken, and what that means for
    /// the browser add-on. Kept because the file log is not where the user
    /// is looking, and a click that goes to another doris is a search they
    /// started and cannot find.
    pub bridge_taken: Option<String>,
}

/// Run one CLI subcommand. See [`cli_commands`].
pub async fn run_command(args: &Args, command: &crate::cli::Command) -> Result<i32> {
    cli_commands::dispatch(args, command).await
}

impl App {
    pub async fn new(args: Args, config: Config) -> Result<Self> {
        Self::build(args, config, true).await
    }

    /// The same app with no keyboard and no bridge port.
    ///
    /// A CLI command is the same program asked one question and then
    /// answered, so it gets the same `App` rather than a parallel
    /// implementation: the sources, the cache, the login walk and the
    /// event rules are the ones the TUI uses. What it does not get is a
    /// keyboard (see [`crate::event::EventHandler::new`]) and the
    /// extension bridge, which would take a port that nothing is going to
    /// answer on.
    pub async fn headless(args: Args, config: Config) -> Result<Self> {
        Self::build(args, config, false).await
    }

    async fn build(args: Args, config: Config, interactive: bool) -> Result<Self> {
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
        // Said once the UI exists, when the bridge was already lost above.
        let mut bridge_taken: Option<String> = None;

        let bridge_port = if interactive { config.bridge_port } else { 0 };
        if bridge_port > 0 {
            // The server task owns its own handle (listener + router with a
            let mut bridge = BridgeServer::new(search_tx.clone(), bridge_port);
            if let Err(e) = bridge.start().await {
                // Said in the UI as well as the file, because the file is
                // where nobody looks. Another doris already holds this port,
                // so every click from the browser add-on goes *there* --
                // into a window this one knows nothing about, which is a
                // search the user started and cannot find. Continuing is
                // still right: the rest of the app works, and a doris that
                // refuses to start over a bridge would be a worse answer
                // than a doris that says which bridge it lost.
                let said = format!(
                    "Bridge port {bridge_port} is taken -- another doris has it, \
                     so searches from the browser extension go there, not here"
                );
                crate::log::log("bridge", &format!("bridge server failed to start: {e}"));
                bridge_taken = Some(said.clone());
                crate::log::log("bridge", &said);
            }
        }

        let event_handler =
            EventHandler::new(std::time::Duration::from_millis(EVENT_POLL_MS), interactive);
        let torrserver = TorrServer::new(&torrserver_url);
        // The daemon's credentials come out of the encrypted store rather
        // than out of the config: a password in a TOML file is a password
        // in a backup, in a dotfile repo and in `ps`. `doris login
        // transmission` is what puts one there.
        let transmission = crate::transmission::Transmission::with_auth(
            &config.transmission_url,
            crate::credentials::load_credential(TRANSMISSION_RESOURCE),
        );
        crate::torrent::Manager::spawn(
            torrserver.clone(),
            config.update_ms,
            event_handler.sender(),
        );
        if interactive {
            crate::transmission::poller::Poller::spawn(
                transmission.clone(),
                config.update_ms,
                event_handler.sender(),
            );
        }

        let mut ui = UiApp::new(torrserver_url.clone(), config.theme_name.as_deref())
            .with_group_tabs(&config);
        // The zones know nothing about Config, so the arrangement left by the
        // last session is applied here. It wins over the preset list: it
        // is what the user actually left the app in, sizes and all, and
        // a preset only names a shape.
        let restored = crate::config::load_layout(args.config.as_deref())
            .is_some_and(|saved| ui.zones.restore(&saved));
        if !restored && !config.disable_presets {
            if let Some(spec) = config.presets.get(config.preset_index) {
                let spec = spec.clone();
                ui.zones.apply_preset(&spec);
            }
        }

        // Take over whatever the daemon is already doing, so the Torrent
        // panel is not blank until the user presses play on something they
        // are already downloading. Only in the TUI: a one-shot command
        // should not wait on a daemon it may not even need, and the
        // `torrent` commands ask it themselves.
        if interactive {
            // A daemon that is not running is not an error at startup, it
            // is a daemon that is not running. The short deadline is for
            // the same reason: a hung one must not hold the whole launch.
            match tokio::time::timeout(
                std::time::Duration::from_millis(ADOPT_DEADLINE_MS),
                crate::transmission::adopt::adopt(&transmission),
            )
            .await
            {
                Ok(Ok(rows)) => {
                    ui.downloads = rows;
                    ui.download_cursor = 0;
                }
                Ok(Err(e)) => {
                    ui.add_log(&format!("Downloads not adopted: {e}"));
                }
                Err(_) => {
                    ui.add_log("Downloads not adopted: the daemon did not answer");
                }
            }
        }

        Ok(Self {
            ui,
            event_handler,
            torrserver,
            browser: None,
            transmission,
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
            editing_welcome_text: None,
            bridge_taken,
        })
    }

    /// Say the bridge was lost, once, in the Log zone and in the file.
    ///
    /// In the Log zone because the file is where nobody looks, and the
    /// symptom of a lost bridge is a search the add-on says it sent and this
    /// window never ran -- which reads as the add-on lying. Cleared after
    /// saying it, because a line about a port that was lost once belongs
    /// once and not on every redraw.
    pub(crate) fn report_bridge_taken(&mut self) {
        if let Some(taken) = self.bridge_taken.take() {
            self.ui.add_log(&taken);
            self.ui.add_detail(&taken);
        }
    }

    /// Every event that is not a key, a mouse, a tick or a resize, applied
    /// to the state.
    ///
    /// One function rather than a match arm inside the draw loop, for a
    /// reason that has nothing to do with tidiness: the CLI drives the same
    /// one. `run` waits on the event handler and calls this;
    /// `pump_until_idle` waits on the same handler and calls this, with no
    /// terminal in sight. A second copy of these rules would be a second
    /// answer to "what does a source's page do when it lands", and the
    /// two would drift on the first bug fix.
    pub async fn apply_event(&mut self, event: Event) -> Result<()> {
        match event {
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
                if applied {
                    let status = SourceStatus::from_event(count, error.as_deref(), timed_out);
                    self.ui.source_status.insert(source.clone(), status);
                    self.source_has_more.insert(source.clone(), has_more);
                    // Failures deliver no rows and no cursor,
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
                self.ui.last_stream_url = Some(url.clone());
                self.ui.last_stream_error = None;
                self.ui.add_log(&format!("Stream launched: {url}"));
            }
            Event::StreamError(err) => {
                self.ui.state = AppState::Idle;
                self.ui.last_stream_error = Some(err.clone());
                self.ui.last_stream_url = None;
                self.ui.add_log(&format!("Stream error: {err}"));
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
            Event::DownloadFiles { id, files, error } => {
                // Only the modal that asked: a poll landing while the user
                // is looking at another torrent's files must not replace
                // them.
                if let Modal::Files(state) = &mut self.ui.modal {
                    if state.id == id {
                        state.files = files;
                        state.error = error;
                        state.pending = false;
                        state.cursor = state.cursor.min(state.files.len().saturating_sub(1));
                    }
                }
            }
            Event::DownloadDaemonDown => {
                // The rows are left alone: the daemon not answering is not
                // evidence that the downloads stopped existing.
                self.ui.daemon_reachable = Some(false);
                self.ui.free_space = None;
            }
            Event::DownloadListUpdate(rows) => {
                // The cursor is kept on the same download across polls, by
                // id: a daemon that reorders its list must not move the
                // row the user is acting on.
                let keep = self.ui.downloads.get(self.ui.download_cursor).map(|r| r.id);
                self.ui.downloads = rows;
                self.ui.daemon_reachable = Some(true);
                self.ui.download_cursor = keep
                    .and_then(|id| self.ui.downloads.iter().position(|r| r.id == id))
                    .unwrap_or(0)
                    .min(self.ui.downloads.len().saturating_sub(1));
                // Free space comes from the same daemon as the same poll,
                // so asking it here costs no extra round trip.
                if let Ok(free) = self.transmission.free_space().await {
                    self.ui.free_space = Some(free);
                }
            }
            Event::TorrentListUpdate(list) => {
                // Prefer the torrent we're actively
                let chosen = match &self.ui.active_torrent_hash {
                    Some(hash) => list
                        .iter()
                        .find(|t| &t.hash == hash)
                        .or_else(|| list.first()),
                    None => list.first(),
                };
                if let Some(t) = chosen {
                    self.ui.torrent_status = TorrentStatus {
                        hash: t.hash.clone(),
                        title: if t.name.is_empty() {
                            self.ui.torrent_status.title.clone()
                        } else {
                            t.name.clone()
                        },
                        progress: t.progress(),
                        download_speed: t.download_speed.max(0.0) as u64,
                        upload_speed: t.upload_speed.max(0.0) as u64,
                        seeds: t.connected_seeders.max(0) as u32,
                        peers: t.active_peers.max(0) as u32,
                        downloaded: t.loaded_size.max(0) as u64,
                        total_size: t.total_size.max(0) as u64,
                        status: t.status_string.clone(),
                        ratio: None,
                        eta: None,
                        dir: String::new(),
                    };
                    // Cap history length -- a very wide terminal
                    const MAX_HISTORY: usize = 600;
                    self.ui
                        .progress_history
                        .push_back(self.ui.torrent_status.progress);
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
            Event::DetailLoaded {
                page_url,
                files,
                error,
            } => {
                apply_detail_loaded(&mut self.ui, &page_url, files, error.as_deref());
            }
            // Keys, mouse, ticks and resizes belong to `run`, the only
            // thing with a terminal to draw them on.
            _ => {}
        }
        Ok(())
    }

    /// Apply events until `done` says the answer is in, or `deadline`
    /// runs out.
    ///
    /// The CLI's "do this one thing, then print the answer" shape. The TUI
    /// never waits for anything -- it draws whatever has arrived and
    /// redraws -- but a one-shot command has no screen to redraw, so it
    /// has to know when the work is finished.
    ///
    /// The condition is a predicate rather than a state, because "the work
    /// is done" is not one state: a search finishes by leaving `Searching`,
    /// a stream by leaving `Streaming`, and a file list by no longer being
    /// pending. A version of this that only knew about `Searching` returned
    /// immediately on the other two, and reported a stream that had not
    /// been tried yet as one that had not answered.
    pub async fn pump_until(
        &mut self,
        deadline: std::time::Duration,
        done: impl Fn(&Self) -> bool,
    ) -> Result<()> {
        let give_up = tokio::time::Instant::now() + deadline;
        loop {
            if done(self) {
                return Ok(());
            }
            let Some(remaining) = give_up.checked_duration_since(tokio::time::Instant::now())
            else {
                return Ok(());
            };
            match tokio::time::timeout(remaining, self.event_handler.next()).await {
                // Nothing arrived before the deadline, or the handler is
                // gone: hand back what is on screen and let the caller
                // report the shortfall.
                Err(_) | Ok(Err(_)) => return Ok(()),
                Ok(Ok(event)) => self.apply_event(event).await?,
            }
        }
    }

    pub async fn run(&mut self) -> Result<()> {
        // The keyboard protocol is what makes Shift+Enter arrive as
        let mut terminal = tui::init(!self.config.false_tty, !self.config.disable_mouse)?;
        self.terminal_size = terminal
            .size()
            .map(|s| (s.width, s.height))
            .unwrap_or((80, 24));

        Self::spawn_termination_watch(Arc::clone(&self.exit_signal));

        // The TorrServer this setting is about: started here when the setting
        // is on and nothing is answering, and deliberately left running when
        // doris exits. Only in an interactive run -- a CLI command that
        // quietly left a server behind would be the kind of surprise nobody
        // asked for.
        if self.config.enable_torrserver {
            self.ensure_torrserver().await;
        }

        self.report_bridge_taken();

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
                        // See `Event::OpenPath`: only this loop has the
                        // terminal to hand over and take back.
                        Event::OpenPath { manager, path } => {
                            open::open_path(&mut terminal, &self.config, manager, &path)?;
                            self.terminal_size = terminal.size()
                                .map(|s| (s.width, s.height))
                                .unwrap_or(self.terminal_size);
                        }
                        other => self.apply_event(other).await?,
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
        // The arrangement goes out whatever `save_config_on_exit` says:
        // the next session has to start where this one stopped, sizes
        // and all, and that is not a preference to be opted into.
        if let Err(e) =
            crate::config::save_layout(&self.ui.zones.snapshot(), self.args.config.as_deref())
        {
            eprintln!("Failed to save the panel layout: {}", e);
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
