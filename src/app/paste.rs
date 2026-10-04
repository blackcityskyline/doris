//! Text that arrived in one piece: a paste, and a file dragged in.
//!
//! A file dragged from a file manager into a terminal is a paste, not a
//! keystroke: the terminal sends the path as one string. Read as text it is
//! also exactly what somebody copies off a magnet link, so one handler
//! answers both -- a path ending in `.torrent` or anything starting with
//! `magnet:` goes to the download daemon, and everything else goes into
//! whichever box is being typed into.
//!
//! The daemon has to be asked from here rather than from the view: the view
//! has no HTTP client and a dialog that cannot start anything is a text
//! field.

use anyhow::Result;

use super::App;

/// What a paste turned out to be.
///
/// A path is only a path if the file is there: a terminal pastes a `file://`
/// URL for a drag and a bare path for a copy, and both arrive here.
fn dropped_torrent(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(path) = line.strip_prefix("file://") {
            // `file:///home/u/x.torrent`: the empty authority is not part of
            // the path, and handing the daemon `file:///...` works too, so
            // nothing is decoded here.
            return Some(path.to_string());
        }
        if line.starts_with("magnet:") {
            return Some(line.to_string());
        }
        if line.to_lowercase().ends_with(".torrent") && std::path::Path::new(line).exists() {
            return Some(line.to_string());
        }
    }
    None
}

impl App {
    /// One pasted block of text.
    pub(super) async fn handle_paste(&mut self, text: &str) -> Result<()> {
        // The magnet field is a box like the search one: a paste into it is
        // text, and it has to land there rather than start a download the
        // user is only half-way through typing.
        if let crate::ui::view::Modal::Magnet(state) = &mut self.ui.modal {
            for c in text.chars() {
                state.type_char(c);
            }
            return Ok(());
        }

        crate::log::log(
            "paste",
            &crate::ui::torrents_panel::truncate(text.trim(), 200),
        );
        if let Some(link) = dropped_torrent(text) {
            self.ui.add_log(&format!(
                "Dropped: {}",
                crate::ui::torrents_panel::truncate(&link, 80)
            ));
            self.add_link_to_daemon(&link).await;
            return Ok(());
        }

        // Not a download: it is text for whatever box is open. The search box
        // is the only one that takes typing.
        if self.ui.input_mode {
            for c in text.chars() {
                self.ui.type_char(c);
            }
        } else if self.ui.zones.filter_mode {
            self.ui.zones.filter_input.push_str(text);
        } else if !text.trim().is_empty() {
            self.ui
                .add_log("That is not a magnet link and not a .torrent file.");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_magnet_is_recognised_however_it_was_copied() {
        assert_eq!(
            dropped_torrent("magnet:?xt=urn:btih:0123456789abcdef&dn=x"),
            Some("magnet:?xt=urn:btih:0123456789abcdef&dn=x".into())
        );
        assert_eq!(
            dropped_torrent("  magnet:?xt=urn:btih:abc  \n"),
            Some("magnet:?xt=urn:btih:abc".into()),
            "a copied link arrives with its whitespace"
        );
    }

    /// A file manager drops a `file://` URL; a copy of the same file pastes a
    /// bare path. Both are the same file and both have to work.
    #[test]
    fn a_dropped_file_is_recognised_as_a_url_and_as_a_path() {
        assert_eq!(
            dropped_torrent("file:///home/u/Movie.2024.1080p.torrent"),
            Some("/home/u/Movie.2024.1080p.torrent".into())
        );

        let path = std::env::temp_dir().join("doris-drop-test.torrent");
        std::fs::write(&path, b"d8:announce").expect("a scratch .torrent");
        assert_eq!(
            dropped_torrent(path.to_str().expect("utf-8 path")),
            Some(path.to_str().expect("utf-8 path").to_string())
        );

        // A path that is not there is not handed over: the daemon would say
        // "unrecognized info", which says less than "there is no such file".
        let missing = std::env::temp_dir().join("doris-not-here.torrent");
        assert_eq!(dropped_torrent(missing.to_str().expect("utf-8")), None);

        std::fs::remove_file(&path).ok();
    }

    /// A search query is text, not a download, and mistaking one for a
    /// dropped file would start a fetch nobody asked for.
    #[test]
    fn ordinary_text_is_not_a_dropped_torrent() {
        assert_eq!(dropped_torrent("the matrix"), None);
        assert_eq!(dropped_torrent("dune part two"), None);
        assert_eq!(dropped_torrent(""), None);
        assert_eq!(dropped_torrent("   \n  \n"), None);
    }
}
