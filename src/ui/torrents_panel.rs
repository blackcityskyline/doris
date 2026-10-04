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

/// The columns, left to right, as the reference draws them.
///
/// Name first, then the numbers, and the name is the elastic one: it takes
/// whatever is left. It used to be the other way round -- `% state down up eta
/// size ratio name` -- which meant the table did not match the reference by
/// name or by order, and a panel the user already knows how to read was
/// reading as a different table.
///
/// No `eta`: the reference has no ETA column, and it is the one number here
/// that is a guess about the future rather than a reading of the torrent. It
/// is still in the detail view, where there is room to explain it.
///
/// There is no "essential" flag, because there was one that did nothing:
/// `plan` gave the name the leftover width by construction, so a flag
/// saying the same thing was a second copy of the rule that could be
/// flipped without changing the answer -- which is exactly what a mutation
/// showed. The rule that holds is the order: the first entry is the one
/// that never goes, and what is dropped is the tail.
pub const COLUMNS: &[Column] = &[
    Column {
        key: "name",
        title: "Name",
        width: 24,
    },
    Column {
        key: "percent",
        title: "Progress",
        width: 8,
    },
    Column {
        key: "state",
        // `downloading` is eleven characters and it is the state a panel is
        // looked at most; the queued states are shortened in `status_cell`
        // rather than truncated here.
        title: "Status",
        width: 11,
    },
    Column {
        key: "size",
        title: "Size",
        width: 9,
    },
    Column {
        key: "down",
        title: "Down",
        width: 10,
    },
    Column {
        key: "up",
        title: "Up",
        width: 10,
    },
    Column {
        key: "seeds",
        title: "Seeds",
        width: 7,
    },
    Column {
        key: "peers",
        title: "Peers",
        width: 7,
    },
    Column {
        key: "ratio",
        title: "Ratio",
        width: 5,
    },
];

/// The column that takes the leftover width, and the one that is never
/// dropped.
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

    // The name is first and elastic, so what is decided here is how much of
    // the tail fits beside a name of at least `name_min`, and the name takes
    // the rest. `name_min` is reserved on every pass, not just at the end:
    // taken greedily and trimmed afterwards, the name ended up with nothing,
    // and the `.max(name_min)` that papered over it pushed the row past the
    // border -- so the name was the part that got cut, the one column this
    // whole ordering exists to protect.
    let mut fixed: Vec<(&'static str, usize)> = Vec::new();
    let mut used = name_min + 2;
    for column in COLUMNS.iter().skip(1) {
        // `break` and not `continue`: a narrow panel loses the tail, not the
        // columns in between. Skipping a column too wide for the width and
        // keeping the one after it puts `Ratio` next to `Name` on a narrow
        // screen, which reads as a table with its middle missing.
        if used + column.width + 2 > width {
            break;
        }
        fixed.push((column.key, column.width));
        used += column.width + 2;
    }
    let mut plan = Vec::with_capacity(fixed.len() + 1);
    plan.push((NAME, width.saturating_sub(used).max(name_min)));
    plan.extend(fixed);
    Some(plan)
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
            // One decimal, as the reference prints it: `100.0%` reads as a
            // measurement and `99%` reads as a rounding of one.
            "percent" => format!("{:.1}%", row.percent()),
            "state" => status_cell(row).to_string(),
            "size" => crate::transmission::human_bytes(row.total_size.max(0) as u64),
            "down" => crate::transmission::human_speed(row.download_speed),
            "up" => crate::transmission::human_speed(row.upload_speed),
            "seeds" => peers_cell(row.seeds, row.trackers.iter().map(|t| t.seeders)),
            "peers" => peers_cell(row.peers, row.trackers.iter().map(|t| t.leechers)),
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

/// The word in the `Status` column.
///
/// The states that are all waiting are one word here. `queued to verify`,
/// `queued to download` and `queued to seed` are nineteen columns of a table
/// that has eleven, and a panel watched live tells them apart by the number
/// next to them, not by the queue. The exact word is in the detail view,
/// where there is room for it.
fn status_cell(row: &DownloadRow) -> &'static str {
    match row.state() {
        "queued to verify" | "queued to download" | "queued to seed" => "queued",
        other => other,
    }
}

