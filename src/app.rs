use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};

use crate::browser::cdp::{Browser, BrowserMode};
use crate::browser::detect;
use crate::event::{Event, EventHandler};
use crate::search::rutracker::RutrackerSearcher;
use crate::torrserver::api::TorrServer;
use crate::bridge::handler::BridgeServer;
use crate::tui;
use crate::ui::app::{App as UiApp, AppState};
use crate::cli::Args;
use crate::config::Config;

pub struct App {
    args: Args,
    #[allow(dead_code)]
    config: Config,
    ui: UiApp,
    event_handler: EventHandler,
    searcher: Option<RutrackerSearcher>,
    torrserver: TorrServer,
    browser: Option<Arc<Mutex<Browser>>>,
    browser_mode: BrowserMode,
    #[allow(dead_code)]
    search_tx: mpsc::UnboundedSender<String>,
    search_rx: mpsc::UnboundedReceiver<String>,
    bridge: Option<BridgeServer>,
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
            searcher: None,
            torrserver: TorrServer::new(&torrserver_url),
            browser: None,
            browser_mode,
            search_tx,
            search_rx,
            bridge,
            args,
            config,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut terminal = tui::init()?;

        if let Some(query) = self.args.query.clone() {
            self.ui.search_input = query.clone();
            self.start_search(query).await;
        } else {
            self.ui.add_log("T-Hunter started. Press 's' to focus search, Enter to search.");
            if self.bridge.is_some() {
                self.ui.add_log(&format!("Extension Bridge listening on port {}", self.config.bridge_port));
            }
        }

        loop {
            terminal.draw(|frame| self.ui.render(frame))?;

            tokio::select! {
                event = self.event_handler.next() => {
                    match event? {
                        Event::Key(key) => self.handle_key(key).await?,
                        Event::Tick => {},
                        Event::Resize(_, _) => {},
                        Event::SearchComplete(results) => {
                            self.ui.results = results.clone();
                            self.ui.selected = 0;
                            self.ui.state = AppState::Idle;
                            self.ui.add_log(&format!("Found {} results", results.len()));
                        }
                        Event::SearchError(err) => {
                            self.ui.state = AppState::Error(err.clone());
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

    async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.ui.running = false;
            }
            KeyCode::Char('j') | KeyCode::Down if !self.ui.input_mode => {
                if !self.ui.results.is_empty() {
                    self.ui.selected = (self.ui.selected + 1).min(self.ui.results.len() - 1);
                }
            }
            KeyCode::Char('k') | KeyCode::Up if !self.ui.input_mode => {
                self.ui.selected = self.ui.selected.saturating_sub(1);
            }
            KeyCode::PageUp if !self.ui.input_mode => self.ui.scroll_logs_up(),
            KeyCode::PageDown if !self.ui.input_mode => self.ui.scroll_logs_down(),
            KeyCode::Char('s') if !self.ui.input_mode => {
                self.ui.input_mode = true;
            }
            KeyCode::Esc => {
                self.ui.input_mode = false;
            }
            KeyCode::Enter => {
                if !self.ui.input_mode {
                    self.spawn_stream().await;
                } else {
                    let query = self.ui.search_input.clone();
                    self.ui.input_mode = false;
                    self.start_search(query).await;
                }
            }
            KeyCode::Char(c) if self.ui.input_mode => {
                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    match c {
                        'u' => self.ui.search_input.clear(),
                        'w' => {
                            let words: Vec<&str> = self.ui.search_input.split_whitespace().collect();
                            if let Some(first_word) = words.last() {
                                let cut_pos = self.ui.search_input.len() - first_word.len();
                                self.ui.search_input.truncate(cut_pos);
                            }
                        }
                        _ => {}
                    }
                } else {
                    self.ui.search_input.push(c);
                }
            }
            KeyCode::Backspace if self.ui.input_mode => {
                self.ui.search_input.pop();
            }
            KeyCode::PageUp => self.ui.scroll_logs_up(),
            KeyCode::PageDown => self.ui.scroll_logs_down(),
            _ => {}
        }
        Ok(())
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

        let searcher = RutrackerSearcher::new(browser);
        self.searcher = Some(searcher);

        if let Some(ref mut s) = self.searcher {
            let cookie_file = self.args.cookie_file.clone();
            let username = self.args.username.clone();
            let password = self.args.password.clone();

            match s.ensure_logged_in(cookie_file.as_deref(), username.as_deref(), password.as_deref()).await {
                Ok(true) => self.ui.add_log("Logged in successfully"),
                Ok(false) => self.ui.add_log("Login failed - continuing anyway"),
                Err(e) => self.ui.add_log(&format!("Login error: {}", e)),
            }

            let search_result = s.search(&query).await;
            match search_result {
                Ok(results) => {
                    self.ui.results = results.clone();
                    self.ui.selected = 0;
                    self.ui.state = AppState::Idle;
                    self.ui.add_log(&format!("Found {} results", results.len()));
                }
                Err(e) => {
                    self.ui.state = AppState::Error(e.to_string());
                    self.ui.add_log(&format!("Search failed: {}", e));
                }
            }
        }
    }

    async fn spawn_stream(&mut self) {
        if self.ui.selected >= self.ui.results.len() {
            self.ui.add_log("No result selected");
            return;
        }
        let item = self.ui.results[self.ui.selected].clone();
        self.ui.state = AppState::Streaming;

        let torrserver = self.torrserver.clone();
        let searcher = self.searcher.clone();
        let event_tx = self.event_handler.sender();

        tokio::spawn(async move {
            let log = |msg: &str| { let _ = event_tx.send(Event::StreamLog(msg.to_string())); };

            log(&format!("Downloading torrent: {}", item.title));

            if !torrserver.is_reachable().await {
                log("TorrServer is not reachable! Start TorrServer on localhost:8090");
                let _ = event_tx.send(Event::StreamError("TorrServer unreachable".into()));
                return;
            }

            let searcher = match searcher {
                Some(s) => s,
                None => {
                    log("No browser session - search first");
                    let _ = event_tx.send(Event::StreamError("No browser".into()));
                    return;
                }
            };

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
