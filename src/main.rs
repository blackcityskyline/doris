use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    doris::log::init();
    let args = doris::cli::parse();

    // A subcommand is the whole program asked one question, and `--cli` is
    // one of them: `resolve_command` has already turned it into `search`
    // before this point, which is why there is no second search below.
    if let Some(command) = args.resolve_command() {
        let code = doris::app::run_command(&args, &command).await?;
        // Non-zero is a real answer: a pipeline has to be able to tell
        // "found nothing" from "found something". `exit` rather than an
        // `Err`, because an `Err` prints a message and `--json` output
        // with a debug line under it is not output a script can read.
        std::process::exit(code);
    }

    // Only the TUI reads the config up front: a command loads the one file
    // it needs when it needs it, so `--config somewhere/new.toml config
    // set ...` can create that file instead of failing on it.
    let config = doris::config::load(args.config.as_deref())?;

    // Before the terminal is taken over, and only on the way into the UI: a
    // greeting in front of a pipe would be the first thing in it.
    doris::welcome::player::play(&config);
    let mut app = doris::app::App::new(args, config).await?;
    app.run().await
}