/// `connected/known`, the way the reference prints its `Seeds` and `Peers`.
///
/// The second number is the most any tracker reported, which is what "known"
/// means: trackers disagree, they go stale, and the largest is the only one
/// of them that is not understating. With no tracker answering there is
/// nothing to compare against, so the cell is the connected count alone --
/// a `/0` would read as a measurement of zero seeder.
fn peers_cell(connected: i64, known: impl Iterator<Item = i64>) -> String {
    let total = known.max().unwrap_or(0).max(0);
    if total > connected {
        format!("{connected}/{total}")
    } else {
        format!("{connected}")
    }
}

/// The numbers the panel answers with, split into the three framed
/// sections the full-frame view draws them in.
///
/// Three sections rather than one row because "is the daemon there", "what
/// is moving" and "how much room is left" are three questions, and run
/// together on one line they read as one long row of noise.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    /// `connected`, `[daemon unreachable]`, or empty before the first poll.
    pub status: String,
    /// What is moving right now.
    pub speeds: String,
    /// How many torrents the daemon holds.
    pub active: String,
    /// This run's totals, and the ratio they add up to.
    pub session: String,
    /// Room on the disk they are being written to.
    pub free: String,
}

impl Summary {
    /// The same numbers on one line, for a zone too short for three boxes.
    pub fn one_line(&self) -> String {
        let mut line = format!("{}   {}", self.speeds, self.active);
        if !self.session.is_empty() {
            line.push_str(&format!("   {}", self.session));
        }
        if !self.free.is_empty() {
            line.push_str(&format!("   free {}", self.free));
        }
        if !self.status.is_empty() {
            line.push_str(&format!("   {}", self.status));
        }
        line
    }
}

/// Both directions of "is the daemon there", because the absence of an
/// answer is not an answer of yes -- and an unasked daemon is not a daemon
/// that failed to answer, which is what the word claimed before the first
/// poll.
pub fn summary(rows: &[DownloadRow], free: Option<i64>, daemon: Option<bool>) -> Summary {
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
    Summary {
        status: match daemon {
            Some(true) => "connected".to_string(),
            Some(false) => "[daemon unreachable]".to_string(),
            None => String::new(),
        },
        speeds: format!(
            "{} dl {} up",
            crate::transmission::human_speed(down),
            crate::transmission::human_speed(up)
        ),
        active: format!("{} torrents", rows.len()),
        session: format!(
            "session {} up {} ({})",
            crate::transmission::human_bytes(uploaded.max(0) as u64),
            crate::transmission::human_bytes(downloaded.max(0) as u64),
            ratio
        ),
        free: free
            .map(|f| crate::transmission::human_bytes(f.max(0) as u64))
            .unwrap_or_default(),
    }
}

/// How the three summary sections share `width`, or `None` when three
/// boxes cannot each hold their longest line.
///
/// Status is offered the most because its second line is the longest of
/// the three; the other two are then what is left, each with a floor.
pub fn section_widths(width: usize) -> Option<[usize; 3]> {
    // Active is offered what its session line needs: `session 0 B up 0 B
    // (0.00)` is longer than either of the others' second lines, and a box
    // that clips it mid-word is worse than a narrower neighbour.
    const STATUS_MIN: usize = 24;
    const ACTIVE_MIN: usize = 30;
    const FREE_MIN: usize = 14;
    if width < STATUS_MIN + ACTIVE_MIN + FREE_MIN {
        return None;
    }
    // Three borders, and one column of air between each pair of boxes.
    let free = FREE_MIN;
    let rest = width - free - 3;
    let active = ACTIVE_MIN;
    Some([rest - active, active, free])
}
