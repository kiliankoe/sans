//! The word lists and the bigram table derived from them.

use std::collections::{HashMap, HashSet};

const LISTS: [&str; 2] = [
    include_str!("../../data/words/de.txt"),
    include_str!("../../data/words/en.txt"),
];

const EXCLUDE: &str = include_str!("../../data/words/exclude.txt");

#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    /// Dampened frequency, so the most common words do not swamp everything.
    pub weight: f64,
}

pub struct Corpus {
    words: Vec<Word>,
    bigrams: HashMap<(char, char), f64>,
}

impl Corpus {
    pub fn load() -> Self {
        Self::parse(&LISTS, EXCLUDE)
    }

    /// Lines of `word count`; words present in several lists keep their highest weight.
    /// `exclude` lists words to drop, one per line, a trailing `*` matching a prefix.
    pub fn parse(lists: &[&str], exclude: &str) -> Self {
        let excluded = Exclusions::parse(exclude);
        let mut weights: HashMap<String, f64> = HashMap::new();
        let mut order = Vec::new();
        for list in lists {
            for line in list.lines() {
                let Some((word, count)) = line.split_once(' ') else {
                    continue;
                };
                if excluded.matches(word) {
                    continue;
                }
                let Ok(count) = count.trim().parse::<f64>() else {
                    continue;
                };
                let weight = count.powf(0.4);
                match weights.get_mut(word) {
                    Some(existing) => *existing = existing.max(weight),
                    None => {
                        weights.insert(word.to_string(), weight);
                        order.push(word.to_string());
                    }
                }
            }
        }
        let words: Vec<Word> = order
            .into_iter()
            .map(|text| Word {
                weight: weights[&text],
                text,
            })
            .collect();
        let mut bigrams: HashMap<(char, char), f64> = HashMap::new();
        for word in &words {
            let chars: Vec<char> = word.text.chars().collect();
            for pair in chars.windows(2) {
                *bigrams.entry((pair[0], pair[1])).or_default() += word.weight;
            }
        }
        Self { words, bigrams }
    }

    /// Words made only of characters in `charset`, in list order.
    pub fn words_within(&self, charset: &HashSet<char>) -> Vec<&Word> {
        self.words
            .iter()
            .filter(|word| word.text.chars().all(|c| charset.contains(&c)))
            .collect()
    }

    /// Bigrams made only of characters in `charset`, most frequent first.
    pub fn bigrams_within(&self, charset: &HashSet<char>) -> Vec<((char, char), f64)> {
        let mut bigrams: Vec<_> = self
            .bigrams
            .iter()
            .filter(|((a, b), _)| charset.contains(a) && charset.contains(b))
            .map(|(pair, weight)| (*pair, *weight))
            .collect();
        bigrams.sort_by(|(pair_a, weight_a), (pair_b, weight_b)| {
            weight_b.total_cmp(weight_a).then(pair_a.cmp(pair_b))
        });
        bigrams
    }
}

struct Exclusions {
    exact: HashSet<String>,
    prefixes: Vec<String>,
}

impl Exclusions {
    fn parse(text: &str) -> Self {
        let mut exact = HashSet::new();
        let mut prefixes = Vec::new();
        for line in text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            match line.strip_suffix('*') {
                Some(prefix) => prefixes.push(prefix.to_string()),
                None => {
                    exact.insert(line.to_string());
                }
            }
        }
        Self { exact, prefixes }
    }

    fn matches(&self, word: &str) -> bool {
        self.exact.contains(word) || self.prefixes.iter().any(|prefix| word.starts_with(prefix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excluded_words_and_prefixes_never_load() {
        let corpus = Corpus::parse(
            &["darn 10\ndamn 9\ndamned 8\nnice 7\n"],
            "# comment\ndarn\ndamn*\n",
        );
        let all: HashSet<char> = "darnmicedm".chars().collect();
        let words: Vec<&str> = corpus
            .words_within(&all)
            .iter()
            .map(|w| w.text.as_str())
            .collect();
        assert_eq!(words, ["nice"]);
    }

    #[test]
    fn parses_merges_and_filters() {
        let corpus = Corpus::parse(&["ende 100\nnenne 10\n", "end 50\nende 400\n"], "");
        let all: HashSet<char> = "end".chars().collect();
        let words: Vec<&str> = corpus
            .words_within(&all)
            .iter()
            .map(|w| w.text.as_str())
            .collect();
        assert_eq!(words, ["ende", "nenne", "end"]);
        let ende = corpus.words_within(&all)[0];
        assert!(
            ende.weight > corpus.words_within(&all)[2].weight,
            "higher count keeps the higher weight"
        );
        let en: HashSet<char> = "en".chars().collect();
        assert_eq!(corpus.words_within(&en).len(), 1, "only nenne fits e and n");
        let bigrams = corpus.bigrams_within(&en);
        assert_eq!(bigrams[0].0, ('e', 'n'), "{bigrams:?}");
        assert!(bigrams.iter().any(|(pair, _)| *pair == ('n', 'e')));
    }

    #[test]
    fn embedded_lists_load() {
        let corpus = Corpus::load();
        let charset: HashSet<char> = "enarudtilgch".chars().collect();
        let words = corpus.words_within(&charset);
        assert!(words.len() > 500, "{}", words.len());
        assert!(words.iter().any(|w| w.text == "und"));
        assert!(words.iter().any(|w| w.text == "the"));
    }
}
