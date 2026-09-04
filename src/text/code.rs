//! Code-flavoured text for the symbol lessons: tokens and expressions from templates, with
//! words from the lists as identifiers, plus lines from a small snippet corpus.

use std::collections::HashSet;

use rand::distr::weighted::WeightedIndex;
use rand::prelude::*;

use super::{Corpus, fill};

/// Share of expression lines taken from the snippet corpus when any fit.
const SNIPPET_SHARE: f64 = 0.35;

const SNIPPETS: &str = include_str!("../../data/code/snippets.txt");

/// Short units for the tokens stage. `%w` is an identifier, `%n` a number.
const TOKENS: &[&str] = &[
    "%w()", "(%w)", "%w(%w)", "()", "%w = %w", "==", "!=", "\"%w\"", "'%w'", "{%w}", "{ %w }",
    "{}", "%w_%w", "%w: %w", "%w;", "%w::%w", "%w/%w", "\\%w", "%w\\n", "<%w>", "%w < %w",
    "%w > %w", "%w <= %w", "=>", "->", "[%w]", "%w[%n]", "[%w, %w]", "%w - %w", "%w + %w",
    "%w * %w", "%w += %n", "%w++", "!%w", "%w?", "%w != %w", "&%w", "&&", "%w | %w", "||", "#%w",
    "#[%w]", "@%w", "~%w", "`%w`", "%w ^ %w", "%w % %n", "$%w", "${%w}", "%n", "%n.%n", "%w = %n",
    "%n + %n",
];

/// Line-like units for the expressions stage.
const EXPRESSIONS: &[&str] = &[
    "let %w = %w(%w);",
    "if %w == %w {",
    "fn %w(%w: %w) -> %w {",
    "return %w;",
    "%w.%w(%w)",
    "const %w = \"%w\";",
    "for %w in %w {",
    "while (%w < %n) {",
    "%w = [%w, %w, %w];",
    "print(\"%w\")",
    "echo \"%w\"",
    "import %w from '%w';",
    "def %w(%w):",
    "%w := %w + %n",
    "if [ -f %w ]; then",
    "%w | %w -%w",
    "--%w=%w",
    "#!/bin/%w",
    "// %w %w",
    "# %w %w",
    "/* %w */",
    "%w && %w || %w",
    "%w.%w = { %w: %n };",
    "match %w { %w => %w, }",
    "%w?.%w ?? %w",
    "<%w>%w</%w>",
    "{ %w = %w; }",
    "${%w}/%w",
    "%w@%w.%w",
    "%w(\"%w\", %n)",
    "let %w: %w<%w> = %w::new();",
    "assert!(%w != %w);",
    "%w[%n] = %w;",
    "x = %n * %n + %n",
];

pub struct Code<'a> {
    pub corpus: &'a Corpus,
    pub charset: &'a HashSet<char>,
    pub new: &'a HashSet<char>,
}

impl Code<'_> {
    /// Short units joined with spaces.
    pub fn tokens(&self, target_len: usize, rng: &mut StdRng) -> String {
        let mut units = self.units(TOKENS);
        fill(std::iter::from_fn(|| Some(units.next(rng))), target_len)
    }

    /// Line-like units, one per line, expressions from templates with snippet lines mixed in.
    pub fn expressions(&self, target_len: usize, rng: &mut StdRng) -> String {
        let snippets = self.snippet_lines();
        let mut units = self.units(EXPRESSIONS);
        let mut text = String::new();
        while text.chars().count() < target_len {
            let line = match snippets.choose(rng) {
                Some(snippet) if rng.random::<f64>() < SNIPPET_SHARE => (*snippet).to_string(),
                _ => units.next(rng),
            };
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&line);
        }
        text
    }

    /// Snippet lines every character of which is unlocked.
    pub fn snippet_lines(&self) -> Vec<&'static str> {
        SNIPPETS
            .lines()
            .filter(|line| !line.is_empty() && !line.starts_with("%%"))
            .filter(|line| line.chars().all(|c| c == ' ' || self.charset.contains(&c)))
            .collect()
    }

    fn digits_unlocked(&self) -> bool {
        ('0'..='9').all(|d| self.charset.contains(&d))
    }

    /// Whether every literal character of a template is unlocked.
    fn fits(&self, template: &str) -> bool {
        let literal = template.replace("%w", "").replace("%n", "");
        let digits_ok = !template.contains("%n") || self.digits_unlocked();
        digits_ok
            && literal
                .chars()
                .all(|c| c == ' ' || self.charset.contains(&c))
    }

    /// Templates that fit, weighted toward the ones exercising a new key. Falls back to
    /// plain identifiers when no template fits at all.
    fn units(&self, pool: &[&'static str]) -> Units<'_> {
        let templates: Vec<&'static str> = pool.iter().copied().filter(|t| self.fits(t)).collect();
        let weights: Vec<f64> = templates
            .iter()
            .map(|t| {
                if t.chars().any(|c| self.new.contains(&c)) {
                    3.0
                } else {
                    1.0
                }
            })
            .collect();
        let identifiers: Vec<&str> = self
            .corpus
            .words_within(self.charset)
            .into_iter()
            .map(|word| word.text.as_str())
            .filter(|word| (3..=8).contains(&word.len()) && word.is_ascii())
            .take(3000)
            .collect();
        Units {
            code: self,
            templates,
            dist: WeightedIndex::new(&weights).ok(),
            identifiers,
            last: None,
        }
    }

    fn identifier(&self, identifiers: &[&str], rng: &mut StdRng) -> String {
        let pick = |rng: &mut StdRng| {
            identifiers
                .choose(rng)
                .map_or("x".to_string(), |w| (*w).to_string())
        };
        let mut word = pick(rng);
        if self.charset.contains(&'_') && rng.random::<f64>() < 0.2 {
            word = format!("{word}_{}", pick(rng));
        } else if rng.random::<f64>() < 0.1 {
            let mut chars = word.chars();
            if let Some(first) = chars.next() {
                let upper: String = first.to_uppercase().collect();
                if upper.chars().all(|c| self.charset.contains(&c)) {
                    word = upper + chars.as_str();
                }
            }
        }
        word
    }

    fn render(&self, template: &str, identifiers: &[&str], rng: &mut StdRng) -> String {
        let mut out = String::new();
        let mut rest = template;
        while let Some(index) = rest.find('%') {
            out.push_str(&rest[..index]);
            match rest.as_bytes().get(index + 1) {
                Some(b'w') => out.push_str(&self.identifier(identifiers, rng)),
                Some(b'n') => out.push_str(&rng.random_range(0..1000_u32).to_string()),
                _ => out.push('%'),
            }
            rest = &rest[(index + 2).min(rest.len())..];
        }
        out.push_str(rest);
        out
    }
}

/// A stream of rendered units without immediate repeats.
struct Units<'a> {
    code: &'a Code<'a>,
    templates: Vec<&'static str>,
    dist: Option<WeightedIndex<f64>>,
    identifiers: Vec<&'a str>,
    last: Option<usize>,
}

