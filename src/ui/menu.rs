use ratatui::prelude::*;
use ratatui::widgets::*;
use unicode_width::UnicodeWidthStr;
use super::theme::Theme;

const BANNER: &[&str] = &[
    "██████╗  ██████╗ ██████╗ ██╗███████╗",
    "██╔══██╗██╔═══██╗██╔══██╗██║██╔════╝",
    "██║  ██║██║   ██║██████╔╝██║███████╗",
    "██║  ██║██║   ██║██╔══██╗██║╚════██║",
    "██████╔╝╚██████╔╝██║  ██║██║███████║",
    "╚═════╝  ╚═════╝ ╚═╝  ╚═╝╚═╝╚══════╝",
];

const MENU_ITEMS: &[&[&str]] = &[
    &["┌─┐┌─┐╶┬╴╷┌─┐┌┐╷┌─┐", "│ │├─┘ │ ││ ││└┤└─┐", "└─┘╵   ╵ ╵└─┘╵ ╵└─┘"],
    &["╷ ╷┌─╴╷  ┌─┐", "├─┤├╴ │  ├─┘", "╵ ╵└─╴└─╴╵  "],
    &["┌─┐╷ ╷╷╶┬╴", "│┐││ ││ │ ", "└┴┘└─┘╵ ╵ "],
];

#[derive(Debug, Clone, PartialEq)]
pub enum MenuItem {
    Options,
    Help,
    Quit,
}

impl MenuItem {
    pub fn all() -> &'static [MenuItem] {
        &[MenuItem::Options, MenuItem::Help, MenuItem::Quit]
    }

    pub fn label(&self) -> &'static str {
        match self {
            MenuItem::Options => "OPTIONS",
            MenuItem::Help => "HELP",
            MenuItem::Quit => "QUIT",
        }
    }
}

pub struct MenuState {
    pub selected: usize,
    pub active: bool,
    pub show_help: bool,
}

impl MenuState {
    pub fn new() -> Self {
        Self {
            selected: 0,
            active: true,
            show_help: false,
        }
    }

    pub fn next(&mut self) {
        self.selected = (self.selected + 1) % MenuItem::all().len();
    }

    pub fn prev(&mut self) {
        self.selected = if self.selected == 0 {
            MenuItem::all().len() - 1
        } else {
            self.selected - 1
        };
    }

    pub fn select(&self) -> MenuItem {
        MenuItem::all()[self.selected].clone()
    }
}

pub fn render_menu(frame: &mut Frame, area: Rect, state: &MenuState, theme: &Theme) {
    let banner_w = BANNER[0].width() as u16;
    let banner_h = BANNER.len() as u16;
    let menu_count = MENU_ITEMS.len() as u16;
    let hints_lines: u16 = 2;
    let spacing: u16 = 1;

    let total_h = banner_h + spacing + menu_count * 4 + spacing + hints_lines;
    let start_y = area.y + area.height.saturating_sub(total_h) / 2;
    let start_x = area.x + area.width.saturating_sub(banner_w) / 2;

    for (i, line) in BANNER.iter().enumerate() {
        let render_area = Rect::new(start_x, start_y + i as u16, banner_w, 1);
        frame.render_widget(
            Paragraph::new(Span::styled(
                *line,
                Style::default()
                    .fg(theme.hi_fg.to_color())
                    .add_modifier(Modifier::BOLD),
            )),
            render_area,
        );
    }

    let menu_y = start_y + banner_h + spacing;

    for (idx, block) in MENU_ITEMS.iter().enumerate() {
        let selected = idx == state.selected;
        let mw = block[0].width() as u16;
        let x = area.x + area.width.saturating_sub(mw) / 2;
        let y = menu_y + idx as u16 * 4;

        let style = if selected {
            Style::default()
                .fg(theme.menu_selected_fg.to_color())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.menu_fg.to_color())
        };

        for (j, line) in block.iter().enumerate() {
            let render_area = Rect::new(x, y + j as u16, mw, 1);
            frame.render_widget(
                Paragraph::new(Span::styled(*line, style)),
                render_area,
            );
        }
    }

    let hints_y = menu_y + menu_count * 4 + spacing;
    let hints = [
        "j/k move  Enter select  q quit",
        "m menu  1-4 toggle zones  f fullscreen",
    ];

    for (i, hint) in hints.iter().enumerate() {
        let w = hint.len() as u16;
        let x = area.x + area.width.saturating_sub(w) / 2;
        let y = hints_y + i as u16;
        let render_area = Rect::new(x, y, w, 1);
        frame.render_widget(
            Paragraph::new(Span::styled(
                *hint,
                Style::default().fg(theme.inactive_fg.to_color()),
            )),
            render_area,
        );
    }
}
