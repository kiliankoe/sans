//! The curriculum: lessons in order, which keys each unlocks, and what counts as passed.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use crate::layout;

/// Error rate at or below which a test stage passes its lesson.
pub const PASS_ERROR_RATE: f64 = 0.03;

const COURSE_TOML: &str = include_str!("../data/course.toml");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageKind {
    Intro,
    Bigrams,
    Words,
    Test,
}

impl StageKind {
    pub fn name(self) -> &'static str {
        match self {
            StageKind::Intro => "intro",
            StageKind::Bigrams => "bigrams",
            StageKind::Words => "words",
            StageKind::Test => "test",
        }
    }

    /// What the stage is called on screen; code lessons practise tokens and expressions.
    pub fn label(self, code: bool) -> &'static str {
        match (self, code) {
            (StageKind::Bigrams, true) => "tokens",
            (StageKind::Words, true) => "expressions",
            (kind, _) => kind.name(),
        }
    }

    /// Characters a stage aims for; about two minutes at a beginner's pace.
    pub fn target_len(self) -> usize {
        match self {
            StageKind::Intro => 120,
            StageKind::Bigrams => 150,
            StageKind::Words => 200,
            StageKind::Test => 250,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lesson {
    pub id: String,
    pub title: String,
    /// Graphemes this lesson introduces; empty for a review.
    pub new: Vec<String>,
    /// Code-flavoured text (tokens, expressions, snippet lines) instead of words.
    pub code: bool,
    /// A heading shown above this lesson in the list: the start of a track.
    pub section: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CourseFile {
    lesson: Vec<LessonDef>,
}

#[derive(Debug, Deserialize)]
struct LessonDef {
    id: String,
    title: String,
    #[serde(default)]
    new: Vec<String>,
    /// The new keys are the capitals of every letter learnt so far.
    #[serde(default)]
    shift: bool,
    #[serde(default)]
    code: bool,
    section: Option<String>,
}

pub struct Course {
    lessons: Vec<Lesson>,
}

impl Course {
    pub fn load() -> Result<Self> {
        Self::parse(COURSE_TOML)
    }

    pub fn parse(toml: &str) -> Result<Self> {
        let file: CourseFile = toml::from_str(toml).context("parsing the course")?;
        let mut lessons: Vec<Lesson> = Vec::new();
        let mut ids = HashSet::new();
        for def in file.lesson {
            if !ids.insert(def.id.clone()) {
                bail!("duplicate lesson id {}", def.id);
            }
            let new = if def.shift {
                capitals_of(&unlocked_by(&lessons))
            } else {
                def.new
            };
            for key in &new {
                if layout::primary(key).is_none() {
                    bail!("lesson {}: {key:?} is not on the layout", def.id);
                }
            }
            lessons.push(Lesson {
                id: def.id,
                title: def.title,
                new,
                code: def.code,
                section: def.section,
            });
        }
        Ok(Self { lessons })
    }

    pub fn lessons(&self) -> &[Lesson] {
        &self.lessons
    }

    /// Everything unlocked by the lessons before `index`.
    pub fn unlocked_before(&self, index: usize) -> Vec<String> {
        unlocked_by(&self.lessons[..index.min(self.lessons.len())])
    }

    /// Everything unlocked by the lessons up to and including `index`.
    pub fn unlocked_through(&self, index: usize) -> Vec<String> {
        self.unlocked_before(index + 1)
    }

    pub fn stages(&self, index: usize) -> Vec<StageKind> {
        let mut stages = vec![StageKind::Bigrams, StageKind::Words, StageKind::Test];
        if !self.lessons[index].new.is_empty() {
            stages.insert(0, StageKind::Intro);
        }
        stages
    }
}

/// Keys introduced by `lessons`, in order, without repeats.
fn unlocked_by(lessons: &[Lesson]) -> Vec<String> {
    let mut seen = HashSet::new();
    lessons
        .iter()
        .flat_map(|lesson| lesson.new.iter().cloned())
        .filter(|key| seen.insert(key.clone()))
        .collect()
}

/// Capitals of the keys that have one on the layout (`ß` has none).
fn capitals_of(keys: &[String]) -> Vec<String> {
    keys.iter()
        .map(|key| key.to_uppercase())
        .zip(keys)
        .filter(|(upper, key)| upper != *key && layout::primary(upper).is_some())
        .map(|(upper, _)| upper)
        .collect()
}

/// Best test error rate per lesson, derived from stored sessions.
#[derive(Debug, Default, Clone)]
pub struct Progress {
    best: HashMap<String, f64>,
}

impl Progress {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, lesson: &str, error_rate: f64) {
        let best = self.best.entry(lesson.to_string()).or_insert(error_rate);
        *best = best.min(error_rate);
    }

    pub fn best(&self, lesson: &str) -> Option<f64> {
        self.best.get(lesson).copied()
    }

    pub fn passed(&self, lesson: &str) -> bool {
        self.best(lesson)
            .is_some_and(|rate| rate <= PASS_ERROR_RATE)
    }

    /// A lesson is available once the one before it is passed.
    pub fn available(&self, course: &Course, index: usize) -> bool {
        index == 0
            || course
                .lessons
                .get(index - 1)
                .is_some_and(|prev| self.passed(&prev.id))
    }

    /// The first lesson not yet passed, or the last one when everything is.
    pub fn next_index(&self, course: &Course) -> usize {
        course
            .lessons
            .iter()
            .position(|lesson| !self.passed(&lesson.id))
            .unwrap_or(course.lessons.len().saturating_sub(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn course() -> Course {
        Course::load().unwrap()
    }

    #[test]
    fn embedded_course_parses_with_unique_ids_and_known_keys() {
        let course = course();
        assert_eq!(course.lessons().len(), 42);
        let mut ids: Vec<_> = course.lessons().iter().map(|l| l.id.as_str()).collect();
        ids.dedup();
        assert_eq!(ids.len(), 42);
        for lesson in course.lessons() {
            for key in &lesson.new {
                assert!(layout::primary(key).is_some(), "{} has no position", key);
            }
        }
    }

    #[test]
    fn unlocked_sets_accumulate() {
        let course = course();
        assert!(course.unlocked_before(0).is_empty());
        assert_eq!(course.unlocked_before(2), ["e", "n", "a", "r"]);
        assert_eq!(course.unlocked_through(2), ["e", "n", "a", "r", "u", "d"]);
    }

    #[test]
    fn stages_skip_the_intro_for_reviews() {
        let course = course();
        assert_eq!(
            course.stages(0),
            [
                StageKind::Intro,
                StageKind::Bigrams,
                StageKind::Words,
                StageKind::Test
            ]
        );
        assert_eq!(course.lessons()[4].title, "Home row check");
        assert_eq!(
            course.stages(4),
            [StageKind::Bigrams, StageKind::Words, StageKind::Test]
        );
    }

    #[test]
    fn shift_lesson_introduces_the_capitals_of_everything_before_it() {
        let course = course();
        let shift = course.lessons().iter().position(|l| l.id == "a22").unwrap();
        let new = &course.lessons()[shift].new;
        assert_eq!(new.len(), 20, "{new:?}");
        assert_eq!(new[0], "E");
        assert!(new.contains(&"B".to_string()));
        assert!(!new.contains(&",".to_string()));
        assert!(course.unlocked_through(shift).contains(&"E".to_string()));
    }

    #[test]
    fn track_b_starts_after_the_letters_with_a_section_heading() {
        let course = course();
        let b01 = course.lessons().iter().position(|l| l.id == "b01").unwrap();
        assert_eq!(b01, 29);
        let lesson = &course.lessons()[b01];
        assert_eq!(lesson.new, ["(", ")"]);
        assert!(lesson.code);
        assert_eq!(
            lesson.section.as_deref(),
            Some("Track B: symbols (layer 3)")
        );
        assert!(!course.lessons()[0].code && course.lessons()[0].section.is_none());
        let unlocked = course.unlocked_before(b01);
        assert!(unlocked.contains(&"ß".to_string()) && unlocked.contains(&"E".to_string()));
        assert!(!unlocked.contains(&"(".to_string()));
        assert_eq!(StageKind::Bigrams.label(true), "tokens");
        assert_eq!(StageKind::Words.label(true), "expressions");
        assert_eq!(StageKind::Words.label(false), "words");
        let digits = course.lessons().iter().find(|l| l.id == "b12").unwrap();
        assert_eq!(digits.new.len(), 10);
        assert_eq!(
            course.stages(41),
            [StageKind::Bigrams, StageKind::Words, StageKind::Test]
        );
    }

    #[test]
    fn parse_rejects_duplicate_ids() {
        let toml = "[[lesson]]\nid = \"x\"\ntitle = \"a\"\nnew = [\"e\"]\n[[lesson]]\nid = \"x\"\ntitle = \"b\"\n";
        assert!(Course::parse(toml).is_err());
    }

    #[test]
    fn progress_tracks_the_best_test_and_gates_lessons() {
        let course = course();
        let mut progress = Progress::new();
        assert!(progress.available(&course, 0));
        assert!(!progress.available(&course, 1));
        assert_eq!(progress.next_index(&course), 0);
        progress.record("a01", 0.05);
        assert!(!progress.passed("a01"));
        progress.record("a01", 0.02);
        progress.record("a01", 0.04);
        assert_eq!(progress.best("a01"), Some(0.02));
        assert!(progress.passed("a01"));
        assert!(progress.available(&course, 1));
        assert!(!progress.available(&course, 2));
        assert_eq!(progress.next_index(&course), 1);
        for lesson in course.lessons() {
            progress.record(&lesson.id, 0.0);
        }
        assert_eq!(progress.next_index(&course), 41);
    }
}
