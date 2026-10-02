use anyhow::Result;

use doris::sources::source::{AuthContext, LogFn, SearchRequest};

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    doris::log::init();
    let args = doris::cli::parse();
    let config = doris::config::load(args.config.as_deref())?;

    if args.cli {
        run_cli(args, config).await
    } else {
        // Before the terminal is taken over, and only on the way into the
        // UI: `--cli` prints results a pipe is waiting for, so a greeting
        // in front of them would be the first thing in the pipe.
        doris::welcome::player::play(&config);
        let mut app = doris::app::App::new(args, config).await?;
        app.run().await
    }
}

async fn run_cli(args: doris::cli::Args, config: doris::config::Config) -> Result<()> {
    let query = args.query.as_deref().unwrap_or("");
    if query.is_empty() {
        anyhow::bail!("Search query required in CLI mode.");
    }

    // Which sources to ask: `--source <id>` names exactly one,
    let selected =
        doris::sources::source::cli_sources(args.source.as_deref(), &config.enabled_sources)?;
    if selected.is_empty() {
        anyhow::bail!("no enabled sources in Options -> streaming -> Sources");
    }

    // A browser only when some selected source needs one: rutor answers
    let needs_browser = selected.iter().any(|info| info.requires_browser);
    let browser = if needs_browser {
        let browser_choice = args.browser.as_deref().or(config.browser.as_deref());
        let browser_priority = doris::browser::detect::parse_priority(&config.browser_priority);
        let (kind, path) = doris::browser::detect::detect_browser_with_priority(
            browser_choice,
            &browser_priority,
        )?;
        let visibility_str = args
            .browser_visibility
            .clone()
            .unwrap_or_else(|| config.browser_visibility.clone());
        let visibility: doris::browser::cdp::BrowserVisibility = visibility_str.parse()?;
        println!(
            "Using browser: {} [{}] ({})",
            kind,
            visibility,
            path.display()
        );

        // One browser shared by every source, the same sharing the TUI
        let launched = doris::browser::cdp::Browser::launch(
            &path,
            visibility,
            selected[0].home_url,
            true,
            selected[0].block_hosts,
        )
        .await?;
        Some(std::sync::Arc::new(tokio::sync::Mutex::new(launched)))
    } else {
        None
    };

    // CLI flags override, otherwise the credentials the app's own login
    let credentials = match (args.username.as_deref(), args.password.as_deref()) {
        (Some(u), Some(p)) => Some((u.to_string(), p.to_string())),
        _ => doris::credentials::load_credentials(),
    };
    let (username, password) = match &credentials {
        Some((u, p)) => (Some(u.clone()), Some(p.clone())),
        None => (None, None),
    };
    let auth = AuthContext {
        cookie_file: args.cookie_file.as_deref().map(std::path::PathBuf::from),
        username,
        password,
    };
    // Progress chatter, not results: stderr keeps `doris q | grep` clean.
    let log: LogFn = std::sync::Arc::new(|msg: &str| eprintln!("[log] {}", msg));

    for info in &selected {
        println!("\n=== {} ===", info.label);

        // Same registry path as the TUI: the instance from
        let source = doris::sources::source::build_source(
            info.id,
            doris::sources::source::SourceEnv {
                browser: browser.clone(),
            },
        )?;

        match source.ensure_logged_in(&auth, &log).await {
            Ok(true) => println!("Logged in successfully."),
            Ok(false) => println!("Not logged in."),
            Err(e) => eprintln!("Login error: {}", e),
        }

        match source.search(&SearchRequest::new(query, 0)).await {
            Ok(page) => {
                let results = page.items;
                if results.is_empty() {
                    println!("No results found.");
                } else {
                    println!("Found {} results (sorted by seeds):\n", results.len());
                    for (i, item) in results.iter().take(20).enumerate() {
                        println!("{}. {}", i + 1, item.title);
                        println!(
                            "   Size: {} | Seeds: {} | Date: {}",
                            item.size, item.seeds, item.date
                        );
                        println!("   Page: {}", item.page_url);
                        println!("   Download: {}", item.download_url);
                        println!();
                    }
                }
            }
            Err(e) => {
                eprintln!("Search failed: {}", e);
            }
        }
    }

    // Same reason as in `App::run`: the session DELETE has to happen while
    if let Some(launched) = browser {
        launched.lock().await.shutdown().await;
    }

    Ok(())
}
