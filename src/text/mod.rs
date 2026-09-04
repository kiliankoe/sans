//! Text for a stage, generated from the lesson's keys and the word lists. Deterministic for
//! a given seed, so a stage can be reproduced from its stored session.

pub mod corpus;
pub mod drill;
pub mod file;
pub mod ngram;
pub mod words;

use std::collections::HashSet;

use rand::prelude::*;

use crate::course::StageKind;
pub use corpus::Corpus;

pub struct StageSpec<'a> {
    pub kind: StageKind,
    /// Keys this lesson introduces.
    pub new: &'a [String],
    /// Every key available, the new ones included.
    pub unlocked: &'a [String],
    pub seed: u64,
}

pub fn generate(spec: &StageSpec, corpus: &Corpus) -> String {
    let mut rng = rng(spec.seed);
    let unlocked = charset(spec.unlocked);
    let new = charset(spec.new);
    let target = spec.kind.target_len();
    let syllables = |rng: &mut StdRng| ngram::syllables(corpus, &unlocked, &new, target, rng);
    match spec.kind {
        StageKind::Intro => {
            let new_chars: Vec<char> = spec.new.iter().filter_map(|g| g.chars().next()).collect();
            let anchors: Vec<char> = spec
                .unlocked
                .iter()
                .filter_map(|g| g.chars().next())
                .filter(|c| !new.contains(c))
                .collect();
            drill::intro(&new_chars, &anchors, target, &mut rng)
        }
        StageKind::Bigrams => syllables(&mut rng),
        StageKind::Words => words::words(corpus, &unlocked, &new, 3.0, target, &mut rng)
            .unwrap_or_else(|| syllables(&mut rng)),
        StageKind::Test => words::words(corpus, &unlocked, &new, 1.5, target, &mut rng)
            .unwrap_or_else(|| syllables(&mut rng)),
    }
}

/// Single-character graphemes as a set. Every key on layers 1 to 3 is one `char`.
pub fn charset(graphemes: &[String]) -> HashSet<char> {
    graphemes.iter().filter_map(|g| g.chars().next()).collect()
}

/// Joins units with spaces until the text reaches `target_len`, never cutting a unit.
pub fn fill(mut units: impl Iterator<Item = String>, target_len: usize) -> String {
    let mut text = String::new();
    while text.chars().count() < target_len {
        let Some(unit) = units.next() else { break };
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str(&unit);
    }
    text
}

pub fn rng(seed: u64) -> StdRng {
    StdRng::seed_from_u64(seed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(keys: &str) -> Vec<String> {
        keys.chars().map(String::from).collect()
    }

    fn assert_within(text: &str, unlocked: &str) {
        let allowed: HashSet<char> = unlocked.chars().chain([' ']).collect();
        for c in text.chars() {
            assert!(allowed.contains(&c), "{c:?} is not unlocked in {text:?}");
        }
    }

    fn stage_text(kind: StageKind, new: &str, before: &str, seed: u64) -> String {
        let new = strings(new);
        let unlocked = [strings(before), new.clone()].concat();
        let spec = StageSpec {
            kind,
            new: &new,
            unlocked: &unlocked,
            seed,
        };
        generate(&spec, &Corpus::load())
    }

    #[test]
    fn intro_drills_only_the_new_keys_at_first() {
        let text = stage_text(StageKind::Intro, "en", "", 1);
        assert_within(&text, "en");
        assert!(text.contains("eee"), "{text}");
        assert!(text.contains("nnn"), "{text}");
        assert!(
            (100..=160).contains(&text.chars().count()),
            "{}",
            text.len()
        );
    }

    #[test]
    fn intro_mixes_new_keys_with_earlier_ones() {
        let text = stage_text(StageKind::Intro, "ar", "en", 1);
        assert_within(&text, "enar");
        assert!(text.contains("aaa"));
        assert!(
            text.split(' ')
                .any(|unit| unit.contains('a') && unit.contains('e')),
            "{text}"
        );
    }

    #[test]
    fn bigram_stage_stays_within_the_unlocked_keys() {
        let text = stage_text(StageKind::Bigrams, "ar", "en", 2);
        assert_within(&text, "enar");
        assert!(text.chars().count() >= 140, "{text}");
        assert!(
            text.split(' ').all(|unit| (2..=3).contains(&unit.len())),
            "{text}"
        );
    }

    #[test]
    fn words_stage_uses_real_words_and_stays_within_the_unlocked_keys() {
        let text = stage_text(StageKind::Words, "h", "enarudtilgc", 3);
        assert_within(&text, "enarudtilgch");
        assert!(
            text.split(' ')
                .any(|w| ["und", "dich", "nicht", "the", "that"].contains(&w)),
            "{text}"
        );
        assert!(text.chars().count() >= 180, "{text}");
    }

    #[test]
    fn words_stage_falls_back_to_syllables_when_the_alphabet_is_tiny() {
        let text = stage_text(StageKind::Words, "en", "", 4);
        assert_within(&text, "en");
        assert!(!text.is_empty());
    }

    #[test]
    fn same_seed_same_text_different_seed_different_text() {
        let a = stage_text(StageKind::Test, "h", "enarudtilgc", 7);
        let b = stage_text(StageKind::Test, "h", "enarudtilgc", 7);
        let c = stage_text(StageKind::Test, "h", "enarudtilgc", 8);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert!(a.chars().count() >= 230);
    }

    #[test]
    fn punctuation_and_capitals_appear_once_unlocked() {
        let text = stage_text(
            StageKind::Words,
            "ENARUDTILGCHOSWKPMZB",
            "enarudtilgchoswkpmzb,.",
            5,
        );
        assert_within(&text, "enarudtilgchoswkpmzb,.ENARUDTILGCHOSWKPMZB");
        assert!(text.chars().any(|c| c.is_uppercase()), "{text}");
        assert!(text.contains(',') || text.contains('.'), "{text}");
    }

    #[test]
    fn fill_never_cuts_a_unit() {
        let text = fill(["abc", "def", "ghi"].into_iter().map(String::from), 5);
        assert_eq!(text, "abc def");
    }
}
