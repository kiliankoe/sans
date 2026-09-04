//! Intro drills: the new keys alone, then woven into the keys learnt before.

use rand::prelude::*;

use super::fill;

/// `new` are the keys being introduced, `anchors` keys already known, most useful first.
pub fn intro(new: &[char], anchors: &[char], target_len: usize, rng: &mut StdRng) -> String {
    // Symbols drill against more letters than a new letter does, since they have no solo form.
    let anchor_count = if new.iter().all(|k| k.is_alphanumeric()) {
        3
    } else {
        5
    };
    let letters: Vec<char> = anchors
        .iter()
        .copied()
        .filter(|c| c.is_lowercase())
        .take(anchor_count)
        .collect();
    // Ten digits at three repeats each would fill the whole stage on their own.
    let solo_repeats = if new.len() <= 3 { 3 } else { 1 };
    let mut solo = Vec::new();
    let mut woven = Vec::new();
    let mut paired = Vec::new();
    for &key in new {
        let partners: Vec<char> = new
            .iter()
            .copied()
            .filter(|&other| other != key && other.is_lowercase())
            .chain(letters.iter().copied())
            .collect();
        match key {
            k if k.is_uppercase() => {
                let base: String = k.to_lowercase().collect();
                solo.push(format!("{base}{k}{base}"));
                for p in partners {
                    woven.extend([format!("{k}{base}"), format!("{k}{base}{p}")]);
                }
            }
            k if k.is_alphanumeric() => {
                solo.extend(std::iter::repeat_n(format!("{k}{k}{k}"), solo_repeats));
                for p in partners {
                    woven.extend([
                        format!("{k}{p}{k}"),
                        format!("{p}{k}{p}"),
                        format!("{k}{k}{p}"),
                        format!("{p}{k}{k}"),
                    ]);
                }
            }
            '-' => {
                for pair in letters.windows(2) {
                    woven.push(format!("{}-{}", pair[0], pair[1]));
                }
            }
            k => {
                if let Some(close) =
                    closing(k).filter(|close| new.contains(close) && !paired.contains(&k))
                {
                    paired.push(k);
                    for a in &letters {
                        woven.push(format!("{k}{a}{close}"));
                    }
                    for pair in letters.windows(2) {
                        woven.push(format!("{k}{}{}{close}", pair[0], pair[1]));
                    }
                    woven.push(format!("{k}{close}"));
                } else if !paired.iter().any(|open| closing(*open) == Some(k)) {
                    for a in &letters {
                        woven.extend([format!("{a}{k}"), format!("{k}{a}")]);
                    }
                    for pair in letters.windows(2) {
                        woven.push(format!("{}{k}{}", pair[0], pair[1]));
                    }
                }
            }
        }
    }
    woven.shuffle(rng);
    if woven.is_empty() {
        woven = solo.clone();
    }
    let units = solo.into_iter().chain(woven.into_iter().cycle());
    fill(units, target_len)
}

/// The closing half of a bracket or quote pair, drilled as `(en)` rather than `e(n`.
fn closing(open: char) -> Option<char> {
    match open {
        '(' => Some(')'),
        '{' => Some('}'),
        '[' => Some(']'),
        '<' => Some('>'),
        '"' | '\'' | '`' => Some(open),
        _ => None,
    }
}
