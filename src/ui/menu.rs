use super::theme::Theme;
use ratatui::prelude::*;
use ratatui::widgets::*;
use unicode_width::UnicodeWidthStr;

const BANNER: &[&str] = &[
    "██████╗  ██████╗ ██████╗ ██╗███████╗",
    "██╔══██╗██╔═══██╗██╔══██╗██║██╔════╝",
    "██║  ██║██║   ██║██████╔╝██║███████╗",
    "██║  ██║██║   ██║██╔══██╗██║╚════██║",
    "██████╔╝╚██████╔╝██║  ██║██║███████║",
    "╚═════╝  ╚═════╝ ╚═╝  ╚═╝╚═╝╚══════╝",
];

const MENU_ITEMS: &[&[&str]] = &[
    &[
        "┌─┐┌─┐╶┬╴╷┌─┐┌┐╷┌─┐",
        "│ │├─┘ │ ││ ││└┤└─┐",
        "└─┘╵   ╵ ╵└─┘╵ ╵└─┘",
    ],
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
}

impl Default for MenuState {
    fn default() -> Self {
        Self::new()
    }
}

impl MenuState {
    pub fn new() -> Self {
        Self {
            selected: 0,
            active: true,
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
    let spacing: u16 = 1;

    // No keybind footer: btop's main menu (`btop_menu.cpp:1219`,
    // `mainMenu`) draws a banner and three items and nothing else --
    // where the keys live is the help page's job, which is what П.3
    // moved ours to.
    let total_h = banner_h + spacing + menu_count * 4;
    let start_y = area.y + area.height.saturating_sub(total_h) / 2;
    let start_x = area.x + area.width.saturating_sub(banner_w) / 2;

    for (i, line) in BANNER.iter().enumerate() {
        let render_area = Rect::new(start_x, start_y + i as u16, banner_w, 1);
        frame.render_widget(
            Paragraph::new(Span::styled(
                *line,
                Style::default()
                    .fg(theme.primary_color())
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

        for (j, line) in block.iter().enumerate() {
            // Highlight the glyphs of the picked item, not the row they
            // sit in: a background behind the spaces would be a solid
            // stripe across the menu. So the style is per character --
            // a space keeps the plain menu style, everything else gets
            // the selection colours when this item is the picked one.
            let spans: Vec<Span> = line
                .chars()
                .map(|c| {
                    let style = if selected && c != ' ' {
                        Style::default()
                            .fg(theme.menu_selected_fg.to_color())
                            .bg(theme.menu_selected_bg.to_color())
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.menu_fg.to_color())
                    };
                    Span::styled(c.to_string(), style)
                })
                .collect();
            let render_area = Rect::new(x, y + j as u16, mw, 1);
            frame.render_widget(Paragraph::new(Line::from(spans)), render_area);
        }
    }
}
