use ratatui::prelude::*;
use ratatui::widgets::*;
use super::theme::Theme;

pub fn banner_block(theme: &Theme) -> Vec<Line<'static>> {
    let art = [
        "██████╗  ██████╗ ██████╗ ██╗███████╗",
        "██╔══██╗██╔═══██╗██╔══██╗██║██╔════╝",
        "██║  ██║██║   ██║██████╔╝██║███████╗",
        "██║  ██║██║   ██║██╔══██╗██║╚════██║",
        "██████╔╝╚██████╔╝██║  ██║██║███████║",
        "╚═════╝  ╚═════╝ ╚═╝  ╚═╝╚═╝╚══════╝",
    ];
    art.iter()
        .map(|line| {
            Line::from(Span::styled(
                line.to_string(),
                Style::default()
                    .fg(theme.hi_fg.to_color())
                    .add_modifier(Modifier::BOLD),
            ))
        })
        .collect()
}

pub fn banner_lines(theme: &Theme) -> Vec<Line<'static>> {
    banner_block(theme)
}

pub fn create_box<'a>(
    x: u16,
    y: u16,
    w: u16,
    h: u16,
    border_color: Color,
    title: Option<&'a str>,
    num: Option<u8>,
) -> (Rect, Block<'a>) {
    let area = Rect::new(x, y, w, h);
    let title_str = match (title, num) {
        (Some(t), Some(n)) => format!(" {} {} ", superscript_digit(n), t),
        (Some(t), None) => format!(" {} ", t),
        (None, Some(n)) => format!(" {} ", superscript_digit(n)),
        (None, None) => String::new(),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(Span::styled(
            title_str,
            Style::default()
                .fg(border_color)
                .add_modifier(Modifier::BOLD),
        ));
    (area, block)
}

pub fn superscript_digit(n: u8) -> &'static str {
    match n {
        0 => "\u{2070}",
        1 => "\u{00B9}",
        2 => "\u{00B2}",
        3 => "\u{00B3}",
        4 => "\u{2074}",
        5 => "\u{2075}",
        6 => "\u{2076}",
        7 => "\u{2077}",
        8 => "\u{2078}",
        9 => "\u{2079}",
        _ => "?",
    }
}

pub fn meter_block(value: u8, width: usize, colors: &[Color]) -> Line<'static> {
    let filled = (value as usize * width / 100).min(width);
    let empty = width.saturating_sub(filled);
    let color_idx = (value as usize * (colors.len() - 1) / 100).min(colors.len() - 1);
    let color = colors[color_idx];

    let bar: String = "\u{2588}".repeat(filled);
    let empty_str: String = "\u{2591}".repeat(empty);

    Line::from(vec![
        Span::styled(bar, Style::default().fg(color)),
        Span::styled(empty_str, Style::default().fg(Color::DarkGray)),
        Span::raw(format!(" {}%", value)),
    ])
}

pub fn status_line(
    left: &str,
    center: &str,
    right: &str,
    theme: &Theme,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!(" {} ", left),
            Style::default().fg(theme.main_fg.to_color()),
        ),
        Span::styled(
            format!(" {} ", center),
            Style::default().fg(theme.inactive_fg.to_color()),
        ),
        Span::styled(
            format!(" {} ", right),
            Style::default().fg(theme.inactive_fg.to_color()),
        ),
    ])
}
