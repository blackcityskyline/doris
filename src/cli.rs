use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "t-hunter", about = "Rutracker TUI search & TorrServer stream")]
pub struct Args {
    pub query: Option<String>,

    #[arg(short, long)]
    pub cookie_file: Option<PathBuf>,

    #[arg(short, long)]
    pub username: Option<String>,

    #[arg(short, long)]
    pub password: Option<String>,

    #[arg(short, long)]
    pub browser: Option<String>,

    #[arg(long)]
    pub browser_mode: Option<String>,

    #[arg(long, default_value = "http://127.0.0.1:8090")]
    pub torrserver: String,

    #[arg(long, default_value_t = 14141)]
    pub bridge_port: u16,

    #[arg(long)]
    pub cli: bool,

    #[arg(long)]
    pub config: Option<PathBuf>,
}

impl Args {
    pub fn from_env() -> Self {
        let mut args = Self::parse();
        if args.username.is_none() {
            args.username = std::env::var("RUTRACKER_USER")
                .or_else(|_| std::env::var("RUTRACKER_USERNAME"))
                .ok();
        }
        if args.password.is_none() {
            args.password = std::env::var("RUTRACKER_PASS")
                .or_else(|_| std::env::var("RUTRACKER_PASSWORD"))
                .ok();
        }
        args
    }
}

pub fn parse() -> Args {
    Args::from_env()
}
