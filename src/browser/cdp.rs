use anyhow::Result;
use fantoccini::{ClientBuilder, Client};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowserMode {
    Gui,
    Headless,
}

impl std::fmt::Display for BrowserMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrowserMode::Gui => write!(f, "gui"),
            BrowserMode::Headless => write!(f, "headless"),
        }
    }
}

impl std::str::FromStr for BrowserMode {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "gui" | "window" | "visible" => Ok(BrowserMode::Gui),
            "headless" | "hidden" | "bg" => Ok(BrowserMode::Headless),
            _ => anyhow::bail!("Unknown browser mode '{}'. Use 'gui' or 'headless'", s),
        }
    }
}

pub struct Browser {
    client: Client,
    #[allow(dead_code)]
    child: Option<std::process::Child>,
}

impl Browser {
    pub async fn launch(binary: &Path, mode: BrowserMode) -> Result<Self> {
        let browser_major = detect_browser_major_version(binary)?;
        let chromedriver_path = get_or_patch_chromedriver(browser_major).await?;

        let port = find_free_port()?;

        let mut cmd = std::process::Command::new(&chromedriver_path);
        cmd.arg(format!("--port={}", port))
            .arg("--silent")
            .stderr(Stdio::piped())
            .stdout(Stdio::null());

        let child = cmd.spawn()?;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        let mut chrome_args: Vec<String> = vec![
            "--no-sandbox".into(),
            "--disable-dev-shm-usage".into(),
            "--disable-blink-features=AutomationControlled".into(),
            "--no-first-run".into(),
            "--no-default-browser-check".into(),
            "--disable-infobars".into(),
            "--disable-background-timer-throttling".into(),
            "--disable-backgrounding-occluded-windows".into(),
            "--disable-renderer-backgrounding".into(),
            "--lang=ru-RU".into(),
        ];

        if mode == BrowserMode::Headless {
            chrome_args.push("--headless=new".into());
        } else {
            chrome_args.push("--window-size=1920,1080".into());
        }

        if let Some(user_data_dir) = detect_user_data_dir(binary) {
            chrome_args.push(format!("--user-data-dir={}", user_data_dir.display()));
        }

        let mut capabilities = serde_json::Map::new();
        let chrome_opts = serde_json::json!({
            "binary": binary.to_str().unwrap_or_default(),
            "args": chrome_args,
            "excludeSwitches": ["enable-automation"],
            "useAutomationExtension": false
        });
        capabilities.insert("goog:chromeOptions".into(), chrome_opts);

        let webdriver_url = format!("http://127.0.0.1:{}", port);

        let client = ClientBuilder::native()
            .capabilities(capabilities)
            .connect(&webdriver_url)
            .await?;

        eprintln!("[browser] {} via patched chromedriver on port {}", mode, port);

        Ok(Self {
            client,
            child: Some(child),
        })
    }

    pub async fn navigate(&self, url: &str) -> Result<()> {
        self.client.goto(url).await?;
        Ok(())
    }

    pub async fn get_page_source(&self) -> Result<String> {
        Ok(self.client.source().await?)
    }

    pub async fn eval_js(&self, script: &str) -> Result<serde_json::Value> {
        let trimmed = script.trim_start();
        let wrapped = if trimmed.starts_with("return ") || trimmed.starts_with("throw ") || trimmed.starts_with("async ") {
            script.to_string()
        } else {
            let s = script.trim_end().trim_end_matches(';');
            format!("return {};", s)
        };
        Ok(self.client.execute(&wrapped, vec![]).await?)
    }

    pub async fn get_cookies(&self) -> Result<Vec<serde_json::Value>> {
        let cookies = self.client.get_all_cookies().await?;
        let values: Vec<serde_json::Value> = cookies
            .into_iter()
            .map(|c| {
                let mut map = serde_json::Map::new();
                map.insert("name".into(), c.name().into());
                map.insert("value".into(), c.value().into());
                map.insert("domain".into(), c.domain().unwrap_or_default().into());
                map.insert("path".into(), c.path().unwrap_or_default().into());
                map.insert("secure".into(), c.secure().into());
                map.insert("httpOnly".into(), c.http_only().into());
                serde_json::Value::Object(map)
            })
            .collect();
        Ok(values)
    }

    #[allow(dead_code)]
    pub async fn add_cookie(&self, cookie: fantoccini::cookies::Cookie<'static>) -> Result<()> {
        self.client.add_cookie(cookie).await?;
        Ok(())
    }

