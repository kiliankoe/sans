//! What the statistics say is weak: a factor per key and per bigram that generators use to
//! show those more often. Keybr's focus key and Tipp10's Intelligenz, at the bigram level.

use std::collections::HashMap;

use crate::stats::{BigramStat, KeyStat};

/// Fewer attempts than this and a key or bigram is not judged at all.
pub const MIN_ATTEMPTS: u32 = 10;
/// The most a weak key or bigram is favoured over a solid one.
pub const MAX_FACTOR: f64 = 3.0;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Weakness {
    keys: HashMap<char, f64>,
    bigrams: HashMap<(char, char), f64>,
}

impl Weakness {
    /// Nothing is weak: every factor is 1.
    pub fn none() -> Self {
        Self::default()
    }

    /// A factor of `1 + 10 × error rate`, so 5% errors doubles the odds of that key, capped at
    /// `MAX_FACTOR`; bigrams slower than one and a half times the typical bigram get another
    /// half point. Only keys and bigrams with at least `MIN_ATTEMPTS` count.
    pub fn from_stats(keys: &[KeyStat], bigrams: &[BigramStat]) -> Self {
        let factor = |error_rate: f64| (1.0 + 10.0 * error_rate).min(MAX_FACTOR);
        let mut weak = Self::default();
        for stat in keys.iter().filter(|k| k.attempts >= MIN_ATTEMPTS) {
            let f = factor(stat.error_rate);
            if let (true, Some(c)) = (f > 1.0, single(&stat.key)) {
                weak.keys.insert(c, f);
            }
        }
        let judged: Vec<&BigramStat> = bigrams
            .iter()
            .filter(|b| b.attempts >= MIN_ATTEMPTS)
            .collect();
        let mut medians: Vec<u64> = judged.iter().filter_map(|b| b.median_ms).collect();
        medians.sort_unstable();
        let typical = medians.get(medians.len() / 2).map(|&ms| ms as f64);
        for stat in judged {
            let mut f = factor(stat.error_rate);
            if let (Some(ms), Some(typical)) = (stat.median_ms, typical)
                && ms as f64 > 1.5 * typical
            {
                f = (f + 0.5).min(MAX_FACTOR);
            }
            if let (true, Some(pair)) = (f > 1.0, pair(&stat.bigram)) {
                weak.bigrams.insert(pair, f);
            }
        }
        weak
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty() && self.bigrams.is_empty()
    }

    pub fn key(&self, c: char) -> f64 {
        self.keys.get(&c).copied().unwrap_or(1.0)
    }

    pub fn bigram(&self, a: char, b: char) -> f64 {
        self.bigrams.get(&(a, b)).copied().unwrap_or(1.0)
    }

    /// The largest factor of any key or bigram in `text`.
    pub fn text(&self, text: &str) -> f64 {
        let chars: Vec<char> = text.chars().collect();
        let keys = chars.iter().map(|&c| self.key(c));
        let bigrams = chars.windows(2).map(|w| self.bigram(w[0], w[1]));
        keys.chain(bigrams).fold(1.0, f64::max)
    }

    /// The weakest keys, strongest factor first, factors above 1 only.
    pub fn weakest_keys(&self, limit: usize) -> Vec<(char, f64)> {
        let mut keys: Vec<(char, f64)> = self.keys.iter().map(|(&c, &f)| (c, f)).collect();
        keys.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        keys.truncate(limit);
        keys
    }

    /// The weakest bigrams, strongest factor first, factors above 1 only.
    pub fn weakest_bigrams(&self, limit: usize) -> Vec<(String, f64)> {
        let mut bigrams: Vec<(String, f64)> = self
            .bigrams
            .iter()
            .map(|(&(a, b), &f)| (format!("{a}{b}"), f))
            .collect();
        bigrams.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        bigrams.truncate(limit);
        bigrams
    }
}

fn single(text: &str) -> Option<char> {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => Some(c),
        _ => None,
    }
}

fn pair(text: &str) -> Option<(char, char)> {
    let mut chars = text.chars();
    match (chars.next(), chars.next(), chars.next()) {
        (Some(a), Some(b), None) => Some((a, b)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(key: &str, attempts: u32, errors: u32) -> KeyStat {
        KeyStat {
            key: key.into(),
            attempts,
            errors,
            error_rate: errors as f64 / attempts as f64,
            median_ms: Some(200),
        }
    }

    fn bigram(bigram: &str, attempts: u32, errors: u32, median_ms: u64) -> BigramStat {
        BigramStat {
            bigram: bigram.into(),
            attempts,
            errors,
            error_rate: errors as f64 / attempts as f64,
            median_ms: Some(median_ms),
        }
    }

    #[test]
    fn factors_follow_error_rates_with_a_cap_and_a_minimum_sample() {
        let weak = Weakness::from_stats(
            &[
                key("n", 40, 2),
                key("e", 40, 0),
                key("x", 5, 5),
                key("k", 20, 10),
            ],
            &[],
        );
        assert!((weak.key('n') - 1.5).abs() < 1e-9, "{}", weak.key('n'));
        assert_eq!(weak.key('e'), 1.0);
        assert_eq!(weak.key('x'), 1.0, "five attempts are not enough to judge");
        assert_eq!(weak.key('k'), MAX_FACTOR);
        assert_eq!(weak.key('z'), 1.0, "unknown keys are not weak");
        assert_eq!(weak.weakest_keys(5), [('k', 3.0), ('n', 1.5)]);
        assert!(Weakness::none().is_empty() && !weak.is_empty());
    }

    #[test]
    fn slow_bigrams_count_as_weak_too() {
        let weak = Weakness::from_stats(
            &[],
            &[
                bigram("en", 30, 0, 200),
                bigram("ne", 30, 0, 220),
                bigram("ar", 30, 0, 600),
                bigram("ra", 30, 3, 210),
                bigram("ka", 30, 6, 900),
            ],
        );
        assert_eq!(weak.bigram('e', 'n'), 1.0);
        assert!(
            (weak.bigram('a', 'r') - 1.5).abs() < 1e-9,
            "slow: {}",
            weak.bigram('a', 'r')
        );
        assert!(
            (weak.bigram('r', 'a') - 2.0).abs() < 1e-9,
            "10% errors: {}",
            weak.bigram('r', 'a')
        );
        assert_eq!(weak.bigram('k', 'a'), MAX_FACTOR, "capped");
        assert_eq!(
            weak.weakest_bigrams(2),
            [("ka".to_string(), 3.0), ("ra".to_string(), 2.0)]
        );
    }

    #[test]
    fn a_text_is_as_weak_as_its_weakest_part() {
        let weak = Weakness::from_stats(&[key("n", 40, 2)], &[bigram("ar", 30, 3, 200)]);
        assert_eq!(weak.text("eee"), 1.0);
        assert!((weak.text("ene") - 1.5).abs() < 1e-9);
        assert!(
            (weak.text("bar") - 2.0).abs() < 1e-9,
            "the ar bigram at 10% errors"
        );
        assert!(
            (weak.text("narr") - 2.0).abs() < 1e-9,
            "the weakest part wins"
        );
    }
}
