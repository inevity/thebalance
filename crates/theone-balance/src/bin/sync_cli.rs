mod cli;

use anyhow::Result;
use clap::Parser;
use cli::{
    app::App,
    args::{Cli, Commands},
    utils::init_tracing,
};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let cli = Cli::parse();
    let result = match cli.command {
        Commands::Sync(args) => App::sync(args).await,
    };

    if let Err(e) = result {
        tracing::error!("CLI operation failed: {:?}", e);
        std::process::exit(1);
    }

    Ok(())
}
