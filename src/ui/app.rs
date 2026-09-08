use ratatui::prelude::*;
use ratatui::widgets::*;
use crate::search::models::TorrentItem;
use std::collections::VecDeque;
use super::theme::Theme;
use super::zones::{ZoneId, ZoneLayout};
use super::menu::MenuState;

#[derive(PartialEq)]
pub enum AppState {
    Idle,
    Searching,
    Streaming,
    Error(String),
}

#[derive(PartialEq, Clone, Debug)]
pub enum Modal {
    None,
    Login(LoginState),
    Settings(SettingsState),
    HealthCheck(Vec<String>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettingsState {
    pub selected_category: usize,
    pub selected: usize,
    pub page: usize,
    pub categories: Vec<SettingsCategory>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettingsCategory {
    pub name: String,
    pub items: Vec<SettingsItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettingsItem {
    pub label: String,
    pub value: String,
    pub description: Vec<String>,
    pub action: SettingsAction,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SettingsAction {
    ToggleBrowserVisibility,
    ToggleMode,
    SetDownloadDir,
    RunHealthCheck,
    OpenLog,
    CycleTheme,
    ToggleThemeBackground,
    ToggleTruecolor,
    ToggleVimKeys,
    ToggleMouse,
    SetUpdateMs,
    ToggleRoundedCorners,
    ToggleTerminalSync,
    SetLogLevel,
    ToggleSaveOnExit,
    Close,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoginState {
    pub username: String,
    pub password: String,
    pub focus: LoginField,
    pub message: Option<String>,
}

#[derive(PartialEq, Clone, Debug)]
pub enum LoginField {
    Username,
    Password,
}

impl LoginState {
    pub fn new() -> Self {
        Self {
            username: String::new(),
            password: String::new(),
            focus: LoginField::Username,
            message: None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct TorrentStatus {
    pub hash: String,
    pub title: String,
    pub progress: f64,
    pub download_speed: u64,
    pub upload_speed: u64,
    pub seeds: u32,
    pub peers: u32,
    pub downloaded: u64,
    pub total_size: u64,
    pub status: String,
}

pub struct App {
    pub search_input: String,
    pub results: Vec<TorrentItem>,
    pub selected: usize,
    pub logs: VecDeque<String>,
    pub log_scroll: usize,
    pub detail_logs: Vec<String>,
    pub detail_log_mode: bool,
    pub detail_log_scroll: usize,
    pub state: AppState,
    pub browser_info: String,
    pub torrserver_url: String,
    pub running: bool,
    pub input_mode: bool,
    pub modal: Modal,
    pub search_query: Option<String>,
    pub search_offset: usize,
    pub all_loaded: bool,
    /// True = browser runs hidden (background). False = visible window.
    pub browser_hidden: bool,
    pub stream_mode: bool,
    pub download_dir: String,
    pub theme: Theme,
    pub zones: ZoneLayout,
    pub menu: MenuState,
    pub show_menu: bool,
    pub torrent_status: TorrentStatus,
    pub filtered_indices: Vec<usize>,
}

impl App {
    pub fn new(torrserver_url: String, browser_info: String) -> Self {
        Self {
            search_input: String::new(),
            results: Vec::new(),
            selected: 0,
            logs: VecDeque::new(),
            log_scroll: 0,
            detail_logs: Vec::new(),
            detail_log_mode: false,
            detail_log_scroll: 0,
            state: AppState::Idle,
            browser_info,
            torrserver_url,
            running: true,
            input_mode: false,
            modal: Modal::None,
            search_query: None,
            search_offset: 0,
            all_loaded: false,
            browser_hidden: true,
            stream_mode: true,
            download_dir: dirs::download_dir()
                .map(|d| d.display().to_string())
                .unwrap_or_else(|| "/tmp".to_string()),
            theme: Theme::default(),
            zones: ZoneLayout::new(),
            menu: MenuState::new(),
            show_menu: false,
            torrent_status: TorrentStatus::default(),
            filtered_indices: Vec::new(),
        }
    }

    pub fn add_log(&mut self, msg: &str) {
        let ts = chrono::Local::now().format("%H:%M:%S").to_string();
        self.logs.push_back(format!("[{}] {}", ts, msg));
        if self.logs.len() > 500 {
            self.logs.pop_front();
        }
        self.log_scroll = self.logs.len();
    }

    pub fn add_detail(&mut self, msg: &str) {
        let ts = chrono::Local::now().format("%H:%M:%S%.3f").to_string();
        self.detail_logs.push(format!("[{}] {}", ts, msg));
        self.detail_log_scroll = self.detail_logs.len();
    }

    pub fn toggle_detail_log(&mut self) {
        self.detail_log_mode = !self.detail_log_mode;
    }

    pub fn scroll_logs_up(&mut self) {
        self.log_scroll = self.log_scroll.saturating_sub(1);
    }

    pub fn scroll_logs_down(&mut self) {
        self.log_scroll = (self.log_scroll + 1).min(self.logs.len());
    }

    pub fn scroll_logs_page_up(&mut self) {
        self.log_scroll = self.log_scroll.saturating_sub(10);
    }

    pub fn scroll_logs_page_down(&mut self) {
        self.log_scroll = (self.log_scroll + 10).min(self.logs.len());
    }

    pub fn mouse_scroll_logs(&mut self, delta: i16) {
        if delta > 0 {
            for _ in 0..delta {
                self.scroll_logs_up();
            }
        } else {
            for _ in 0..(-delta) {
                self.scroll_logs_down();
            }
        }
    }

    pub fn click_results_at(&mut self, row: u16, _area: Rect) {
        let results_area = self.zones.get_area(ZoneId::Results);
        if row >= results_area.y && row < results_area.y + results_area.height {
            let table_row = (row - results_area.y) as usize;
            if table_row == 0 { return; }
            let data_row = table_row - 1;
            let idx = data_row;
            if idx < self.filtered_indices.len() {
                self.selected = self.filtered_indices[idx];
            }
        }
    }

    pub fn open_login_modal(&mut self) {
        self.modal = Modal::Login(LoginState::new());
    }

    pub fn open_settings(&mut self) {
        let visibility_str = if self.browser_hidden { "Hidden".to_string() } else { "Visible".to_string() };
        let mode_str = if self.stream_mode { "Streaming (TorrServer)".to_string() } else { "Download (.torrent file)".to_string() };
        let theme_name = self.theme.name.clone();
        let themes = Theme::load_themes();
        let theme_idx = themes.iter().position(|t| t.name == theme_name).unwrap_or(0);
        let theme_str = format!("{}/{}", theme_idx + 1, themes.len());
        
        self.modal = Modal::Settings(SettingsState {
            selected_category: 0,
            selected: 0,
            page: 0,
            categories: vec![
                SettingsCategory {
                    name: "general".into(),
                    items: vec![
                        SettingsItem {
                            label: "Color theme".into(),
                            value: theme_str,
                            description: vec![
                                "Set color theme.".into(),
                                "".into(),
                                "Choose from all theme files in".into(),
                                "\"~/.config/doris/themes\".".into(),
                                "".into(),
                                "Use Left/Right to cycle.".into(),
                            ],
                            action: SettingsAction::CycleTheme,
                        },
                        SettingsItem {
                            label: "Theme background".into(),
                            value: "True".into(),
                            description: vec![
                                "If the theme set background".into(),
                                "should be shown.".into(),
                                "".into(),
                                "Set to False if you want".into(),
                                "terminal background".into(),
                                "transparency.".into(),
                            ],
                            action: SettingsAction::ToggleThemeBackground,
                        },
                        SettingsItem {
                            label: "Truecolor".into(),
                            value: "True".into(),
                            description: vec![
                                "Sets if 24-bit truecolor".into(),
                                "should be used.".into(),
                                "".into(),
                                "Will convert 24-bit colors to".into(),
                                "256 color if False.".into(),
                                "".into(),
                                "Set to False if your terminal".into(),
                                "doesn't have truecolor".into(),
                                "support.".into(),
                            ],
                            action: SettingsAction::ToggleTruecolor,
                        },
                        SettingsItem {
                            label: "Vim keys".into(),
                            value: "True".into(),
                            description: vec![
                                "Enable vim keys.".into(),
                                "".into(),
                                "Set to True to enable".into(),
                                "\"h,j,k,l\" keys for".into(),
                                "directional control in lists.".into(),
                            ],
                            action: SettingsAction::ToggleVimKeys,
                        },
                        SettingsItem {
                            label: "Disable mouse".into(),
                            value: "False".into(),
                            description: vec![
                                "Disable all mouse events.".into(),
                            ],
                            action: SettingsAction::ToggleMouse,
                        },
                        SettingsItem {
                            label: "Update ms".into(),
                            value: "1000".into(),
                            description: vec![
                                "Update time in milliseconds.".into(),
                                "".into(),
                                "Recommended 2000 ms or above".into(),
                                "for better sample times.".into(),
                                "".into(),
                                "Min value: 100 ms".into(),
                                "Max value: 86400000 ms".into(),
                            ],
                            action: SettingsAction::SetUpdateMs,
                        },
                        SettingsItem {
                            label: "Rounded corners".into(),
                            value: "True".into(),
                            description: vec![
                                "Rounded corners on boxes.".into(),
                                "".into(),
                                "True or False.".into(),
                                "".into(),
                                "Is always False if TTY mode".into(),
                                "is ON.".into(),
                            ],
                            action: SettingsAction::ToggleRoundedCorners,
                        },
                        SettingsItem {
                            label: "Terminal sync".into(),
                            value: "True".into(),
                            description: vec![
                                "Output synchronization.".into(),
                                "".into(),
                                "Use terminal synchronized".into(),
                                "output sequences to reduce".into(),
                                "flickering on supported".into(),
                                "terminals.".into(),
                            ],
                            action: SettingsAction::ToggleTerminalSync,
                        },
                    ],
                },
                SettingsCategory {
                    name: "app".into(),
                    items: vec![
                        SettingsItem {
                            label: "Browser visible".into(),
                            value: visibility_str,
                            description: vec![
                                "Show or hide the automated".into(),
                                "browser window.".into(),
                                "".into(),
                                "\"Hidden\" (default) runs it in".into(),
                                "the background.".into(),
                                "\"Visible\" shows the real".into(),
                                "browser window.".into(),
                            ],
                            action: SettingsAction::ToggleBrowserVisibility,
                        },
                        SettingsItem {
                            label: "Play mode".into(),
                            value: mode_str,
                            description: vec![
                                "Set playback mode.".into(),
                                "".into(),
                                "\"Streaming\" uses TorrServer,".into(),
                                "\"Download\" saves .torrent files.".into(),
                            ],
                            action: SettingsAction::ToggleMode,
                        },
                        SettingsItem {
                            label: "Download folder".into(),
                            value: self.download_dir.clone(),
                            description: vec![
                                "Set download directory path.".into(),
                                "".into(),
                                "Files will be saved here when".into(),
                                "using Download play mode.".into(),
                            ],
                            action: SettingsAction::SetDownloadDir,
                        },
                        SettingsItem {
                            label: "Open detailed log".into(),
                            value: "L".into(),
                            description: vec![
                                "Toggle detailed log view.".into(),
                                "".into(),
                                "Shows detailed application logs".into(),
                                "for debugging purposes.".into(),
                            ],
                            action: SettingsAction::OpenLog,
                        },
                        SettingsItem {
                            label: "Health check".into(),
                            value: "press Enter".into(),
                            description: vec![
                                "Run system health check.".into(),
                                "".into(),
                                "Verifies browser, TorrServer,".into(),
                                "and network connectivity.".into(),
                            ],
                            action: SettingsAction::RunHealthCheck,
                        },
                        SettingsItem {
                            label: "Log level".into(),
                            value: "INFO".into(),
                            description: vec![
                                "Set loglevel for error.log".into(),
                                "".into(),
                                "\"ERROR\", \"WARNING\", \"INFO\"".into(),
                                "and \"DEBUG\".".into(),
                                "".into(),
                                "The level set includes all".into(),
                                "lower levels.".into(),
                            ],
                            action: SettingsAction::SetLogLevel,
                        },
                        SettingsItem {
                            label: "Save on exit".into(),
                            value: "True".into(),
                            description: vec![
                                "Save config on exit.".into(),
                                "".into(),
                                "Automatically save current".into(),
                                "settings to config file on".into(),
                                "exit.".into(),
                            ],
                            action: SettingsAction::ToggleSaveOnExit,
                        },
                    ],
                },
            ],
        });
    }

    pub fn settings_key(&mut self, key: crossterm::event::KeyEvent) -> Option<SettingsAction> {
        if let Modal::Settings(ref mut state) = self.modal {
            match key.code {
                crossterm::event::KeyCode::Esc => {
                    self.modal = Modal::None;
                    return Some(SettingsAction::Close);
                }
                crossterm::event::KeyCode::Char('j') | crossterm::event::KeyCode::Down => {
                    let cat = &state.categories[state.selected_category];
                    state.selected = (state.selected + 1).min(cat.items.len() - 1);
                    let visible_items = 10;
                    let page = state.selected / visible_items;
                    if page != state.page {
                        state.page = page;
                    }
                }
                crossterm::event::KeyCode::Char('k') | crossterm::event::KeyCode::Up => {
                    state.selected = state.selected.saturating_sub(1);
                    let visible_items = 10;
                    let page = state.selected / visible_items;
                    if page != state.page {
                        state.page = page;
                    }
                }
                crossterm::event::KeyCode::Left => {
                    let cat = &state.categories[state.selected_category];
                    if let Some(item) = cat.items.get(state.selected) {
                        return Some(item.action.clone());
                    }
                }
                crossterm::event::KeyCode::Right => {
                    let cat = &state.categories[state.selected_category];
                    if let Some(item) = cat.items.get(state.selected) {
                        return Some(item.action.clone());
                    }
                }
                crossterm::event::KeyCode::Tab => {
                    state.selected_category = (state.selected_category + 1) % state.categories.len();
                    state.selected = 0;
                    state.page = 0;
                }
                crossterm::event::KeyCode::BackTab => {
                    state.selected_category = if state.selected_category == 0 {
                        state.categories.len() - 1
                    } else {
                        state.selected_category - 1
                    };
                    state.selected = 0;
                    state.page = 0;
                }
                crossterm::event::KeyCode::Char('1') => {
                    if state.categories.len() > 0 {
                        state.selected_category = 0;
                        state.selected = 0;
                        state.page = 0;
                    }
                }
                crossterm::event::KeyCode::Char('2') => {
                    if state.categories.len() > 1 {
                        state.selected_category = 1;
                        state.selected = 0;
                        state.page = 0;
                    }
                }
                crossterm::event::KeyCode::Enter => {
                    let cat = &state.categories[state.selected_category];
                    if let Some(item) = cat.items.get(state.selected) {
                        return Some(item.action.clone());
                    }
                }
                _ => {}
            }
        }
        None
    }

    pub fn health_check(&self) -> Vec<String> {
        let mut results = Vec::new();

        results.push("=== HEALTH CHECK ===".into());

        match crate::browser::detect::detect_browser(None) {
            Ok((kind, path)) => results.push(format!("{} Browser: {} [{}]", "\u{2714}", kind, path.display())),
            Err(e) => results.push(format!("{} Browser: NOT FOUND ({})", "\u{2718}", e)),
        }

        let has_xvfb = std::process::Command::new("which")
            .arg("Xvfb")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if has_xvfb { results.push(format!("{} Xvfb: available", "\u{2714}")); }
        else { results.push(format!("{} Xvfb: not found (needed to run browser hidden)", "\u{2718}")); }

        let has_chromedriver = std::path::Path::new(&dirs::data_local_dir()
            .unwrap_or_default().join("doris").join("chromedriver_patched")).exists()
            || std::process::Command::new("which").arg("chromedriver")
                .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
                .status().map(|s| s.success()).unwrap_or(false);
        if has_chromedriver { results.push(format!("{} Chromedriver: patched/available", "\u{2714}")); }
        else { results.push(format!("{} Chromedriver: will be downloaded on first run", "\u{26a0}")); }

        let ts_url = self.torrserver_url.clone();
        let ts_reachable = {
            let rt = tokio::runtime::Handle::current();
            rt.block_on(async {
                let client = reqwest::Client::new();
                client.get(&ts_url).timeout(std::time::Duration::from_secs(2)).send().await
                    .map(|r| r.status().is_success()).unwrap_or(false)
            })
        };
        if ts_reachable { results.push(format!("{} TorrServer: reachable ({})", "\u{2714}", ts_url)); }
        else { results.push(format!("{} TorrServer: NOT reachable ({})", "\u{2718}", ts_url)); }

        match crate::credentials::load_credentials() {
            Some((user, _)) => results.push(format!("{} Saved credentials: user='{}'", "\u{2714}", user)),
            None => results.push(format!("{} Saved credentials: none", "\u{2718}")),
        }

        let cookie_path = std::path::Path::new("cookies.txt");
        if cookie_path.exists() {
            match crate::search::cookies::load_from_file(cookie_path) {
                Ok(c) if !c.is_empty() => results.push(format!("{} Cookie file: {} cookies", "\u{2714}", c.len())),
                _ => results.push(format!("{} Cookie file: empty/invalid", "\u{26a0}")),
            }
        } else {
            results.push(format!("{} Cookie file: not found", "\u{26a0}"));
        }

        results.push("".into());
        results.push("Press Esc to close".into());
        results
    }

    pub fn close_login_modal(&mut self) {
        self.modal = Modal::None;
    }

    pub fn login_modal_key(&mut self, key: crossterm::event::KeyEvent) -> Option<(String, String)> {
        if let Modal::Login(ref mut state) = self.modal {
            match key.code {
                crossterm::event::KeyCode::Esc => {
                    self.modal = Modal::None;
                    return None;
                }
                crossterm::event::KeyCode::Tab => {
                    state.focus = match state.focus {
                        LoginField::Username => LoginField::Password,
                        LoginField::Password => LoginField::Username,
                    };
                }
                crossterm::event::KeyCode::Enter => {
                    if !state.username.is_empty() && !state.password.is_empty() {
                        let result = (state.username.clone(), state.password.clone());
                        self.modal = Modal::None;
                        return Some(result);
                    }
                }
                crossterm::event::KeyCode::Char(c) if !key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                    match state.focus {
                        LoginField::Username => state.username.push(c),
                        LoginField::Password => state.password.push(c),
                    }
                }
                crossterm::event::KeyCode::Backspace => {
                    match state.focus {
                        LoginField::Username => { state.username.pop(); }
                        LoginField::Password => { state.password.pop(); }
                    }
                }
                _ => {}
            }
        }
        None
    }

    pub fn enter_input_mode(&mut self) {
        self.input_mode = true;
    }

    pub fn exit_input_mode(&mut self) {
        self.input_mode = false;
    }

    pub fn type_char(&mut self, c: char) {
        if self.input_mode {
            self.search_input.push(c);
        }
    }

    pub fn backspace(&mut self) {
        if self.input_mode {
            self.search_input.pop();
        }
    }

    pub fn clear_input(&mut self) {
        self.search_input.clear();
    }

    pub fn delete_word(&mut self) {
        let words: Vec<&str> = self.search_input.split_whitespace().collect();
        if let Some(last) = words.last() {
            let cut_pos = self.search_input.len() - last.len();
            self.search_input.truncate(cut_pos);
        }
    }

    pub fn navigate_down(&mut self) -> bool {
        if !self.results.is_empty() && !self.input_mode && self.modal == Modal::None {
            let filtered_len = self.filtered_indices.len();
            if filtered_len == 0 { return false; }
            let local_idx = self.filtered_indices.iter().position(|&i| i == self.selected).unwrap_or(0);
            if local_idx < filtered_len - 1 {
                self.selected = self.filtered_indices[local_idx + 1];
                true
            } else if !self.all_loaded && self.state == AppState::Idle {
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    pub fn needs_more(&self) -> bool {
        self.search_query.is_some()
            && !self.all_loaded
            && self.state == AppState::Idle
            && self.selected >= self.results.len().saturating_sub(3)
            && !self.results.is_empty()
    }

    pub fn navigate_up(&mut self) -> bool {
        if !self.input_mode && self.modal == Modal::None {
            let local_idx = self.filtered_indices.iter().position(|&i| i == self.selected).unwrap_or(0);
            if local_idx > 0 {
                self.selected = self.filtered_indices[local_idx - 1];
            }
            true
        } else {
            false
        }
    }

    pub fn navigate_first(&mut self) {
        if let Some(&first) = self.filtered_indices.first() {
            self.selected = first;
        }
    }

    pub fn navigate_last(&mut self) {
        if let Some(&last) = self.filtered_indices.last() {
            self.selected = last;
        }
    }

    pub fn quit(&mut self) {
        self.running = false;
    }

    pub fn submit_search(&mut self) -> Option<String> {
        if self.input_mode {
            let query = self.search_input.clone();
            self.input_mode = false;
            if !query.is_empty() {
                Some(query)
            } else {
                None
            }
        } else {
            None
        }
    }

    pub fn submit_selection(&self) -> Option<usize> {
        if !self.input_mode && self.modal == Modal::None && !self.filtered_indices.is_empty() {
            Some(self.selected)
        } else {
            None
        }
    }

    pub fn update_filter(&mut self) {
        let filter = self.zones.filter_input.clone();
        if filter.is_empty() {
            self.filtered_indices = (0..self.results.len()).collect();
        } else {
            let lower = filter.to_lowercase();
            self.filtered_indices = self.results.iter()
                .enumerate()
                .filter(|(_, item)| item.title.to_lowercase().contains(&lower))
                .map(|(i, _)| i)
                .collect();
        }
        if !self.filtered_indices.is_empty() && !self.filtered_indices.contains(&self.selected) {
            self.selected = self.filtered_indices[0];
        }
    }

    pub fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();

        if self.show_menu {
            self.render_menu_view(frame, area);
            return;
        }

        self.render_main_view(frame, area);
    }

    fn render_menu_view(&mut self, frame: &mut Frame, area: Rect) {
        self.zones.update_areas(area);
        self.render_search_bar(frame, area);
        for zone_id in ZoneId::all() {
            let zone_area = self.zones.get_area(*zone_id);
            if zone_area.width == 0 || zone_area.height == 0 { continue; }
            let border_color = super::zones::zone_border_color(*zone_id, self.zones.focused, &self.theme);
            let title = super::zones::zone_title(*zone_id, &self.theme);
            match zone_id {
                ZoneId::Results => self.render_results_zone(frame, zone_area, border_color, title),
                ZoneId::Torrent => self.render_torrent_zone(frame, zone_area, border_color, title),
                ZoneId::Log => self.render_log_zone(frame, zone_area, border_color, title),
                ZoneId::Extra => self.render_extra_zone(frame, zone_area, border_color, title),
            }
        }
        super::menu::render_menu(frame, area, &self.menu, &self.theme);
    }

    fn render_main_view(&mut self, frame: &mut Frame, area: Rect) {
        self.zones.update_areas(area);

        if self.detail_log_mode {
            self.render_full_log(frame, area);
        } else {
            self.render_search_bar(frame, area);

            for zone_id in ZoneId::all() {
                let zone_area = self.zones.get_area(*zone_id);
                if zone_area.width == 0 || zone_area.height == 0 {
                    continue;
                }

                let border_color = super::zones::zone_border_color(*zone_id, self.zones.focused, &self.theme);
                let title = super::zones::zone_title(*zone_id, &self.theme);

                match zone_id {
                    ZoneId::Results => self.render_results_zone(frame, zone_area, border_color, title),
                    ZoneId::Torrent => self.render_torrent_zone(frame, zone_area, border_color, title),
                    ZoneId::Log => self.render_log_zone(frame, zone_area, border_color, title),
                    ZoneId::Extra => self.render_extra_zone(frame, zone_area, border_color, title),
                }
            }
        }

        if self.modal != Modal::None {
            self.render_modal(frame, area);
        }
    }

    fn render_search_bar(&self, frame: &mut Frame, area: Rect) {
        let bar_area = Rect::new(area.x, area.y, area.width, 3);

        let filter_hint = if self.zones.filter_mode {
            format!(" [F] filter: {} ", self.zones.filter_input)
        } else if !self.zones.filter_input.is_empty() {
            format!(" [F] filter: {} ", self.zones.filter_input)
        } else {
            String::new()
        };

        let header = format!(
            "[{}] {} | {}{}",
            self.browser_info,
            self.torrserver_url,
            if self.input_mode { "INPUT (s/i)" } else { "s: search | a: login | S: settings | L: log | F: filter" },
            filter_hint
        );

        let input_border = Block::default()
            .borders(Borders::ALL)
            .title(header)
            .border_style(Style::default().fg(if self.input_mode {
                Color::Yellow
            } else if self.zones.filter_mode {
                Color::Cyan
            } else {
                self.theme.div_line.to_color()
            }));

        let input = Paragraph::new(self.search_input.as_str())
            .block(input_border)
            .style(Style::default().fg(Color::White));

        frame.render_widget(input, bar_area);
    }

    fn render_results_zone(&self, frame: &mut Frame, area: Rect, border_color: Color, title: String) {
        let header = Row::new(vec![
            Cell::from("Seeds"),
            Cell::from("Size"),
            Cell::from("Date"),
            Cell::from("Title"),
        ])
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));

        let rows: Vec<Row> = self.filtered_indices.iter()
            .filter_map(|&idx| self.results.get(idx))
            .map(|item| {
                Row::new(vec![
                    Cell::from(item.seeds.as_str()),
                    Cell::from(item.size.as_str()),
                    Cell::from(item.date.as_str()),
                    Cell::from(item.title.as_str()),
                ])
            })
            .collect();

        let filter_info = if !self.zones.filter_input.is_empty() {
            format!(" [F: {}] ({}/{})", self.zones.filter_input, self.filtered_indices.len(), self.results.len())
        } else {
            format!(" ({}/{})", self.filtered_indices.len(), self.results.len())
        };

        let table = Table::new(
            rows,
            [
                Constraint::Length(6),
                Constraint::Length(8),
                Constraint::Length(8),
                Constraint::Min(20),
            ],
        )
        .header(header)
        .block(Block::default()
            .borders(Borders::ALL)
            .title(format!("{}{}", title, filter_info))
            .border_style(Style::default().fg(border_color)))
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

        let mut state = TableState::default();
        if let Some(local_pos) = self.filtered_indices.iter().position(|&i| i == self.selected) {
            state.select(Some(local_pos));
        }
        frame.render_stateful_widget(table, area, &mut state);
    }

    fn render_torrent_zone(&self, frame: &mut Frame, area: Rect, border_color: Color, title: String) {
        let s = &self.torrent_status;

        let progress_pct = (s.progress * 100.0) as u32;
        let bar_width = (area.width as usize).saturating_sub(4).min(50);
        let filled = (progress_pct as usize * bar_width / 100).min(bar_width);
        let empty = bar_width.saturating_sub(filled);

        let dl_speed = format_bytes(s.download_speed);
        let ul_speed = format_bytes(s.upload_speed);
        let dl_total = format_bytes(s.downloaded);
        let total = format_bytes(s.total_size);

        let lines = vec![
            Line::from(vec![
                Span::styled("Hash: ", Style::default().fg(Color::Yellow)),
                Span::raw(&s.hash),
                Span::styled("  Status: ", Style::default().fg(Color::Yellow)),
                Span::raw(&s.status),
            ]),
            Line::from(vec![
                Span::styled("Progress: ", Style::default().fg(Color::Yellow)),
                Span::styled(
                    format!("[{}{}] {}%", "\u{2588}".repeat(filled), "\u{2591}".repeat(empty), progress_pct),
                    Style::default().fg(if progress_pct >= 100 { Color::Green } else { Color::Cyan }),
                ),
            ]),
            Line::from(vec![
                Span::styled("DL: ", Style::default().fg(Color::Green)),
                Span::raw(&dl_speed),
                Span::raw("  "),
                Span::styled("UL: ", Style::default().fg(Color::Blue)),
                Span::raw(&ul_speed),
            ]),
            Line::from(vec![
                Span::styled("Downloaded: ", Style::default().fg(Color::Yellow)),
                Span::raw(&dl_total),
                Span::raw(" / "),
                Span::raw(&total),
                Span::styled("  Seeds: ", Style::default().fg(Color::Yellow)),
                Span::raw(s.seeds.to_string()),
                Span::styled("  Peers: ", Style::default().fg(Color::Yellow)),
                Span::raw(s.peers.to_string()),
            ]),
        ];

        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(border_color));

        let paragraph = Paragraph::new(lines).block(block);
        frame.render_widget(paragraph, area);
    }

    fn render_log_zone(&self, frame: &mut Frame, area: Rect, border_color: Color, title: String) {
        let total = self.logs.len();
        let visible = (area.height as usize).saturating_sub(2);
        let offset = self.log_scroll.saturating_sub(visible);

        let visible_logs: Vec<Line> = self.logs
            .iter()
            .skip(offset)
            .take(visible)
            .map(|l| Line::from(l.as_str()))
            .collect();

        let scroll_title = if total > 0 {
            format!("{} ({}/{})", title, offset + visible.min(total), total)
        } else {
            title
        };

        let log_panel = Paragraph::new(visible_logs)
            .block(Block::default().borders(Borders::ALL).title(scroll_title).border_style(Style::default().fg(border_color)));

        frame.render_widget(log_panel, area);
    }

    fn render_extra_zone(&self, frame: &mut Frame, area: Rect, border_color: Color, title: String) {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(border_color));

        let paragraph = Paragraph::new("Zone 4 — TBD").block(block);
        frame.render_widget(paragraph, area);
    }

    fn render_full_log(&self, frame: &mut Frame, area: Rect) {
        let total = self.detail_logs.len();
        let visible = (area.height as usize).saturating_sub(2);
        let scroll = self.detail_log_scroll.saturating_sub(visible);

        let lines: Vec<Line> = self.detail_logs
            .iter()
            .skip(scroll)
            .take(visible)
            .map(|l| {
                if l.contains("ERROR") || l.contains("FAIL") || l.contains("error:") {
                    Line::from(Span::styled(l.as_str(), Style::default().fg(Color::Red)))
                } else if l.contains("OK") || l.contains("SUCCESS") || l.contains("logged in") {
                    Line::from(Span::styled(l.as_str(), Style::default().fg(Color::Green)))
                } else if l.contains("WARN") {
                    Line::from(Span::styled(l.as_str(), Style::default().fg(Color::Yellow)))
                } else {
                    Line::from(l.as_str())
                }
            })
            .collect();

        let title = format!(" Detailed Log ({}/{}) [L/Esc] close [j/k] scroll ", 
            scroll + visible.min(total), total);

        let log_panel = Paragraph::new(lines)
            .block(Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::Cyan)));

        frame.render_widget(log_panel, area);
    }

    fn render_modal(&self, frame: &mut Frame, area: Rect) {
        if let Modal::Login(ref state) = self.modal {
            let popup = centered_rect(50, 40, area);

            let overlay_block = Block::default()
                .style(Style::default().bg(Color::Black));
            frame.render_widget(overlay_block, popup);

            let block = Block::default()
                .title(" Login to Rutracker ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .style(Style::default().bg(Color::DarkGray));

            let inner = block.inner(popup);
            frame.render_widget(block, popup);

            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(1),
                    Constraint::Length(3),
                    Constraint::Length(1),
                    Constraint::Length(1),
                ])
                .split(inner);

            let user_style = if state.focus == LoginField::Username {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let pass_style = if state.focus == LoginField::Password {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let user_block = Block::default()
                .title("Username")
                .borders(Borders::ALL)
                .border_style(user_style)
                .style(Style::default().bg(Color::DarkGray));
            frame.render_widget(
                Paragraph::new(state.username.as_str())
                    .style(Style::default().bg(Color::DarkGray).fg(Color::White))
                    .block(user_block),
                rows[0],
            );

            let pass_display = if state.password.is_empty() {
                String::new()
            } else {
                "*".repeat(state.password.len())
            };

            let pass_block = Block::default()
                .title("Password")
                .borders(Borders::ALL)
                .border_style(pass_style)
                .style(Style::default().bg(Color::DarkGray));
            frame.render_widget(
                Paragraph::new(pass_display.as_str())
                    .style(Style::default().bg(Color::DarkGray).fg(Color::White))
                    .block(pass_block),
                rows[2],
            );

            frame.render_widget(
                Paragraph::new(Span::styled(
                    "[Tab] switch  [Enter] login  [Esc] cancel",
                    Style::default().fg(Color::DarkGray).bg(Color::DarkGray),
                )),
                rows[3],
            );
        } else if let Modal::Settings(ref state) = self.modal {
            let popup = centered_rect(80, 80, area);

            let overlay_block = Block::default()
                .style(Style::default().bg(Color::Black));
            frame.render_widget(overlay_block, popup);

            let main_block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(self.theme.hi_fg.to_color()))
                .style(Style::default().bg(self.theme.main_bg.to_color()));
            let inner = main_block.inner(popup);
            frame.render_widget(main_block, popup);

            let bw = inner.width as usize;
            let divider_col = 30.min(bw.saturating_sub(3));

            let tab_y = inner.y;
            let div_y = tab_y + 2;
            let content_y = div_y + 1;
            let content_h = inner.height.saturating_sub(4) as usize;

            let mut tab_line = String::new();
            let mut tab_styles: Vec<(usize, usize, bool)> = Vec::new();
            let mut pos = 2;
            for (i, cat) in state.categories.iter().enumerate() {
                let is_sel = i == state.selected_category;
                let label = if is_sel {
                    format!("[{}]", cat.name)
                } else {
                    format!("{}{}", i + 1, cat.name)
                };
                tab_styles.push((pos, label.len(), is_sel));
                tab_line.push_str(&label);
                for _ in label.len()..10 {
                    tab_line.push(' ');
                }
                pos += 10;
            }

            let hi_color = self.theme.hi_fg.to_color();
            let title_color = self.theme.title.to_color();
            let div_color = self.theme.div_line.to_color();
            let fg_color = self.theme.main_fg.to_color();

            let mut spans = Vec::new();
            let chars: Vec<char> = tab_line.chars().collect();
            let mut ci = 0;
            for (start, len, is_sel) in &tab_styles {
                while ci < chars.len() && ci < *start + *len {
                    let ch = chars[ci].to_string();
                    let style = if *is_sel {
                        Style::default().fg(hi_color).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(title_color)
                    };
                    spans.push(Span::styled(ch, style));
                    ci += 1;
                }
                while ci < chars.len() && ci < *start + 10 {
                    ci += 1;
                }
            }
            frame.render_widget(
                Paragraph::new(Line::from(spans)).style(Style::default().bg(self.theme.main_bg.to_color())),
                Rect::new(inner.x, tab_y, inner.width, 1),
            );

            let mut div_spans: Vec<Span> = Vec::new();
            div_spans.push(Span::styled("├", Style::default().fg(hi_color)));
            for _ in 1..divider_col {
                div_spans.push(Span::styled("─", Style::default().fg(div_color)));
            }
            div_spans.push(Span::styled("┬", Style::default().fg(hi_color)));
            for _ in divider_col + 1..bw.saturating_sub(1) {
                div_spans.push(Span::styled("─", Style::default().fg(div_color)));
            }
            div_spans.push(Span::styled("┤", Style::default().fg(hi_color)));
            frame.render_widget(
                Paragraph::new(Line::from(div_spans)).style(Style::default().bg(self.theme.main_bg.to_color())),
                Rect::new(inner.x, div_y, inner.width, 1),
            );

            for row in 0..content_h {
                frame.render_widget(
                    Paragraph::new(Span::styled("│", Style::default().fg(div_color)))
                        .style(Style::default().bg(self.theme.main_bg.to_color())),
                    Rect::new(inner.x + divider_col as u16, content_y + row as u16, 1, 1),
                );
            }

            let cat = &state.categories[state.selected_category];
            let visible_items = content_h / 2;
            let page = state.page;
            let start_idx = page * visible_items;

            let left_x = inner.x + 1;
            let right_x = inner.x + divider_col as u16 + 2;
            let right_w = bw.saturating_sub(divider_col as usize + 3) as u16;

            for row_idx in 0..visible_items {
                let item_idx = start_idx + row_idx;
                let y = content_y + (row_idx * 2) as u16;

                if item_idx < cat.items.len() {
                    let item = &cat.items[item_idx];
                    let is_sel = item_idx == state.selected;

                    let label = if is_sel {
                        format!("{} 3/{}", item.label, cat.items.len())
                    } else {
                        item.label.clone()
                    };
                    let label_style = if is_sel {
                        Style::default().fg(hi_color).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(title_color)
                    };
                    let centered_label = center_str(&label, divider_col as usize - 2);
                    frame.render_widget(
                        Paragraph::new(Span::styled(centered_label, label_style))
                            .style(Style::default().bg(self.theme.main_bg.to_color())),
                        Rect::new(left_x, y, divider_col as u16 - 1, 1),
                    );

                    let val_style = if is_sel {
                        Style::default().fg(fg_color)
                    } else {
                        Style::default().fg(fg_color)
                    };
                    let val_display = if is_sel {
                        format!("← {} →", item.value)
                    } else {
                        item.value.clone()
                    };
                    let centered_val = center_str(&val_display, divider_col as usize - 2);
                    frame.render_widget(
                        Paragraph::new(Span::styled(centered_val, val_style))
                            .style(Style::default().bg(self.theme.main_bg.to_color())),
                        Rect::new(left_x, y + 1, divider_col as u16 - 1, 1),
                    );
                }
            }

            if let Some(item) = cat.items.get(state.selected) {
                let desc_style = Style::default().fg(fg_color);
                for (i, line) in item.description.iter().enumerate() {
                    if (content_y as usize + i) < (content_y as usize + content_h) {
                        frame.render_widget(
                            Paragraph::new(Span::styled(line.as_str(), desc_style))
                                .style(Style::default().bg(self.theme.main_bg.to_color())),
                            Rect::new(right_x, content_y + i as u16, right_w, 1),
                        );
                    }
                }
            }

            let pages = (cat.items.len() + visible_items - 1) / visible_items;
            if pages > 1 {
                let page_line = format!("↑ page {}/{} ↓", page + 1, pages);
                let page_y = content_y + content_h as u16;
                let page_x = inner.x + (bw / 2).saturating_sub(page_line.len() / 2) as u16;
                frame.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled("┘", Style::default().fg(hi_color)),
                        Span::styled("↑ ", Style::default().fg(hi_color)),
                        Span::styled(format!("page {}/{} ", page + 1, pages), Style::default().fg(title_color)),
                        Span::styled("↓", Style::default().fg(hi_color)),
                        Span::styled("└", Style::default().fg(hi_color)),
                    ])).style(Style::default().bg(self.theme.main_bg.to_color())),
                    Rect::new(page_x.saturating_sub(1), page_y, page_line.len() as u16 + 4, 1),
                );
            }
        } else if let Modal::HealthCheck(ref lines) = self.modal {
            let popup = centered_rect(70, 80, area);

            let overlay_block = Block::default()
                .style(Style::default().bg(Color::Black));
            frame.render_widget(overlay_block, popup);

            let block = Block::default()
                .title(" Health Check ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Green))
                .style(Style::default().bg(Color::DarkGray));

            let inner = block.inner(popup);
            frame.render_widget(block, popup);

            let display_lines: Vec<Line> = lines.iter().map(|l| {
                if l.contains("\u{2714}") {
                    Line::from(Span::styled(l.as_str(), Style::default().fg(Color::Green)))
                } else if l.contains("\u{2718}") {
                    Line::from(Span::styled(l.as_str(), Style::default().fg(Color::Red)))
                } else if l.contains("\u{26a0}") {
                    Line::from(Span::styled(l.as_str(), Style::default().fg(Color::Yellow)))
                } else if l.starts_with("===") {
                    Line::from(Span::styled(l.as_str(), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
                } else {
                    Line::from(l.as_str())
                }
            }).collect();

            let list = Paragraph::new(display_lines)
                .style(Style::default().bg(Color::DarkGray));
            frame.render_widget(list, inner);
        }
    }
}

fn center_str(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len >= width {
        s.to_string()
    } else {
        let pad = width - len;
        let left = pad / 2;
        let right = pad - left;
        format!("{}{}{}", " ".repeat(left), s, " ".repeat(right))
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
