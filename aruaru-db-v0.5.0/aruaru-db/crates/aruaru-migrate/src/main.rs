//! aruaru-migrate CLI

use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "aruaru-migrate", about = "aruaru-DB migration tool")]
struct Cli {
    #[arg(long)]
    source: String,
    #[arg(long)]
    source_uri: String,
    #[arg(long, default_value = "postgres://root@localhost:5432/aruaru")]
    target_uri: String,
    #[arg(long, default_value = "Migration import")]
    commit_message: String,
    #[arg(long, default_value_t = 10000)]
    batch_size: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    println!("aruaru-migrate: {} → aruaru-DB", cli.source);
    // TODO: lib.rs の run_migration() を呼び出す
    Ok(())
}
