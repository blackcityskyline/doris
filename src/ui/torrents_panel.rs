//! The Torrents panel's columns, and how many of them fit.
//!
//! Borrowed from qbittorrent-tui's list, which carries the one idea worth
//! copying whole: **every column has a minimum, a maximum and a priority**,
//! and when the panel is too narrow the tail goes and the name stays.
//!
//! The table here is a list of downloads, not one status readout, so the
//! question "what does this panel show when it is 40 columns wide?" gets
//! asked on every resize and on every terminal. It is decided by this
//! function rather than by the renderer, so the answer is one pure call
//! that a test can ask about every width without a terminal.
//!
//! The order is the priority order: the first entry is the one that is
//! never dropped. What differs from qbittorrent-tui is what is left out
//! for good -- `category`, `tags` and `tracker` are its concepts, not ours,
//! and `added` is a timestamp nobody reads in a panel they watch live.

use crate::ui::view::DownloadRow;

/// One column: what it is called, how wide it wants to be at most, and
/// whether it is the one that must survive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Column {
    pub key: &'static str,
    pub title: &'static str,
    pub width: usize,
}

/// The columns, in the order they are dropped. The name is last because it
/// is the one that takes whatever is left: a table row that cannot say what
/// it is about is not a row.
///
/// There is no "essential" flag, because there was one that did nothing:
/// `plan` gave the name the leftover width by construction, so a flag
/// saying the same thing was a second copy of the rule that could be
/// flipped without changing the answer -- which is exactly what a mutation
/// showed. The rule that holds is the order.
pub const COLUMNS: &[Column] = &[
    Column {
        key: "percent",
        title: "%",
        width: 5,
    },
    Column {
        key: "state",
        title: "state",
        width: 13,
    },
    Column {
        key: "down",
        title: "down",
        width: 7,
    },
    Column {
        key: "up",
        title: "up",
        width: 7,
    },
    Column {
        key: "eta",
        title: "eta",
        width: 6,
    },
    Column {
        key: "size",
        title: "size",
        width: 8,
    },
    Column {
        key: "ratio",
        title: "ratio",
        width: 5,
    },
    Column {
        key: "name",
        title: "name",
        width: 24,
    },
];

/// The column that takes the leftover width.
const NAME: &str = "name";

/// What fits in `width`, as the columns to draw and how much each gets.
///
/// Returns `None` when not even the name fits, which the caller answers by
/// printing nothing but a count: a two-column table of truncated names is
/// worse than saying there are three downloads.
pub fn plan(width: usize) -> Option<Vec<(&'static str, usize)>> {
    // One column of breathing room at each end is all the panel needs; the
    // two spaces after each cell are inside the widths below.
    let name_min = 12;
    if width < name_min + 2 {
        return None;
    }

    // The name takes what is left after everything else has been offered
    // its maximum. That ordering is the whole rule: the name is last and
    // it is elastic, so the columns in front of it are exactly those that
    // fit.
    let mut used = 2;
    let mut kept: Vec<(&'static str, usize)> = Vec::new();
    for column in COLUMNS {
        // The last column is the name, and the name is elastic, so it is
        // not offered a width here: it is what is left after the others.
        if column.key == NAME {
            break;
        }
        if used + column.width + 2 <= width {
            kept.push((column.key, column.width));
            used += column.width + 2;
        }
    }
    let name_width = width.saturating_sub(used).max(name_min);
    kept.push((NAME, name_width));
    Some(kept)
}

/// The columns and their widths, as the header line draws them.
pub fn header(plan: &[(&'static str, usize)]) -> String {
    let mut line = String::new();
    for (key, width) in plan {
        let title = COLUMNS
            .iter()
            .find(|c| c.key == *key)
            .map(|c| c.title)
            .unwrap_or(key);
        if *key == "name" {
            line.push_str(&format!(" {title:<width$}", width = width));
        } else {
            line.push_str(&format!("{title:>width$} ", width = width));
        }
    }
    line
}

/// One row's text under a plan.
///
/// The name is truncated with an ellipsis rather than cut. Two reasons,
/// and the second is the one that bites: `{:<width$}` pads to a width but
/// does not shorten to it, so a long name simply made the row longer than
/// the panel and pushed the rest of the line off the screen -- and two
/// rutracker titles that share a prefix then render identically, which is
/// worse than a name that is visibly cut.
pub fn row(row: &DownloadRow, plan: &[(&'static str, usize)]) -> String {
    let mut line = String::new();
    for (key, width) in plan {
        let cell = match *key {
            "percent" => format!("{:.0}%", row.percent()),
            "state" => row.state().to_string(),
            "down" => crate::transmission::human_speed(row.download_speed),
            "up" => crate::transmission::human_speed(row.upload_speed),
            "eta" => row.eta_text().unwrap_or_else(|| "--".into()),
            "size" => crate::transmission::human_bytes(row.total_size.max(0) as u64),
            "ratio" => row
                .ratio()
                .map(|r| format!("{r:.2}"))
                .unwrap_or_else(|| "--".into()),
            "name" => row.name.clone(),
            _ => String::new(),
        };
        // Every cell is shortened to its width, not just the name: a state
        // of "queued to download" is wider than its column, and a cell that
        // overflows pushes the rest of the row past the frame instead of
        // losing its own tail.
        let cell = truncate(&cell, *width);
        if *key == "name" {
            line.push_str(&format!(" {cell:<width$}", width = width));
        } else {
            line.push_str(&format!("{cell:>width$} ", width = width));
        }
    }
    line
}

/// Shorten to `width` characters, marking that something was lost.
///
/// By characters and not bytes: a title that crosses the cut mid-codepoint
/// would panic in the renderer, and the titles here are routinely Russian.
pub fn truncate(text: &str, width: usize) -> String {
    let count = text.chars().count();
    if count <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    // One column for the mark, so the result is exactly `width`.
    let keep = width - 1;
    let mut out: String = text.chars().take(keep).collect();
    out.push('…');
    out
}

/// The one-line summary above the table: qbittorrent-tui draws this across
/// three framed sections, which is six rows a doris zone often does not
/// have. The numbers are the same, in one row.
pub fn stats(rows: &[DownloadRow], free: Option<i64>, daemon: Option<bool>) -> String {
    let mut down = 0i64;
    let mut up = 0i64;
    let mut uploaded = 0i64;
    let mut downloaded = 0i64;
    for row in rows {
        down += row.download_speed.max(0);
        up += row.upload_speed.max(0);
        uploaded += row.uploaded.max(0);
        downloaded += row.downloaded.max(0);
    }
    let ratio = if downloaded > 0 {
        format!("{:.2}", uploaded as f64 / downloaded as f64)
    } else {
        "--".to_string()
    };
    let mut line = format!(
        "{} dl {} up   {} torrents   session {} up {} ({})",
        crate::transmission::human_speed(down),
        crate::transmission::human_speed(up),
        rows.len(),
        crate::transmission::human_bytes(uploaded.max(0) as u64),
        crate::transmission::human_bytes(downloaded.max(0) as u64),
        ratio
    );
    if let Some(free) = free {
        line.push_str(&format!(
            "   free {}",
            crate::transmission::human_bytes(free.max(0) as u64)
        ));
    }
    // Both forms, because the question is "is the daemon there", and the
    // answer being absent is not the same as the answer being yes. Before
    // the first poll `None` says nothing: an unknown daemon is not a
    // daemon that failed to answer.
    match daemon {
        Some(true) => line.push_str("   connected"),
        Some(false) => line.push_str("   [daemon unreachable]"),
        None => {}
    }
    line
}
