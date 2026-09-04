//! Terminal UI. Everything that knows about ratatui or crossterm lives under here.

mod app;
mod results;
mod typing;

use std::io::stdout;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, EnableFocusChange,
};
use crossterm::execute;
use ratatui::DefaultTerminal;

pub use app::{App, Drill};

use crate::config;
use crate::store::Store;

/// Redraw interval while idle, so the live cpm and clock keep moving.
const TICK: Duration = Duration::from_millis(50);

pub fn run(drill: Drill) -> Result<()> {
    let db = config::db_path()?;
    let mut store = Store::open(&db)?;
    let mut app = App::new(drill);
    ratatui::run(|terminal| {
        execute!(stdout(), EnableBracketedPaste, EnableFocusChange)?;
        let result = event_loop(terminal, &mut app, &mut store);
        let _ = execute!(stdout(), DisableFocusChange, DisableBracketedPaste);
        result
    })
}

fn event_loop(terminal: &mut DefaultTerminal, app: &mut App, store: &mut Store) -> Result<()> {
    while !app.should_quit() {
        terminal.draw(|frame| app.render(frame, Instant::now()))?;
        if event::poll(TICK)? {
            let event = event::read()?;
            if let Some(end) = app.handle(event, Instant::now()) {
                store
                    .record(&end.meta(), &end.summary, &end.log)
                    .context("saving the session")?;
            }
        }
    }
    Ok(())
}
