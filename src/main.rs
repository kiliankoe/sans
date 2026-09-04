mod keys;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Echo key events as the terminal delivers them, for diagnosing layout and terminal setup
    Keys {
        /// Ask the terminal for the kitty keyboard protocol's disambiguated escape codes
        #[arg(long)]
        enhanced: bool,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Some(Command::Keys { enhanced }) => keys::run(enhanced),
        None => {
            println!("The lesson TUI is not built yet. Try `neotype keys` to inspect key events.");
            Ok(())
        }
    }
}
