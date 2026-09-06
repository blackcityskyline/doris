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

#[derive(Debug, PartialEq)]
pub enum KeyAction {
    None,
    StartSearch(String),
    StartStream(usize),
    Quit,
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
        if !self.results.is_empty() && !self.input_mode {
            self.selected = (self.selected + 1).min(self.results.len() - 1);
            true
        } else {
            false
        }
    }

    pub fn navigate_up(&mut self) -> bool {
        if !self.input_mode {
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
        if !self.input_mode && self.selected < self.results.len() {
            Some(self.selected)
        } else {
            None
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(5),
                Constraint::Min(5),
            ])
            .split(frame.area());

        self.render_search_bar(frame, chunks[0]);
        self.render_results(frame, chunks[1]);
        self.render_logs(frame, chunks[2]);
    }

    fn render_search_bar(&self, frame: &mut Frame, area: Rect) {
        let header = format!(
            "[{}] {} | {}",
            self.browser_info,
            self.torrserver_url,
            if self.input_mode { "INPUT MODE" } else { "" }
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
            format!("Logs ({}/{})", offset + visible.min(total), total)
        } else {
            "Logs".to_string()
        };

        let log_panel = Paragraph::new(visible_logs)
            .block(Block::default().borders(Borders::ALL).title(scroll_title));

        frame.render_widget(log_panel, area);
    }
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

    // === Input mode ===

    #[test]
    fn test_enter_input_mode() {
        let mut app = test_app();
        assert!(!app.input_mode);
        app.enter_input_mode();
        assert!(app.input_mode);
    }

    #[test]
    fn test_exit_input_mode() {
        let mut app = test_app();
        app.enter_input_mode();
        assert!(app.input_mode);
        app.exit_input_mode();
        assert!(!app.input_mode);
    }

    // === Typing ===

    #[test]
    fn test_type_char_in_input_mode() {
        let mut app = test_app();
        app.enter_input_mode();
        app.type_char('h');
        app.type_char('e');
        app.type_char('l');
        app.type_char('l');
        app.type_char('o');
        assert_eq!(app.search_input, "hello");
    }

    #[test]
    fn test_type_char_not_in_input_mode_ignored() {
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
        app.type_char('c');
        app.backspace();
        assert_eq!(app.search_input, "ab");
        app.backspace();
        assert_eq!(app.search_input, "a");
        app.backspace();
        assert_eq!(app.search_input, "");
        app.backspace();
        assert_eq!(app.search_input, "");
    }

    #[test]
    fn test_backspace_not_in_input_mode_ignored() {
        let mut app = test_app();
        app.search_input = "test".into();
        app.backspace();
        assert_eq!(app.search_input, "test");
    }

    #[test]
    fn test_clear_input() {
        let mut app = test_app();
        app.enter_input_mode();
        app.type_char('x');
        app.type_char('y');
        app.clear_input();
        assert!(app.search_input.is_empty());
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
    fn test_delete_word_single() {
        let mut app = test_app();
        app.enter_input_mode();
        for c in "test".chars() {
            app.type_char(c);
        }
        app.delete_word();
        assert_eq!(app.search_input, "");
    }

    #[test]
    fn test_delete_word_empty() {
        let mut app = test_app();
        app.enter_input_mode();
        app.delete_word();
        assert!(app.search_input.is_empty());
    }

    // === Navigation ===

    #[test]
    fn test_navigate_down() {
        let mut app = app_with_results(5);
        assert_eq!(app.selected, 0);
        assert!(app.navigate_down());
        assert_eq!(app.selected, 1);
        assert!(app.navigate_down());
        assert_eq!(app.selected, 2);
    }

    #[test]
    fn test_navigate_down_clamps() {
        let mut app = app_with_results(3);
        app.selected = 2;
        assert!(app.navigate_down());
        assert_eq!(app.selected, 2);
    }

    #[test]
    fn test_navigate_down_empty() {
        let mut app = test_app();
        assert!(!app.navigate_down());
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn test_navigate_down_in_input_mode_ignored() {
        let mut app = app_with_results(5);
        app.enter_input_mode();
        assert!(!app.navigate_down());
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn test_navigate_up() {
        let mut app = app_with_results(5);
        app.selected = 3;
        assert!(app.navigate_up());
        assert_eq!(app.selected, 2);
        assert!(app.navigate_up());
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn test_navigate_up_clamps_at_zero() {
        let mut app = app_with_results(5);
        app.selected = 0;
        assert!(app.navigate_up());
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn test_navigate_up_in_input_mode_ignored() {
        let mut app = app_with_results(5);
        app.selected = 3;
        app.enter_input_mode();
        assert!(!app.navigate_up());
        assert_eq!(app.selected, 3);
    }

    #[test]
    fn test_navigate_first() {
        let mut app = app_with_results(5);
        app.selected = 4;
        app.navigate_first();
        assert_eq!(app.selected, 0);
    }

    #[test]
    fn test_navigate_last() {
        let mut app = app_with_results(5);
        app.navigate_last();
        assert_eq!(app.selected, 4);
    }

    #[test]
    fn test_navigate_last_empty() {
        let mut app = test_app();
        app.navigate_last();
        assert_eq!(app.selected, 0);
    }

    // === Submit ===

    #[test]
    fn test_submit_search_in_input_mode() {
        let mut app = test_app();
        app.enter_input_mode();
        app.type_char('u');
        app.type_char('b');
        app.type_char('u');
        app.type_char('n');
        app.type_char('t');
        app.type_char('u');
        let result = app.submit_search();
        assert_eq!(result, Some("ubuntu".into()));
        assert!(!app.input_mode);
    }

    #[test]
    fn test_submit_search_empty_query() {
        let mut app = test_app();
        app.enter_input_mode();
        let result = app.submit_search();
        assert_eq!(result, None);
        assert!(!app.input_mode);
    }

    #[test]
    fn test_submit_search_not_in_input_mode() {
        let mut app = test_app();
        app.search_input = "test".into();
        let result = app.submit_search();
        assert_eq!(result, None);
    }

    #[test]
    fn test_submit_selection_valid() {
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
    fn test_submit_selection_in_input_mode() {
        let mut app = app_with_results(5);
        app.selected = 2;
        app.enter_input_mode();
        assert_eq!(app.submit_selection(), None);
    }

    #[test]
    fn test_submit_selection_out_of_bounds() {
        let mut app = app_with_results(3);
        app.selected = 5;
        assert_eq!(app.submit_selection(), None);
    }

    // === Quit ===

    #[test]
    fn test_quit() {
        let mut app = test_app();
        assert!(app.running);
        app.quit();
        assert!(!app.running);
    }

    // === Full flow ===

    #[test]
    fn test_full_search_flow() {
        let mut app = test_app();

        app.enter_input_mode();
        assert!(app.input_mode);

        for c in "world war".chars() {
            app.type_char(c);
        }
        assert_eq!(app.search_input, "world war");

        let query = app.submit_search().unwrap();
        assert_eq!(query, "world war");
        assert!(!app.input_mode);

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

        assert_eq!(app.selected, 0);
        for _ in 0..10 {
            app.navigate_down();
        }
        assert_eq!(app.selected, 10);

        let idx = app.submit_selection().unwrap();
        assert_eq!(idx, 10);
    }

    #[test]
    fn test_esc_exits_input_mid_typing() {
        let mut app = test_app();
        app.enter_input_mode();
        app.type_char('h');
        app.type_char('e');
        app.exit_input_mode();
        assert!(!app.input_mode);
        assert_eq!(app.search_input, "he");
    }

    #[test]
    fn test_j_k_navigation_full_cycle() {
        let mut app = app_with_results(10);

        for _ in 0..9 {
            app.navigate_down();
        }
        assert_eq!(app.selected, 9);

        for _ in 0..9 {
            app.navigate_up();
        }
        assert_eq!(app.selected, 0);
    }
}
