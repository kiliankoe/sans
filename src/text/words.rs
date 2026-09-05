//! Real words from the lists, restricted to the unlocked keys and weighted toward new ones,
//! with punctuation, capitals and hyphens sprinkled in once those are unlocked.

use std::collections::{HashSet, VecDeque};

use rand::distr::weighted::WeightedIndex;
use rand::prelude::*;

use super::{Corpus, Weakness, fill};

/// Fewer candidate words than this and the stage falls back to syllables.
pub const MIN_WORDS: usize = 12;

/// `new_bonus` multiplies the weight of words containing a new key; weak keys and bigrams
/// multiply it by their factor up to `weak_cap`.
#[allow(clippy::too_many_arguments)]
pub fn words(
    corpus: &Corpus,
    charset: &HashSet<char>,
    new: &HashSet<char>,
    new_bonus: f64,
    weakness: &Weakness,
    weak_cap: f64,
    target_len: usize,
    rng: &mut StdRng,
) -> Option<String> {
    let candidates = corpus.words_within(charset);
    if candidates.len() < MIN_WORDS {
        return None;
    }
    let weights: Vec<f64> = candidates
        .iter()
        .map(|word| {
            let bonus = if word.text.chars().any(|c| new.contains(&c)) {
                new_bonus
            } else {
                1.0
            };
            word.weight * bonus * weakness.text(&word.text).min(weak_cap)
        })
        .collect();
    let dist = WeightedIndex::new(&weights).ok()?;
    let decor = Decor::for_keys(charset, new);
    let mut recent = VecDeque::new();
    let mut pick = |rng: &mut StdRng| loop {
        let index = dist.sample(rng);
        if !recent.contains(&index) {
            if recent.len() == 4 {
                recent.pop_front();
            }
            recent.push_back(index);
            return candidates[index].text.clone();
        }
    };
    let units = std::iter::from_fn(|| {
        let mut word = pick(rng);
        if rng.random::<f64>() < decor.hyphen {
            word = format!("{word}-{}", pick(rng));
        }
        Some(decor.apply(word, charset, rng))
    });
    Some(fill(units, target_len))
}

/// Probabilities for dressing a word up, higher while the key in question is new.
struct Decor {
    comma: f64,
    period: f64,
    hyphen: f64,
    capital: f64,
}

impl Decor {
    fn for_keys(charset: &HashSet<char>, new: &HashSet<char>) -> Self {
        let rate = |key: char, when_new: f64, later: f64| {
            if new.contains(&key) {
                when_new
            } else if charset.contains(&key) {
                later
            } else {
                0.0
            }
        };
        let capitals_new = new.iter().any(|c| c.is_uppercase());
        let capitals = charset.iter().any(|c| c.is_uppercase());
        Self {
            comma: rate(',', 0.3, 0.12),
            period: rate('.', 0.25, 0.1),
            hyphen: rate('-', 0.3, 0.06),
            capital: if capitals_new {
                0.6
            } else if capitals {
                0.3
            } else {
                0.0
            },
        }
    }

    fn apply(&self, mut word: String, charset: &HashSet<char>, rng: &mut StdRng) -> String {
        if rng.random::<f64>() < self.capital {
            let mut chars = word.chars();
            if let Some(first) = chars.next() {
                let upper: String = first.to_uppercase().collect();
                if upper.chars().all(|c| charset.contains(&c)) {
                    word = upper + chars.as_str();
                }
            }
        }
        let roll = rng.random::<f64>();
        if roll < self.comma {
            word.push(',');
        } else if roll < self.comma + self.period {
            word.push('.');
        }
        word
    }
}