    pub async fn add_cookies(&self, cookies: &[serde_json::Value]) -> Result<()> {
        for c in cookies {
            let name = c.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let value = c.get("value").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let mut builder = fantoccini::cookies::Cookie::build((name, value));
            if let Some(d) = c.get("domain").and_then(|v| v.as_str()) {
                builder = builder.domain(d.to_string());
            }
            if let Some(p) = c.get("path").and_then(|v| v.as_str()) {
                builder = builder.path(p.to_string());
            }
            if let Some(s) = c.get("secure").and_then(|v| v.as_bool()) {
                builder = builder.secure(s);
            }
            if let Some(h) = c.get("httpOnly").and_then(|v| v.as_bool()) {
                builder = builder.http_only(h);
            }
            let cookie = builder.build();
            self.client.add_cookie(cookie).await?;
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn client(&self) -> &Client {
        &self.client
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
        }
    }
}

fn find_free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

async fn get_or_patch_chromedriver(browser_major: u32) -> Result<PathBuf> {
    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("t-hunter");
    std::fs::create_dir_all(&data_dir)?;

    let patched_path = data_dir.join("chromedriver_patched");

    if patched_path.exists() {
        return Ok(patched_path);
    }

    let chromedriver_path = find_or_download_chromedriver(browser_major).await?;
    let original = std::fs::read(&chromedriver_path)?;

    let patched = patch_chromedriver_binary(&original);

    std::fs::write(&patched_path, &patched)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&patched_path, std::fs::Permissions::from_mode(0o755))?;
    }

    eprintln!("[browser] Patched chromedriver -> {}", patched_path.display());
    Ok(patched_path)
}

fn patch_chromedriver_binary(content: &[u8]) -> Vec<u8> {
    let replacement = b"{console.log(\"undetected chromedriver 1337!\")}";
    let mut result = content.to_vec();

    let mut search_start = 0;
    while search_start < result.len() {
        if let Some(pos) = find_window_cdc_block(&result[search_start..]) {
            let abs_pos = search_start + pos;
            let block_len = measure_cdc_block(&result[abs_pos..]);
            if block_len > 0 {
                let end = (abs_pos + block_len).min(result.len());
                let actual_len = end - abs_pos;
                let patch: Vec<u8> = replacement
                    .iter()
                    .chain(std::iter::repeat(&b' ').take(actual_len.saturating_sub(replacement.len())))
                    .take(actual_len)
                    .copied()
                    .collect();
                result[abs_pos..end].copy_from_slice(&patch);
                search_start = abs_pos + actual_len;
                eprintln!("[browser] Patched cdc block at offset {}, length {}", abs_pos, actual_len);
            } else {
                search_start += pos + 1;
            }
        } else {
            break;
        }
    }

    result
}

