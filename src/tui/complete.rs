//! The screen after a passed test: the lesson is behind you, and this is a good place to stop.

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Paragraph};

use crate::stats::{Habit, Summary};
use crate::summary::habit_line;

/// What the lesson-complete screen says.
pub struct View<'a> {
    /// The lesson's place in the course, counting from one.
    pub number: usize,
    pub lessons: usize,
    pub title: &'a str,
    /// The keys the lesson introduced, empty for a review.
    pub keys: &'a str,
    pub summary: &'a Summary,
    /// Lessons passed so far.
    pub passed: usize,
    pub habit: &'a Habit,
    /// What Enter or Space does next.
    pub next_label: &'a str,
}

pub fn draw(frame: &mut Frame, area: Rect, view: &View) {
    let View {
        number,
        lessons,
        title,
        keys,
        summary,
        passed,
        habit,
        next_label,
    } = *view;
    let mut lines = vec![
        Line::styled(
            format!("Lesson {number} of {lessons} done"),
            Style::new().fg(Color::Green).bold(),
        ),
        Line::from(title.to_string().bold()),
    ];
    if !keys.is_empty() {
        lines.push(Line::styled(
            format!("keys  {keys}"),
            Style::new().fg(Color::DarkGray),
        ));
    }
    lines.extend([
        Line::from(""),
        Line::from(format!(
            "test passed at {:.1} % errors and {:.0} cpm ({:.0} wpm)",
            summary.error_rate * 100.0,
            summary.cpm,
            summary.wpm
        )),
        Line::from(""),
        Line::from(format!("{passed} of {lessons} lessons passed")),
        Line::styled(habit_line(habit), Style::new().fg(Color::DarkGray)),
        Line::from(""),
        Line::styled(rest(habit), Style::new().fg(Color::Cyan)),
    ]);
    let help = Line::styled(
        format!("Enter/Space: {next_label}    r: repeat    p: practice    Esc: back    q: quit"),
        Style::new().fg(Color::DarkGray),
    );
    // A frame, so a finished lesson does not look like one more stage report.
    let width = lines
        .iter()
        .map(|line| line.width() as u16 + 6)
        .max()
        .unwrap_or(1);
    let [middle, _, bottom] = Layout::vertical([
        Constraint::Length(lines.len() as u16 + 2),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .flex(Flex::Center)
    .areas(area);
    let [framed] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(middle);
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .centered()
            .block(Block::bordered().border_style(Style::new().fg(Color::Green))),
        framed,
    );
    frame.render_widget(Paragraph::new(help).centered(), bottom);
}

/// The nudge to stop, firmer once the day's minutes are in.
fn rest(habit: &Habit) -> String {
    if habit.target_minutes > 0 && habit.today_minutes >= f64::from(habit.target_minutes) {
        format!(
            "{:.0} minutes done today. A good place to stop.",
            habit.today_minutes
        )
    } else {
        "A good place to stop before the next lesson.".to_string()
    }
}
