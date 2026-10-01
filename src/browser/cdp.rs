use anyhow::Result;
use fantoccini::{Client, ClientBuilder};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

/// Xvfb pid of a launch, recorded inside that launch's own temp profile so
/// a later launch can reap the Xvfb if its doris never reached `Drop`.
const XVFB_PID_FILE: &str = ".xvfb-pid";

// --- the waits, named -------------------------------------------------
//
// Three of the four shutdown paths below do the same thing -- SIGTERM,
// wait, then SIGKILL -- and they had drifted apart into 400ms, 600ms,
// 600ms with nothing written down saying why one process deserved less
// patience than another. One knob now; splitting them again is a
// deliberate edit to this file rather than a number typed in four spots.
//
// The values are calibration, not derivation: a browser that ignores
// SIGTERM needs a moment, a wedged one needs the kill. Raise this and
// shutdown gets slower on the machines that *do* respond; lower it and a
// slow machine gets SIGKILL'd mid-flush.

/// Grace between SIGTERM and SIGKILL for anything we are tearing down.
const SIGTERM_GRACE: Duration = Duration::from_millis(600);

/// How often those short reaps check whether the process is gone.
const REAP_POLL: Duration = Duration::from_millis(50);

/// A stale lock file means a *previous doris run* still held the profile and has not finished
/// releasing it.
const STALE_LOCK_GRACE: Duration = Duration::from_secs(10);
const STALE_LOCK_POLL: Duration = Duration::from_millis(200);

/// Settle after SIGKILL, before the lock file is removed: the file is how
/// a later run decides the profile is free, and removing it while the
/// killed processes are still closing their files hands the next launch a
/// directory something is still writing to.
const SIGKILL_SETTLE: Duration = Duration::from_secs(2);

// --- the startup waits -----------------------------------------------
//
// All three are a fixed sleep standing in for "no one has told us this is
// ready yet", which is the honest description: there is no signal from
// Xvfb's socket, from a spawned driver, or from a page load that we
// currently watch for. They are the least defensible numbers in the
// file and the most worth revisiting -- a readiness probe would delete
// all three.

/// Xvfb creates its display socket asynchronously; connecting before it
/// exists fails, and there is no API that says "ready".
const XVFB_SOCKET_WAIT: Duration = Duration::from_secs(1);

/// chromedriver binds its port during startup.
const DRIVER_BIND_WAIT: Duration = Duration::from_secs(2);

/// The cookie-injection navigation has to finish before the cookies are
/// written, or they land on a document that has not yet parsed.
const PAGE_SETTLE: Duration = Duration::from_secs(3);

fn wait_until(grace: Duration, poll: Duration, mut done: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + grace;
    loop {
        if done() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(poll);
    }
}

/// Browser window visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrowserVisibility {
    Visible,
    #[default]
    Hidden,
}

impl std::fmt::Display for BrowserVisibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrowserVisibility::Visible => write!(f, "visible"),
            BrowserVisibility::Hidden => write!(f, "hidden"),
        }
    }
}

impl std::str::FromStr for BrowserVisibility {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            // Legacy aliases kept so old configs/CLI flags keep working.
            "visible" | "gui" | "window" => Ok(BrowserVisibility::Visible),
            "hidden" | "headless" | "bg" => Ok(BrowserVisibility::Hidden),
            _ => anyhow::bail!(
                "Unknown browser visibility '{}'. Use 'visible' or 'hidden'",
                s
            ),
        }
    }
}

pub struct Browser {
    /// `None` only after [`Browser::shutdown`] took it to close the session;
    /// every other method goes through [`Browser::client`] and reports the
    /// session being gone instead of panicking.
    client: Option<Client>,
    child: Option<std::process::Child>,
    temp_profile: Option<PathBuf>,
    xvfb_child: Option<std::process::Child>,
    close_on_drop: bool,
}

