//! Size/date parsing and formatting shared by every source . Every HTML source feeds its
//! display string here instead of growing its own size regex.

use regex::Regex;
use std::sync::OnceLock;

/// Number + unit, e.g.
const SIZE_PATTERN: &str = r"(?i)([\d.]+)\s*((?:[KMGT]I?)?B|КБ|МБ|ГБ|ТБ)";

fn size_regex() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(SIZE_PATTERN).ok()).as_ref()
}

/// Bytes per unit, or `None` for a spelling outside the table.
fn multiplier(unit: &str) -> Option<f64> {
    match unit {
        // Russian units: binary, matching how trackers use them (torio
        "КБ" => Some(1024.0),
        "МБ" => Some(1024.0_f64.powi(2)),
        "ГБ" => Some(1024.0_f64.powi(3)),
        "ТБ" => Some(1024.0_f64.powi(4)),
        // Latin: `KB`/`MB`/`GB`/`TB` are decimal (SI), `KiB`/`MiB`/...
        "B" => Some(1.0),
        "KIB" => Some(1024.0),
        "MIB" => Some(1024.0_f64.powi(2)),
        "GIB" => Some(1024.0_f64.powi(3)),
        "TIB" => Some(1024.0_f64.powi(4)),
        "KB" => Some(1_000.0),
        "MB" => Some(1_000_000.0),
        "GB" => Some(1_000_000_000.0),
        "TB" => Some(1_000_000_000_000.0),
        _ => None,
    }
}

/// `parseFloat` semantics: digits plus at most one dot, stopping at the
/// first character that doesn't fit -- so `"2.27.5"` yields `2.27`
/// instead of failing outright the way `str::parse::<f64>` would.
fn js_parse_float(s: &str) -> Option<f64> {
    let mut out = String::with_capacity(s.len());
    let mut dots = 0_usize;
    for c in s.chars() {
        if c == '.' {
            dots += 1;
            if dots > 1 {
                break;
            }
        } else if !c.is_ascii_digit() {
            break;
        }
        out.push(c);
    }
    out.parse::<f64>()
        .ok()
        .filter(|n| n.is_finite() && *n >= 0.0)
}

/// Parse a human-readable size into bytes: `"1.45 GiB"`, `"82.73 MB"`, `"2,27 ГБ"`, `"750 мб"`
/// all work; a plain digit string is taken as an already-raw byte count; anything unparseable
/// is `0` (the "unknown size" sentinel every field of that type uses).
pub fn parse_size(s: &str) -> u64 {
    let normalized = s.replace(',', ".");
    if let Some(caps) = size_regex().and_then(|re| re.captures(&normalized)) {
        let Some(num) = js_parse_float(&caps[1]) else {
            return 0;
        };
        let unit = caps[2].to_uppercase();
        // An unmapped-but-matched unit falls back to the bare number,
        let per_unit = multiplier(&unit).unwrap_or(1.0);
        return (num * per_unit).round() as u64;
    }
    normalized.trim().parse::<u64>().unwrap_or(0)
}

/// torio's `unescapeEntities` from `rss.ts`, in the same order: `&amp;` first, then the
/// typographic pairs, then the angle brackets.
pub fn unescape_entities(input: &str) -> String {
    input
        .replace("&#038;", "&")
        .replace("&amp;", "&")
        .replace("&#8211;", "-")
        .replace("&#8212;", "-")
        .replace("&#8217;", "'")
        .replace("&#039;", "'")
        .replace("&apos;", "'")
        .replace("&#8220;", "\"")
        .replace("&#8221;", "\"")
        .replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
}

/// Tags out, entities decoded, whitespace collapsed to single spaces -- torio's `stripHtml` +
/// `unescapeEntities` in that order, which is what turns `<b>Фрирен&#039;s</b>&nbsp;<span...>`
/// back into a title.
pub fn strip_html(input: &str, tags: &Regex) -> String {
    let bare = tags.replace_all(input, "");
    let bare = bare.replace("&nbsp;", " ").replace('\u{a0}', " ");
    unescape_entities(&bare)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The display string for a source that hands us bytes rather than a pre-rendered size (the
/// JSON API sources, B8 wave 1) -- a port of torio's `formatBytes`, kept byte-for-byte
/// compatible with it: step by 1024, print two decimals past the byte unit, and label the steps
/// `KB`/`MB`/`GB` even though the step is binary, which is torio's own quirk.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes == 0 {
        return "0 B".to_string();
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} B", bytes)
    } else {
        format!("{:.2} {}", value, UNITS[unit])
    }
}

/// `YYYY-MM-DD` for a source that reports a unix timestamp (yts's
/// `date_uploaded_unix`, ez'tv's `date_released_unix`, apibay's `added`),
/// or `""` for the zero value -- `1970-01-01` in the Date column would
/// claim knowledge the source never gave us.
pub fn format_date(unix: i64) -> String {
    if unix <= 0 {
        return String::new();
    }
    chrono::DateTime::from_timestamp(unix, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}
