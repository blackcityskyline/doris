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
            state: AppState::Idle,
            browser_info,
            torrserver_url,
            running: true,
            input_mode: false,
        }
    }

    pub fn add_log(&mut self, msg: &str) {
        self.logs.push_back(format!("[{}] {}", chrono::Local::now().format("%H:%M:%S"), msg));
        if self.logs.len() > 100 {
            self.logs.pop_front();
        }
    }

    pub fn render(&self, frame: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(5),
                Constraint::Length(8),
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
        let logs: Vec<Line> = self
            .logs
            .iter()
            .map(|l| Line::from(l.as_str()))
            .collect();

        let log_panel = Paragraph::new(logs)
            .block(Block::default().borders(Borders::ALL).title("Logs"));

        frame.render_widget(log_panel, area);
    }
}