impl Browser {
    /// `cookie_injection_url` is where a hidden-mode session navigates to before injecting
    /// cookies extracted from the browser's native (real) profile — it must be a page on the
    /// same domain those cookies belong to.
    pub async fn launch(
        binary: &Path,
        mode: BrowserVisibility,
        cookie_injection_url: &str,
        close_on_drop: bool,
        block_hosts: &[&str],
    ) -> Result<Self> {
        // A run killed outright (closed terminal, `kill -9`) never reaches
        // `Drop`: its temp profile, its Xvfb and any browser process that
        // outlived chromedriver stay behind, the browser keeping a page
        // loaded and burning CPU forever. Sweep previous runs first -- it
        // scans /proc and only blocks when there is actually something to
        // kill, so it belongs off the runtime worker.
        let sweep = tokio::task::spawn_blocking(cleanup_stale_profiles);
        if let Err(e) = sweep.await {
            crate::log::log("browser", &format!("stale run sweep failed: {}", e));
        }

        let browser_major = detect_browser_major_version(binary)?;
        let chromedriver_path = get_or_patch_chromedriver(browser_major).await?;

        let native_profile = detect_user_data_dir(binary);
        let mut injected_cookies: Vec<serde_json::Value> = Vec::new();

        let temp_profile = if mode == BrowserVisibility::Hidden {
            let tmp = std::env::temp_dir().join(format!("doris-hidden-{}", std::process::id()));
            std::fs::create_dir_all(&tmp)?;
            crate::log::log(
                "browser",
                &format!("hidden mode: temp profile {}", tmp.display()),
            );

            if let Some(ref native) = native_profile {
                // Which profile cookies are worth is a question about the
                // site, so the site answers it: no URL, no host, no
                // reading anyone's profile for nothing.
                match cookie_host_for(cookie_injection_url) {
                    None => crate::log::log(
                        "browser",
                        &format!(
                            "no host in '{}'; not reading cookies from the native profile",
                            cookie_injection_url
                        ),
                    ),
                    Some(host) => match extract_cookies_from_native_profile(native, &host) {
                        Ok(cookies) => {
                            crate::log::log(
                                "browser",
                                &format!(
                                    "extracted {} {} cookies from native profile",
                                    cookies.len(),
                                    host
                                ),
                            );
                            injected_cookies = cookies;
                        }
                        Err(e) => {
                            crate::log::log(
                                "browser",
                                &format!("could not extract native cookies: {}", e),
                            );
                        }
                    },
                }
            }
            Some(tmp)
        } else {
            if let Some(dir) = native_profile.clone() {
                // Waits up to ~12s for a browser already holding the profile
                // to shut down -- a blocking loop full of `pgrep`/`kill` and
                // sleeps, so run it off the runtime worker thread.
                let old = tokio::task::spawn_blocking(move || graceful_shutdown_if_running(&dir));
                if let Err(e) = old.await {
                    crate::log::log("browser", &format!("old browser shutdown failed: {}", e));
                }
            }
            native_profile.clone()
        };

        let use_xvfb = mode == BrowserVisibility::Hidden && has_xvfb();
        let mut xvfb_child = None;
        if use_xvfb {
            crate::log::log("browser", "using xvfb virtual display");
        }

        let port = find_free_port()?;

        let mut cmd = if use_xvfb {
            let display_num = find_free_display();
            xvfb_child = start_xvfb(display_num);
            record_xvfb_pid(xvfb_child.as_ref(), temp_profile.as_ref());
            // Give Xvfb time to create its socket before anything renders
            // into it. This used to be a `std::thread::sleep` inside
            // `start_xvfb`, which parked a runtime worker for a second.
            tokio::time::sleep(XVFB_SOCKET_WAIT).await;
            let mut c = std::process::Command::new(&chromedriver_path);
            c.env("DISPLAY", format!(":{}", display_num));
            c
        } else {
            std::process::Command::new(&chromedriver_path)
        };

        cmd.arg(format!("--port={}", port))
            .stderr(Stdio::piped())
            .stdout(Stdio::null());

        let mut child = cmd.spawn()?;
        if let Some(stderr) = child.stderr.take() {
            spawn_chromedriver_log_drain(stderr);
        }
        tokio::time::sleep(DRIVER_BIND_WAIT).await;

        // NB: `--disable-background-timer-throttling` and
        // `--disable-backgrounding-occluded-windows` are deliberately *not*
        // listed here: we tried removing them to let Chrome throttle idle
        // pages, but chromedriver injects both itself (they're compiled into
        // its default switch list) and Chrome has no counter-switch, so idle
        // throttling can never be relied upon. Unloading the page instead
        // (`Browser::park`) is the only fix that actually works.
        let chrome_args = build_chrome_args(mode, use_xvfb, temp_profile.as_deref(), block_hosts);

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

        crate::log::log(
            "browser",
            &format!("{} via patched chromedriver on port {}", mode, port),
        );

        let browser = Self {
            client: Some(client),
            child: Some(child),
            temp_profile: if mode == BrowserVisibility::Hidden {
                temp_profile
            } else {
                None
            },
            xvfb_child,
            close_on_drop,
        };

        if !injected_cookies.is_empty() {
            crate::log::log("browser", "navigating to domain for cookie injection...");
            browser.navigate(cookie_injection_url).await.ok();
            tokio::time::sleep(PAGE_SETTLE).await;
            crate::log::log(
                "browser",
                &format!(
                    "injecting {} cookies into hidden session",
                    injected_cookies.len()
                ),
            );
            browser.add_cookies(&injected_cookies).await?;
            // Cookies live in the profile, not in the tab: don't leave the
            // source's home page (ads and all) loaded before Doris has even
            // been asked to do anything.
            browser.park().await?;
        }

        Ok(browser)
    }

