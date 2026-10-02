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
pub const BANNER_ROWS: u16 = 6;
const SPACING: u16 = 1;

/// Height of one menu item in rows: 3 glyph rows + 1 blank row of
/// breathing room between items.
const MENU_ITEM_HEIGHT: u16 = 4;

pub const MENU_ITEMS: &[&[&str]] = &[
    &[
        "┌─┐┌─┐─┬─│┌─┐┌┐│┌─┐",
        "│ │├─┘ │ ││ ││└┤└─┐",
        "└─┘│   │ │└─┘│ │└─┘",
    ],
    &["│ │┌──│  ┌─┐", "├─┤├─ │  ├─┘", "│ │└──└──│  "],
    &["┌─┐│ ││─┬─", "│┐││ ││ │ ", "└┴┘└─┘│ │ "],
];

/// The same three items with every single stroke doubled: `│` becomes
/// `║`, `─` becomes `═`, and so on, so the picked one is the same word
/// drawn in a heavier line.
///
/// This is how the reference marks focus in its menu, and it is the reason
/// the mark is visible on *every* theme: colour alone cannot carry it,
/// because a theme whose accent and whose plain foreground sit next to
/// each other leaves nothing to see -- which is what the unpicked
/// `menu_fg` and the picked accent are in several bundled themes. A
/// heavier line is a shape, and a shape does not depend on the palette.
/// -- two tables, and the picked one comes from the second.
pub const MENU_ITEMS_BOLD: &[&[&str]] = &[
    &[
        "╔═╗╔═╗═╦═║╔═╗╔╗║╔═╗",
        "║ ║╠═╝ ║ ║║ ║║╚╣╚═╗",
        "╚═╝║   ║ ║╚═╝║ ║╚═╝",
    ],
    &["║ ║╔══║  ╔═╗", "╠═╣╠═ ║  ╠═╝", "║ ║╚══╚══║  "],
    &["╔═╗║ ║║═╦═", "║╗║║ ║║ ║ ", "╚╩╝╚═╝║ ║ "],
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
/// items -- the wider of the two wins, and both are centred, so the rect
/// covers them either way -- with a row of padding so the glyphs do not
/// touch its edge.
pub fn menu_box_rect(area: Rect) -> Option<Rect> {
    let content_w = MENU_ITEMS
        .iter()
        .fold(BANNER[0].width() as u16, |w, block| {
            w.max(block[0].width() as u16)
        });
    let content_h = BANNER_ROWS + SPACING + MENU_ITEMS.len() as u16 * MENU_ITEM_HEIGHT;
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

/// The menu is glyphs and nothing else.
///
/// The reference draws the frame, then prints `Global::overlay` on top of
/// it, and that overlay is pure text: no box, no fill, no
/// `Clear`. Every panel stays readable around and between the glyphs, which
/// is the whole point of a menu you can open without losing the app you
/// were looking at.
///
/// So this paints no background either -- not a slab under the banner, not
/// a halo around the items, not `menu_selected_bg` behind the picked word.
/// "Theme background" has nothing to say about the menu, which is why it
/// takes no colour argument.
pub fn render_menu(frame: &mut Frame, area: Rect, state: &MenuState, theme: &Theme) {
    let banner_w = BANNER[0].width() as u16;
    let start_y = area.y
        + area
            .height
            .saturating_sub(BANNER_ROWS + SPACING + MENU_ITEMS.len() as u16 * MENU_ITEM_HEIGHT)
            / 2;

    for (i, line) in BANNER.iter().enumerate() {
        frame.render_widget(
            Paragraph::new(Span::styled(
                *line,
                Style::default()
                    .fg(theme.primary_color())
                    .add_modifier(Modifier::BOLD),
            )),
            Rect::new(
                area.x + area.width.saturating_sub(banner_w) / 2,
                start_y + i as u16,
                banner_w,
                1,
            ),
        );
    }

    for (idx, rect) in menu_item_rects(area).into_iter().enumerate() {
        let selected = idx == state.selected;
        // The picked item is drawn from the doubled-line table, so the
        // focus reads as a heavier line rather than only as a colour.
        let block = if selected {
            &MENU_ITEMS_BOLD[idx]
        } else {
            &MENU_ITEMS[idx]
        };
        for (j, line) in block.iter().enumerate() {
            // Unpicked items are the theme's plain menu colour, the
            // picked one the accent -- and the picked one also comes
            // from the doubled-line table, so on a theme whose accent
            // and whose plain colour sit next to each other the mark is
            // still a shape and not only a hue.
            let spans: Vec<Span> = line
                .chars()
                .map(|c| {
                    let style = if selected && c != ' ' {
                        Style::default()
                            .fg(theme.primary_color())
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.menu_fg.to_color())
                    };
                    Span::styled(c.to_string(), style)
                })
                .collect();
            frame.render_widget(
                Paragraph::new(Line::from(spans)),
                Rect::new(rect.x, rect.y + j as u16, rect.width, 1),
            );
        }
    }
}

/// Where each item is drawn, one rect per item, in the order of
/// [`MenuItem::all`].
///
/// The renderer draws into these and the mouse hit-test reads them, so a
/// click cannot land anywhere but on the word. The menu swallows every
/// other mouse event, so without this the three items are reachable only
/// from the keyboard -- and a click on Quit then does nothing at all,
/// which reads as a program that hung.
pub fn menu_item_rects(area: Rect) -> Vec<Rect> {
    let total_h = BANNER_ROWS + SPACING + MENU_ITEMS.len() as u16 * MENU_ITEM_HEIGHT;
    let start_y = area.y + area.height.saturating_sub(total_h) / 2;
    let menu_y = start_y + BANNER_ROWS + SPACING;
    MENU_ITEMS
        .iter()
        .enumerate()
        .map(|(idx, block)| {
            let mw = block[0].width() as u16;
            Rect::new(
                area.x + area.width.saturating_sub(mw) / 2,
                menu_y + idx as u16 * MENU_ITEM_HEIGHT,
                mw,
                MENU_ITEM_HEIGHT,
            )
        })
        .collect()
}
