//! `neotype stats`: the plain-text view of what is stored. The charts come in phase 3.

use anyhow::Result;

use crate::config;
use crate::store::{SessionRow, Store};

pub fn print_recent(limit: usize) -> Result<()> {
    let store = Store::open(&config::db_path()?)?;
    let sessions = store.recent_sessions(limit)?;
    if sessions.is_empty() {
        println!("No sessions yet.");
        return Ok(());
    }
    println!("{}", header());
    for session in &sessions {
        println!("{}", format_row(session));
    }
    Ok(())
}

fn header() -> String {
    format!(
        "{:<20} {:<12} {:>5} {:>6} {:>7} {:>5} {}",
        "started", "lesson", "chars", "errors", "err %", "cpm", "state"
    )
}

pub fn format_row(session: &SessionRow) -> String {
    let lesson = match (&session.lesson, session.stage) {
        (Some(lesson), Some(stage)) => format!("{lesson}/{stage}"),
        (Some(lesson), None) => lesson.clone(),
        (None, _) => session.kind.clone(),
    };
    format!(
        "{:<20} {:<12} {:>5} {:>6} {:>6.2}% {:>5.0} {}",
        session.started_at,
        lesson,
        session.chars,
        session.errors,
        session.error_rate * 100.0,
        session.cpm,
        if session.finished { "done" } else { "aborted" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_shows_lesson_stage_and_percentages() {
        let row = SessionRow {
            id: 1,
            started_at: "2026-09-04T18:00:00Z".into(),
            kind: "lesson".into(),
            lesson: Some("a01".into()),
            stage: Some(2),
            chars: 150,
            errors: 3,
            active_ms: 60_000,
            cpm: 150.0,
            error_rate: 3.0 / 153.0,
            finished: true,
        };
        let line = format_row(&row);
        assert!(line.contains("a01/2"), "{line}");
        assert!(line.contains("1.96%"), "{line}");
        assert!(line.contains("150"), "{line}");
        assert!(line.ends_with("done"), "{line}");
    }
}