    fn client(&self) -> Result<&Client> {
        self.client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("browser session is already closed"))
    }

    /// Park the tab on `about:blank` so nothing keeps running while Doris is idle.
    pub async fn park(&self) -> Result<()> {
        self.client()?.goto("about:blank").await?;
        Ok(())
    }

    /// Close the session properly: end the WebDriver session first (that DELETE is what makes
    /// chromedriver take the browser down with it), and only then reap chromedriver itself.
    pub async fn shutdown(&mut self) {
        if let Some(client) = self.client.take() {
            if self.close_on_drop {
                match client.close().await {
                    Ok(()) => crate::log::log("browser", "webdriver session closed"),
                    Err(e) => crate::log::log("browser", &format!("webdriver close failed: {}", e)),
                }
            } else {
                // The browser is meant to outlive Doris: tell fantoccini not
                // to send the session DELETE when the handle goes away.
                if let Err(e) = client.persist().await {
                    crate::log::log("browser", &format!("persist failed: {}", e));
                }
            }
        }
        self.reap();
    }

    /// Kill chromedriver (and Xvfb), sweep browser processes that outlived them, and drop the
    /// temp profile.
    fn reap(&mut self) {
        if self.close_on_drop {
            if let Some(mut child) = self.child.take() {
                terminate_child(&mut child);
            }
            if let Some(mut xvfb) = self.xvfb_child.take() {
                let _ = xvfb.kill();
            }
            if let Some(ref profile) = self.temp_profile {
                // Last line of defence for the paths where the session
                // DELETE never happened (a crash, or `Drop` running without
                // `shutdown`): whatever still holds our temp profile is a
                // leftover from this very launch and must not stay behind.
                sweep_profile(profile);
            }
        }
        if let Some(path) = self.temp_profile.take() {
            crate::log::log(
                "browser",
                &format!("cleanup temp profile {}", path.display()),
            );
            let _ = std::fs::remove_dir_all(path);
        }
    }

    pub async fn navigate(&self, url: &str) -> Result<()> {
        self.client()?.goto(url).await?;
        Ok(())
    }

    pub async fn get_page_source(&self) -> Result<String> {
        Ok(self.client()?.source().await?)
    }

    pub async fn eval_js(&self, script: &str) -> Result<serde_json::Value> {
        let trimmed = script.trim_start();
        let wrapped = if trimmed.starts_with("return ")
            || trimmed.starts_with("throw ")
            || trimmed.starts_with("async ")
        {
            script.to_string()
        } else {
            let s = script.trim_end().trim_end_matches(';');
            format!("return {};", s)
        };
        Ok(self.client()?.execute(&wrapped, vec![]).await?)
    }

    pub async fn get_cookies(&self) -> Result<Vec<serde_json::Value>> {
        let cookies = self.client()?.get_all_cookies().await?;
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

    pub async fn add_cookies(&self, cookies: &[serde_json::Value]) -> Result<()> {
        let client = self.client()?;
        for c in cookies {
            let name = c
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let value = c
                .get("value")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
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
            client.add_cookie(cookie).await?;
        }
        Ok(())
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        // The session DELETE is async, and by the time Drop runs the runtime
        // may already be gone -- so this is the belt-and-braces path: end
        // chromedriver first (SIGTERM, short grace, then SIGKILL) and sweep
        // anything that outlived it. Call `shutdown()` on the normal exit
        // path for a clean, fully awaited close.
        self.reap();
    }
}

/// Send `SIGTERM` (or `SIGKILL` when `hard`) to `pid` via `kill(1)`.
fn send_signal(pid: u32, hard: bool) {
    let mut cmd = std::process::Command::new("kill");
    if hard {
        cmd.arg("-9");
    }
    cmd.arg(pid.to_string());
    let _ = cmd.stdout(Stdio::null()).stderr(Stdio::null()).output();
}

/// Terminate a child, giving it a short grace period to exit on SIGTERM before escalating.
fn terminate_child(child: &mut std::process::Child) {
    send_signal(child.id(), false);
    // `Ok(None)` is the only "still running" answer, so it is also the
    // only one worth waiting on: an exited child *and* a child we can no
    // longer ask about both end the wait, the second one straight into
    // the kill below.
    let gone = wait_until(SIGTERM_GRACE, REAP_POLL, || {
        !matches!(child.try_wait(), Ok(None))
    });
    if !gone {
        let _ = child.kill();
    }
}

/// Kill every browser process still holding `profile_dir` (matched by its `--user-data-dir=`),
/// escalating to SIGKILL if they don't go away.
fn sweep_profile(profile_dir: &Path) {
    let pids = find_pids_by_profile(profile_dir);
    if pids.is_empty() {
        return;
    }
    for pid in &pids {
        send_signal(*pid, false);
    }
    if wait_until(SIGTERM_GRACE, REAP_POLL, || {
        find_pids_by_profile(profile_dir).is_empty()
    }) {
        crate::log::log(
            "browser",
            &format!("swept {} orphaned processes", pids.len()),
        );
        return;
    }
    for pid in &pids {
        send_signal(*pid, true);
    }
    crate::log::log(
        "browser",
        &format!("SIGKILLed {} orphaned processes", pids.len()),
    );
}

