//! The Neo family as data: where each character sits, which finger types it, and how to say
//! so. Bone and Neo 2 differ only in where the letters of layers 1 and 2 sit; layer 3 is the
//! same for both, and the character set of layers 1 to 3 is identical.
//!
//! The app never sees keys, only characters, so this exists for hints, the keyboard widget
//! and the curriculum. Tables are transcribed from the installed keylayouts; layer 4
//! (navigation and numpad) only produces characters the number row already covers.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    /// The computer-optimised sibling the Neo project recommends to newcomers.
    #[default]
    Bone,
    /// Classic Neo 2.
    Neo,
}

impl Layout {
    pub fn name(self) -> &'static str {
        match self {
            Layout::Bone => "Bone",
            Layout::Neo => "Neo 2",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hand {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finger {
    Pinky,
    Ring,
    Middle,
    Index,
    Thumb,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Number,
    Top,
    Home,
    Bottom,
    Space,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub layer: u8,
    pub row: Row,
    pub col: usize,
    pub hand: Hand,
    pub finger: Finger,
}

/// Rows as physical key columns. `·` marks a dead key or an empty slot, so that column
/// numbers stay aligned with the keyboard. The number rows and layer 3 are shared.
const NUMBER_ROW_1: &str = "·1234567890-·";
const NUMBER_ROW_2: &str = "·°§ℓ»«$€„“”—·";
const BONE_1: [&str; 3] = ["jduaxphlmwß·", "ctieobnrsgq", "fvüäöyz,.k"];
const BONE_2: [&str; 3] = ["JDUAXPHLMWẞ·", "CTIEOBNRSGQ", "FVÜÄÖYZ–•K"];
const NEO_1: [&str; 3] = ["xvlcwkhgfqß·", "uiaeosnrtdy", "üöäpzbm,.j"];
const NEO_2: [&str; 3] = ["XVLCWKHGFQẞ·", "UIAEOSNRTDY", "ÜÖÄPZBM–•J"];
const LAYER_3: [&str; 4] = [
    "·¹²³›‹¢¥‚‘’··",
    "…_[]^!<>=&ſ·",
    "\\/{}*?()-:@",
    "#$|~`+%\"';",
];

const ROWS: [Row; 4] = [Row::Number, Row::Top, Row::Home, Row::Bottom];

/// Finger per column on the letter rows; the number row is shifted by one.
const FINGERS: [(Hand, Finger); 12] = [
    (Hand::Left, Finger::Pinky),
    (Hand::Left, Finger::Ring),
    (Hand::Left, Finger::Middle),
    (Hand::Left, Finger::Index),
    (Hand::Left, Finger::Index),
    (Hand::Right, Finger::Index),
    (Hand::Right, Finger::Index),
    (Hand::Right, Finger::Middle),
    (Hand::Right, Finger::Ring),
    (Hand::Right, Finger::Pinky),
    (Hand::Right, Finger::Pinky),
    (Hand::Right, Finger::Pinky),
];

fn layers(layout: Layout) -> [[&'static str; 4]; 3] {
    let (letters_1, letters_2) = match layout {
        Layout::Bone => (BONE_1, BONE_2),
        Layout::Neo => (NEO_1, NEO_2),
    };
    [
        [NUMBER_ROW_1, letters_1[0], letters_1[1], letters_1[2]],
        [NUMBER_ROW_2, letters_2[0], letters_2[1], letters_2[2]],
        LAYER_3,
    ]
}

fn finger_at(row: Row, col: usize) -> (Hand, Finger) {
    let index = match row {
        Row::Number => col.saturating_sub(1),
        _ => col,
    };
    FINGERS[index.min(FINGERS.len() - 1)]
}

fn all_positions(layout: Layout) -> impl Iterator<Item = (char, Position)> {
    layers(layout)
        .into_iter()
        .enumerate()
        .flat_map(|(layer_index, rows)| {
            rows.into_iter().zip(ROWS).flat_map(move |(keys, row)| {
                keys.chars()
                    .enumerate()
                    .filter(|(_, c)| *c != '·')
                    .map(move |(col, c)| {
                        let (hand, finger) = finger_at(row, col);
                        (
                            c,
                            Position {
                                layer: layer_index as u8 + 1,
                                row,
                                col,
                                hand,
                                finger,
                            },
                        )
                    })
            })
        })
}

/// Every place a character can be produced, lowest layer first.
pub fn positions(layout: Layout, grapheme: &str) -> Vec<Position> {
    if grapheme == " " {
        return vec![Position {
            layer: 1,
            row: Row::Space,
            col: 0,
            hand: Hand::Right,
            finger: Finger::Thumb,
        }];
    }
    let mut chars = grapheme.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return Vec::new();
    };
    all_positions(layout)
        .filter(|(key, _)| *key == c)
        .map(|(_, position)| position)
        .collect()
}

/// Whether the layout family can produce the character at all, on layers 1 to 3. The set
/// is the same for Bone and Neo, only the places differ.
pub fn is_typeable(grapheme: &str) -> bool {
    !positions(Layout::Bone, grapheme).is_empty()
}

/// The position the curriculum teaches: off the number row where possible, then the lowest
/// layer. `-` and `$` are the cases where this matters.
pub fn primary(layout: Layout, grapheme: &str) -> Option<Position> {
    positions(layout, grapheme)
        .into_iter()
        .min_by_key(|position| (position.row == Row::Number, position.layer))
}

/// One line telling a learner how to type the character, or `None` for characters the
/// layout does not produce on layers 1 to 3.
pub fn hint(layout: Layout, grapheme: &str) -> Option<String> {
    let position = primary(layout, grapheme)?;
    if position.row == Row::Space {
        return Some("space  thumb".to_string());
    }
    let finger = finger_phrase(position);
    let row = row_name(position.row);
    Some(match position.layer {
        1 => format!("{grapheme}  {finger}, {row}"),
        layer => {
            let base = base_key(layout, position)
                .map(String::from)
                .unwrap_or_default();
            let modifier = match (layer, position.hand) {
                (2, Hand::Left) => "Shift with the right pinky",
                (2, Hand::Right) => "Shift with the left pinky",
                (_, Hand::Left) => "Mod3 with the right pinky (# key)",
                (_, Hand::Right) => "Mod3 with the left pinky (Caps Lock)",
            };
            format!("{grapheme}  {modifier}, then {base} with the {finger}")
        }
    })
}

/// The layer 1 character on the same physical key.
fn base_key(layout: Layout, position: Position) -> Option<&'static str> {
    let row_index = ROWS.iter().position(|row| *row == position.row)?;
    let keys = layers(layout)[0][row_index];
    let (start, c) = keys.char_indices().nth(position.col)?;
    (c != '·').then(|| &keys[start..start + c.len_utf8()])
}

fn finger_phrase(position: Position) -> String {
    let hand = match position.hand {
        Hand::Left => "left",
        Hand::Right => "right",
    };
    let finger = match position.finger {
        Finger::Pinky => "pinky",
        Finger::Ring => "ring finger",
        Finger::Middle => "middle finger",
        Finger::Index => "index finger",
        Finger::Thumb => "thumb",
    };
    let reach = match (position.row, position.col) {
        (Row::Number, _) => "",
        (_, 4) => " (reach right)",
        (_, 5) => " (reach left)",
        (_, col) if col >= 10 => " (reach right)",
        _ => "",
    };
    format!("{hand} {finger}{reach}")
}

fn row_name(row: Row) -> &'static str {
    match row {
        Row::Number => "number row",
        Row::Top => "top row",
        Row::Home => "home row",
        Row::Bottom => "bottom row",
        Row::Space => "space",
    }
}

/// Labels of layer 1, 2 or 3 per physical key, row by row. Dead keys and empty slots are `""`.
pub fn layer_rows(layout: Layout, layer: u8) -> [Vec<&'static str>; 4] {
    let index = usize::from(layer.clamp(1, 3)) - 1;
    layers(layout)[index].map(|keys| {
        keys.char_indices()
            .map(|(start, c)| {
                if c == '·' {
                    ""
                } else {
                    &keys[start..start + c.len_utf8()]
                }
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(layout: Layout, grapheme: &str) -> Position {
        primary(layout, grapheme).unwrap_or_else(|| panic!("no position for {grapheme:?}"))
    }

    fn hand_finger(layout: Layout, grapheme: &str) -> (Hand, Finger) {
        let p = pos(layout, grapheme);
        (p.hand, p.finger)
    }

    #[test]
    fn neo_home_row_letters_sit_under_the_expected_fingers() {
        let neo = Layout::Neo;
        let e = pos(neo, "e");
        assert_eq!(
            (e.layer, e.row, e.col, e.hand, e.finger),
            (1, Row::Home, 3, Hand::Left, Finger::Index)
        );
        assert_eq!(hand_finger(neo, "n"), (Hand::Right, Finger::Index));
        assert_eq!(hand_finger(neo, "a"), (Hand::Left, Finger::Middle));
        assert_eq!(hand_finger(neo, "r"), (Hand::Right, Finger::Middle));
        assert_eq!(hand_finger(neo, "u"), (Hand::Left, Finger::Pinky));
        assert_eq!(hand_finger(neo, "d"), (Hand::Right, Finger::Pinky));
        assert_eq!(hand_finger(neo, "i"), (Hand::Left, Finger::Ring));
        assert_eq!(hand_finger(neo, "t"), (Hand::Right, Finger::Ring));
        assert_eq!(
            (pos(neo, "o").finger, pos(neo, "s").finger),
            (Finger::Index, Finger::Index)
        );
        assert_eq!(
            (pos(neo, "y").row, pos(neo, "y").finger),
            (Row::Home, Finger::Pinky)
        );
    }

    #[test]
    fn bone_home_row_letters_sit_under_the_expected_fingers() {
        let bone = Layout::Bone;
        assert_eq!(hand_finger(bone, "e"), (Hand::Left, Finger::Index));
        assert_eq!(hand_finger(bone, "n"), (Hand::Right, Finger::Index));
        assert_eq!(hand_finger(bone, "i"), (Hand::Left, Finger::Middle));
        assert_eq!(hand_finger(bone, "r"), (Hand::Right, Finger::Middle));
        assert_eq!(hand_finger(bone, "t"), (Hand::Left, Finger::Ring));
        assert_eq!(hand_finger(bone, "s"), (Hand::Right, Finger::Ring));
        assert_eq!(hand_finger(bone, "c"), (Hand::Left, Finger::Pinky));
        assert_eq!(hand_finger(bone, "g"), (Hand::Right, Finger::Pinky));
        assert_eq!(
            (pos(bone, "o").col, pos(bone, "b").col),
            (4, 5),
            "index stretch keys"
        );
        assert_eq!(
            (pos(bone, "a").row, pos(bone, "a").finger),
            (Row::Top, Finger::Index)
        );
        assert_eq!(
            (pos(bone, "ü").row, pos(bone, "ü").finger),
            (Row::Bottom, Finger::Middle)
        );
        assert_eq!((pos(bone, "q").row, pos(bone, "q").col), (Row::Home, 10));
        assert_eq!(
            (pos(bone, ",").finger, pos(bone, ".").finger),
            (Finger::Middle, Finger::Ring)
        );
    }

    #[test]
    fn layer_three_is_shared_and_the_number_row_too() {
        for layout in [Layout::Bone, Layout::Neo] {
            let brace = pos(layout, "{");
            assert_eq!(
                (brace.layer, brace.row, brace.col, brace.finger),
                (3, Row::Home, 2, Finger::Middle)
            );
            assert_eq!(
                (pos(layout, "5").row, pos(layout, "5").hand),
                (Row::Number, Hand::Left)
            );
            assert_eq!(
                (pos(layout, "-").layer, pos(layout, "-").row),
                (3, Row::Home)
            );
            assert_eq!(
                (pos(layout, "$").layer, pos(layout, "$").row),
                (3, Row::Bottom)
            );
            assert_eq!(positions(layout, "-").len(), 2);
            assert_eq!(
                positions(layout, "^").len(),
                1,
                "only the live ^ on layer 3"
            );
            assert!(positions(layout, "´").is_empty());
            assert_eq!(
                (pos(layout, " ").row, pos(layout, " ").finger),
                (Row::Space, Finger::Thumb)
            );
            assert_eq!((pos(layout, "E").layer, pos(layout, "E").col), (2, 3));
        }
        assert!(is_typeable("ß") && is_typeable("{") && !is_typeable("ʘ") && !is_typeable("´"));
    }

    #[test]
    fn hints_name_finger_row_and_modifier_per_layout() {
        let (bone, neo) = (Layout::Bone, Layout::Neo);
        assert_eq!(
            hint(bone, "e").as_deref(),
            Some("e  left index finger, home row")
        );
        assert_eq!(hint(bone, "c").as_deref(), Some("c  left pinky, home row"));
        assert_eq!(
            hint(bone, "o").as_deref(),
            Some("o  left index finger (reach right), home row")
        );
        assert_eq!(
            hint(bone, "b").as_deref(),
            Some("b  right index finger (reach left), home row")
        );
        assert_eq!(
            hint(bone, "q").as_deref(),
            Some("q  right pinky (reach right), home row")
        );
        assert_eq!(
            hint(neo, "y").as_deref(),
            Some("y  right pinky (reach right), home row")
        );
        assert_eq!(
            hint(neo, "l").as_deref(),
            Some("l  left middle finger, top row")
        );
        assert_eq!(
            hint(bone, "4").as_deref(),
            Some("4  left index finger, number row")
        );
        assert_eq!(hint(bone, " ").as_deref(), Some("space  thumb"));
        assert_eq!(
            hint(bone, "E").as_deref(),
            Some("E  Shift with the right pinky, then e with the left index finger")
        );
        assert_eq!(
            hint(bone, "{").as_deref(),
            Some("{  Mod3 with the right pinky (# key), then i with the left middle finger")
        );
        assert_eq!(
            hint(neo, "{").as_deref(),
            Some("{  Mod3 with the right pinky (# key), then a with the left middle finger")
        );
        assert_eq!(
            hint(bone, ")").as_deref(),
            Some(")  Mod3 with the left pinky (Caps Lock), then r with the right middle finger")
        );
        assert_eq!(hint(bone, "ʘ"), None);
    }

    #[test]
    fn keyboard_rows_carry_the_layout_labels() {
        assert_eq!(layer_rows(Layout::Bone, 1)[2].join(""), "ctieobnrsgq");
        assert_eq!(layer_rows(Layout::Neo, 1)[2].join(""), "uiaeosnrtdy");
        assert_eq!(layer_rows(Layout::Bone, 2)[2].join(""), "CTIEOBNRSGQ");
        assert_eq!(layer_rows(Layout::Bone, 1)[0][0], "", "dead ^ is blank");
        assert_eq!(layer_rows(Layout::Bone, 1)[3].len(), 10);
        let rows = layer_rows(Layout::Neo, 3);
        assert_eq!(rows[2].join(""), "\\/{}*?()-:@");
        assert_eq!(rows[1][1], "_");
        assert_eq!(rows[3][0], "#");
        assert_eq!(layer_rows(Layout::Bone, 3), layer_rows(Layout::Neo, 3));
    }
}
