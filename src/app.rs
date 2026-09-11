use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseButton, MouseEventKind};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

use crate::browser::cdp::{Browser, BrowserVisibility};
use crate::browser::detect;
use crate::event::{Event, EventHandler};
use crate::search::rutracker::RutrackerSearcher;
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
fn resolve_download_dir(config: &Config) -> String {
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

pub struct App {
    args: Args,
    #[allow(dead_code)]
    config: Config,
    ui: UiApp,
    event_handler: EventHandler,
    torrserver: TorrServer,
    browser: Option<Arc<Mutex<Browser>>>,
    /// Shared across start_search/load_more/do_login (all run in spawned
    /// tasks) so RutrackerSearcher's `logged_in` flag actually persists
    /// between calls. Previously each of those constructed its own fresh
    /// RutrackerSearcher::new(browser), which reset `logged_in` to false
    /// every time -- so paginating past the first page of results (or any
    /// action after the first) went through a full re-login/cookie
    /// re-injection sequence every single time, which is slow. The
    /// browser itself was always reused correctly via get_browser(); this
    /// mirrors that same lazy-cache pattern for the searcher.
    searcher: Option<Arc<Mutex<RutrackerSearcher>>>,
    browser_visibility: BrowserVisibility,
    #[allow(dead_code)]
    search_tx: mpsc::UnboundedSender<String>,
    search_rx: mpsc::UnboundedReceiver<String>,
    bridge: Option<BridgeServer>,
    terminal_size: (u16, u16),
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
            searcher: None,
            browser_visibility,
            search_tx,
            search_rx,
            bridge,
            args,
            config,
            terminal_size: (0, 0),
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut terminal = tui::init()?;
        self.terminal_size = terminal.size().map(|s| (s.width, s.height)).unwrap_or((80, 24));

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
                        Event::SearchComplete(results) => {
                            let count = results.len();
                            if self.ui.search_offset > 0 {
                                self.ui.results.extend(results);
                                self.ui.add_log(&format!("Loaded {} more results (total: {})", count, self.ui.results.len()));
                            } else {
                                self.ui.results = results;
                                self.ui.selected = 0;
                                self.ui.add_log(&format!("Found {} results", count));
                            }
                            if count < 50 {
                                self.ui.all_loaded = true;
                            }
                            self.ui.search_offset = self.ui.results.len();
                            self.ui.state = AppState::Idle;
                            self.ui.update_filter();
                        }
                        Event::SearchError(err) => {
                            self.ui.state = AppState::Idle;
                            self.ui.add_log(&format!("Search error: {}", err));
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
                        Event::LoadMore(query, offset) => {
                            self.load_more(query, offset).await;
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

            if !self.ui.running {
                break;
            }
        }

        tui::restore(&mut terminal)?;

        if self.config.save_config_on_exit {
            if let Err(e) = crate::config::save(&self.config, None) {
                eprintln!("Failed to save config on exit: {}", e);
            }
        }

        Ok(())
    }

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

    /// See the free function of the same name for the resolution logic;
    /// this just supplies `&self.config`.
    fn resolve_download_dir(&self) -> String {
        resolve_download_dir(&self.config)
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
                        let offset = self.ui.search_offset;
                        self.load_more(q, offset).await;
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
                            Some(i) => ORDER[(i + 1) % ORDER.len()],
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
                    SettingsAction::OpenLog => {
                        self.ui.modal = Modal::None;
                        self.ui.detail_log_mode = true;
                    }
                    SettingsAction::RunHealthCheck => {
                        let results = self.ui.health_check();
                        self.ui.modal = Modal::HealthCheck(results);
                    }
                    SettingsAction::CycleTheme => {
                        let themes = Theme::load_themes();
                        if let Some(pos) = themes.iter().position(|t| t.name == self.ui.theme.name) {
                            let next = (pos + 1) % themes.len();
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
                                (self.config.preset_index + 1) % self.config.presets.len();
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
                            Some(i) => STEPS[(i + 1) % STEPS.len()],
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
                            Some(i) => SYMBOLS[(i + 1) % SYMBOLS.len()],
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
                            Some(i) => MODES[(i + 1) % MODES.len()],
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
                            Some(i) => STEPS[(i + 1) % STEPS.len()],
                            None => STEPS[0],
                        };
                        self.config.download_speed_limit_kbps = next;
                        self.ui.open_settings(&self.config);
                    }
                    SettingsAction::CycleUploadSpeedLimit => {
                        const STEPS: &[u32] = &[0, 64, 128, 256, 512, 1024, 2048, 5120];
                        let next = match STEPS.iter().position(|&v| v == self.config.upload_speed_limit_kbps) {
                            Some(i) => STEPS[(i + 1) % STEPS.len()],
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
                if let Some(query) = self.ui.submit_search() {
                    self.start_search(query).await;
                } else if let Some(_idx) = self.ui.submit_selection() {
                    self.spawn_stream().await;
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

        let searcher = match self.get_searcher().await {
            Ok(s) => s,
            Err(e) => {
                self.ui.add_log(&format!("Browser error: {}", e));
                return;
            }
        };

        let username = username.to_string();
        let password = password.to_string();
        // If "Save cookies" is off, don't pass a cookie file path through
        // at all -- ensure_logged_in only persists cookies to disk when it
        // has somewhere to write them.
        let cookie_file = if self.config.save_cookies {
            self.args.cookie_file.clone()
        } else {
            None
        };
        let event_tx_login = self.event_handler.sender();
        let event_tx_result = self.event_handler.sender();
        let log = Arc::new(move |msg: &str| { let _ = event_tx_login.send(Event::StreamLog(msg.to_string())); });

        tokio::spawn(async move {
            let mut searcher = searcher.lock().await;

            match searcher.ensure_logged_in(cookie_file.as_deref(), Some(&username), Some(&password), log.clone()).await {
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
        if !self.config.enabled_sources.iter().any(|s| s == "rutracker") {
            self.ui.add_log("Rutracker is disabled in Options -> streaming -> Sources.");
            self.ui.state = AppState::Idle;
            return;
        }

        self.ui.state = AppState::Searching;
        self.ui.search_query = Some(query.clone());
        self.ui.search_offset = 0;
        self.ui.all_loaded = false;
        self.ui.add_log(&format!("Searching for '{}'...", query));

        let searcher = match self.get_searcher().await {
            Ok(s) => s,
            Err(e) => {
                self.ui.add_log(&format!("Browser error: {}", e));
                self.ui.state = AppState::Idle;
                return;
            }
        };

        // If "Save cookies" is off, don't pass a cookie file path
        // through at all -- see do_login for the same gating.
        let cookie_file = if self.config.save_cookies { self.args.cookie_file.clone() } else { None };
        let username = self.args.username.clone();
        let password = self.args.password.clone();
        let saved_creds = crate::credentials::load_credentials();
        let event_tx_log = self.event_handler.sender();
        let event_tx_result = self.event_handler.sender();
        let log = Arc::new(move |msg: &str| { let _ = event_tx_log.send(Event::StreamLog(msg.to_string())); });

        tokio::spawn(async move {
            let mut searcher = searcher.lock().await;

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

            match searcher.ensure_logged_in(cookie_file.as_deref(), cred_user.as_deref(), cred_pass.as_deref(), log.clone()).await {
                Ok(true) => log("SEARCH: logged in, proceeding with search"),
                Ok(false) => log("SEARCH: not logged in, proceeding anyway"),
                Err(e) => log(&format!("SEARCH: login error: {}", e)),
            }

            match searcher.search(&query).await {
                Ok(results) => {
                    let _ = event_tx_result.send(Event::SearchComplete(results));
                }
                Err(e) => {
                    let _ = event_tx_result.send(Event::SearchError(e.to_string()));
                }
            }
        });
    }

    async fn spawn_stream(&mut self) {
        if self.ui.selected >= self.ui.results.len() {
            self.ui.add_log("No result selected");
            return;
        }
        let item = self.ui.results[self.ui.selected].clone();
        self.ui.state = AppState::Streaming;

        let browser_present = self.browser.is_some();
        if !browser_present {
            self.ui.add_log("No browser session - search first");
            return;
        }
        let searcher = match self.get_searcher().await {
            Ok(s) => s,
            Err(e) => {
                self.ui.add_log(&format!("Browser error: {}", e));
                return;
            }
        };

        let torrserver = self.torrserver.clone();
        let event_tx = self.event_handler.sender();

        tokio::spawn(async move {
            let log = |msg: &str| { let _ = event_tx.send(Event::StreamLog(msg.to_string())); };

            log(&format!("Fetching .torrent file: {}", item.title));

            if !torrserver.is_reachable().await {
                log("TorrServer is not reachable! Start TorrServer on localhost:8090");
                let _ = event_tx.send(Event::StreamError("TorrServer unreachable".into()));
                return;
            }

            let searcher = searcher.lock().await;

            match searcher.download_torrent(&item.download_url).await {
                Ok(bytes) => {
                    log(&format!("Downloaded {} bytes", bytes.len()));
                    match torrserver.upload_torrent(&bytes, &item.title).await {
                        Ok(hash) => {
                            log(&format!("Uploaded, hash: {}", hash));
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
                        }
                        Err(e) => {
                            log(&format!("Upload error: {}", e));
                            let _ = event_tx.send(Event::StreamError(e.to_string()));
                        }
                    }
                }
                Err(e) => {
                    log(&format!("Download error: {}", e));
                    let _ = event_tx.send(Event::StreamError(e.to_string()));
                }
            }
        });
    }

    async fn get_browser(&mut self) -> Result<Arc<Mutex<Browser>>> {
        if let Some(ref b) = self.browser {
            return Ok(Arc::clone(b));
        }

        let browser_choice = self.args.browser.as_deref().or(self.config.browser.as_deref());
        let browser_priority = detect::parse_priority(&self.config.browser_priority);
        let (kind, path) = detect::detect_browser_with_priority(browser_choice, &browser_priority)?;
        self.ui.add_log(&format!("Launching {} ({})...", kind, self.browser_visibility));

        // TODO(Phase 3): this should come from the active Source
        // (`Source::home_url()`) once the Source trait lands, instead of
        // being rutracker-specific here.
        let browser = Browser::launch(&path, self.browser_visibility, RutrackerSearcher::HOME_URL).await?;
        let browser = Arc::new(Mutex::new(browser));
        self.browser = Some(Arc::clone(&browser));

        Ok(browser)
    }

    /// Lazily create (once) and reuse the same `RutrackerSearcher` for the
    /// lifetime of the browser session, so its `logged_in` flag actually
    /// means something across calls. See the field doc comment on
    /// `searcher` for why this exists.
    async fn get_searcher(&mut self) -> Result<Arc<Mutex<RutrackerSearcher>>> {
        if let Some(ref s) = self.searcher {
            return Ok(Arc::clone(s));
        }
        let browser = self.get_browser().await?;
        let searcher = Arc::new(Mutex::new(RutrackerSearcher::new(browser)));
        self.searcher = Some(Arc::clone(&searcher));
        Ok(searcher)
    }

    async fn load_more(&mut self, query: String, offset: usize) {
        self.ui.state = AppState::Searching;

        let searcher = match self.get_searcher().await {
            Ok(s) => s,
            Err(e) => {
                self.ui.add_log(&format!("Browser error: {}", e));
                self.ui.state = AppState::Idle;
                return;
            }
        };

        let event_tx_log = self.event_handler.sender();
        let event_tx_result = self.event_handler.sender();
        // If "Save cookies" is off, don't pass a cookie file path
        // through at all -- see do_login for the same gating.
        let cookie_file = if self.config.save_cookies { self.args.cookie_file.clone() } else { None };
        let username = self.args.username.clone();
        let password = self.args.password.clone();
        let saved_creds = crate::credentials::load_credentials();
        let log = Arc::new(move |msg: &str| { let _ = event_tx_log.send(Event::StreamLog(msg.to_string())); });

        tokio::spawn(async move {
            let mut searcher = searcher.lock().await;

            let (cred_user, cred_pass) = match (username, password) {
                (Some(u), Some(p)) => (Some(u), Some(p)),
                _ => match saved_creds {
                    Some((u, p)) => (Some(u), Some(p)),
                    None => (None, None),
                },
            };

            let _ = searcher.ensure_logged_in(cookie_file.as_deref(), cred_user.as_deref(), cred_pass.as_deref(), log.clone()).await;

            match searcher.search_page(&query, offset).await {
                Ok(results) => {
                    let _ = event_tx_result.send(Event::SearchComplete(results));
                }
                Err(e) => {
                    let _ = event_tx_result.send(Event::SearchError(e.to_string()));
                }
            }
        });
    }
}