impl Units<'_> {
    fn next(&mut self, rng: &mut StdRng) -> String {
        let Some(dist) = &self.dist else {
            return self.code.identifier(&self.identifiers, rng);
        };
        let mut index = dist.sample(rng);
        if self.templates.len() > 1 && self.last == Some(index) {
            index = dist.sample(rng);
        }
        self.last = Some(index);
        self.code
            .render(self.templates[index], &self.identifiers, rng)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{charset, rng};

    fn strings(keys: &str) -> Vec<String> {
        keys.chars().map(String::from).collect()
    }

    const LETTERS: &str = "abcdefghijklmnopqrstuvwxyzäöüßABCDEFGHIJKLMNOPQRSTUVWXYZÄÖÜ,.- ";

    fn assert_within(text: &str, allowed: &HashSet<char>) {
        for c in text.chars() {
            assert!(
                allowed.contains(&c) || c == ' ' || c == '\n',
                "{c:?} is not unlocked in {text:?}"
            );
        }
    }

    #[test]
    fn tokens_use_only_unlocked_symbols_and_favour_new_ones() {
        let corpus = Corpus::load();
        let unlocked = charset(&strings(&format!("{LETTERS}()")));
        let new = charset(&strings("()"));
        let code = Code {
            corpus: &corpus,
            charset: &unlocked,
            new: &new,
        };
        let text = code.tokens(150, &mut rng(1));
        assert_within(&text, &unlocked);
        assert!(text.contains('(') && text.contains(')'), "{text}");
        assert!(text.chars().count() >= 140, "{text}");
        assert!(!text.contains('='), "{text}");
    }

    #[test]
    fn expressions_are_lines_and_include_snippets_once_they_fit() {
        let corpus = Corpus::load();
        let all = charset(&strings(&format!(
            "{LETTERS}()=\"'{{}}_:;/\\<>[]-+*!?&|#@~`^%$0123456789"
        )));
        let new = charset(&strings("$"));
        let code = Code {
            corpus: &corpus,
            charset: &all,
            new: &new,
        };
        assert!(
            code.snippet_lines().len() > 40,
            "{}",
            code.snippet_lines().len()
        );
        let text = code.expressions(200, &mut rng(2));
        assert_within(&text, &all);
        assert!(text.contains('\n'), "{text}");
        assert!(text.lines().all(|line| !line.is_empty()));
        let few = charset(&strings(&format!("{LETTERS}()")));
        let code = Code {
            corpus: &corpus,
            charset: &few,
            new: &new,
        };
        let text = code.expressions(200, &mut rng(2));
        assert_within(&text, &few);
        assert!(code.snippet_lines().len() < 5, "{:?}", code.snippet_lines());
    }

    #[test]
    fn numbers_only_appear_once_digits_are_unlocked() {
        let corpus = Corpus::load();
        let without = charset(&strings(&format!("{LETTERS}()=[]")));
        let code = Code {
            corpus: &corpus,
            charset: &without,
            new: &without,
        };
        let text = code.tokens(300, &mut rng(3));
        assert!(!text.chars().any(|c| c.is_ascii_digit()), "{text}");
        let with = charset(&strings(&format!("{LETTERS}()=[]0123456789")));
        let digits = charset(&strings("0123456789"));
        let code = Code {
            corpus: &corpus,
            charset: &with,
            new: &digits,
        };
        let text = code.tokens(300, &mut rng(3));
        assert!(text.chars().any(|c| c.is_ascii_digit()), "{text}");
    }
}
