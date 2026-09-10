//! SQLite persistence. Sessions and their keystroke logs are the source of truth; every
//! aggregate is derived from them.

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde::Serialize;

use crate::course::{PASS_ERROR_RATE, Progress, Resume, StageKind};
use crate::engine::{Keystroke, KeystrokeKind};
use crate::stats::{self, DayStat, LessonStat, Snapshot, Stroke, Summary};

const SCHEMA_VERSION: i64 = 3;

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

/// Files: which file a session typed, and where each file resumes.
const SCHEMA_V3: &str = "
ALTER TABLE session ADD COLUMN file TEXT;
CREATE TABLE file_progress (
    path         TEXT    PRIMARY KEY,
    content_hash TEXT    NOT NULL,
    next_chunk   INTEGER NOT NULL,
    chunks       INTEGER NOT NULL,
    updated_at   TEXT    NOT NULL
);
";

pub struct Store {
    conn: Connection,
}

/// What a session was, beyond its numbers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionMeta<'a> {
    pub kind: &'a str,
    pub lesson: Option<&'a str>,
    pub file: Option<&'a str>,
    pub stage: Option<u32>,
    pub stage_kind: Option<&'a str>,
    pub seed: Option<u64>,
    pub started_at: SystemTime,
    /// False when the stage was abandoned before its last character.
    pub finished: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionRow {
    pub id: i64,
    pub started_at: String,
    pub kind: String,
    pub lesson: Option<String>,
    pub file: Option<String>,
    pub stage: Option<u32>,
    pub stage_kind: Option<String>,
    pub chars: usize,
    pub errors: usize,
    pub active_ms: u64,
    pub cpm: f64,
    pub error_rate: f64,
    pub finished: bool,
}

