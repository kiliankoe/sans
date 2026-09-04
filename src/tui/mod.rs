//! Terminal UI. Everything that knows about ratatui or crossterm lives under here.

mod app;
mod home;
mod keyboard;
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

pub use app::App;

use crate::config;
use crate::course::Course;
use crate::store::Store;
use crate::text::Corpus;

/// Redraw interval while idle, so the live cpm and clock keep moving.
const TICK: Duration = Duration::from_millis(50);

pub fn run() -> Result<()> {
    let db = config::db_path()?;
    let mut store = Store::open(&db)?;
    let course = Course::load()?;
    let progress = store.lesson_progress()?;
    let mut app = App::new(course, Corpus::load(), progress);
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
