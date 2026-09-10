//! The lesson list: where you are, what is passed, what is locked.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};

use crate::course::{Course, Progress, Resume};
use crate::layout::Layout as KeyLayout;
use crate::stats::Habit;
use crate::summary::habit_line;

/// What the lesson list shows.
pub struct View<'a> {
    pub course: &'a Course,
    pub progress: &'a Progress,
    pub resume: &'a Resume,
    pub habit: &'a Habit,
    pub selected: usize,
    pub layout: KeyLayout,
    /// A pending question, in place of the keys until it is answered.
    pub question: Option<&'a str>,
}

pub fn draw(frame: &mut Frame, area: Rect, view: &View) {
    let View {
        course,
        progress,
        resume,
        habit,
        selected,
        layout,
        question,
    } = *view;
    let [title_area, list_area, help_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);
    let passed = course
        .lessons()
        .iter()
        .filter(|l| progress.passed(&l.id))
        .count();
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec!["sans".bold(), format!("   {}", layout.name()).into()]),
            Line::from(format!(
                "Track A, letters: {passed} of {} lessons passed",
                course.lessons().len()
            )),
            Line::styled(habit_line(habit), Style::new().fg(Color::DarkGray)),
        ]),
        title_area,
    );

    let items: Vec<ListItem> = course
        .lessons()
        .iter()
        .enumerate()
        .map(|(index, lesson)| {
            let available = progress.available(course, index);
            let (status, status_style) = match progress.best(&lesson.id) {
                Some(best) if progress.passed(&lesson.id) => (
                    format!("passed {:.1}%", best * 100.0),
                    Style::new().fg(Color::Green),
                ),
                Some(best) => (
                    format!("best {:.1}%", best * 100.0),
                    Style::new().fg(Color::Yellow),
                ),
                None if available => ("next".to_string(), Style::new().fg(Color::Cyan)),
                None => ("locked".to_string(), Style::new().fg(Color::DarkGray)),
            };
            // Where the lesson resumes is how many of its stages are behind it.
            let done = resume.stage(&lesson.id);
            let started = if done > 0 && !progress.passed(&lesson.id) {
                format!("{done} of {} stages", course.stages(index).len())
            } else {
                String::new()
            };
            let keys = lesson.new.join(" ");
            let base = if available {
                Style::new()
            } else {
                Style::new().fg(Color::DarkGray)
            };
            let row = Line::from(vec![
                Span::styled(format!("{:>2}  {:<22}", index + 1, lesson.title), base),
                Span::styled(format!("{:<24}", keys), base),
                Span::styled(format!("{status:<13}"), status_style),
                Span::styled(started, Style::new().fg(Color::DarkGray)),
            ]);
            let mut lines = Vec::new();
            if let Some(section) = &lesson.section {
                lines.push(Line::styled(
                    section.clone(),
                    Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ));
            }
            lines.push(row);
            ListItem::new(lines)
        })
        .collect();
    let list = List::new(items)
        .highlight_style(Style::new().add_modifier(Modifier::REVERSED))
        .highlight_symbol("› ");
    let mut state = ListState::default().with_selected(Some(selected));
    frame.render_stateful_widget(list, list_area, &mut state);

    let help = match question {
        Some(text) => Line::styled(text.to_string(), Style::new().fg(Color::Yellow)),
        None => Line::styled(
            "Enter/Space: start    arrows: move    r: reset    p: practice    f: files    \
             s: stats    q: quit",
            Style::new().fg(Color::DarkGray),
        ),
    };
    frame.render_widget(Paragraph::new(help), help_area);
}
