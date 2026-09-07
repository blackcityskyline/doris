use ratatui::prelude::*;
use ratatui::widgets::*;
use crate::search::models::TorrentItem;
use std::collections::VecDeque;

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
    pub selected: usize,
    pub items: Vec<SettingsItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SettingsItem {
    pub label: String,
    pub value: String,
    pub action: SettingsAction,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SettingsAction {
    ToggleHeadless,
    ToggleMode,
    SetDownloadDir,
    RunHealthCheck,
    OpenLog,
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
    pub headless: bool,
    pub stream_mode: bool,
    pub download_dir: String,
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
            headless: true,
            stream_mode: true,
            download_dir: dirs::download_dir()
                .map(|d| d.display().to_string())
                .unwrap_or_else(|| "/tmp".to_string()),
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

    pub fn click_results_at(&mut self, row: u16, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(5),
                Constraint::Length(8),
            ])
            .split(area);

        let results_area = chunks[1];
        if row >= results_area.y && row < results_area.y + results_area.height {
            let table_row = (row - results_area.y) as usize;
            if table_row == 0 {
                return;
            }
            let data_row = table_row - 1;
            let offset = self.selected.saturating_sub(
                (results_area.height as usize).saturating_sub(2) / 2,
            );
            let idx = offset + data_row;
            if idx < self.results.len() {
                self.selected = idx;
            }
        }
    }

    pub fn open_login_modal(&mut self) {
        self.modal = Modal::Login(LoginState::new());
    }

    pub fn open_settings(&mut self) {
        let headless_str = if self.headless { "Headless (hidden)" } else { "GUI (visible)" };
        let mode_str = if self.stream_mode { "Streaming (TorrServer)" } else { "Download (.torrent file)" };
        self.modal = Modal::Settings(SettingsState {
            selected: 0,
            items: vec![
                SettingsItem { label: "Browser mode".into(), value: headless_str.into(), action: SettingsAction::ToggleHeadless },
                SettingsItem { label: "Play mode".into(), value: mode_str.into(), action: SettingsAction::ToggleMode },
                SettingsItem { label: "Download folder".into(), value: self.download_dir.clone(), action: SettingsAction::SetDownloadDir },
                SettingsItem { label: "Open detailed log".into(), value: "L".into(), action: SettingsAction::OpenLog },
                SettingsItem { label: "Health check".into(), value: "press Enter".into(), action: SettingsAction::RunHealthCheck },
                SettingsItem { label: "Close".into(), value: "Esc".into(), action: SettingsAction::Close },
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
                    state.selected = (state.selected + 1).min(state.items.len() - 1);
                }
                crossterm::event::KeyCode::Char('k') | crossterm::event::KeyCode::Up => {
                    state.selected = state.selected.saturating_sub(1);
                }
                crossterm::event::KeyCode::Enter => {
                    if let Some(item) = state.items.get(state.selected) {
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
        else { results.push(format!("{} Xvfb: not found (needed for headless)", "\u{2718}")); }

        let has_chromedriver = std::path::Path::new(&dirs::data_local_dir()
            .unwrap_or_default().join("t-hunter").join("chromedriver_patched")).exists()
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
            if self.selected < self.results.len() - 1 {
                self.selected += 1;
            } else if !self.all_loaded && self.state == AppState::Idle {
                return true;
            }
            true
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
            self.selected = self.selected.saturating_sub(1);
            true
        } else {
            false
        }
    }

    pub fn navigate_first(&mut self) {
        self.selected = 0;
    }

    pub fn navigate_last(&mut self) {
        if !self.results.is_empty() {
            self.selected = self.results.len() - 1;
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
        if !self.input_mode && self.modal == Modal::None && self.selected < self.results.len() {
            Some(self.selected)
        } else {
            None
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        if self.detail_log_mode {
            self.render_full_log(frame, area);
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(5),
                Constraint::Length(8),
            ])
            .split(area);

        self.render_search_bar(frame, chunks[0]);
        self.render_results(frame, chunks[1]);
        self.render_logs(frame, chunks[2]);

        if self.modal != Modal::None {
            self.render_modal(frame, area);
        }
    }

    fn render_search_bar(&self, frame: &mut Frame, area: Rect) {
        let header = format!(
            "[{}] {} | {}",
            self.browser_info,
            self.torrserver_url,
            if self.input_mode { "INPUT MODE (s/i)" } else { "s: search | a: login | S: settings | L: log" }
        );

        let input_border = Block::default()
            .borders(Borders::ALL)
            .title(header)
            .border_style(Style::default().fg(if self.input_mode {
                Color::Yellow
            } else {
                Color::Green
            }));

        let input = Paragraph::new(self.search_input.as_str())
            .block(input_border)
            .style(Style::default().fg(Color::White));

        frame.render_widget(input, area);
    }

    fn render_results(&self, frame: &mut Frame, area: Rect) {
        let header = Row::new(vec![
            Cell::from("Seeds"),
            Cell::from("Size"),
            Cell::from("Date"),
            Cell::from("Title"),
        ])
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));

        let rows: Vec<Row> = self
            .results
            .iter()
            .map(|item| {
                Row::new(vec![
                    Cell::from(item.seeds.as_str()),
                    Cell::from(item.size.as_str()),
                    Cell::from(item.date.as_str()),
                    Cell::from(item.title.as_str()),
                ])
            })
            .collect();

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
        .block(Block::default().borders(Borders::ALL).title("Results"))
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED));

        let mut state = TableState::default();
        if !self.results.is_empty() {
            state.select(Some(self.selected));
        }
        frame.render_stateful_widget(table, area, &mut state);
    }

    fn render_logs(&self, frame: &mut Frame, area: Rect) {
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
            format!("Logs ({}/{}) [scroll: mouse/pgup/pgdn]", offset + visible.min(total), total)
        } else {
            "Logs".to_string()
        };

        let log_panel = Paragraph::new(visible_logs)
            .block(Block::default().borders(Borders::ALL).title(scroll_title))
            .scroll((0, 0));

        frame.render_widget(log_panel, area);
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
            let popup = centered_rect(60, 60, area);

            let overlay_block = Block::default()
                .style(Style::default().bg(Color::Black));
            frame.render_widget(overlay_block, popup);

            let block = Block::default()
                .title(" Settings ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .style(Style::default().bg(Color::DarkGray));

            let inner = block.inner(popup);
            frame.render_widget(block, popup);

            let mut lines: Vec<Line> = Vec::new();
            for (i, item) in state.items.iter().enumerate() {
                let marker = if i == state.selected { "> " } else { "  " };
                let style = if i == state.selected {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                lines.push(Line::from(vec![
                    Span::styled(marker, style),
                    Span::styled(&item.label, style),
                    Span::raw("  "),
                    Span::styled(&item.value, Style::default().fg(Color::DarkGray)),
                ]));
            }

            let help = Line::from(vec![
                Span::styled("[j/k] navigate  [Enter] select  [Esc] close", Style::default().fg(Color::DarkGray)),
            ]);
            lines.push(help);

            let list = Paragraph::new(lines)
                .style(Style::default().bg(Color::DarkGray));
            frame.render_widget(list, inner);
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
