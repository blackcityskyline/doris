//! Size parsing shared by every source (ROADMAP.md B1).
//!
//! [`parse_size`] is a port of torio's `util/format.ts` `parseSize`,
//! keeping its unit table and its one non-obvious rule: Russian units
//! are read as *binary* (`КБ` = 1024) while Latin ones are read as
//! *decimal* (`KB` = 1000), with `GiB`/`MiB`-style spellings binary as
//! usual. Every HTML source feeds its display string here instead of
//! growing its own size regex.

use regex::Regex;
use std::sync::OnceLock;

/// Number + unit, e.g. `2.27`, `1.45 GiB`, `82.73&nbsp;MB` (after the
/// caller normalizes entities), `500 KiB`. `(?i)` accepts any case, so
/// the Cyrillic alternative covers `гб`/`мб` as well -- rutor serves
/// both casings.
const SIZE_PATTERN: &str = r"(?i)([\d.]+)\s*((?:[KMGT]I?)?B|КБ|МБ|ГБ|ТБ)";

fn size_regex() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    RE.get_or_init(|| Regex::new(SIZE_PATTERN).ok()).as_ref()
}

/// Bytes per unit, or `None` for a spelling outside the table. Units are
/// upper-cased first, so `гб`/`GB`/`ГБ` all land on one arm each.
fn multiplier(unit: &str) -> Option<f64> {
    match unit {
        // Russian units: binary, matching how trackers use them (torio
        // does the same in its RU_UNITS map).
        "КБ" => Some(1024.0),
        "МБ" => Some(1024.0_f64.powi(2)),
        "ГБ" => Some(1024.0_f64.powi(3)),
        "ТБ" => Some(1024.0_f64.powi(4)),
        // Latin: `KB`/`MB`/`GB`/`TB` are decimal (SI), `KiB`/`MiB`/...
        // binary, `B` is bytes.
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
    out.parse::<f64>().ok().filter(|n| n.is_finite() && *n >= 0.0)
}

/// Parse a human-readable size into bytes: `"1.45 GiB"`, `"82.73 MB"`,
/// `"2,27 ГБ"`, `"750 мб"` all work; a plain digit string is taken as an
/// already-raw byte count; anything unparseable is `0` (the "unknown
/// size" sentinel every field of that type uses).
///
/// The raw-digit branch is a deliberate addition over torio: it handles
/// the `<td>12345678</td>` rows torio catches in its source-specific
/// size regex instead of in `parseSize`.
pub fn parse_size(s: &str) -> u64 {
    let normalized = s.replace(',', ".");
    if let Some(caps) = size_regex().and_then(|re| re.captures(&normalized)) {
        let Some(num) = js_parse_float(&caps[1]) else {
            return 0;
        };
        let unit = caps[2].to_uppercase();
        // An unmapped-but-matched unit falls back to the bare number,
        // exactly as torio does; in practice the pattern only matches
        // units the table above knows.
        let per_unit = multiplier(&unit).unwrap_or(1.0);
        return (num * per_unit).round() as u64;
    }
    normalized.trim().parse::<u64>().unwrap_or(0)
}
