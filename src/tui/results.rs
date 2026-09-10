//! The results screen after a stage: the numbers and whether it passed.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Text};
use ratatui::widgets::Paragraph;

use super::typing::clock;
use crate::course::PASS_ERROR_RATE;
use crate::stats::Summary;

/// What the results screen says about a session.
pub struct View<'a> {
    pub title: &'a str,
    pub summary: &'a Summary,
    pub finished: bool,
    /// "Stage" or "Chunk".
    pub what: &'a str,
    /// Test stages get a pass or fail verdict.
    pub is_test: bool,
    /// What Enter or Space does next.
    pub next_label: &'a str,
    /// A pending question, shown under the keys until it is answered.
    pub question: Option<&'a str>,
}

pub fn draw(frame: &mut Frame, area: Rect, view: &View) {
    let View {
        title,
        summary,
        finished,
        what,
        is_test,
        next_label,
        question,
    } = *view;
    let threshold = PASS_ERROR_RATE;
    let passed = finished && summary.error_rate <= threshold;
    let (headline, color) = match (finished, is_test, passed) {
        (false, _, _) => (format!("{what} aborted"), Color::Yellow),
        (true, false, _) => (format!("{what} complete"), Color::Green),
        (true, true, true) => ("Test passed".to_string(), Color::Green),
        (true, true, false) => (
            "Test complete, but the error rate is too high".to_string(),
            Color::Red,
        ),
    };
    // Only a test gates the lesson, so only it names the threshold.
    let guard = if is_test {
        format!("    (pass at {:.0} % or below)", threshold * 100.0)
    } else {
        String::new()
    };
    let lines = vec![
        Line::from(title.to_string().bold()),
        Line::from(""),
        Line::styled(headline, Style::new().fg(color).bold()),
        Line::from(""),
        Line::from(format!(
            "error rate   {:>6.2} %{guard}",
            summary.error_rate * 100.0
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
            format!(
                "Enter/Space: {next_label}    r: repeat    p: practice    Esc: back    q: quit"
            ),
            Style::new().fg(Color::DarkGray),
        ),
        // Always a line, so an appearing question does not shift the box.
        Line::styled(
            question.unwrap_or("").to_string(),
            Style::new().fg(Color::Yellow),
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
