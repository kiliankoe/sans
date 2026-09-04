//! A small keyboard drawing: layer 1 labels, unlocked keys plain, locked keys dim, the keys
//! being introduced highlighted.

use std::collections::HashSet;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::layout;

/// Rough physical stagger of the four rows, in cells, with three cells per key.
const STAGGER: [usize; 4] = [0, 2, 3, 4];

pub const HEIGHT: u16 = 4;
pub const WIDTH: u16 = 13 * 3 + 4;

/// `highlight` and `unlocked` are compared case-insensitively, so capitals light up their key.
pub fn draw(
    frame: &mut Frame,
    area: Rect,
    highlight: &HashSet<String>,
    unlocked: &HashSet<String>,
) {
    let lower = |set: &HashSet<String>| -> HashSet<String> {
        set.iter().map(|k| k.to_lowercase()).collect()
    };
    let (highlight, unlocked) = (lower(highlight), lower(unlocked));
    draw_styled(frame, area, |label| {
        if highlight.contains(label) {
            Style::new()
                .fg(Color::Black)
                .bg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else if unlocked.contains(label) {
            Style::new()
        } else {
            Style::new().fg(Color::DarkGray)
        }
    });
}

/// The keyboard with one style per key label, for heatmaps.
pub fn draw_styled(frame: &mut Frame, area: Rect, style_of: impl Fn(&str) -> Style) {
    draw_layer(frame, area, 1, style_of);
}

/// The keyboard showing the labels of `layer` (1 to 3), one style per label.
pub fn draw_layer(frame: &mut Frame, area: Rect, layer: u8, style_of: impl Fn(&str) -> Style) {
    for (index, row) in layout::layer_rows(layer).iter().enumerate() {
        let y = area.y + index as u16;
        if y >= area.bottom() {
            break;
        }
        let mut spans = vec![Span::raw(" ".repeat(STAGGER[index]))];
        for label in row {
            let style = if label.is_empty() {
                Style::new()
            } else {
                style_of(label)
            };
            let text = if label.is_empty() {
                "   ".to_string()
            } else {
                format!(" {label} ")
            };
            spans.push(Span::styled(text, style));
        }
        frame.render_widget(
            Line::from(spans),
            Rect {
                x: area.x,
                y,
                width: area.width,
                height: 1,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    #[test]
    fn highlights_new_keys_and_dims_locked_ones() {
        let mut terminal = Terminal::new(TestBackend::new(60, 5)).unwrap();
        let highlight: HashSet<String> = ["E".to_string()].into();
        let unlocked: HashSet<String> = ["e".to_string(), "n".to_string()].into();
        terminal
            .draw(|frame| draw(frame, frame.area(), &highlight, &unlocked))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let find = |needle: char| {
            (0..buffer.area.height)
                .flat_map(|y| (0..buffer.area.width).map(move |x| (x, y)))
                .find(|&(x, y)| buffer.cell((x, y)).unwrap().symbol() == needle.to_string())
                .unwrap()
        };
        assert_eq!(buffer.cell(find('e')).unwrap().bg, Color::Yellow);
        assert_eq!(buffer.cell(find('n')).unwrap().fg, Color::Reset);
        assert_eq!(buffer.cell(find('a')).unwrap().fg, Color::DarkGray);
    }
}
