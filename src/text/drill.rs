//! Intro drills: the new keys alone, then woven into the keys learnt before.

use rand::prelude::*;

use super::fill;

/// `new` are the keys being introduced, `anchors` keys already known, most useful first.
pub fn intro(new: &[char], anchors: &[char], target_len: usize, rng: &mut StdRng) -> String {
    let letters: Vec<char> = anchors
        .iter()
        .copied()
        .filter(|c| c.is_lowercase())
        .take(3)
        .collect();
    let mut solo = Vec::new();
    let mut woven = Vec::new();
    for &key in new {
        let partners: Vec<char> = new
            .iter()
            .copied()
            .filter(|&other| other != key && other.is_lowercase())
            .chain(letters.iter().copied())
            .collect();
        match key {
            k if k.is_lowercase() => {
                solo.extend(std::iter::repeat_n(format!("{k}{k}{k}"), 3));
                for p in partners {
                    woven.extend([
                        format!("{k}{p}{k}"),
                        format!("{p}{k}{p}"),
                        format!("{k}{k}{p}"),
                        format!("{p}{k}{k}"),
                    ]);
                }
            }
            k if k.is_uppercase() => {
                let base: String = k.to_lowercase().collect();
                solo.push(format!("{base}{k}{base}"));
                for p in partners {
                    woven.extend([format!("{k}{base}"), format!("{k}{base}{p}")]);
                }
            }
            '-' => {
                for pair in letters.windows(2) {
                    woven.push(format!("{}-{}", pair[0], pair[1]));
                }
            }
            k => {
                for pair in letters.windows(2) {
                    woven.push(format!("{}{}{k}", pair[0], pair[1]));
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
