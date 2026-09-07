//! The lesson list: where you are, what is passed, what is locked.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};

use crate::course::{Course, Progress};
use crate::layout::Layout as KeyLayout;
use crate::stats::Habit;
use crate::summary::habit_line;

pub fn draw(
    frame: &mut Frame,
    area: Rect,
    course: &Course,
    progress: &Progress,
    habit: &Habit,
    selected: usize,
    layout: KeyLayout,
) {
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
            let status = match progress.best(&lesson.id) {
                Some(best) if progress.passed(&lesson.id) => Span::styled(
                    format!("passed {:.1}%", best * 100.0),
                    Style::new().fg(Color::Green),
                ),
                Some(best) => Span::styled(
                    format!("best {:.1}%", best * 100.0),
                    Style::new().fg(Color::Yellow),
                ),
                None if available => Span::styled("next", Style::new().fg(Color::Cyan)),
                None => Span::styled("locked", Style::new().fg(Color::DarkGray)),
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
                status,
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

    frame.render_widget(
        Paragraph::new(Line::styled(
            "Enter/Space: start    arrows: move    p: practice    f: files    s: stats    q: quit",
            Style::new().fg(Color::DarkGray),
        )),
        help_area,
    );
}
