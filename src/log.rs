use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Mutex;

static LOG_FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

pub fn init() {
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let log_dir = home.join(".local").join("share").join("doris");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("doris.log");

    // Rotate instead of truncating. Truncating wiped whatever a second
    // instance had logged the moment it started -- leaving an empty file
    // exactly when the log is needed most -- and append lets concurrently
    // running instances share one history.
    const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
    if let Ok(meta) = std::fs::metadata(&log_path) {
        if meta.len() >= MAX_LOG_BYTES {
            let _ = std::fs::rename(&log_path, log_dir.join("doris.log.1"));
        }
    }

    if let Ok(file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        *LOG_FILE.lock().unwrap() = Some(file);
    }
}

pub fn log(module: &str, msg: &str) {
    let ts = chrono::Local::now().format("%H:%M:%S%.3f");
    let line = format!("[{}] [{}] {}\n", ts, module, msg);
    if let Some(ref mut file) = *LOG_FILE.lock().unwrap() {
        let _ = file.write_all(line.as_bytes());
        let _ = file.flush();
    }
}