/// Remember which Xvfb this run started, inside the run's own profile dir,
/// so a later launch can reap it if its doris dies without reaching `Drop`.
fn record_xvfb_pid(child: Option<&std::process::Child>, profile: Option<&PathBuf>) {
    if let (Some(child), Some(profile)) = (child, profile) {
        let _ = std::fs::write(profile.join(XVFB_PID_FILE), child.id().to_string());
    }
}

/// Sweep the leftovers of runs whose doris is gone: their temp profile (a ~100MB directory),
/// any browser still holding it, and the Xvfb they started.
fn cleanup_stale_profiles() {
    let me = std::process::id();
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        let pid = match name
            .strip_prefix("doris-hidden-")
            .and_then(|p| p.parse::<u32>().ok())
        {
            Some(p) => p,
            None => continue,
        };
        // Still running (or its pid got recycled): not ours to touch.
        if pid == me || process_exists(pid) {
            continue;
        }
        sweep_profile(&path);
        kill_recorded_xvfb(&path);
        if std::fs::remove_dir_all(&path).is_ok() {
            crate::log::log(
                "browser",
                &format!("removed leftovers of dead run {}", path.display()),
            );
        }
    }
}

fn process_exists(pid: u32) -> bool {
    Path::new(&format!("/proc/{}", pid)).exists()
}

fn kill_recorded_xvfb(profile_dir: &Path) {
    let Ok(record) = std::fs::read_to_string(profile_dir.join(XVFB_PID_FILE)) else {
        return;
    };
    let Ok(pid) = record.trim().parse::<u32>() else {
        return;
    };
    let cmdline = std::fs::read_to_string(format!("/proc/{}/cmdline", pid))
        .unwrap_or_default()
        .replace('\0', " ");
    if !cmdline.starts_with("Xvfb ") || !cmdline.contains("1920x1080x24") {
        return;
    }
    send_signal(pid, false);
    if !wait_until(SIGTERM_GRACE, REAP_POLL, || !process_exists(pid)) {
        send_signal(pid, true);
    }
    crate::log::log(
        "browser",
        &format!("reaped orphaned Xvfb of dead run (pid {})", pid),
    );
}

fn spawn_chromedriver_log_drain(stderr: std::process::ChildStderr) {
    std::thread::spawn(move || {
        use std::io::{BufRead, BufReader};
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let low = line.to_lowercase();
            if low.contains("error") || low.contains("fatal") || low.contains("panic") {
                crate::log::log("chromedriver", &line);
            }
        }
    });
}

fn find_free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

