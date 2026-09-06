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
    pub state: AppState,
    pub browser_info: String,
    pub torrserver_url: String,
    pub running: bool,
    pub input_mode: bool,
    pub modal: Modal,
}

impl App {
    pub fn new(torrserver_url: String, browser_info: String) -> Self {
        Self {
            search_input: String::new(),
            results: Vec::new(),
            selected: 0,
            logs: VecDeque::new(),
            log_scroll: 0,
            state: AppState::Idle,
            browser_info,
            torrserver_url,
            running: true,
            input_mode: false,
            modal: Modal::None,
        }
    }

    pub fn add_log(&mut self, msg: &str) {
        self.logs.push_back(format!("[{}] {}", chrono::Local::now().format("%H:%M:%S"), msg));
        if self.logs.len() > 500 {
            self.logs.pop_front();
        }
        self.log_scroll = self.logs.len();
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
            self.selected = (self.selected + 1).min(self.results.len() - 1);
            true
        } else {
            false
        }
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
            if self.input_mode { "INPUT MODE (s/i)" } else { "s: search | l: login" }
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

    fn render_modal(&self, frame: &mut Frame, area: Rect) {
        if let Modal::Login(ref state) = self.modal {
            let popup = centered_rect(50, 40, area);

            let block = Block::default()
                .title(" Login to Rutracker ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow));

            let inner = block.inner(popup);
            frame.render_widget(block, popup);

            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Length(1),
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
                .title("Username (Tab to switch)")
                .borders(Borders::ALL)
                .border_style(user_style);
            frame.render_widget(
                Paragraph::new(state.username.as_str()).block(user_block),
                rows[0],
            );

            frame.render_widget(
                Paragraph::new(Span::styled(
                    "[Tab] switch  [Enter] login  [Esc] cancel",
                    Style::default().fg(Color::DarkGray),
                )),
                rows[1],
            );

            let pass_block = Block::default()
                .title("Password (Tab to switch)")
                .borders(Borders::ALL)
                .border_style(pass_style);
            let pass_display = "*".repeat(state.password.len());
            frame.render_widget(
                Paragraph::new(pass_display.as_str()).block(pass_block),
                rows[2],
            );

            if let Some(ref msg) = state.message {
                let color = if msg.starts_with("OK") { Color::Green } else { Color::Red };
                frame.render_widget(
                    Paragraph::new(Span::styled(msg.as_str(), Style::default().fg(color))),
                    rows[4],
                );
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app() -> App {
        App::new("http://127.0.0.1:8090".into(), "helium".into())
    }

    fn app_with_results(n: usize) -> App {
        let mut app = test_app();
        app.results = (0..n)
            .map(|i| TorrentItem {
                title: format!("Torrent {}", i),
                size: "1 GB".into(),
                seeds: format!("{}", i),
                date: "".into(),
                download_url: format!("/dl.php?t={}", i),
                page_url: "".into(),
                query: "".into(),
            })
            .collect();
        app
    }

    #[test]
    fn test_app_initial_state() {
        let app = test_app();
        assert!(app.search_input.is_empty());
        assert!(app.results.is_empty());
        assert_eq!(app.selected, 0);
        assert!(app.running);
        assert!(!app.input_mode);
        assert_eq!(app.modal, Modal::None);
    }

    #[test]
    fn test_add_log() {
        let mut app = test_app();
        app.add_log("first message");
        app.add_log("second message");
        assert_eq!(app.logs.len(), 2);
        assert!(app.logs[0].contains("first message"));
        assert!(app.logs[1].contains("second message"));
    }

    #[test]
    fn test_add_log_timestamp_format() {
        let mut app = test_app();
        app.add_log("test");
        assert!(app.logs[0].starts_with('['));
        assert!(app.logs[0].contains("] test"));
    }

    #[test]
    fn test_add_log_buffer_limit() {
        let mut app = test_app();
        for i in 0..600 {
            app.add_log(&format!("msg {}", i));
        }
        assert_eq!(app.logs.len(), 500);
    }

    #[test]
    fn test_log_scroll() {
        let mut app = test_app();
        for i in 0..20 {
            app.add_log(&format!("msg {}", i));
        }
        assert_eq!(app.log_scroll, 20);
        app.scroll_logs_up();
        assert_eq!(app.log_scroll, 19);
        app.scroll_logs_down();
        assert_eq!(app.log_scroll, 20);
        app.scroll_logs_page_up();
        assert_eq!(app.log_scroll, 10);
        app.scroll_logs_page_down();
        assert_eq!(app.log_scroll, 20);
    }

    #[test]
    fn test_mouse_scroll_logs() {
        let mut app = test_app();
        for i in 0..30 {
            app.add_log(&format!("msg {}", i));
        }
        app.mouse_scroll_logs(5);
        assert_eq!(app.log_scroll, 25);
        app.mouse_scroll_logs(-3);
        assert_eq!(app.log_scroll, 28);
    }

    #[test]
    fn test_type_char() {
        let mut app = test_app();
        app.enter_input_mode();
        app.type_char('h');
        app.type_char('i');
        assert_eq!(app.search_input, "hi");
    }

    #[test]
    fn test_type_char_not_in_input_mode() {
        let mut app = test_app();
        app.type_char('h');
        assert!(app.search_input.is_empty());
    }

    #[test]
    fn test_backspace() {
        let mut app = test_app();
        app.enter_input_mode();
        app.type_char('a');
        app.type_char('b');
        app.backspace();
        assert_eq!(app.search_input, "a");
    }

    #[test]
    fn test_delete_word() {
        let mut app = test_app();
        app.enter_input_mode();
        for c in "hello world".chars() {
            app.type_char(c);
        }
        app.delete_word();
        assert_eq!(app.search_input, "hello ");
    }

    #[test]
    fn test_navigate_down() {
        let mut app = app_with_results(5);
        assert!(app.navigate_down());
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn test_navigate_down_clamps() {
        let mut app = app_with_results(3);
        app.selected = 2;
        assert!(app.navigate_down());
        assert_eq!(app.selected, 2);
    }

    #[test]
    fn test_navigate_up() {
        let mut app = app_with_results(5);
        app.selected = 3;
        assert!(app.navigate_up());
        assert_eq!(app.selected, 2);
    }

    #[test]
    fn test_navigate_up_clamps() {
        let mut app = app_with_results(5);
        assert!(app.navigate_up());
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn test_navigate_blocked_in_input_mode() {
        let mut app = app_with_results(5);
        app.enter_input_mode();
        assert!(!app.navigate_down());
        assert!(!app.navigate_up());
    }

    #[test]
    fn test_navigate_blocked_in_modal() {
        let mut app = app_with_results(5);
        app.open_login_modal();
        assert!(!app.navigate_down());
        assert!(!app.navigate_up());
        assert_eq!(app.submit_selection(), None);
    }

    #[test]
    fn test_submit_search() {
        let mut app = test_app();
        app.enter_input_mode();
        for c in "ubuntu".chars() {
            app.type_char(c);
        }
        assert_eq!(app.submit_search(), Some("ubuntu".into()));
        assert!(!app.input_mode);
    }

    #[test]
    fn test_submit_search_empty() {
        let mut app = test_app();
        app.enter_input_mode();
        assert_eq!(app.submit_search(), None);
    }

    #[test]
    fn test_submit_selection() {
        let mut app = app_with_results(5);
        app.selected = 2;
        assert_eq!(app.submit_selection(), Some(2));
    }

    #[test]
    fn test_submit_selection_empty() {
        let mut app = test_app();
        assert_eq!(app.submit_selection(), None);
    }

    #[test]
    fn test_quit() {
        let mut app = test_app();
        app.quit();
        assert!(!app.running);
    }

    #[test]
    fn test_login_modal_open_close() {
        let mut app = test_app();
        app.open_login_modal();
        assert!(matches!(app.modal, Modal::Login(_)));
        app.close_login_modal();
        assert_eq!(app.modal, Modal::None);
    }

    #[test]
    fn test_login_modal_typing() {
        let mut app = test_app();
        app.open_login_modal();

        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('b'),
            crossterm::event::KeyModifiers::NONE,
        );
        app.login_modal_key(key);

        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('l'),
            crossterm::event::KeyModifiers::NONE,
        );
        app.login_modal_key(key);

        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('a'),
            crossterm::event::KeyModifiers::NONE,
        );
        app.login_modal_key(key);

        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('c'),
            crossterm::event::KeyModifiers::NONE,
        );
        app.login_modal_key(key);

        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('k'),
            crossterm::event::KeyModifiers::NONE,
        );
        app.login_modal_key(key);

        if let Modal::Login(ref state) = app.modal {
            assert_eq!(state.username, "black");
            assert_eq!(state.focus, LoginField::Username);
        } else {
            panic!("Expected login modal");
        }
    }

    #[test]
    fn test_login_modal_tab_switches_field() {
        let mut app = test_app();
        app.open_login_modal();

        if let Modal::Login(ref mut state) = app.modal {
            assert_eq!(state.focus, LoginField::Username);
        }

        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Tab,
            crossterm::event::KeyModifiers::NONE,
        );
        app.login_modal_key(key);

        if let Modal::Login(ref state) = app.modal {
            assert_eq!(state.focus, LoginField::Password);
        }

        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Tab,
            crossterm::event::KeyModifiers::NONE,
        );
        app.login_modal_key(key);

