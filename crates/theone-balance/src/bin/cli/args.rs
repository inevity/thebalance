use clap::{Args, Parser, Subcommand};

use crate::cli::config::ConfigSource;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    Sync(SyncArgs),
}

#[derive(Args)]
pub struct SyncArgs {
    #[arg(short, long, value_enum)]
    pub source: ConfigSource,

    #[arg(short, long, value_enum)]
    pub target: ConfigSource,

    #[arg(long)]
    pub source_name: Option<String>,

    #[arg(long)]
    pub target_name: Option<String>,
}
