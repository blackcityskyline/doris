use std::fs::OpenOptions;
use std::io::Write;
use std::sync::Mutex;

static LOG_FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

pub fn init() {
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let log_dir = home.join(".local").join("share").join("t-hunter");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("t-hunter.log");
    if let Ok(file) = OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
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
