mod clock;
mod config;
mod engine;
mod keys;
mod stats;
mod store;
mod summary;
mod tui;

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
    /// List recent sessions
    Stats {
        /// How many sessions to show
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Echo key events as the terminal delivers them, for diagnosing layout and terminal setup
    Keys {
        /// Ask the terminal for the kitty keyboard protocol's disambiguated escape codes
        #[arg(long)]
        enhanced: bool,
    },
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Some(Command::Stats { limit }) => summary::print_recent(limit),
        Some(Command::Keys { enhanced }) => keys::run(enhanced),
        None => tui::run(phase_one_drill()),
    }
}

/// The single hard-coded drill of phase 1; the course replaces it in phase 2.
fn phase_one_drill() -> tui::Drill {
    tui::Drill {
        lesson: "a01".into(),
        stage: 1,
        title: "Lesson 1: e and n".into(),
        text: "eee nnn eee nnn ene nen ene nen een nne enn nne ne en ne en nen ene nee enn een \
               nnn eee nnn eee nen ene nen ene nne een nne enn en ne en ne ene nen enn nee nne"
            .into(),
    }
}