        if let Modal::Login(ref state) = app.modal {
            assert_eq!(state.focus, LoginField::Username);
        }
    }

    #[test]
    fn test_login_modal_enter_submits() {
        let mut app = test_app();
        app.open_login_modal();

        for c in "user".chars() {
            let key = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(c),
                crossterm::event::KeyModifiers::NONE,
            );
            app.login_modal_key(key);
        }

        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Tab,
            crossterm::event::KeyModifiers::NONE,
        );
        app.login_modal_key(key);

        for c in "pass123".chars() {
            let key = crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char(c),
                crossterm::event::KeyModifiers::NONE,
            );
            app.login_modal_key(key);
        }

        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        );
        let result = app.login_modal_key(key);

        assert_eq!(result, Some(("user".into(), "pass123".into())));
        assert_eq!(app.modal, Modal::None);
    }

    #[test]
    fn test_login_modal_enter_empty_fails() {
        let mut app = test_app();
        app.open_login_modal();

        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        );
        let result = app.login_modal_key(key);

        assert_eq!(result, None);
        assert!(matches!(app.modal, Modal::Login(_)));
    }

    #[test]
    fn test_login_modal_esc_closes() {
        let mut app = test_app();
        app.open_login_modal();

        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Esc,
            crossterm::event::KeyModifiers::NONE,
        );
        app.login_modal_key(key);

        assert_eq!(app.modal, Modal::None);
    }

    #[test]
    fn test_login_modal_backspace() {
        let mut app = test_app();
        app.open_login_modal();

        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Char('x'),
            crossterm::event::KeyModifiers::NONE,
        );
        app.login_modal_key(key);

        let key = crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Backspace,
            crossterm::event::KeyModifiers::NONE,
        );
        app.login_modal_key(key);

        if let Modal::Login(ref state) = app.modal {
            assert!(state.username.is_empty());
        }
    }

    #[test]
    fn test_full_flow() {
        let mut app = test_app();
        app.enter_input_mode();
        for c in "world war".chars() {
            app.type_char(c);
        }
        assert_eq!(app.submit_search(), Some("world war".into()));
        app.results = (0..50)
            .map(|i| TorrentItem {
                title: format!("Result {}", i),
                size: "1 GB".into(),
                seeds: format!("{}", i),
                date: "".into(),
                download_url: format!("/dl.php?t={}", i),
                page_url: "".into(),
                query: "world war".into(),
            })
            .collect();
        for _ in 0..10 {
            app.navigate_down();
        }
        assert_eq!(app.submit_selection(), Some(10));
    }

    #[test]
    fn test_render_does_not_panic() {
        let mut app = test_app();
        app.results = app_with_results(10).results;
        app.add_log("test");

        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
    }

    #[test]
    fn test_render_with_modal() {
        let mut app = test_app();
        app.open_login_modal();

        let backend = ratatui::backend::TestBackend::new(120, 40);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
    }
}