fn find_window_cdc_block(data: &[u8]) -> Option<usize> {
    let marker = b"{window.cdc";
    let mut i = 0;
    while i + marker.len() <= data.len() {
        if &data[i..i + marker.len()] == marker {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn measure_cdc_block(data: &[u8]) -> usize {
    if !data.starts_with(b"{window.cdc") {
        return 0;
    }
    let mut depth = 0;
    for (i, &b) in data.iter().enumerate() {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
    }
    0
}

async fn find_or_download_chromedriver(browser_major: u32) -> Result<PathBuf> {
    if let Ok(path) = which::which("chromedriver") {
        return Ok(path);
    }

    for path in &[
        "/usr/lib/chromium/chromedriver",
        "/usr/bin/chromedriver",
        "/snap/chromium/current/usr/lib/chromium-browser/chromedriver",
    ] {
        if Path::new(path).exists() {
            return Ok(PathBuf::from(path));
        }
    }

    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("t-hunter")
        .join("chromedriver");
    std::fs::create_dir_all(&data_dir)?;

    let downloaded = data_dir.join("chromedriver-linux64/chromedriver");

    if downloaded.exists() {
        return Ok(downloaded);
    }

    eprintln!("[browser] chromedriver not found, downloading for Chromium {}...", browser_major);

    let url = download_chromedriver_url(browser_major).await?;

    let status = std::process::Command::new("curl")
        .args(["-sL", "-o", "/tmp/chromedriver.zip", &url])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| anyhow::anyhow!("curl failed: {}", e))?;
    if !status.success() {
        anyhow::bail!("curl failed with status {}", status);
    }

    let status = std::process::Command::new("unzip")
        .args(["-o", "/tmp/chromedriver.zip", "-d", data_dir.to_str().unwrap()])
        .stdout(std::process::Stdio::null())
        .status()
        .map_err(|e| anyhow::anyhow!("unzip failed: {}", e))?;
    if !status.success() {
        anyhow::bail!("unzip failed with status {}", status);
    }

    if !downloaded.exists() {
        anyhow::bail!("chromedriver download failed: binary not found after extraction");
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&downloaded, std::fs::Permissions::from_mode(0o755))?;
    }

    eprintln!("[browser] chromedriver downloaded -> {}", downloaded.display());
    Ok(downloaded)
}

async fn download_chromedriver_url(browser_major: u32) -> Result<String> {
    let client = reqwest::Client::new();
    let versions_url = "https://googlechromelabs.github.io/chrome-for-testing/known-good-versions-with-downloads.json";
    let resp: serde_json::Value = client.get(versions_url).send().await?.json().await?;

    let versions = resp["versions"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Invalid Chrome for Testing response"))?;

    for v in versions.iter().rev() {
        let version_str = v["version"].as_str().unwrap_or("");
        let major = version_str.split('.').next().and_then(|s| s.parse::<u32>().ok());
        if major != Some(browser_major) {
            continue;
        }

        if let Some(downloads) = v["downloads"]["chromedriver"].as_array() {
            for dl in downloads {
                if dl["platform"].as_str() == Some("linux64") {
                    let url = dl["url"].as_str().unwrap_or("");
                    if !url.is_empty() {
                        return Ok(url.to_string());
                    }
                }
            }
        }
    }

    anyhow::bail!(
        "No chromedriver found for Chromium {}. Download manually from https://googlechromelabs.github.io/chrome-for-testing/",
        browser_major
    )
}

fn kill_browser_by_name(name: &str) {
    let output = std::process::Command::new("pkill")
        .args(["-9", "-f", name])
        .output();
    if let Ok(o) = output {
        if o.status.success() {
            eprintln!("[browser] killed {} processes", name);
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
    }
}

fn detect_user_data_dir(binary: &Path) -> Option<PathBuf> {
    let bin_name = binary.file_name()?.to_str()?;
    let home = dirs::home_dir()?;

    let profile_names = ["helium", "brave", "chromium", "google-chrome", "google-chrome-stable"];

    for profile_name in &profile_names {
        if !bin_name.contains(profile_name) {
            continue;
        }
        let dirs_to_check = [
            home.join(".config").join(format!("net.imput.{}", profile_name)),
            home.join(".config").join(*profile_name),
            home.join(".config").join(format!("{}-browser", profile_name)),
        ];
        for config_dir in &dirs_to_check {
            if config_dir.join("Default").exists() {
                let lock = config_dir.join("SingletonLock");
                let has_lock = lock.symlink_metadata().is_ok();
                eprintln!("[browser] profile dir: {}, lock exists: {}", config_dir.display(), has_lock);
                if has_lock {
                    let cache_dir = dirs::cache_dir()
                        .unwrap_or_else(|| PathBuf::from("/tmp"))
                        .join("t-hunter")
                        .join("profile-copy");
                    if cache_dir.exists() {
                        let _ = std::fs::remove_dir_all(&cache_dir);
                    }
                    copy_essential_profile(config_dir, &cache_dir);
                    eprintln!("[browser] browser running, using profile copy: {}", cache_dir.display());
                    return Some(cache_dir);
                }
                eprintln!("[browser] using user profile: {}", config_dir.display());
                return Some(config_dir.clone());
            }
        }
    }

    None
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let file_type = match entry.file_type() {
            Ok(t) => t,
            Err(_) => continue,
        };
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let name = entry.file_name().to_string_lossy().to_string();
        if file_type.is_dir() {
            if name == "SingletonLock" || name == "SingletonSocket" || name == "SingletonCookie"
                || name == "lockfile" || name == "Lock" || name == "LOCK"
                || name.starts_with("DevToolsActivePort")
                || name == "GPUPersistentCache" || name == "BrowserMetrics"
                || name == "ShaderCache" || name == "Crashpad"
                || name == "Code Cache" {
                continue;
            }
            if let Err(e) = copy_dir_recursive(&src_path, &dst_path) {
                eprintln!("[browser] skip dir {}: {}", name, e);
            }
        } else {
            if name == "LOCK" || name == "lockfile" || name == "LOG" || name == "LOG.old"
                || name.starts_with("DevToolsActivePort") || name == "chrome_debug.log"
                || name == "BrowserMetrics-spare.pma" {
                continue;
            }
            if let Err(e) = std::fs::copy(&src_path, &dst_path) {
                eprintln!("[browser] skip file {}: {}", name, e);
            }
        }
    }
    Ok(())
}

fn copy_cookies_sqlite(src_default: &Path, dst_default: &Path) {
    let _ = std::fs::create_dir_all(dst_default);
    let src_db = src_default.join("Cookies");
    if !src_db.exists() {
        return;
    }
    let dst_db = dst_default.join("Cookies");
    let backup_cmd = format!(".backup \"{}\"", dst_db.display());
    match std::process::Command::new("sqlite3")
        .args([src_db.to_str().unwrap(), &backup_cmd])
        .output()
    {
        Ok(out) if out.status.success() => eprintln!("[browser] backed up Cookies via sqlite3"),
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            eprintln!("[browser] sqlite3 backup failed: {}", stderr.trim());
            let _ = std::fs::copy(&src_db, &dst_db);
        }
        Err(e) => {
            eprintln!("[browser] sqlite3 not found: {}, copying raw", e);
            let _ = std::fs::copy(&src_db, &dst_db);
        }
    }
}

fn copy_essential_profile(src: &Path, dst: &Path) {
    let _ = std::fs::create_dir_all(dst);

    // Copy top-level files
    for entry in &["Local State", "First Run", "Preferences", "Secure Preferences"] {
        let s = src.join(entry);
        if s.exists() {
            let _ = std::fs::copy(&s, &dst.join(entry));
        }
    }

    let src_default = src.join("Default");
    let dst_default = dst.join("Default");
    if src_default.exists() {
        let _ = std::fs::create_dir_all(&dst_default);

        // Cookies via sqlite3 backup (handles locked DB from running browser)
        for _attempt in 0..3 {
            copy_cookies_sqlite(&src_default, &dst_default);
            if dst_default.join("Cookies").exists() {
                if let Ok(m) = std::fs::metadata(dst_default.join("Cookies")) {
                    if m.len() > 0 {
                        break;
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }

        // Copy essential profile files
        for entry in &["Preferences", "Secure Preferences", "Web Data", "Login Data",
                        "History", "Bookmarks", "Favicons", "Top Sites"] {
            let s = src_default.join(entry);
            if s.exists() {
                let _ = std::fs::copy(&s, &dst_default.join(entry));
            }
        }

        // Copy Extensions (includes all installed extensions)
        let src_ext = src_default.join("Extensions");
        let dst_ext = dst_default.join("Extensions");
        if src_ext.exists() {
            if let Err(e) = copy_dir_recursive(&src_ext, &dst_ext) {
                eprintln!("[browser] extensions copy error: {}", e);
            } else {
                eprintln!("[browser] copied extensions");
            }
        }

        // Copy Local Storage & Session Storage (contains site login state)
        for storage_dir in &["Local Storage", "Session Storage", "IndexedDB"] {
            let src_s = src_default.join(storage_dir);
            let dst_s = dst_default.join(storage_dir);
            if src_s.exists() {
                let _ = copy_dir_recursive(&src_s, &dst_s);
            }
        }
    }

    // Clean up stale state files
    for name in &["DevToolsActivePort", "chrome_debug.log"] {
        let _ = std::fs::remove_file(dst.join(name));
    }
    let dst_def = dst.join("Default");
    for name in &["LOCK", "LOCK-journal", "LOG", "LOG.old", "LOG-journal"] {
        let _ = std::fs::remove_file(dst_def.join(name));
    }
    let _ = std::fs::remove_file(dst.join("SingletonLock"));
    let _ = std::fs::remove_file(dst.join("SingletonSocket"));
    let _ = std::fs::remove_file(dst.join("SingletonCookie"));
}

fn detect_browser_major_version(binary: &Path) -> Result<u32> {
    let output = std::process::Command::new(binary)
        .arg("--version")
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run {}: {}", binary.display(), e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let version_str = if !stdout.trim().is_empty() { stdout } else { stderr };

    let version_str = version_str.trim();

    for part in version_str.split_whitespace().rev() {
        if let Some(major) = part.split('.').next().and_then(|s| s.parse::<u32>().ok()) {
            if major > 10 {
                return Ok(major);
            }
        }
    }

    anyhow::bail!(
        "Could not detect browser version from '{}' ({})",
        binary.display(),
        version_str
    )
}

#[allow(dead_code)]
pub type SharedBrowser = Arc<Mutex<Browser>>;

#[allow(dead_code)]
pub async fn create_browser(binary: &Path, mode: BrowserMode) -> Result<SharedBrowser> {
    let browser = Browser::launch(binary, mode).await?;
    Ok(Arc::new(Mutex::new(browser)))
}
