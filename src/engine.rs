//! The typing engine: a target text, a cursor, and the correct-mode rule.
//!
//! Correct mode: a wrong key is inserted and the cursor visually advances past it; from then
//! on only Backspace is accepted until the wrong character is gone. The engine knows nothing
//! about terminals; it takes graphemes and offsets since session start.

use std::time::Duration;

use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

/// A key as the app hands it to the engine. `Char` holds one grapheme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    Char(String),
    Backspace,
    Enter,
    Tab,
}

/// What happened to an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Correct,
    Wrong,
    /// A key other than Backspace while a wrong character is in the buffer.
    Refused,
    /// Backspace removed the wrong character.
    Corrected,
    /// Backspace with nothing to correct.
    Ignored,
    /// The last character was typed correctly.
    Finished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeystrokeKind {
    Correct,
    Wrong,
    Backspace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keystroke {
    pub at: Duration,
    pub expected: String,
    pub typed: String,
    pub kind: KeystrokeKind,
}

pub struct Engine {
    target: Vec<String>,
    /// Graphemes the engine types by itself: auto-inserted indentation and characters the
    /// layout cannot produce. Skipped when the cursor reaches them, never logged.
    given: Vec<bool>,
    /// Index of the next expected grapheme.
    cursor: usize,
    /// The wrong grapheme sitting in the buffer, if any. Correct mode allows at most one.
    wrong: Option<String>,
    log: Vec<Keystroke>,
}

impl Engine {
    pub fn new(text: &str) -> Self {
        let normalised: String = text.nfc().collect();
        let target: Vec<String> = normalised.graphemes(true).map(str::to_string).collect();
        let given = vec![false; target.len()];
        Self::with_given(target, given)
    }

    /// `given` marks, per grapheme of `target`, what the engine types by itself.
    pub fn with_given(target: Vec<String>, mut given: Vec<bool>) -> Self {
        given.resize(target.len(), false);
        let mut engine = Self {
            target,
            given,
            cursor: 0,
            wrong: None,
            log: Vec::new(),
        };
        engine.skip_given();
        engine
    }

    fn skip_given(&mut self) {
        while self.cursor < self.target.len() && self.given[self.cursor] {
            self.cursor += 1;
        }
    }

    pub fn is_given(&self, index: usize) -> bool {
        self.given.get(index).copied().unwrap_or(false)
    }

    pub fn target(&self) -> &[String] {
        &self.target
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn wrong(&self) -> Option<&str> {
        self.wrong.as_deref()
    }

    pub fn is_finished(&self) -> bool {
        self.cursor >= self.target.len()
    }

    pub fn log(&self) -> &[Keystroke] {
        &self.log
    }

    pub fn input(&mut self, key: Key, at: Duration) -> Outcome {
        if self.is_finished() {
            return Outcome::Finished;
        }
        let typed = match key {
            Key::Backspace => return self.backspace(at),
            Key::Char(grapheme) => grapheme,
            Key::Enter => "\n".to_string(),
            Key::Tab => "\t".to_string(),
        };
        if self.wrong.is_some() {
            return Outcome::Refused;
        }
        let expected = self.target[self.cursor].clone();
        if typed == expected {
            self.record(at, expected, typed, KeystrokeKind::Correct);
            self.cursor += 1;
            self.skip_given();
            if self.is_finished() {
                Outcome::Finished
            } else {
                Outcome::Correct
            }
        } else {
            self.record(at, expected, typed.clone(), KeystrokeKind::Wrong);
            self.wrong = Some(typed);
            Outcome::Wrong
        }
    }

    fn backspace(&mut self, at: Duration) -> Outcome {
        if self.wrong.take().is_none() {
            return Outcome::Ignored;
        }
        let expected = self.target[self.cursor].clone();
        self.record(at, expected, String::new(), KeystrokeKind::Backspace);
        Outcome::Corrected
    }

    fn record(&mut self, at: Duration, expected: String, typed: String, kind: KeystrokeKind) {
        self.log.push(Keystroke {
            at,
            expected,
            typed,
            kind,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(c: &str) -> Key {
        Key::Char(c.to_string())
    }

    fn at(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    fn kinds(engine: &Engine) -> Vec<KeystrokeKind> {
        engine.log().iter().map(|k| k.kind).collect()
    }

    #[test]
    fn splits_target_into_graphemes() {
        let engine = Engine::new("ab ä");
        assert_eq!(engine.target(), ["a", "b", " ", "ä"]);
        assert_eq!(engine.cursor(), 0);
        assert!(!engine.is_finished());
    }

    #[test]
    fn typing_everything_correctly_finishes() {
        let mut engine = Engine::new("en");
        assert_eq!(engine.input(ch("e"), at(0)), Outcome::Correct);
        assert_eq!(engine.cursor(), 1);
        assert_eq!(engine.input(ch("n"), at(100)), Outcome::Finished);
        assert!(engine.is_finished());
        assert_eq!(
            kinds(&engine),
            [KeystrokeKind::Correct, KeystrokeKind::Correct]
        );
        assert_eq!(engine.log()[1].at, at(100));
        assert_eq!(engine.log()[1].expected, "n");
        assert_eq!(engine.log()[1].typed, "n");
    }

    #[test]
    fn wrong_key_must_be_corrected_before_anything_else() {
        let mut engine = Engine::new("en");
        assert_eq!(engine.input(ch("n"), at(0)), Outcome::Wrong);
        assert_eq!(engine.wrong(), Some("n"));
        assert_eq!(engine.cursor(), 0, "the expected position does not move");
        assert_eq!(engine.input(ch("e"), at(50)), Outcome::Refused);
        assert_eq!(engine.input(Key::Enter, at(60)), Outcome::Refused);
        assert_eq!(engine.input(Key::Backspace, at(100)), Outcome::Corrected);
        assert_eq!(engine.wrong(), None);
        assert_eq!(engine.input(ch("e"), at(150)), Outcome::Correct);
        assert_eq!(
            kinds(&engine),
            [
                KeystrokeKind::Wrong,
                KeystrokeKind::Backspace,
                KeystrokeKind::Correct
            ],
            "refused keys are not logged"
        );
        let wrong = &engine.log()[0];
        assert_eq!((wrong.expected.as_str(), wrong.typed.as_str()), ("e", "n"));
    }

    #[test]
    fn backspace_on_a_clean_buffer_is_ignored_and_not_logged() {
        let mut engine = Engine::new("e");
        assert_eq!(engine.input(Key::Backspace, at(0)), Outcome::Ignored);
        assert!(engine.log().is_empty());
    }

    #[test]
    fn input_after_finishing_is_ignored() {
        let mut engine = Engine::new("e");
        assert_eq!(engine.input(ch("e"), at(0)), Outcome::Finished);
        assert_eq!(engine.input(ch("e"), at(10)), Outcome::Finished);
        assert_eq!(engine.log().len(), 1);
    }

    #[test]
    fn target_is_nfc_normalised() {
        let mut engine = Engine::new("a\u{308}");
        assert_eq!(engine.target(), ["ä"]);
        assert_eq!(engine.input(ch("ä"), at(0)), Outcome::Finished);
    }

    fn given_engine(text: &str, given_indices: &[usize]) -> Engine {
        let target: Vec<String> = text.graphemes(true).map(str::to_string).collect();
        let given = (0..target.len())
            .map(|i| given_indices.contains(&i))
            .collect();
        Engine::with_given(target, given)
    }

    #[test]
    fn given_graphemes_are_skipped_without_keystrokes() {
        let mut engine = given_engine("  ab\n  c", &[0, 1, 5, 6]);
        assert_eq!(engine.cursor(), 2, "leading indentation is skipped at once");
        assert_eq!(engine.input(ch("a"), at(0)), Outcome::Correct);
        assert_eq!(engine.input(ch("b"), at(10)), Outcome::Correct);
        assert_eq!(engine.input(Key::Enter, at(20)), Outcome::Correct);
        assert_eq!(
            engine.cursor(),
            7,
            "the next line's indentation is skipped after Enter"
        );
        assert!(engine.is_given(5) && !engine.is_given(7));
        assert_eq!(engine.input(ch("c"), at(30)), Outcome::Finished);
        assert_eq!(engine.log().len(), 4);
    }

    #[test]
    fn trailing_given_graphemes_finish_the_text() {
        let mut engine = given_engine("a\u{1F600}", &[1]);
        assert_eq!(engine.input(ch("a"), at(0)), Outcome::Finished);
        assert!(engine.is_finished());
        assert!(
            given_engine("\u{1F600}", &[0]).is_finished(),
            "nothing to type at all"
        );
    }

    #[test]
    fn enter_and_tab_match_newline_and_tab() {
        let mut engine = Engine::new("\n\t");
        assert_eq!(engine.input(Key::Enter, at(0)), Outcome::Correct);
        assert_eq!(engine.input(Key::Tab, at(10)), Outcome::Finished);
    }
}
