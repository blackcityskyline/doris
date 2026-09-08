use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    doris::log::init();
    let args = doris::cli::parse();
    let config = doris::config::load(args.config.as_deref())?;

    if args.cli {
        run_cli(args, config).await
    } else {
        let mut app = doris::app::App::new(args, config).await?;
        app.run().await
    }
}

async fn run_cli(args: doris::cli::Args, config: doris::config::Config) -> Result<()> {
    let query = args.query.as_deref().unwrap_or("");
    if query.is_empty() {
        anyhow::bail!("Search query required in CLI mode.");
    }

    let browser_choice = args.browser.as_deref().or(config.browser.as_deref());
    let (kind, path) = doris::browser::detect::detect_browser(browser_choice)?;
    let mode_str = args.browser_mode
        .clone()
        .unwrap_or_else(|| config.browser_mode.clone());
    let mode: doris::browser::cdp::BrowserVisibility = mode_str.parse()?;
    println!("Using browser: {} [{}] ({})", kind, mode, path.display());

    let browser = doris::browser::cdp::Browser::launch(&path, mode).await?;
    let browser = std::sync::Arc::new(tokio::sync::Mutex::new(browser));

    let mut searcher = doris::search::rutracker::RutrackerSearcher::new(browser);

    println!("Searching for '{}'...", query);
    println!("{}", "-".repeat(60));

    let cookie_file = args.cookie_file.as_deref();
    let username = args.username.as_deref();
    let password = args.password.as_deref();

    let log = std::sync::Arc::new(|msg: &str| println!("[log] {}", msg));

    match searcher.ensure_logged_in(cookie_file, username, password, log).await {
        Ok(true) => println!("Logged in successfully."),
        Ok(false) => println!("Not logged in."),
        Err(e) => println!("Login error: {}", e),
    }

    match searcher.search(query).await {
        Ok(results) => {
            if results.is_empty() {
                println!("\nNo results found.");
            } else {
                println!("\nFound {} results (sorted by seeds):\n", results.len());
                for (i, item) in results.iter().take(20).enumerate() {
                    println!("{}. {}", i + 1, item.title);
                    println!("   Size: {} | Seeds: {} | Date: {}", item.size, item.seeds, item.date);
                    println!("   Page: {}", item.page_url);
                    println!("   Download: {}", item.download_url);
                    println!();
                }
            }
        }
        Err(e) => {
            println!("Search failed: {}", e);
        }
    }

    Ok(())
}
