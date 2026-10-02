//! The greeting that plays before the UI comes up.
//!
//! One format, one parser, and the built-in animation is a file in that
//! format (`doris.anim`) rather than a table in Rust: the template
//! somebody drops in `~/.config/doris/welcome/` has to be the same kind of
//! thing the program ships, or "add your own" means a second dialect
//! nobody has tested.
//!
//! A template file is plain text:
//!
//! ```text
//! # `#` starts a comment, `--` alone on a line starts the next frame.
//! --
//! ██████╗
//! ██╔══██╗
//! --
//! ██████╗
//! ╚════██║
//! ```
//!
//! `{text}` inside a line is replaced with the configured greeting, which
//! is the whole of the templating: one placeholder, so a custom template
//! cannot express something the built-in does not.
//!
//! Deliberately not a general animation engine. No per-frame commands, no
//! colours, no interpolation: everything here is lines of glyphs and how
//! long to wait between them, and a template that wants more is a fork,
//! not a config value.

pub mod player;

use std::path::PathBuf;

/// The greeting animations that ship with doris, in the same format a
/// user template is written in.
const BUILT_IN: &[(&str, &str)] = &[("doris", include_str!("doris.anim"))];

/// One animation: a name, and its frames as lines of glyphs.
///
/// Every frame is the same size as every other one. `parse` pads them, so
/// the player can redraw a frame in place by moving the cursor up
/// instead of clearing the screen, which is the difference between a
/// greeting and a flicker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Template {
    pub name: String,
    pub frames: Vec<Vec<String>>,
    /// False for a template read out of the user's directory, which is
    /// what lets the Options list mark one as theirs.
    pub built_in: bool,
}

impl Template {
    /// Widest line of any frame.
    pub fn width(&self) -> usize {
        self.frames
            .iter()
            .flatten()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0)
    }

    /// Rows one frame takes.
    pub fn height(&self) -> usize {
        self.frames.first().map(|f| f.len()).unwrap_or(0)
    }

    /// Whether this animation can be drawn whole in a terminal this size.
    ///
    /// A frame that does not fit wraps, and a wrapped frame redraws over
    /// itself; there is no clipping and no scrolling, so an animation that
    /// cannot fit is not played at all. That is the honest answer for a
    /// small window -- the alternative is garbage on screen -- and it
    /// means a template has to be drawn to fit a terminal, which is a
    /// thing the format's author can see.
    pub fn fits(&self, width: u16, height: u16) -> bool {
        self.width() <= usize::from(width) && self.height() <= usize::from(height)
    }
}

/// Read a template out of its text form.
///
/// Frames are separated by a line of three or more `-`; a `#` line is a
/// comment. A file with no separator at all is one frame, which is what
/// makes the smallest possible custom template three lines of art.
pub fn parse(name: &str, text: &str, built_in: bool) -> Template {
    let mut frames: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_end_matches('\r');
        if trimmed.starts_with('#') {
            continue;
        }
        if is_separator(trimmed) {
            if !current.is_empty() {
                frames.push(std::mem::take(&mut current));
            }
            continue;
        }
        current.push(trimmed.to_string());
    }
    if !current.is_empty() {
        frames.push(current);
    }
    Template {
        name: name.to_string(),
        frames: pad(&frames),
        built_in,
    }
}

/// A line made only of dashes, three or more. Long enough that art using
/// `-` as a rule does not split itself into frames by accident.
fn is_separator(line: &str) -> bool {
    line.len() >= 3 && line.chars().all(|c| c == '-')
}

/// Give every frame the same box, so a redraw overwrites exactly what the
/// last one drew.
fn pad(frames: &[Vec<String>]) -> Vec<Vec<String>> {
    let width = frames
        .iter()
        .flatten()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0);
    let height = frames.iter().map(|f| f.len()).max().unwrap_or(0);
    frames
        .iter()
        .map(|frame| {
            let mut frame = frame.clone();
            while frame.len() < height {
                frame.push(String::new());
            }
            for line in &mut frame {
                let pad_to = width - line.chars().count();
                line.extend(std::iter::repeat_n(' ', pad_to));
            }
            frame
        })
        .collect()
}

/// Every animation available: the built-ins, then whatever is in
/// `~/.config/doris/welcome/`.
///
/// A user file named after a built-in replaces it rather than sitting
/// beside it, which is the same rule `Theme::load_themes` follows and for
/// the same reason -- a user who overrides `doris` means it, and a second
/// entry with the same name is a list with two answers to one question.
pub fn load_templates() -> Vec<Template> {
    let mut templates: Vec<Template> = BUILT_IN
        .iter()
        .map(|(name, text)| parse(name, text, true))
        .collect();
    let Some(dir) = user_dir() else {
        return templates;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return templates;
    };
    for path in entries.flatten().map(|e| e.path()) {
        if path.extension().and_then(|e| e.to_str()) != Some("anim") {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let template = parse(name, &text, false);
        if template.frames.is_empty() {
            continue;
        }
        match templates.iter_mut().find(|t| t.name == template.name) {
            Some(existing) => *existing = template,
            None => templates.push(template),
        }
    }
    templates
}

/// The animation named `name`, or the first one there is.
///
/// A name that matches nothing falls back rather than playing nothing:
/// the greeting is decoration, and decoration that has silently turned
/// itself off because of a typo is worse than the wrong greeting.
pub fn by_name(name: &str) -> Option<Template> {
    let templates = load_templates();
    templates
        .iter()
        .find(|t| t.name == name)
        .or_else(|| templates.first())
        .cloned()
}

/// Where user templates live.
pub fn user_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".config").join("doris").join("welcome"))
}

/// How many times the whole animation repeats.
///
/// `duration_ms` of zero means "once", which is what an author of a
/// template with one frame means by it. Otherwise the duration is rounded
/// up to a whole run, so the last frame is never shown for a sliver of
/// its time and cut off.
pub fn plays(frame_ms: u64, duration_ms: u64, frame_count: usize) -> usize {
    if frame_count == 0 {
        return 0;
    }
    let cycle = frame_ms.saturating_mul(frame_count as u64);
    if duration_ms == 0 || cycle == 0 {
        return 1;
    }
    usize::try_from(duration_ms.div_ceil(cycle))
        .unwrap_or(1)
        .max(1)
}

/// One frame with the greeting put into it.
pub fn render_line(line: &str, text: &str) -> String {
    line.replace("{text}", text)
}