fn has_xvfb() -> bool {
    std::process::Command::new("which")
        .arg("Xvfb")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn find_free_display() -> u32 {
    for num in 99..200 {
        let sock = std::path::PathBuf::from(format!("/tmp/.X11-unix/X{}", num));
        if !sock.exists() {
            return num;
        }
    }
    99
}

fn start_xvfb(display_num: u32) -> Option<std::process::Child> {
    let child = std::process::Command::new("Xvfb")
        .args([
            &format!(":{}", display_num),
            "-screen",
            "0",
            "1920x1080x24",
            "-nolisten",
            "tcp",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    // The caller waits for Xvfb to come up (see `Browser::launch`): a sleep
    // here would block a runtime worker thread.
    crate::log::log("browser", &format!("started Xvfb on :{}", display_num));
    Some(child)
}

fn graceful_shutdown_if_running(profile_dir: &Path) {
    let lock = profile_dir.join("SingletonLock");
    if !lock.symlink_metadata().is_ok() {
        return;
    }

    crate::log::log("browser", "shutting down browser...");

    let pids = find_pids_by_profile(profile_dir);
    if pids.is_empty() {
        let _ = std::fs::remove_file(&lock);
        return;
    }

    for pid in &pids {
        send_signal(*pid, false);
    }
    crate::log::log(
        "browser",
        &format!("sent SIGTERM to {} processes", pids.len()),
    );

    if wait_until(STALE_LOCK_GRACE, STALE_LOCK_POLL, || {
        lock.symlink_metadata().is_err()
    }) {
        crate::log::log("browser", "browser exited cleanly");
        return;
    }
    crate::log::log("browser", "timeout, sending SIGKILL");
    for pid in &pids {
        send_signal(*pid, true);
    }
    std::thread::sleep(SIGKILL_SETTLE);
    let _ = std::fs::remove_file(&lock);
}

fn find_pids_by_profile(profile_dir: &Path) -> Vec<u32> {
    let profile_str = profile_dir.to_string_lossy();
    let output = match std::process::Command::new("pgrep")
        .args(["-f", &format!("user-data-dir={}", profile_str)])
        .output()
    {
        Ok(o) => o,
        Err(_) => return vec![],
    };

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .collect()
}

fn detect_user_data_dir(binary: &Path) -> Option<PathBuf> {
    let bin_name = binary.file_name()?.to_str()?;
    let home = dirs::home_dir()?;

    let profile_names = [
        "helium",
        "brave",
        "chromium",
        "google-chrome",
        "google-chrome-stable",
    ];

    for profile_name in &profile_names {
        if !bin_name.contains(profile_name) {
            continue;
        }
        let dirs_to_check = [
            home.join(".config")
                .join(format!("net.imput.{}", profile_name)),
            home.join(".config").join(*profile_name),
            home.join(".config")
                .join(format!("{}-browser", profile_name)),
        ];
        for config_dir in &dirs_to_check {
            if config_dir.join("Default").exists() {
                crate::log::log("browser", &format!("profile: {}", config_dir.display()));
                return Some(config_dir.clone());
            }
        }
    }

    None
}

async fn get_or_patch_chromedriver(browser_major: u32) -> Result<PathBuf> {
    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("doris");
    std::fs::create_dir_all(&data_dir)?;
    drop_un_keyed_cache(&data_dir);

    let patched_path = patched_chromedriver_path(&data_dir, browser_major);

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

    crate::log::log(
        "browser",
        &format!("patched chromedriver -> {}", patched_path.display()),
    );
    Ok(patched_path)
}

pub fn patched_chromedriver_path(data_dir: &Path, browser_major: u32) -> PathBuf {
    data_dir.join(format!("chromedriver_patched-{}", browser_major))
}

/// Whether `data_dir` already holds a patched driver for exactly this
/// major -- the health check's idea of "no download needed", and the
/// reason a driver built for another browser never counts.
pub fn has_patched_chromedriver(data_dir: &Path, browser_major: u32) -> bool {
    patched_chromedriver_path(data_dir, browser_major).exists()
}

/// Drop the un-suffixed cache file the pre-versioning code left behind.
fn drop_un_keyed_cache(data_dir: &Path) {
    let legacy = data_dir.join("chromedriver_patched");
    if !legacy.exists() {
        return;
    }
    match std::fs::remove_file(&legacy) {
        Ok(()) => crate::log::log(
            "browser",
            "dropped the un-keyed chromedriver cache (rebuilt per browser major)",
        ),
        Err(e) => crate::log::log(
            "browser",
            &format!("could not drop the un-keyed chromedriver cache: {}", e),
        ),
    }
}

/// Re-home the download made before the cache was keyed by browser major: it sits at
/// `root/chromedriver-linux64/chromedriver`, a path nothing reads anymore, and is good for
/// exactly one browser.
pub fn adopt_legacy_download(root: &Path, browser_major: u32) -> Option<PathBuf> {
    let legacy_dir = root.join("chromedriver-linux64");
    let legacy = legacy_dir.join("chromedriver");
    let target_dir = root
        .join(browser_major.to_string())
        .join("chromedriver-linux64");
    let target = target_dir.join("chromedriver");

    if target.exists() || !legacy.exists() || !driver_serves(&legacy, browser_major) {
        return None;
    }
    std::fs::create_dir_all(&target_dir).ok()?;
    match std::fs::rename(&legacy, &target) {
        Ok(()) => {
            crate::log::log(
                "browser",
                &format!("adopted the existing chromedriver for {}", browser_major),
            );
            let _ = std::fs::remove_dir_all(&legacy_dir);
            Some(target)
        }
        Err(e) => {
            crate::log::log(
                "browser",
                &format!("could not adopt the existing chromedriver: {}", e),
            );
            None
        }
    }
}

pub fn driver_serves(path: &Path, browser_major: u32) -> bool {
    let output = match std::process::Command::new(path).arg("--version").output() {
        Ok(o) => o,
        Err(_) => return false,
    };
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    driver_major(&text) == Some(browser_major)
}

/// The major a `--version` line reports, `None` when it carries no version at all.
fn driver_major(version_output: &str) -> Option<u32> {
    for part in version_output.split_whitespace() {
        if let Some(major) = part.split('.').next().and_then(|s| s.parse::<u32>().ok()) {
            if major > 10 {
                return Some(major);
            }
        }
    }
    None
}

/// The drivers already on this machine, in the order they are worth trying.
fn system_driver_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(path) = which::which("chromedriver") {
        candidates.push(path);
    }
    for path in [
        "/usr/lib/chromium/chromedriver",
        "/usr/bin/chromedriver",
        "/snap/chromium/current/usr/lib/chromium-browser/chromedriver",
    ] {
        let path = PathBuf::from(path);
        if path.exists() {
            candidates.push(path);
        }
    }
    candidates
}

/// Whether the first launch of `binary` can start without fetching anything: a patched driver
/// cached for its major, or a system driver reporting the same major.
pub fn driver_ready_for(binary: &Path) -> bool {
    let browser_major = match detect_browser_major_version(binary) {
        Ok(major) => major,
        Err(_) => return false,
    };
    let data_dir = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("doris");
    if has_patched_chromedriver(&data_dir, browser_major) {
        return true;
    }
    system_driver_candidates()
        .into_iter()
        .any(|path| driver_serves(&path, browser_major))
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
                    .chain(std::iter::repeat_n(
                        &b' ',
                        actual_len.saturating_sub(replacement.len()),
                    ))
                    .take(actual_len)
                    .copied()
                    .collect();
                result[abs_pos..end].copy_from_slice(&patch);
                search_start = abs_pos + actual_len;
                crate::log::log(
                    "browser",
                    &format!(
                        "patched cdc block at offset {}, length {}",
                        abs_pos, actual_len
                    ),
                );
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
    for path in system_driver_candidates() {
        if driver_serves(&path, browser_major) {
            return Ok(path);
        }
        crate::log::log(
            "browser",
            &format!(
                "ignoring {} -- built for another browser major",
                path.display()
            ),
        );
    }

    // Keyed by major for the same reason the patched cache is: a driver
    // downloaded for one browser cannot serve another, and the download
    // below is the expensive part of finding that out late.
    let root = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("doris")
        .join("chromedriver");
    let data_dir = root.join(browser_major.to_string());
    std::fs::create_dir_all(&data_dir)?;

    let downloaded = data_dir.join("chromedriver-linux64/chromedriver");

    if downloaded.exists() {
        return Ok(downloaded);
    }
    if let Some(adopted) = adopt_legacy_download(&root, browser_major) {
        return Ok(adopted);
    }

    crate::log::log(
        "browser",
        &format!("downloading chromedriver for Chromium {}...", browser_major),
    );

    let url = download_chromedriver_url(browser_major).await?;

    let zip = std::env::temp_dir().join(format!("chromedriver-{}.zip", browser_major));
    let zip_str = zip
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("temp path is not valid UTF-8"))?;
    let status = std::process::Command::new("curl")
        .args(["-sL", "-o", zip_str, &url])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| anyhow::anyhow!("curl failed: {}", e))?;
    if !status.success() {
        anyhow::bail!("curl failed with status {}", status);
    }

    let status = std::process::Command::new("unzip")
        .args(["-o", zip_str, "-d"])
        .arg(data_dir)
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

    crate::log::log(
        "browser",
        &format!("chromedriver downloaded -> {}", downloaded.display()),
    );
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
        let major = version_str
            .split('.')
            .next()
            .and_then(|s| s.parse::<u32>().ok());
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

fn detect_browser_major_version(binary: &Path) -> Result<u32> {
    let output = std::process::Command::new(binary)
        .arg("--version")
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to run {}: {}", binary.display(), e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let version_str = if !stdout.trim().is_empty() {
        stdout
    } else {
        stderr
    };

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

/// The cookie query, with the host as a bound parameter.
fn cookie_query() -> &'static str {
    "SELECT host_key, name, value, path, is_secure, is_httponly, encrypted_value \
     FROM cookies WHERE host_key LIKE ?"
}

