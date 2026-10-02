//! `doris downloads` -- the Torrents panel, on a terminal.
//!
//! `torrent` is about TorrServer, which streams. This is about the daemon
//! that actually writes to a directory, and it exists because the panel's
//! contents were unreachable from anywhere but the screen: a script could
//! ask TorrServer what it was streaming and had no way to ask what was
//! being fetched.
//!
//! It is the same adopt path the panel uses, so the list here and the list
//! on screen are the same list by construction rather than by agreement.

use anyhow::{anyhow, Result};

use crate::cli::Args;
use crate::config::Config;
use crate::transmission::adopt;
use crate::transmission::{Added, Transmission};

/// What `doris downloads` was asked to do beyond listing.
///
/// A clap argument type rather than something the command reads off a
/// struct elsewhere, so the flags and this cannot drift apart.
#[derive(clap::Args, Debug, Clone)]
pub struct Ask {
    /// Add a magnet or a `.torrent` URL and print the daemon's id for it.
    #[arg(long)]
    pub add: Option<String>,

    /// Watch one download until it finishes, or the deadline runs out.
    #[arg(long)]
    pub watch: Option<i64>,

    /// How long `--watch` waits.
    #[arg(long, default_value_t = 600_000)]
    pub deadline_ms: u64,
}

/// The daemon, with its credentials out of the encrypted store rather than
/// out of the config: a password in a TOML file is a password in a backup,
/// in a dotfile repository and in `ps`.
fn daemon(args: &Args, config: &Config) -> Transmission {
    let auth = crate::credentials::load_credential(crate::app::TRANSMISSION_RESOURCE);
    // The flag wins over the config, because a command that has to edit a
    // file to point at a different daemon is not a flag.
    let url = args
        .transmission_url
        .clone()
        .unwrap_or_else(|| config.transmission_url.clone());
    Transmission::with_auth(&url, auth)
}

/// Where a new download goes: the same resolution the panel's own download
/// path uses, so a row started from the screen and one started from a
/// script land in the same place.
fn download_dir(config: &Config) -> String {
    crate::app::resolve_download_dir(config)
}

pub async fn run(args: &Args, config: &Config, ask: &Ask, json: bool) -> Result<i32> {
    let transmission = daemon(args, config);

    if let Some(link) = ask.add.as_deref() {
        let dir = download_dir(config);
        return match transmission.add(link, Some(&dir)).await? {
            Added::Fresh(id) => {
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "id": id, "added": true, "link": link, "dir": dir,
                        }))?
                    );
                } else {
                    println!("{id}");
                }
                Ok(0)
            }
            Added::AlreadyThere(id) => {
                // Not an error: what the user asked for is true. Saying so
                // is the difference between "already had it" and
                // "nothing happened".
                if json {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&serde_json::json!({
                            "id": id, "added": false, "already": true,
                        }))?
                    );
                } else {
                    println!("already downloading, id {id}");
                }
                Ok(0)
            }
            Added::Refused(why) => Err(anyhow!(why)),
        };
    }

    let rows = adopt::adopt(&transmission).await?;

    if let Some(id) = ask.watch {
        let last = transmission
            .watch(
                id,
                std::time::Duration::from_millis(ask.deadline_ms),
                |_| false,
            )
            .await?
            .ok_or_else(|| anyhow!("the daemon no longer holds {id}"))?;
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&to_json(&adopt::to_row(&last)))?
            );
        } else {
            println!("{:.1}%  {}  {}", last.percent(), last.state(), last.name);
        }
        return Ok(i32::from(!last.finished && last.fraction < 1.0));
    }

    if json {
        let payload: Vec<serde_json::Value> = rows.iter().map(to_json).collect();
        println!("{}", serde_json::to_string_pretty(&payload)?);
    } else if rows.is_empty() {
        println!("nothing is downloading");
    } else {
        println!(
            "{:>4}  {:>9}  {:>7}  {:>7}  {:<16}  name",
            "id", "progress", "down", "up", "state"
        );
        for row in &rows {
            println!(
                "{:>4}  {:>8.1}%  {:>7}  {:>7}  {:<16}  {}",
                row.id,
                row.percent(),
                speed(row.download_speed),
                speed(row.upload_speed),
                row.state(),
                row.name
            );
        }
    }
    Ok(0)
}

fn to_json(row: &crate::ui::view::DownloadRow) -> serde_json::Value {
    serde_json::json!({
        "id": row.id,
        "hash": row.hash,
        "name": row.name,
        "percent": row.percent(),
        "state": row.state(),
        "download_speed": row.download_speed,
        "upload_speed": row.upload_speed,
        "seeds": row.seeds,
        "peers": row.peers,
        "downloaded": row.downloaded,
        "uploaded": row.uploaded,
        "ratio": row.ratio(),
        "total_size": row.total_size,
        "left": row.left,
        "bytes_done": row.bytes_done(),
        "dir": row.dir,
        "eta": eta_text(row.eta, row.left),
        "finished": row.finished,
        "error": row.error,
    })
}

/// The same ETA wording the panel and the `Download` type use, from the
/// two numbers Transmission reports rather than from a formatted string
/// stored anywhere -- so the CLI and the screen cannot say different
/// things about the same torrent.
fn eta_text(eta: i64, left: i64) -> Option<String> {
    match eta {
        e if e < 0 => None,
        0 if left > 0 => Some(format!(
            "{} left",
            crate::transmission::human_bytes(left as u64)
        )),
        0 => Some("done".to_string()),
        secs => Some(format!("{} left", crate::transmission::eta_secs(secs))),
    }
}

fn speed(bytes: i64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes}B")
    } else {
        format!("{value:.1}{}", UNITS[unit])
    }
}