/// Where a file resumes. `next_chunk == chunks` means it was typed to the end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileProgress {
    pub path: String,
    pub content_hash: String,
    pub next_chunk: usize,
    pub chunks: usize,
    pub updated_at: String,
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
        let migrations = [SCHEMA_V1, SCHEMA_V2, SCHEMA_V3];
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
                (started_at, kind, lesson, file, stage, stage_kind, seed, chars, errors, active_ms,
                 cpm, error_rate, finished)
             VALUES (strftime('%Y-%m-%dT%H:%M:%SZ', ?1, 'unixepoch'), ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9,
                 ?10, ?11, ?12, ?13)",
            params![
                started_at,
                meta.kind,
                meta.lesson,
                meta.file,
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

    /// Where each lesson resumes, read off its most recently finished stage.
    pub fn lesson_resume(&self) -> Result<Resume> {
        let mut select = self.conn.prepare(
            "SELECT lesson, stage, stage_kind, error_rate FROM session s
             WHERE finished = 1 AND lesson IS NOT NULL AND stage IS NOT NULL
               AND id = (SELECT max(id) FROM session
                         WHERE lesson = s.lesson AND finished = 1 AND stage IS NOT NULL)",
        )?;
        let rows = select.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, f64>(3)?,
            ))
        })?;
        let mut resume = Resume::new();
        for row in rows {
            let (lesson, stage, stage_kind, error_rate) = row?;
            let kind = stage_kind
                .as_deref()
                .and_then(StageKind::from_name)
                .unwrap_or(StageKind::Words);
            // The stored stage counts from one.
            let stage = (stage as usize).saturating_sub(1);
            resume.finished(&lesson, stage, kind, error_rate <= PASS_ERROR_RATE);
        }
        Ok(resume)
    }

    pub fn file_progress(&self, path: &str) -> Result<Option<FileProgress>> {
        let mut select = self.conn.prepare(
            "SELECT path, content_hash, next_chunk, chunks, updated_at FROM file_progress WHERE path = ?1",
        )?;
        let mut rows = select.query_map(params![path], file_progress_row)?;
        Ok(rows.next().transpose()?)
    }

    pub fn set_file_progress(
        &mut self,
        path: &str,
        content_hash: &str,
        next_chunk: usize,
        chunks: usize,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO file_progress (path, content_hash, next_chunk, chunks, updated_at)
             VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
             ON CONFLICT(path) DO UPDATE SET
                content_hash = excluded.content_hash,
                next_chunk = excluded.next_chunk,
                chunks = excluded.chunks,
                updated_at = excluded.updated_at",
            params![path, content_hash, next_chunk as i64, chunks as i64],
        )?;
        Ok(())
    }

    /// Most recently practised first.
    pub fn recent_files(&self, limit: usize) -> Result<Vec<FileProgress>> {
        let mut select = self.conn.prepare(
            "SELECT path, content_hash, next_chunk, chunks, updated_at FROM file_progress
             ORDER BY updated_at DESC, path LIMIT ?1",
        )?;
        let rows = select.query_map(params![limit as i64], file_progress_row)?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Local date of today, from SQLite's clock, `YYYY-MM-DD`.
    pub fn today(&self) -> Result<String> {
        Ok(self
            .conn
            .query_row("SELECT date('now', 'localtime')", [], |row| row.get(0))?)
    }

    /// Practice per local day, oldest first.
    pub fn daily(&self) -> Result<Vec<DayStat>> {
        let mut select = self.conn.prepare(
            "SELECT date(started_at, 'localtime') AS day, count(*), sum(chars), sum(errors), sum(active_ms)
             FROM session GROUP BY day ORDER BY day",
        )?;
        let rows = select.query_map([], |row| {
            Ok(DayStat::new(
                row.get(0)?,
                row.get::<_, i64>(1)? as u32,
                row.get::<_, i64>(2)? as u64,
                row.get::<_, i64>(3)? as u64,
                row.get::<_, i64>(4)? as u64,
            ))
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn lesson_stats(&self) -> Result<Vec<LessonStat>> {
        let mut select = self.conn.prepare(
            "SELECT lesson,
                    count(*),
                    sum(CASE WHEN stage_kind = 'test' AND finished THEN 1 ELSE 0 END),
                    min(CASE WHEN stage_kind = 'test' AND finished THEN error_rate END),
                    max(CASE WHEN finished THEN cpm END),
                    min(CASE WHEN stage_kind = 'test' AND finished AND error_rate <= ?1
                             THEN started_at END)
             FROM session WHERE lesson IS NOT NULL GROUP BY lesson ORDER BY lesson",
        )?;
        let rows = select.query_map(params![PASS_ERROR_RATE], |row| {
            Ok(LessonStat {
                lesson: row.get(0)?,
                attempts: row.get::<_, i64>(1)? as u32,
                tests: row.get::<_, i64>(2)? as u32,
                best_error_rate: row.get(3)?,
                best_cpm: row.get(4)?,
                passed_at: row.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    /// Every keystroke with its predecessor in the same session.
    pub fn strokes(&self) -> Result<Vec<Stroke>> {
        let mut select = self.conn.prepare(
            "SELECT expected, kind, lag(expected) OVER w, lag(kind) OVER w,
                    offset_ms - lag(offset_ms) OVER w
             FROM keystroke WINDOW w AS (PARTITION BY session_id ORDER BY seq)
             ORDER BY session_id, seq",
        )?;
        let rows = select.query_map([], |row| {
            Ok(Stroke {
                expected: row.get(0)?,
                kind: kind_from_name(&row.get::<_, String>(1)?),
                prev_expected: row.get(2)?,
                prev_kind: row
                    .get::<_, Option<String>>(3)?
                    .map(|name| kind_from_name(&name)),
                interval_ms: row.get::<_, Option<i64>>(4)?.map(|ms| ms.max(0) as u64),
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn snapshot(&self, target_minutes: u32) -> Result<Snapshot> {
        let days = self.daily()?;
        let strokes = self.strokes()?;
        let today = self.today()?;
        Ok(Snapshot {
            habit: stats::habit(&days, &today, target_minutes),
            today,
            days,
            lessons: self.lesson_stats()?,
            keys: stats::aggregate_keys(&strokes),
            bigrams: stats::aggregate_bigrams(&strokes),
        })
    }

    /// Most recent sessions first.
    pub fn recent_sessions(&self, limit: usize) -> Result<Vec<SessionRow>> {
        let mut select = self.conn.prepare(
            "SELECT id, started_at, kind, lesson, file, stage, stage_kind, chars, errors, active_ms,
                    cpm, error_rate, finished
             FROM session ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = select.query_map(params![limit as i64], |row| {
            Ok(SessionRow {
                id: row.get(0)?,
                started_at: row.get(1)?,
                kind: row.get(2)?,
                lesson: row.get(3)?,
                file: row.get(4)?,
                stage: row.get(5)?,
                stage_kind: row.get(6)?,
                chars: row.get::<_, i64>(7)? as usize,
                errors: row.get::<_, i64>(8)? as usize,
                active_ms: row.get::<_, i64>(9)? as u64,
                cpm: row.get(10)?,
                error_rate: row.get(11)?,
                finished: row.get(12)?,
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

fn file_progress_row(row: &rusqlite::Row) -> rusqlite::Result<FileProgress> {
    Ok(FileProgress {
        path: row.get(0)?,
        content_hash: row.get(1)?,
        next_chunk: row.get::<_, i64>(2)? as usize,
        chunks: row.get::<_, i64>(3)? as usize,
        updated_at: row.get(4)?,
    })
}

fn kind_from_name(name: &str) -> KeystrokeKind {
    match name {
        "wrong" => KeystrokeKind::Wrong,
        "backspace" => KeystrokeKind::Backspace,
        _ => KeystrokeKind::Correct,
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
            file: None,
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
    fn a_lesson_resumes_after_its_last_finished_stage() {
        let mut store = Store::open_in_memory().unwrap();
        let log = sample_log();
        let mut summary = crate::stats::summarize(&log);
        let mut record = |lesson: &str, stage: u32, stage_kind: &str, finished, error_rate| {
            summary.error_rate = error_rate;
            let meta = SessionMeta {
                lesson: Some(lesson),
                stage: Some(stage),
                stage_kind: Some(stage_kind),
                finished,
                ..sample_meta()
            };
            store.record(&meta, &summary, &log).unwrap();
        };
        record("a01", 1, "intro", true, 0.0);
        record("a01", 2, "bigrams", true, 0.0);
        record("a01", 3, "words", false, 0.0);
        record("a02", 4, "test", true, 0.5);
        record("a03", 4, "test", true, 0.0);
        let resume = store.lesson_resume().unwrap();
        assert_eq!(resume.stage("a01"), 2, "an abandoned stage is not finished");
        assert_eq!(resume.stage("a02"), 3, "a failed test is repeated");
        assert_eq!(resume.stage("a03"), 0, "a passed lesson starts over");
        assert_eq!(resume.stage("a04"), 0, "untouched lessons start at the top");
    }

    #[test]
    fn a_version_one_database_is_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sans.db");
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
    fn daily_lesson_and_stroke_queries_feed_a_snapshot() {
        let mut store = Store::open_in_memory().unwrap();
        let log = sample_log();
        let summary = crate::stats::summarize(&log);
        let day = |offset_days: u64| {
            SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000 + offset_days * 86_400)
        };
        let base = sample_meta();
        store
            .record(
                &SessionMeta {
                    started_at: day(0),
                    ..base.clone()
                },
                &summary,
                &log,
            )
            .unwrap();
        store
            .record(
                &SessionMeta {
                    started_at: day(1),
                    stage_kind: Some("test"),
                    ..base.clone()
                },
                &summary,
                &log,
            )
            .unwrap();
        store
            .record(
                &SessionMeta {
                    started_at: day(1),
                    stage_kind: Some("test"),
                    finished: false,
                    ..base
                },
                &summary,
                &log,
            )
            .unwrap();

        let days = store.daily().unwrap();
        assert_eq!(days.len(), 2);
        assert_eq!((days[0].sessions, days[1].sessions), (1, 2));
        assert_eq!(days[1].chars, 4);
        assert_eq!(days[1].active_ms, 1200);

        let lessons = store.lesson_stats().unwrap();
        assert_eq!(lessons.len(), 1);
        let a01 = &lessons[0];
        assert_eq!((a01.attempts, a01.tests), (3, 1));
        assert!((a01.best_error_rate.unwrap() - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(a01.passed_at, None, "a third of errors does not pass");

        let strokes = store.strokes().unwrap();
        assert_eq!(strokes.len(), 12);
        assert_eq!(strokes[0].prev_expected, None);
        assert_eq!(strokes[1].prev_expected.as_deref(), Some("e"));
        assert_eq!(strokes[1].interval_ms, Some(200));
        assert_eq!(strokes[4].prev_expected, None, "sessions do not chain");

        let snapshot = store.snapshot(15).unwrap();
        assert_eq!(snapshot.habit.sessions, 3);
        assert!(snapshot.keys.iter().any(|k| k.key == "n" && k.errors == 3));
        assert_eq!(snapshot.today.len(), 10);
    }

    #[test]
    fn file_progress_is_upserted_and_listed_by_recency() {
        let mut store = Store::open_in_memory().unwrap();
        assert_eq!(store.file_progress("/a.rs").unwrap(), None);
        store.set_file_progress("/a.rs", "hash-a", 1, 4).unwrap();
        store.set_file_progress("/b.rs", "hash-b", 0, 2).unwrap();
        store.set_file_progress("/a.rs", "hash-a", 2, 4).unwrap();
        let a = store.file_progress("/a.rs").unwrap().unwrap();
        assert_eq!(
            (a.next_chunk, a.chunks, a.content_hash.as_str()),
            (2, 4, "hash-a")
        );
        let recent = store.recent_files(10).unwrap();
        assert_eq!(
            recent.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
            ["/a.rs", "/b.rs"]
        );
        assert_eq!(store.recent_files(1).unwrap().len(), 1);
        let log = sample_log();
        let meta = SessionMeta {
            kind: "file",
            lesson: None,
            file: Some("/a.rs"),
            stage_kind: Some("chunk"),
            ..sample_meta()
        };
        store
            .record(&meta, &crate::stats::summarize(&log), &log)
            .unwrap();
        assert_eq!(
            store.recent_sessions(1).unwrap()[0].file.as_deref(),
            Some("/a.rs")
        );
        assert!(
            store.lesson_stats().unwrap().is_empty(),
            "file sessions are not lessons"
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
        let path = dir.path().join("nested").join("sans.db");
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
