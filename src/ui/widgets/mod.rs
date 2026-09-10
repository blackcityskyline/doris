//! Small, reusable, render-agnostic UI widgets -- pure functions that take
//! data and return strings/spans, with no dependency on `ui::App` or any
//! particular zone. New widgets (e.g. a future multi-torrent list) belong
//! here rather than growing directly inside `ui/app.rs`.

pub mod graph;