/// The host whose cookies are worth reading out of a profile, or `None`
/// when the caller named no site.
fn cookie_host_for(url: &str) -> Option<String> {
    let host = url::Url::parse(url).ok()?.host_str()?.to_string();
    (!host.is_empty()).then_some(host)
}

fn extract_cookies_from_native_profile(
    profile_dir: &Path,
    host: &str,
) -> Result<Vec<serde_json::Value>> {
    let cookie_paths = [
        profile_dir.join("Default/Cookies"),
        profile_dir.join("Default/Network/Cookies"),
        profile_dir.join("Default/Cookies-journal"),
        profile_dir.join("Default/Network/Cookies-journal"),
    ];

    let cookie_db = cookie_paths
        .iter()
        .find(|p| p.exists() && p.file_name().map(|n| n == "Cookies").unwrap_or(false))
        .ok_or_else(|| anyhow::anyhow!("No Cookies database found in profile"))?;

    let tmp_copy =
        std::env::temp_dir().join(format!("doris-cookies-{}.sqlite", std::process::id()));
    std::fs::copy(cookie_db, &tmp_copy)?;

    let conn = rusqlite::Connection::open(&tmp_copy)?;
    let mut stmt = conn.prepare(cookie_query())?;
    let pattern = format!("%{host}%");

    let rows = stmt.query_map([pattern], |row| {
        let host: String = row.get(0)?;
        let name: String = row.get(1)?;
        let value: String = row.get(2)?;
        let path: String = row.get(3)?;
        let secure: bool = row.get(4)?;
        let http_only: bool = row.get(5)?;
        let encrypted: Vec<u8> = row.get(6)?;
        Ok((host, name, value, path, secure, http_only, encrypted))
    })?;

    let mut cookies = Vec::new();
    for row in rows {
        let (host, name, value, path, secure, http_only, _encrypted_value) = row?;

        // Only plain-text cookies are usable: an encrypted_value blob
        // can't be decrypted outside Chrome's profile keyring.
        let final_value = if !value.is_empty() { value } else { continue };

        let domain = if host.starts_with('.') {
            host.clone()
        } else {
            format!(".{}", host)
        };

        cookies.push(serde_json::json!({
            "name": name,
            "value": final_value,
            "domain": domain,
            "path": path,
            "secure": secure,
            "httpOnly": http_only,
        }));
    }

    let _ = std::fs::remove_file(&tmp_copy);
    Ok(cookies)
}

