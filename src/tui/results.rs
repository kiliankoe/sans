//! The results screen after a stage: the numbers and whether it passed.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Text};
use ratatui::widgets::Paragraph;

use super::typing::clock;
use crate::course::PASS_ERROR_RATE;
use crate::stats::Summary;

pub fn draw(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    summary: &Summary,
    finished: bool,
    is_test: bool,
    next_label: &str,
) {
    let threshold = PASS_ERROR_RATE;
    let passed = finished && summary.error_rate <= threshold;
    let (headline, color) = match (finished, is_test, passed) {
        (false, _, _) => ("Stage aborted", Color::Yellow),
        (true, false, _) => ("Stage complete", Color::Green),
        (true, true, true) => ("Test passed", Color::Green),
        (true, true, false) => ("Test complete, but the error rate is too high", Color::Red),
    };
    let lines = vec![
        Line::from(title.to_string().bold()),
        Line::from(""),
        Line::styled(headline, Style::new().fg(color).bold()),
        Line::from(""),
        Line::from(format!(
            "error rate   {:>6.2} %    (pass at {:.0} % or below)",
            summary.error_rate * 100.0,
            threshold * 100.0
        )),
        Line::from(format!("errors       {:>6}", summary.errors)),
        Line::from(format!("characters   {:>6}", summary.chars)),
        Line::from(format!(
            "speed        {:>6.0} cpm    {:.0} wpm",
            summary.cpm, summary.wpm
        )),
        Line::from(format!("active time  {:>6}", clock(summary.active))),
        Line::from(""),
        Line::styled(
            format!("Enter: {next_label}    r: repeat    Esc: lessons    q: quit"),
            Style::new().fg(Color::DarkGray),
        ),
    ];
    let width = lines
        .iter()
        .map(|line| line.width() as u16)
        .max()
        .unwrap_or(1);
    let height = lines.len() as u16;
    let [horizontal] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    let [target] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(horizontal);
    frame.render_widget(Paragraph::new(Text::from(lines)), target);
}
