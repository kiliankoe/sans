//! Terminal UI. Everything that knows about ratatui or crossterm lives under here.

mod app;
mod files;
mod home;
mod keyboard;
mod results;
mod stats;
mod typing;

use std::io::stdout;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, EnableBracketedPaste, EnableFocusChange,
};
use crossterm::execute;
use ratatui::DefaultTerminal;

pub use app::App;
use app::Effect;

use crate::config::{self, Config};
use crate::course::Course;
use crate::store::Store;
use crate::text::Corpus;

/// Redraw interval while idle, so the live cpm and clock keep moving.
const TICK: Duration = Duration::from_millis(50);

/// Runs the TUI, straight into `file` when one is given.
pub fn run(file: Option<&Path>) -> Result<()> {
    let db = config::db_path()?;
    let mut store = Store::open(&db)?;
    let config = Config::load()?;
    let course = Course::load()?;
    let progress = store.lesson_progress()?;
    let snapshot = store.snapshot(config.daily_minutes)?;
    let recent = store.recent_files(App::recent_files_limit())?;
    let mut app = App::new(
        course,
        Corpus::load(),
        progress,
        snapshot,
        recent,
        config.indent.into(),
    );
    if let Some(path) = file {
        let session = crate::files::open(path, &store)?;
        remember(&mut store, &session)?;
        app.set_files(store.recent_files(App::recent_files_limit())?);
        app.start_file(session, Instant::now());
    }
    ratatui::run(|terminal| {
        execute!(stdout(), EnableBracketedPaste, EnableFocusChange)?;
        let result = event_loop(terminal, &mut app, &mut store);
        let _ = execute!(stdout(), DisableFocusChange, DisableBracketedPaste);
        result
    })
}

fn event_loop(terminal: &mut DefaultTerminal, app: &mut App, store: &mut Store) -> Result<()> {
    let target_minutes = app.snapshot().habit.target_minutes;
    while !app.should_quit() {
        terminal.draw(|frame| app.render(frame, Instant::now()))?;
        if !event::poll(TICK)? {
            continue;
        }
        let event = event::read()?;
        match app.handle(event, Instant::now()) {
            Some(Effect::Store(end)) => {
                if !end.log.is_empty() {
                    store
                        .record(&end.meta(), &end.summary, &end.log)
                        .context("saving the session")?;
                }
                if let Some(progress) = &end.file_progress {
                    store.set_file_progress(
                        &progress.path,
                        &progress.content_hash,
                        progress.next_chunk,
                        progress.chunks,
                    )?;
                }
                app.set_snapshot(store.snapshot(target_minutes)?);
                app.set_files(store.recent_files(App::recent_files_limit())?);
            }
            Some(Effect::OpenFile(path)) => match crate::files::open(Path::new(&path), store) {
                Ok(session) => {
                    remember(store, &session)?;
                    app.set_files(store.recent_files(App::recent_files_limit())?);
                    app.start_file(session, Instant::now());
                }
                Err(error) => app.notify(&format!("{error:#}"), Instant::now()),
            },
            None => {}
        }
    }
    Ok(())
}

/// Puts a freshly opened file at the top of the files list, progress unchanged.
fn remember(store: &mut Store, session: &crate::files::FileSession) -> Result<()> {
    store.set_file_progress(
        &session.path,
        &session.content_hash,
        session.next_chunk,
        session.chunks.len(),
    )
}
