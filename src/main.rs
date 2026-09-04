mod clock;
mod config;
mod course;
mod engine;
mod keys;
mod layout;
mod stats;
mod store;
mod summary;
mod text;
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
        Some(Command::Text {
            lesson,
            stage,
            seed,
        }) => summary::print_text(&lesson, &stage, seed),
        Some(Command::Stats { limit }) => summary::print_recent(limit),
        Some(Command::Keys { enhanced }) => keys::run(enhanced),
        None => tui::run(),
    }
}
