//! Files to type over: normalisation, chunking, and which characters the engine gives away.

use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;

use crate::layout;

/// A chunk closes at a line boundary once it has this many lines ...
pub const CHUNK_LINES: usize = 12;
/// ... or this many characters, whichever comes first, so prose paragraphs stay short.
pub const CHUNK_CHARS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indent {
    /// Leading whitespace is inserted for you, as code trainers do.
    Skip,
    /// Leading whitespace has to be typed.
    Type,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chunk {
    /// Zero-based line number of the first line in the file.
    pub first_line: usize,
    pub lines: Vec<String>,
}

/// What the engine gets for a chunk: the text and, per grapheme, whether it is given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    pub target: Vec<String>,
    pub given: Vec<bool>,
}

/// NFC, LF line ends, trailing whitespace per line dropped, trailing blank lines dropped.
pub fn normalise(raw: &str) -> Vec<String> {
    let text: String = raw.nfc().collect();
    let mut lines: Vec<String> = text
        .split('\n')
        .map(|line| line.trim_end().to_string())
        .collect();
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

pub fn chunks(lines: &[String]) -> Vec<Chunk> {
    let mut out = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut chars = 0;
    let mut first_line = 0;
    for (index, line) in lines.iter().enumerate() {
        let len = line.chars().count();
        if !current.is_empty() && (current.len() >= CHUNK_LINES || chars + len > CHUNK_CHARS) {
            out.push(Chunk {
                first_line,
                lines: std::mem::take(&mut current),
            });
            chars = 0;
            first_line = index;
        }
        current.push(line.clone());
        chars += len;
    }
    if !current.is_empty() {
        out.push(Chunk {
            first_line,
            lines: current,
        });
    }
    out
}

/// Lines joined with `\n` (typed with Enter). Given: leading whitespace when `indent` is
/// `Skip`, and every grapheme the layout cannot produce.
pub fn prepare(chunk: &Chunk, indent: Indent) -> Prepared {
    let mut target = Vec::new();
    let mut given = Vec::new();
    for (index, line) in chunk.lines.iter().enumerate() {
        if index > 0 {
            target.push("\n".to_string());
            given.push(false);
        }
        let mut in_indent = true;
        for grapheme in line.graphemes(true) {
            let whitespace = grapheme == " " || grapheme == "\t";
            in_indent &= whitespace;
            let unsupported = grapheme != "\t" && !layout::is_typeable(grapheme);
            given.push((in_indent && indent == Indent::Skip) || unsupported);
            target.push(grapheme.to_string());
        }
    }
    Prepared { target, given }
}

/// FNV-1a over the normalised lines, hex. Stable across builds, unlike `DefaultHasher`.
pub fn content_hash(lines: &[String]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in lines.iter().flat_map(|line| line.bytes().chain(*b"\n")) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|l| l.to_string()).collect()
    }

    #[test]
    fn normalise_fixes_line_ends_trailing_whitespace_and_composition() {
        let lines = normalise("fn main() {  \r\n\ta\u{308}();\n}\n\n\n");
        assert_eq!(lines, ["fn main() {", "\tä();", "}"]);
        assert!(normalise("").is_empty());
        assert_eq!(normalise("x").len(), 1);
    }

    #[test]
    fn chunks_close_on_lines_or_characters() {
        let lines: Vec<String> = (0..30).map(|i| format!("line {i}")).collect();
        let short = chunks(&lines);
        assert_eq!(short.len(), 3);
        assert_eq!(short[0].first_line, 0);
        assert_eq!(short[1].first_line, 12);
        assert_eq!(short[2].lines.len(), 6);
        let long: Vec<String> = vec!["x".repeat(400), "y".repeat(400), "z".to_string()];
        let wide = chunks(&long);
        assert_eq!(wide.len(), 2, "400 + 400 characters exceed the limit");
        assert_eq!(wide[1].lines.len(), 2);
        assert!(chunks(&[]).is_empty());
    }

    #[test]
    fn prepare_gives_indentation_and_unsupported_characters() {
        let chunk = Chunk {
            first_line: 0,
            lines: strings(&["if x {", "    y = \"→\";", "\tz"]),
        };
        let prepared = prepare(&chunk, Indent::Skip);
        let text: String = prepared.target.concat();
        assert_eq!(text, "if x {\n    y = \"→\";\n\tz");
        let given_text: String = prepared
            .target
            .iter()
            .zip(&prepared.given)
            .filter(|(_, given)| **given)
            .map(|(g, _)| g.as_str())
            .collect();
        assert_eq!(given_text, "    →\t");
        let typed = prepare(&chunk, Indent::Type);
        let given_typed: String = typed
            .target
            .iter()
            .zip(&typed.given)
            .filter(|(_, g)| **g)
            .map(|(g, _)| g.as_str())
            .collect();
        assert_eq!(
            given_typed, "→",
            "with indent = type only the arrow is given"
        );
    }

    #[test]
    fn content_hash_is_stable_and_sensitive() {
        let a = content_hash(&strings(&["a", "b"]));
        assert_eq!(a, content_hash(&strings(&["a", "b"])));
        assert_ne!(a, content_hash(&strings(&["ab"])));
        assert_eq!(a.len(), 16);
    }
}
