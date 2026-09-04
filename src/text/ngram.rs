//! Syllable drills built from the most frequent bigrams available.

use std::collections::HashSet;

use rand::prelude::*;

use super::{Corpus, fill};

/// Bigrams containing a new key are favoured; three-letter forms extend each bigram with
/// its most frequent neighbour so the transitions get practised in context.
pub fn syllables(
    corpus: &Corpus,
    charset: &HashSet<char>,
    new: &HashSet<char>,
    target_len: usize,
    rng: &mut StdRng,
) -> String {
    let bigrams = corpus.bigrams_within(charset);
    let (with_new, without): (Vec<_>, Vec<_>) = bigrams
        .iter()
        .map(|(pair, _)| *pair)
        .partition(|(a, b)| new.contains(a) || new.contains(b));
    let mut selected: Vec<(char, char)> = if new.is_empty() {
        bigrams.iter().map(|(pair, _)| *pair).take(12).collect()
    } else {
        with_new
            .into_iter()
            .take(8)
            .chain(without.into_iter().take(4))
            .collect()
    };
    if selected.is_empty() {
        // No corpus bigram fits, which only happens with one or two odd keys: pair them up.
        let chars: Vec<char> = charset.iter().copied().collect();
        selected = chars
            .iter()
            .flat_map(|&a| chars.iter().map(move |&b| (a, b)))
            .filter(|(a, b)| a != b)
            .take(12)
            .collect();
    }
    let follow = |c: char| {
        bigrams
            .iter()
            .find(|((a, _), _)| *a == c)
            .map(|((_, b), _)| *b)
    };
    let precede = |c: char| {
        bigrams
            .iter()
            .find(|((_, b), _)| *b == c)
            .map(|((a, _), _)| *a)
    };
    let mut units = Vec::new();
    for (a, b) in selected {
        units.push(format!("{a}{b}"));
        units.push(format!("{a}{b}"));
        if let Some(c) = follow(b) {
            units.push(format!("{a}{b}{c}"));
        }
        if let Some(c) = precede(a) {
            units.push(format!("{c}{a}{b}"));
        }
    }
    units.shuffle(rng);
    fill(units.into_iter().cycle(), target_len)
}
