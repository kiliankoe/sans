//! SQLite persistence. Sessions and their keystroke logs are the source of truth; every
//! aggregate is derived from them.

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{Connection, params};

use crate::course::Progress;
use crate::engine::{Keystroke, KeystrokeKind};
use crate::stats::Summary;

const SCHEMA_VERSION: i64 = 2;

const SCHEMA_V1: &str = "
CREATE TABLE session (
    id          INTEGER PRIMARY KEY,
    started_at  TEXT    NOT NULL,
    kind        TEXT    NOT NULL,
    lesson      TEXT,
    stage       INTEGER,
    seed        INTEGER,
    chars       INTEGER NOT NULL,
    errors      INTEGER NOT NULL,
    active_ms   INTEGER NOT NULL,
    cpm         REAL    NOT NULL,
    error_rate  REAL    NOT NULL,
    finished    INTEGER NOT NULL
);
CREATE TABLE keystroke (
    session_id  INTEGER NOT NULL REFERENCES session(id),
    seq         INTEGER NOT NULL,
    offset_ms   INTEGER NOT NULL,
    expected    TEXT    NOT NULL,
    typed       TEXT    NOT NULL,
    kind        TEXT    NOT NULL,
    PRIMARY KEY (session_id, seq)
);
";

/// Which stage of a lesson a session was, so passing can be derived from test stages.
const SCHEMA_V2: &str = "ALTER TABLE session ADD COLUMN stage_kind TEXT;";

pub struct Store {
    conn: Connection,
}

/// What a session was, beyond its numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionMeta<'a> {
    pub kind: &'a str,
    pub lesson: Option<&'a str>,
    pub stage: Option<u32>,
    pub stage_kind: Option<&'a str>,
    pub seed: Option<u64>,
    pub started_at: SystemTime,
    /// False when the stage was abandoned before its last character.
    pub finished: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionRow {
    pub id: i64,
    pub started_at: String,
    pub kind: String,
    pub lesson: Option<String>,
    pub stage: Option<u32>,
    pub stage_kind: Option<String>,
    pub chars: usize,
    pub errors: usize,
    pub active_ms: u64,
    pub cpm: f64,
    pub error_rate: f64,
    pub finished: bool,
}

impl Store {
    /// Opens (and creates) the database file, creating parent directories as needed.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let conn = Connection::open(path)
            .with_context(|| format!("opening database {}", path.display()))?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        let store = Self { conn };
        let migrations = [SCHEMA_V1, SCHEMA_V2];
        let applied = store.schema_version()?.clamp(0, SCHEMA_VERSION) as usize;
        for (index, migration) in migrations.iter().enumerate().skip(applied) {
            store.conn.execute_batch(migration)?;
            store
                .conn
                .pragma_update(None, "user_version", index as i64 + 1)?;
        }
        Ok(store)
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    /// Stores a session with its keystrokes and returns the session id.
    pub fn record(
        &mut self,
        meta: &SessionMeta,
        summary: &Summary,
        log: &[Keystroke],
    ) -> Result<i64> {
        let started_at = meta
            .started_at
            .duration_since(UNIX_EPOCH)
            .context("session start before the unix epoch")?
            .as_secs() as i64;
        let tx = self.conn.transaction()?;
        tx.execute(
            "INSERT INTO session
                (started_at, kind, lesson, stage, stage_kind, seed, chars, errors, active_ms, cpm,
                 error_rate, finished)
             VALUES (strftime('%Y-%m-%dT%H:%M:%SZ', ?1, 'unixepoch'), ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                 ?10, ?11, ?12)",
            params![
                started_at,
                meta.kind,
                meta.lesson,
                meta.stage,
                meta.stage_kind,
                meta.seed.map(|seed| seed as i64),
                summary.chars as i64,
                summary.errors as i64,
                summary.active.as_millis() as i64,
                summary.cpm,
                summary.error_rate,
                meta.finished,
            ],
        )?;
        let id = tx.last_insert_rowid();
        {
            let mut insert = tx.prepare(
                "INSERT INTO keystroke (session_id, seq, offset_ms, expected, typed, kind)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for (seq, stroke) in log.iter().enumerate() {
                insert.execute(params![
                    id,
                    seq as i64,
                    stroke.at.as_millis() as i64,
                    stroke.expected,
                    stroke.typed,
                    kind_name(stroke.kind),
                ])?;
            }
        }
        tx.commit()?;
        Ok(id)
    }

