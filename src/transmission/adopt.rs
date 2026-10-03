//! Taking over the downloads a daemon already holds.
//!
//! The problem this solves, in the words of the bug it fixes: doris used
//! to describe exactly one torrent -- the one the last search played -- so
//! everything else in the daemon was invisible, and a restart lost even
//! that. Pressing play again on a row that was already downloading started
//! a second copy of it.
//!
//! So there are two halves, and they are different jobs:
//!
//! - **Adopt** is a list. At startup the daemon is asked what it holds and
//!   the answer becomes `ui.downloads`. Nothing is stored in doris: the
//!   daemon is the record, and a list read from it cannot go stale against
//!   itself.
//! - **Recognise** is a join. A search row carries an info hash and so does
//!   a download, so "is this row already being fetched?" is one comparison
//!   and needs no id remembered on either side.
//!
//! Both key on the lower-case hex info hash, and both tolerate the daemon
//! being down: an unreachable daemon is not an error at startup, it is a
//! daemon that is not running yet.

use anyhow::Result;

use crate::transmission::{Download, Transmission};
use crate::ui::view::DownloadRow;

/// Turn one daemon row into a panel row.
///
/// The copy is field by field rather than a transmute because the two types
/// are not the same shape: the panel's row is doris's own, and it must not
/// change because Transmission added a field.
pub fn to_row(d: &Download) -> DownloadRow {
    DownloadRow {
        id: d.id,
        hash: d.hash.to_ascii_lowercase(),
        name: d.name.clone(),
        fraction: d.fraction,
        download_speed: d.download_speed,
        upload_speed: d.upload_speed,
        seeds: d.seeds,
        peers: d.peers,
        total_size: d.total_size,
        left: d.left,
        added: d.added,
        dir: d.dir.clone(),
        status: d.status,
        finished: d.finished,
        error: d.error.clone(),
        uploaded: d.uploaded,
        downloaded: d.downloaded,
        eta: d.eta,
        trackers: d.trackers.clone(),
        limit_bytes: d.limit_bytes(),
    }
}

/// What the daemon holds, as panel rows.
pub async fn adopt(transmission: &Transmission) -> Result<Vec<DownloadRow>> {
    Ok(transmission.list().await?.iter().map(to_row).collect())
}

/// The panel row for `hash`, if the daemon is already fetching it.
///
/// An empty hash never matches, which is what a tracker that reports no
/// info hash deserves: it cannot be recognised as downloaded, and
/// pretending otherwise would skip the download for the one source that
/// does give a magnet.
pub fn row_for_hash<'a>(rows: &'a [DownloadRow], hash: &str) -> Option<&'a DownloadRow> {
    let wanted = hash.trim().to_ascii_lowercase();
    if wanted.is_empty() {
        return None;
    }
    rows.iter().find(|r| r.hash == wanted)
}

/// The first info hash a tracker reported for `item`.
///
/// A row's own hash first, then a magnet link's `xt=urn:btih:`: trackers
/// disagree about which of the two they fill in, and a row that carries a
/// magnet almost always carries the hash inside it.
pub fn hash_of(item: &crate::sources::models::TorrentItem) -> String {
    let own = item.info_hash.trim().to_ascii_lowercase();
    if !own.is_empty() {
        return own;
    }
    item.magnet
        .as_deref()
        .and_then(btih_from_magnet)
        .unwrap_or_default()
}

/// The info hash out of a magnet link, if it carries one.
pub fn btih_from_magnet(magnet: &str) -> Option<String> {
    // `xt` is usually the *first* parameter, so it carries the `magnet:?`
    // prefix with it; stripping it first is what makes the search position
    // independent, which is the whole point of searching for the parameter
    // rather than slicing a fixed offset.
    magnet
        .strip_prefix("magnet:?")
        .unwrap_or(magnet)
        .split('&')
        .find_map(|part| part.strip_prefix("xt=urn:btih:"))
        .map(|hash| hash.to_ascii_lowercase())
}
