//! aruaru-migrate CLI

use aruaru_migrate::{MigrateConfig, MigrateStatus, SourceKind};
use clap::Parser;

#[derive(Debug, Parser)]
#[command(name = "aruaru-migrate", about = "aruaru-DB migration tool")]
struct Cli {
    #[arg(long, value_enum)]
    source: SourceKind,
    #[arg(long)]
    source_uri: String,
    #[arg(long, default_value = "postgres://root@localhost:5432/aruaru")]
    target_uri: String,
    #[arg(long, default_value = "Migration import")]
    commit_message: String,
    #[arg(long, default_value_t = 10_000)]
    batch_size: usize,
    #[arg(long, default_value_t = 4)]
    parallelism: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    println!("aruaru-migrate: {:?} ({}) → {}", cli.source, cli.source_uri, cli.target_uri);

    let config = MigrateConfig {
        source: cli.source,
        source_uri: cli.source_uri,
        target_uri: cli.target_uri,
        batch_size: cli.batch_size,
        commit_message: cli.commit_message,
        parallelism: cli.parallelism,
    };

    aruaru_migrate::run_migration(config, |progress| {
        let status = match &progress.status {
            MigrateStatus::Pending => "pending".to_string(),
            MigrateStatus::Running => "running".to_string(),
            MigrateStatus::Done => "done".to_string(),
            MigrateStatus::Failed(e) => format!("failed: {e}"),
        };
        println!(
            "[{}] {} rows_done={} rows_total={:?}",
            progress.table, status, progress.rows_done, progress.rows_total
        );
    })
    .await?;

    println!("aruaru-migrate: completed");
    Ok(())
}