    /// Best error rate per lesson over finished test stages.
    pub fn lesson_progress(&self) -> Result<Progress> {
        let mut select = self.conn.prepare(
            "SELECT lesson, min(error_rate) FROM session
             WHERE stage_kind = 'test' AND finished = 1 AND lesson IS NOT NULL
             GROUP BY lesson",
        )?;
        let rows = select.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
        })?;
        let mut progress = Progress::new();
        for row in rows {
            let (lesson, best) = row?;
            progress.record(&lesson, best);
        }
        Ok(progress)
    }

    /// Most recent sessions first.
    pub fn recent_sessions(&self, limit: usize) -> Result<Vec<SessionRow>> {
        let mut select = self.conn.prepare(
            "SELECT id, started_at, kind, lesson, stage, stage_kind, chars, errors, active_ms, cpm,
                    error_rate, finished
             FROM session ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = select.query_map(params![limit as i64], |row| {
            Ok(SessionRow {
                id: row.get(0)?,
                started_at: row.get(1)?,
                kind: row.get(2)?,
                lesson: row.get(3)?,
                stage: row.get(4)?,
                stage_kind: row.get(5)?,
                chars: row.get::<_, i64>(6)? as usize,
                errors: row.get::<_, i64>(7)? as usize,
                active_ms: row.get::<_, i64>(8)? as u64,
                cpm: row.get(9)?,
                error_rate: row.get(10)?,
                finished: row.get(11)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    #[cfg(test)]
    pub fn keystroke_count(&self, session_id: i64) -> Result<usize> {
        let count: i64 = self.conn.query_row(
            "SELECT count(*) FROM keystroke WHERE session_id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        Ok(count as usize)
    }
}

fn kind_name(kind: KeystrokeKind) -> &'static str {
    match kind {
        KeystrokeKind::Correct => "correct",
        KeystrokeKind::Wrong => "wrong",
        KeystrokeKind::Backspace => "backspace",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn sample_log() -> Vec<Keystroke> {
        let stroke = |ms, expected: &str, typed: &str, kind| Keystroke {
            at: Duration::from_millis(ms),
            expected: expected.into(),
            typed: typed.into(),
            kind,
        };
        vec![
            stroke(0, "e", "e", KeystrokeKind::Correct),
            stroke(200, "n", "e", KeystrokeKind::Wrong),
            stroke(400, "n", "", KeystrokeKind::Backspace),
            stroke(600, "n", "n", KeystrokeKind::Correct),
        ]
    }

    fn sample_meta() -> SessionMeta<'static> {
        SessionMeta {
            kind: "lesson",
            lesson: Some("a01"),
            stage: Some(1),
            stage_kind: Some("intro"),
            seed: Some(42),
            started_at: SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000),
            finished: true,
        }
    }

    #[test]
    fn fresh_database_has_the_current_schema_version() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn records_a_session_and_its_keystrokes() {
        let mut store = Store::open_in_memory().unwrap();
        let log = sample_log();
        let summary = crate::stats::summarize(&log);
        let id = store.record(&sample_meta(), &summary, &log).unwrap();
        assert_eq!(store.keystroke_count(id).unwrap(), 4);
        let rows = store.recent_sessions(10).unwrap();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.id, id);
        assert_eq!(row.kind, "lesson");
        assert_eq!(row.lesson.as_deref(), Some("a01"));
        assert_eq!(row.stage, Some(1));
        assert_eq!((row.chars, row.errors, row.active_ms), (2, 1, 600));
        assert!((row.error_rate - 1.0 / 3.0).abs() < 1e-9);
        assert!(row.finished);
        assert_eq!(row.started_at, "2027-01-15T08:00:00Z");
    }

    #[test]
    fn lesson_progress_comes_from_finished_test_stages_only() {
        let mut store = Store::open_in_memory().unwrap();
        let log = sample_log();
        let mut summary = crate::stats::summarize(&log);
        let mut record = |lesson: &str, stage_kind: &str, finished: bool, error_rate: f64| {
            summary.error_rate = error_rate;
            let meta = SessionMeta {
                lesson: Some(lesson),
                stage_kind: Some(stage_kind),
                finished,
                ..sample_meta()
            };
            store.record(&meta, &summary, &log).unwrap();
        };
        record("a01", "test", true, 0.05);
        record("a01", "test", true, 0.02);
        record("a02", "words", true, 0.0);
        record("a03", "test", false, 0.0);
        let progress = store.lesson_progress().unwrap();
        assert_eq!(progress.best("a01"), Some(0.02));
        assert!(progress.passed("a01"));
        assert!(
            !progress.passed("a02"),
            "a words stage does not pass a lesson"
        );
        assert!(
            !progress.passed("a03"),
            "an abandoned test does not pass a lesson"
        );
    }

    #[test]
    fn a_version_one_database_is_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("neotype.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(SCHEMA_V1).unwrap();
            conn.pragma_update(None, "user_version", 1).unwrap();
        }
        let mut store = Store::open(&path).unwrap();
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
        let log = sample_log();
        store
            .record(&sample_meta(), &crate::stats::summarize(&log), &log)
            .unwrap();
        assert_eq!(
            store.recent_sessions(1).unwrap()[0].stage_kind.as_deref(),
            Some("intro")
        );
    }

    #[test]
    fn recent_sessions_are_newest_first_and_limited() {
        let mut store = Store::open_in_memory().unwrap();
        let log = sample_log();
        let summary = crate::stats::summarize(&log);
        for stage in 1..=3 {
            let meta = SessionMeta {
                stage: Some(stage),
                ..sample_meta()
            };
            store.record(&meta, &summary, &log).unwrap();
        }
        let rows = store.recent_sessions(2).unwrap();
        assert_eq!(
            rows.iter().map(|r| r.stage).collect::<Vec<_>>(),
            [Some(3), Some(2)]
        );
    }

    #[test]
    fn opens_a_file_creating_parent_directories_and_reopens_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("neotype.db");
        {
            let mut store = Store::open(&path).unwrap();
            let log = sample_log();
            store
                .record(&sample_meta(), &crate::stats::summarize(&log), &log)
                .unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(store.recent_sessions(10).unwrap().len(), 1);
        assert_eq!(store.schema_version().unwrap(), SCHEMA_VERSION);
    }
}
