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
