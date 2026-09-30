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

/// Row counts of the layout below, named once: the backdrop has to
/// measure the same thing the drawing does, or the two disagree about
/// where the menu ends.
const BANNER_ROWS: u16 = 6;
const SPACING: u16 = 1;

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

/// The box the menu takes: one rect around the banner *and* the three
/// items -- the wider of the two wins, and both are centred, so the
/// rect covers them either way -- plus the border row a box needs.
///
/// `None` when the terminal cannot hold it: the menu then draws without
/// a backdrop, the way it always did, rather than clipping itself into
/// a frame whose inside is smaller than what it frames.
pub fn menu_backdrop_rect(area: Rect) -> Option<Rect> {
    let content_w = MENU_ITEMS
        .iter()
        .fold(BANNER[0].width() as u16, |w, block| {
            w.max(block[0].width() as u16)
        });
    let content_h = BANNER_ROWS + SPACING + MENU_ITEMS.len() as u16 * 4;
    if area.width < content_w + 2 || area.height < content_h + 2 {
        return None;
    }
    Some(Rect::new(
        area.x + (area.width - content_w) / 2 - 1,
        area.y + (area.height - content_h) / 2 - 1,
        content_w + 2,
        content_h + 2,
    ))
}

pub fn render_menu(
    frame: &mut Frame,
    area: Rect,
    state: &MenuState,
    theme: &Theme,
    backdrop: Block<'static>,
) {
    // The box goes down first: the glyphs below are painted *over* the
    // zones, and the rows between them carry no glyph to highlight, so
    // without an opaque backdrop those gaps are windows onto the table.
    if let Some(rect) = menu_backdrop_rect(area) {
        frame.render_widget(Clear, rect);
        frame.render_widget(backdrop, rect);
    }

    let banner_w = BANNER[0].width() as u16;
    let banner_h = BANNER_ROWS;
    let menu_count = MENU_ITEMS.len() as u16;
    let spacing = SPACING;

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