/// The `--flag` list handed to Chrome via `goog:chromeOptions`.
fn build_chrome_args(
    mode: BrowserVisibility,
    use_xvfb: bool,
    temp_profile: Option<&Path>,
    block_hosts: &[&str],
) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "--no-sandbox".into(),
        "--disable-dev-shm-usage".into(),
        "--disable-blink-features=AutomationControlled".into(),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
        "--lang=ru-RU".into(),
        // Chrome's own background services (component updater, domain
        // reliability reporting, metrics, component-extension background
        // pages) keep working while Doris sits idle, for no benefit to
        // us -- all off.
        "--disable-component-update".into(),
        "--disable-component-extensions-with-background-pages".into(),
        "--disable-domain-reliability".into(),
        "--metrics-recording-only".into(),
        "--no-pings".into(),
    ];

    // Hosts the caller wants kept away from the browser. Resolving them
    // to nowhere is cheaper than the alternative: an ad CDN serving
    // looping `<video>`/GIF banners keeps the compositor producing frames
    // at full speed for as long as a page stays open, which measured as
    // the largest single idle-CPU cost of a session (VizCompositor pegged
    // near 66% of a core).
    //
    // This list used to be one hardcoded host in the base args, which put
    // one tracker's ad server in a module that drives browsers for every
    // source. The caller knows what its own pages load; this layer only
    // knows how to say "do not resolve that".
    if !block_hosts.is_empty() {
        args.push(format!(
            "--host-resolver-rules={}",
            block_hosts
                .iter()
                .map(|h| format!("MAP {h} ~NOTFOUND"))
                .collect::<Vec<_>>()
                .join(",")
        ));
    }

    if mode == BrowserVisibility::Hidden && !use_xvfb {
        args.push("--headless=new".into());
    }
    if use_xvfb {
        args.push("--ozone-platform=x11".into());
    }
    args.push("--window-size=1920,1080".into());
    if let Some(dir) = temp_profile {
        args.push(format!("--user-data-dir={}", dir.display()));
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `wait_until` decides whether a process gets to die politely or gets killed, and every
    /// shutdown path routes through it.
    #[test]
    fn wait_until_reports_a_condition_that_holds() {
        // Holds immediately: no sleeping, and it must not wait out the
        // grace before noticing.
        let start = Instant::now();
        assert!(wait_until(Duration::from_secs(30), REAP_POLL, || true));
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "an already-satisfied condition returns at once, not after the grace"
        );
    }

    #[test]
    fn wait_until_gives_up_at_the_grace_and_says_so() {
        let start = Instant::now();
        let polls = std::cell::Cell::new(0u32);
        // Never holds. The `false` is the caller's cue to SIGKILL, so it
        // has to arrive when the grace is up -- neither earlier (we would
        // kill a process that was about to exit) nor never.
        let gave_up = wait_until(
            Duration::from_millis(150),
            Duration::from_millis(50),
            || {
                polls.set(polls.get() + 1);
                false
            },
        );
        assert!(!gave_up, "a condition that never held is a timeout");
        assert!(
            start.elapsed() >= Duration::from_millis(150),
            "must not escalate before the grace expires"
        );
        // And it has to be waiting *between* polls, not spinning. A
        // dropped `sleep` here turns every teardown into a busy loop for
        // the length of the grace, which no assertion above would catch.
        assert!(
            polls.get() < 20,
            "polled {} times over 150ms: the poll interval is not being honoured",
            polls.get()
        );
    }

    /// Polling stops the instant the condition holds.
    #[test]
    fn wait_until_stops_polling_once_the_condition_holds() {
        let polls = std::cell::Cell::new(0u32);
        let held = wait_until(Duration::from_secs(30), Duration::from_millis(10), || {
            polls.set(polls.get() + 1);
            polls.get() >= 3
        });
        assert!(held, "held on the third poll, well inside the grace");
        assert_eq!(
            polls.get(),
            3,
            "and did not keep polling a condition that had already held"
        );
    }

    /// The four shutdown paths are the same decision, so they read the same knob.
    #[test]
    fn the_shutdown_grace_is_short_enough_to_be_invisible() {
        assert!(
            SIGTERM_GRACE <= Duration::from_millis(600),
            "the grace is what a quitting user waits through"
        );
        assert!(
            REAP_POLL < SIGTERM_GRACE,
            "otherwise the poll interval, not the grace, is the real deadline"
        );
    }

    // The 40-line chrome-args wall inside
    // `launch` is data, not control flow -- pin its contract so the
    // extraction can't silently drop a flag.
    fn arg(args: &[String], prefix: &str) -> bool {
        args.iter().any(|a| a.starts_with(prefix))
    }

    #[test]
    fn base_args_always_present() {
        let a = build_chrome_args(BrowserVisibility::Visible, false, None, &[]);
        assert!(arg(&a, "--no-sandbox"));
        assert!(arg(&a, "--disable-dev-shm-usage"));
        assert!(arg(&a, "--disable-blink-features=AutomationControlled"));
        assert!(arg(&a, "--window-size=1920,1080"));
        // Visible mode must never go headless.
        assert!(!arg(&a, "--headless"));
        assert!(!arg(&a, "--ozone-platform"));
        assert!(!arg(&a, "--user-data-dir"));
    }

    /// The browser layer does not know which site it is being pointed at.
    #[test]
    fn no_tracker_is_named_inside_the_browser_layer() {
        for mode in [BrowserVisibility::Visible, BrowserVisibility::Hidden] {
            let a = build_chrome_args(mode, false, None, &[]);
            for arg_str in &a {
                assert!(
                    !arg_str.contains("rutracker") && !arg_str.contains("rutrk.org"),
                    "the base args name a tracker: {arg_str}"
                );
            }
        }

        // Comments are allowed to explain what used to be here, and so is
        // the test module -- which has to spell the name out in order to
        // check for it. The production half is what must be free of it.
        const TRACKER: &str = "rutracker";
        let source = include_str!("../browser/cdp.rs");
        let production = source.split("#[cfg(test)]").next().unwrap_or(source);
        for line in production.lines() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            assert!(
                !line.contains(TRACKER),
                "the browser layer still names a tracker in code: {line}"
            );
        }
    }

    /// What a source asks to have blocked is what gets blocked, and only
    /// that.
    #[test]
    fn the_block_list_is_the_callers() {
        let a = build_chrome_args(BrowserVisibility::Visible, false, None, &["rutrk.org"]);
        assert!(
            a.iter()
                .any(|x| x == "--host-resolver-rules=MAP rutrk.org ~NOTFOUND"),
            "{a:?}"
        );

        let two = build_chrome_args(
            BrowserVisibility::Visible,
            false,
            None,
            &["ads.example", "cdn.example"],
        );
        assert!(
            two.iter().any(|x| x.contains("MAP ads.example ~NOTFOUND")),
            "{two:?}"
        );
        assert!(
            two.iter().any(|x| x.contains("MAP cdn.example ~NOTFOUND")),
            "{two:?}"
        );

        let none = build_chrome_args(BrowserVisibility::Visible, false, None, &[]);
        assert!(
            !arg(&none, "--host-resolver-rules"),
            "nothing asked for, nothing blocked: {none:?}"
        );
    }

    /// The cookie query is built from the host the caller names.
    #[test]
    fn the_cookie_query_binds_its_host() {
        let sql = cookie_query();
        assert!(sql.contains("WHERE host_key LIKE"), "{sql}");
        assert!(!sql.contains("rutracker"), "{sql}");
        assert!(
            sql.contains('?'),
            "the host must be a bound parameter, not pasted in: {sql}"
        );
    }

    #[test]
    fn hidden_without_xvfb_goes_headless() {
        let a = build_chrome_args(BrowserVisibility::Hidden, false, None, &[]);
        assert!(arg(&a, "--headless=new"));
        assert!(!arg(&a, "--ozone-platform"));
    }

    #[test]
    fn hidden_with_xvfb_is_not_headless_and_uses_x11() {
        let a = build_chrome_args(BrowserVisibility::Hidden, true, None, &[]);
        assert!(!arg(&a, "--headless"));
        assert!(arg(&a, "--ozone-platform=x11"));
    }

    #[test]
    fn temp_profile_lands_in_user_data_dir() {
        let dir = std::path::Path::new("/tmp/doris-hidden-1");
        let a = build_chrome_args(BrowserVisibility::Hidden, true, Some(dir), &[]);
        assert!(a.iter().any(|x| x == "--user-data-dir=/tmp/doris-hidden-1"));
    }
}
