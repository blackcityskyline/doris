use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseButton, MouseEventKind};
use ratatui::prelude::Rect;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

use crate::browser::cdp::{Browser, BrowserMode};
use crate::browser::detect;
use crate::event::{Event, EventHandler};
use crate::search::rutracker::RutrackerSearcher;
use crate::torrserver::api::TorrServer;
use crate::bridge::handler::BridgeServer;
use crate::tui;
use crate::ui::app::{App as UiApp, AppState, Modal};
use crate::cli::Args;
use crate::config::Config;

pub struct App {
    args: Args,
    #[allow(dead_code)]
    config: Config,
    ui: UiApp,
    event_handler: EventHandler,
    torrserver: TorrServer,
    browser: Option<Arc<Mutex<Browser>>>,
    browser_mode: BrowserMode,
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

        let browser_mode_str = args.browser_mode.clone()
            .unwrap_or_else(|| config.browser_mode.clone());
        let browser_mode: BrowserMode = browser_mode_str.parse()?;

        let browser_choice = args.browser.as_deref().or(config.browser.as_deref());
        let browser_info = detect::detect_browser(browser_choice)
            .map(|(kind, path)| format!("{} [{}] ({})", kind, browser_mode, path.display()))
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

        Ok(Self {
            ui: UiApp::new(torrserver_url.clone(), browser_info),
            event_handler: EventHandler::new(std::time::Duration::from_millis(100)),
            torrserver: TorrServer::new(&torrserver_url),
            browser: None,
            browser_mode,
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
            self.start_search(query).await;
        } else {
            self.ui.add_log("T-Hunter started. Press 's' or 'i' to search, 'a' for login, Enter to play.");
            self.ui.add_log("Press 'L' for detailed log view");
            if self.bridge.is_some() {
                self.ui.add_log(&format!("Extension Bridge listening on port {}", self.config.bridge_port));
            }
            if crate::credentials::load_credentials().is_some() {
                self.ui.add_log("Saved credentials found (will use if cookies fail)");
            }
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
                        Event::Mouse(mouse) => self.handle_mouse(mouse),
                        Event::Tick => {},
                        Event::Resize(w, h) => {
                            self.terminal_size = (w, h);
                        },
                        Event::SearchComplete(results) => {
                            self.ui.results = results.clone();
                            self.ui.selected = 0;
                            self.ui.state = AppState::Idle;
                            self.ui.add_log(&format!("Found {} results", results.len()));
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
                            self.start_search(query).await;
                        }
                    }
                }
                query = self.search_rx.recv() => {
                    if let Some(query) = query {
                        self.ui.search_input = query.clone();
                        self.start_search(query).await;
                    }
                }
            }

            if !self.ui.running {
                break;
            }
        }

        tui::restore(&mut terminal)?;
        Ok(())
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                if self.ui.modal == Modal::None {
                    self.ui.scroll_logs_up();
                }
            }
            MouseEventKind::ScrollDown => {
                if self.ui.modal == Modal::None {
                    self.ui.scroll_logs_down();
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if self.ui.modal == Modal::None {
                    let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
                    self.ui.click_results_at(mouse.row, area);
                }
            }
            _ => {}
        }
    }

    async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if self.ui.detail_log_mode {
            match key.code {
                KeyCode::Char('L') | KeyCode::Esc => {
                    self.ui.detail_log_mode = false;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    self.ui.detail_log_scroll = (self.ui.detail_log_scroll + 1).min(self.ui.detail_logs.len());
                }
                KeyCode::Char('k') | KeyCode::Up => {
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

        if self.ui.modal != Modal::None {
            if let Some((username, password)) = self.ui.login_modal_key(key) {
                self.do_login(&username, &password).await;
            }
            return Ok(());
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.ui.quit();
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.ui.navigate_down();
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.ui.navigate_up();
            }
            KeyCode::PageUp if !self.ui.input_mode => self.ui.scroll_logs_page_up(),
            KeyCode::PageDown if !self.ui.input_mode => self.ui.scroll_logs_page_down(),
            KeyCode::Char('s') | KeyCode::Char('i') if !self.ui.input_mode => {
                self.ui.enter_input_mode();
            }
            KeyCode::Char('a') if !self.ui.input_mode => {
                self.ui.open_login_modal();
            }
            KeyCode::Char('L') if !self.ui.input_mode => {
                self.ui.toggle_detail_log();
            }
            KeyCode::Esc => {
                if self.ui.detail_log_mode {
                    self.ui.detail_log_mode = false;
                } else {
                    self.ui.exit_input_mode();
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

    async fn do_login(&mut self, username: &str, password: &str) {
        self.ui.add_log(&format!("Logging in as '{}'...", username));

        let _ = crate::credentials::save_credentials(username, password);

        let browser = match self.get_browser().await {
            Ok(b) => b,
            Err(e) => {
                self.ui.add_log(&format!("Browser error: {}", e));
                return;
            }
        };

        let username = username.to_string();
        let password = password.to_string();
        let cookie_file = self.args.cookie_file.clone();
        let event_tx_login = self.event_handler.sender();
        let event_tx_result = self.event_handler.sender();
        let log = Arc::new(move |msg: &str| { let _ = event_tx_login.send(Event::StreamLog(msg.to_string())); });

        tokio::spawn(async move {
            let mut searcher = RutrackerSearcher::new(browser);

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
        self.ui.state = AppState::Searching;
        self.ui.add_log(&format!("Searching for '{}'...", query));

        let browser = match self.get_browser().await {
            Ok(b) => b,
            Err(e) => {
                self.ui.add_log(&format!("Browser error: {}", e));
                self.ui.state = AppState::Idle;
                return;
            }
        };

        let cookie_file = self.args.cookie_file.clone();
        let username = self.args.username.clone();
        let password = self.args.password.clone();
        let saved_creds = crate::credentials::load_credentials();
        let event_tx_log = self.event_handler.sender();
        let event_tx_result = self.event_handler.sender();
        let log = Arc::new(move |msg: &str| { let _ = event_tx_log.send(Event::StreamLog(msg.to_string())); });

        tokio::spawn(async move {
            let mut searcher = RutrackerSearcher::new(browser);

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

        let browser = match &self.browser {
            Some(b) => Arc::clone(b),
            None => {
                self.ui.add_log("No browser session - search first");
                return;
            }
        };

        let torrserver = self.torrserver.clone();
        let event_tx = self.event_handler.sender();

        tokio::spawn(async move {
            let log = |msg: &str| { let _ = event_tx.send(Event::StreamLog(msg.to_string())); };

            log(&format!("Downloading torrent: {}", item.title));

            if !torrserver.is_reachable().await {
                log("TorrServer is not reachable! Start TorrServer on localhost:8090");
                let _ = event_tx.send(Event::StreamError("TorrServer unreachable".into()));
                return;
            }

            let searcher = RutrackerSearcher::new(browser);

            match searcher.download_torrent(&item.download_url).await {
                Ok(bytes) => {
                    log(&format!("Downloaded {} bytes", bytes.len()));
                    match torrserver.upload_torrent(&bytes, &item.title).await {
                        Ok(hash) => {
                            log(&format!("Uploaded, hash: {}", hash));
                            match torrserver.play(&hash, &item.title, None).await {
                                Ok(url) => {
                                    let _ = event_tx.send(Event::StreamComplete(url));
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
        let (kind, path) = detect::detect_browser(browser_choice)?;
        self.ui.add_log(&format!("Launching {} in {} mode...", kind, self.browser_mode));

        let browser = Browser::launch(&path, self.browser_mode.clone()).await?;
        let browser = Arc::new(Mutex::new(browser));
        self.browser = Some(Arc::clone(&browser));

        Ok(browser)
    }
}
