pub mod commands;

use clap::{Parser};

#[derive(Parser)]
#[command(author, version, about = "OSX Uninstaller and Cleaner")]
pub struct Cli {
    #[command(subcommand)]
    pub command: commands::Commands,

    /// Dry run mode: show what would be deleted without deleting
    #[arg(long)]
    pub dry_run: bool,
}

pub fn parse() -> Cli {
    Cli::parse()
}
