//! `neotype stats`: the plain-text view of what is stored. The charts come in phase 3.

use anyhow::{Result, bail};
use serde::Serialize;

use crate::config::{self, Config};
use crate::course::{Course, StageKind};
use crate::stats::{Habit, Snapshot};
use crate::store::{SessionRow, Store};
use crate::text::{self, Corpus, StageSpec};

pub fn habit_line(habit: &Habit) -> String {
    format!(
        "streak {} {}   today {:.0} of {} min   total {:.1} h   {} sessions",
        habit.streak_days,
        if habit.streak_days == 1 {
            "day"
        } else {
            "days"
        },
        habit.today_minutes,
        habit.target_minutes,
        habit.total_hours,
        habit.sessions
    )
}

/// `neotype text`: what a stage would look like, without typing it.
pub fn print_text(lesson: &str, stage: &str, seed: u64) -> Result<()> {
    let course = Course::load()?;
    let Some(index) = course.lessons().iter().position(|l| l.id == lesson) else {
        bail!(
            "no lesson {lesson}; ids run a01 to a{:02}",
            course.lessons().len()
        );
    };
    let kind = match stage {
        "intro" => StageKind::Intro,
        "bigrams" => StageKind::Bigrams,
        "words" => StageKind::Words,
        "test" => StageKind::Test,
        other => bail!("unknown stage {other}; use intro, bigrams, words or test"),
    };
    let unlocked = course.unlocked_through(index);
    let lesson = &course.lessons()[index];
    let spec = StageSpec {
        kind,
        new: &lesson.new,
        unlocked: &unlocked,
        code: lesson.code,
        weakness: &text::Weakness::none(),
        seed,
    };
    println!("{}", text::generate(&spec, &Corpus::load()));
    Ok(())
}

#[derive(Serialize)]
struct JsonOutput<'a> {
    #[serde(flatten)]
    snapshot: &'a Snapshot,
    sessions: &'a [SessionRow],
}

pub fn print_recent(limit: usize, json: bool) -> Result<()> {
    let store = Store::open(&config::db_path()?)?;
    let sessions = store.recent_sessions(limit)?;
    let snapshot = store.snapshot(Config::load()?.daily_minutes)?;
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&JsonOutput {
                snapshot: &snapshot,
                sessions: &sessions
            })?
        );
        return Ok(());
    }
    if sessions.is_empty() {
        println!("No sessions yet.");
        return Ok(());
    }
    println!("{}\n", habit_line(&snapshot.habit));
    println!("{}", header());
    for session in &sessions {
        println!("{}", format_row(session));
    }
    Ok(())
}

fn header() -> String {
    format!(
        "{:<20} {:<20} {:>5} {:>6} {:>7} {:>5} {}",
        "started", "lesson", "chars", "errors", "err %", "cpm", "state"
    )
}

pub fn format_row(session: &SessionRow) -> String {
    let file_name = |path: &str| {
        std::path::Path::new(path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string())
    };
    let lesson = match (
        &session.lesson,
        &session.file,
        &session.stage_kind,
        session.stage,
    ) {
        (Some(lesson), _, Some(kind), _) => format!("{lesson}/{kind}"),
        (Some(lesson), _, None, Some(stage)) => format!("{lesson}/{stage}"),
        (Some(lesson), _, None, None) => lesson.clone(),
        (None, Some(file), _, Some(stage)) => format!("{}/{stage}", file_name(file)),
        (None, Some(file), _, None) => file_name(file),
        (None, None, _, _) => session.kind.clone(),
    };
    format!(
        "{:<20} {:<20} {:>5} {:>6} {:>6.2}% {:>5.0} {}",
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
            file: None,
            stage: Some(2),
            stage_kind: Some("bigrams".into()),
            chars: 150,
            errors: 3,
            active_ms: 60_000,
            cpm: 150.0,
            error_rate: 3.0 / 153.0,
            finished: true,
        };
        let line = format_row(&row);
        assert!(line.contains("a01/bigrams"), "{line}");
        assert!(line.contains("1.96%"), "{line}");
        assert!(line.contains("150"), "{line}");
        assert!(line.ends_with("done"), "{line}");
    }
}
