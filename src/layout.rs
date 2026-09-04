//! Neo 2 as data: where each character sits, which finger types it, and how to say so.
//!
//! The app never sees keys, only characters, so this exists for hints, the keyboard widget
//! and the curriculum. Layers 1 to 3 are transcribed from the installed keylayout; layer 4
//! (navigation and numpad) only produces characters the number row already covers.

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

/// Rows per layer as physical key columns. `·` marks a dead key or an empty slot, so that
/// column numbers stay aligned with the keyboard.
const LAYERS: [[&str; 4]; 3] = [
    ["·1234567890-·", "xvlcwkhgfqß·", "uiaeosnrtdy", "üöäpzbm,.j"],
    ["·°§ℓ»«$€„“”—·", "XVLCWKHGFQẞ·", "UIAEOSNRTDY", "ÜÖÄPZBM–•J"],
    [
        "·¹²³›‹¢¥‚‘’··",
        "…_[]^!<>=&ſ·",
        "\\/{}*?()-:@",
        "#$|~`+%\"';",
    ],
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

fn finger_at(row: Row, col: usize) -> (Hand, Finger) {
    let index = match row {
        Row::Number => col.saturating_sub(1),
        _ => col,
    };
    FINGERS[index.min(FINGERS.len() - 1)]
}

fn all_positions() -> impl Iterator<Item = (char, Position)> {
    LAYERS.iter().enumerate().flat_map(|(layer_index, rows)| {
        rows.iter().zip(ROWS).flat_map(move |(keys, row)| {
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
pub fn positions(grapheme: &str) -> Vec<Position> {
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
    all_positions()
        .filter(|(key, _)| *key == c)
        .map(|(_, position)| position)
        .collect()
}

/// The position the curriculum teaches: off the number row where possible, then the lowest
/// layer. `-` and `$` are the cases where this matters.
pub fn primary(grapheme: &str) -> Option<Position> {
    positions(grapheme)
        .into_iter()
        .min_by_key(|position| (position.row == Row::Number, position.layer))
}

/// One line telling a learner how to type the character, or `None` for characters Neo does
/// not produce on layers 1 to 3.
pub fn hint(grapheme: &str) -> Option<String> {
    let position = primary(grapheme)?;
    if position.row == Row::Space {
        return Some("space  thumb".to_string());
    }
    let finger = finger_phrase(position);
    let row = row_name(position.row);
    Some(match position.layer {
        1 => format!("{grapheme}  {finger}, {row}"),
        layer => {
            let base = base_key(position).map(String::from).unwrap_or_default();
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
fn base_key(position: Position) -> Option<&'static str> {
    let row_index = ROWS.iter().position(|row| *row == position.row)?;
    let keys = LAYERS[0][row_index];
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
pub fn layer_rows(layer: u8) -> [Vec<&'static str>; 4] {
    let index = usize::from(layer.clamp(1, 3)) - 1;
    LAYERS[index].map(|keys| {
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

    fn pos(grapheme: &str) -> Position {
        primary(grapheme).unwrap_or_else(|| panic!("no position for {grapheme:?}"))
    }

    #[test]
    fn home_row_letters_sit_under_the_expected_fingers() {
        let e = pos("e");
        assert_eq!(
            (e.layer, e.row, e.col, e.hand, e.finger),
            (1, Row::Home, 3, Hand::Left, Finger::Index)
        );
        assert_eq!(
            (pos("n").hand, pos("n").finger),
            (Hand::Right, Finger::Index)
        );
        assert_eq!(
            (pos("a").hand, pos("a").finger),
            (Hand::Left, Finger::Middle)
        );
        assert_eq!(
            (pos("r").hand, pos("r").finger),
            (Hand::Right, Finger::Middle)
        );
        assert_eq!(
            (pos("u").hand, pos("u").finger),
            (Hand::Left, Finger::Pinky)
        );
        assert_eq!(
            (pos("d").hand, pos("d").finger),
            (Hand::Right, Finger::Pinky)
        );
        assert_eq!((pos("i").hand, pos("i").finger), (Hand::Left, Finger::Ring));
        assert_eq!(
            (pos("t").hand, pos("t").finger),
            (Hand::Right, Finger::Ring)
        );
        assert_eq!(
            (pos("o").finger, pos("s").finger),
            (Finger::Index, Finger::Index)
        );
        assert_eq!((pos("y").row, pos("y").finger), (Row::Home, Finger::Pinky));
    }

    #[test]
    fn other_rows_and_layers() {
        assert_eq!((pos("l").row, pos("l").finger), (Row::Top, Finger::Middle));
        assert_eq!(
            (pos("ü").row, pos("ü").hand, pos("ü").finger),
            (Row::Bottom, Hand::Left, Finger::Pinky)
        );
        assert_eq!(
            (pos("j").row, pos("j").hand, pos("j").finger),
            (Row::Bottom, Hand::Right, Finger::Pinky)
        );
        assert_eq!(
            (pos(",").finger, pos(".").finger),
            (Finger::Middle, Finger::Ring)
        );
        assert_eq!(
            (pos("5").row, pos("5").hand, pos("5").finger),
            (Row::Number, Hand::Left, Finger::Index)
        );
        assert_eq!(
            (pos("6").hand, pos("0").finger),
            (Hand::Right, Finger::Pinky)
        );
        assert_eq!((pos("E").layer, pos("E").col), (2, 3));
        let brace = pos("{");
        assert_eq!(
            (brace.layer, brace.row, brace.col, brace.finger),
            (3, Row::Home, 2, Finger::Middle)
        );
        assert_eq!((pos(" ").row, pos(" ").finger), (Row::Space, Finger::Thumb));
    }

    #[test]
    fn duplicates_prefer_the_letter_rows_and_then_the_lowest_layer() {
        assert_eq!(positions("-").len(), 2);
        assert_eq!((pos("-").layer, pos("-").row), (3, Row::Home));
        assert_eq!((pos("$").layer, pos("$").row), (3, Row::Bottom));
        assert_eq!(pos("1").row, Row::Number);
    }

    #[test]
    fn dead_keys_are_not_positions() {
        assert_eq!(positions("^").len(), 1, "only the live ^ on layer 3");
        assert_eq!(pos("^").layer, 3);
        assert_eq!(pos("`").layer, 3);
        assert!(positions("´").is_empty());
        assert!(positions("ʘ").is_empty());
    }

    #[test]
    fn hints_name_finger_row_and_modifier() {
        assert_eq!(hint("e").as_deref(), Some("e  left index finger, home row"));
        assert_eq!(
            hint("o").as_deref(),
            Some("o  left index finger (reach right), home row")
        );
        assert_eq!(
            hint("s").as_deref(),
            Some("s  right index finger (reach left), home row")
        );
        assert_eq!(
            hint("y").as_deref(),
            Some("y  right pinky (reach right), home row")
        );
        assert_eq!(hint("l").as_deref(), Some("l  left middle finger, top row"));
        assert_eq!(
            hint("4").as_deref(),
            Some("4  left index finger, number row")
        );
        assert_eq!(hint(" ").as_deref(), Some("space  thumb"));
        assert_eq!(
            hint("E").as_deref(),
            Some("E  Shift with the right pinky, then e with the left index finger")
        );
        assert_eq!(
            hint("{").as_deref(),
            Some("{  Mod3 with the right pinky (# key), then a with the left middle finger")
        );
        assert_eq!(
            hint(")").as_deref(),
            Some(")  Mod3 with the left pinky (Caps Lock), then r with the right middle finger")
        );
        assert_eq!(hint("ʘ"), None);
    }

    #[test]
    fn keyboard_rows_of_layer_three_carry_the_symbols() {
        let rows = layer_rows(3);
        assert_eq!(rows[2].join(""), "\\/{}*?()-:@");
        assert_eq!(rows[1][1], "_");
        assert_eq!(rows[3][0], "#");
        assert_eq!(rows[0][0], "", "nothing on the far left of the number row");
        assert_eq!(layer_rows(2)[2].join(""), "UIAEOSNRTDY");
    }

    #[test]
    fn keyboard_rows_carry_layer_one_labels() {
        let rows = layer_rows(1);
        assert_eq!(rows[2].join(""), "uiaeosnrtdy");
        assert_eq!(rows[1][0], "x");
        assert_eq!(rows[0][0], "", "dead ^ is blank");
        assert_eq!(rows[3].len(), 10);
    }
}
