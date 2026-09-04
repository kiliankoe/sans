mod clock;
mod config;
mod course;
mod engine;
mod files;
mod keys;
mod layout;
mod stats;
mod store;
mod summary;
mod text;
mod tui;

use std::path::PathBuf;

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
    /// Type over a file, resuming where you left off
    Type { file: PathBuf },
    /// Print the generated text of a lesson stage, for tuning the generators
    Text {
        /// Lesson id, for example a08
        lesson: String,
        /// Stage: intro, bigrams, words or test
        #[arg(long, default_value = "words")]
        stage: String,
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
    /// List recent sessions and the habit line; --json dumps everything the stats screen shows
    Stats {
        /// How many sessions to show
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(long)]
        json: bool,
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
        Some(Command::Text {
            lesson,
            stage,
            seed,
        }) => summary::print_text(&lesson, &stage, seed),
        Some(Command::Stats { limit, json }) => summary::print_recent(limit, json),
        Some(Command::Keys { enhanced }) => keys::run(enhanced),
        Some(Command::Type { file }) => tui::run(Some(&file)),
        None => tui::run(None),
    }
}
