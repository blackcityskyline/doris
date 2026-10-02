use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Mutex;

static LOG_FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

/// Where [`init`] opens the log, and where a reader should look.
///
/// The CLI has a `logs` command and the TUI has a Log zone, and the zone
/// only ever holds the tail of this file -- so a reader needs the path
/// from somewhere other than `init`, or it would have to guess the same
/// three directories again.
pub fn log_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".local")
        .join("share")
        .join("doris")
        .join("doris.log")
}

pub fn init() {
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let log_dir = home.join(".local").join("share").join("doris");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("doris.log");

    // Rotate instead of truncating. Truncating wiped whatever a second
    const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
    if let Ok(meta) = std::fs::metadata(&log_path) {
        if meta.len() >= MAX_LOG_BYTES {
            let _ = std::fs::rename(&log_path, log_dir.join("doris.log.1"));
        }
    }

    if let Ok(file) = OpenOptions::new().create(true).append(true).open(&log_path) {
        // Poisoned lock still holds a usable file: recover instead of
        *LOG_FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(file);
    }
}

pub fn log(module: &str, msg: &str) {
    let ts = chrono::Local::now().format("%H:%M:%S%.3f");
    let line = format!("[{}] [{}] {}\n", ts, module, msg);
    let mut guard = LOG_FILE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(ref mut file) = *guard {
        let _ = file.write_all(line.as_bytes());
        let _ = file.flush();
    }
}
