//! btop-style history sparklines (ROADMAP.md Phase 8).
//!
//! Replaces the old static `[####    ] 0%` progress bar in the Torrent
//! panel with a compact graph of *recent* progress, matching btop's CPU/
//! mem graph look instead of a plain fill bar. Three character sets are
//! supported, matching Options -> general -> Graph symbol (`braille` /
//! `block` / `dot`), the same three resolutions btop itself offers
//! (braille / block / tty-safe ASCII).

/// Render one line of a history sparkline, `width` characters wide, from
/// `history` (values expected in `0.0..=1.0`, oldest first). Fewer samples
/// than `width` pads with zeros on the left, so a fresh/short history
/// still lines up against the right edge instead of drifting.
pub fn render_sparkline(history: &[f64], width: usize, symbol_set: &str) -> String {
    if width == 0 {
        return String::new();
    }
    match symbol_set {
        "block" => render_block(history, width),
        "dot" => render_ascii(history, width),
        _ => render_braille(history, width),
    }
}

fn take_last_padded(history: &[f64], count: usize) -> Vec<f64> {
    let clamp = |v: f64| v.clamp(0.0, 1.0);
    if history.len() >= count {
        history[history.len() - count..].iter().copied().map(clamp).collect()
    } else {
        let mut padded = vec![0.0; count - history.len()];
        padded.extend(history.iter().copied().map(clamp));
        padded
    }
}

/// Highest-resolution mode: each braille character packs two samples (left
/// dot-column = older, right = newer), each quantized to 4 vertical
/// levels using that column's four dot rows, filled from the bottom like a
/// bar chart. This is the same trick btop uses to fit smooth-looking
/// history graphs into a single terminal line.
fn render_braille(history: &[f64], width: usize) -> String {
    // Unicode braille pattern dot-to-bit mapping:
    //   dot1 dot4      bit0 bit3
    //   dot2 dot5  ->  bit1 bit4
    //   dot3 dot6      bit2 bit5
    //   dot7 dot8      bit6 bit7
    const LEFT_BITS: [u8; 4] = [0x01, 0x02, 0x04, 0x40];
    const RIGHT_BITS: [u8; 4] = [0x08, 0x10, 0x20, 0x80];

    let samples = take_last_padded(history, width * 2);
    let mut out = String::with_capacity(width);

    for pair in samples.chunks(2) {
        let left_level = (pair[0] * 4.0).round() as usize;
        let right_level = pair.get(1).map(|&v| (v * 4.0).round() as usize).unwrap_or(0);

        let mut byte: u8 = 0;
        for row in 0..4 {
            if row >= 4 - left_level.min(4) {
                byte |= LEFT_BITS[row];
            }
            if row >= 4 - right_level.min(4) {
                byte |= RIGHT_BITS[row];
            }
        }
        out.push(char::from_u32(0x2800 + byte as u32).unwrap_or(' '));
    }
    out
}

/// Medium-resolution mode: one eighth-block character per sample (9 levels:
/// blank plus ▁▂▃▄▅▆▇█). Half the horizontal density of braille but works
/// on any UTF-8 terminal and reads a little more like a bar chart.
fn render_block(history: &[f64], width: usize) -> String {
    const LEVELS: [char; 9] =
        [' ', '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}', '\u{2588}'];
    take_last_padded(history, width)
        .iter()
        .map(|&v| LEVELS[((v * 8.0).round() as usize).min(8)])
        .collect()
}

/// Lowest-resolution, TTY-safe mode: plain ASCII characters, no Unicode
/// block/braille glyphs required. For `false_tty`/very limited terminals.
fn render_ascii(history: &[f64], width: usize) -> String {
    const LEVELS: [char; 5] = [' ', '.', ':', '+', '#'];
    take_last_padded(history, width)
        .iter()
        .map(|&v| LEVELS[((v * 4.0).round() as usize).min(4)])
        .collect()
}
