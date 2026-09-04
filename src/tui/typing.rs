//! The typing screen: title, the text with per-character state, and a status line.

use std::time::Duration;

use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Wrap};

use super::app::Status;
use crate::engine::Engine;

/// Lesson text never exceeds this width; longer lines wrap as a safety net.
const MAX_WIDTH: u16 = 60;

#[derive(Clone, Copy)]
enum State {
    Typed,
    Cursor,
    Wrong,
    Pending,
}

fn style(state: State) -> Style {
    match state {
        State::Typed => Style::new(),
        State::Cursor => Style::new().add_modifier(Modifier::UNDERLINED | Modifier::BOLD),
        State::Wrong => Style::new().fg(Color::White).bg(Color::Red),
        State::Pending => Style::new().fg(Color::DarkGray),
    }
}

/// The target text as styled lines, split at newlines, with the wrong grapheme shown in
/// place of the expected one.
pub fn styled_lines(engine: &Engine) -> Vec<Line<'static>> {
    let cursor = engine.cursor();
    let wrong = engine.wrong();
    let mut lines = Vec::new();
    let mut current = Vec::new();
    for (index, grapheme) in engine.target().iter().enumerate() {
        let state = match index.cmp(&cursor) {
            std::cmp::Ordering::Less => State::Typed,
            std::cmp::Ordering::Equal if wrong.is_some() => State::Wrong,
            std::cmp::Ordering::Equal => State::Cursor,
            std::cmp::Ordering::Greater => State::Pending,
        };
        let shown = match state {
            State::Wrong => visible_typed(wrong.unwrap_or_default()),
            _ => visible_expected(grapheme),
        };
        current.push(Span::styled(shown, style(state)));
        if grapheme == "\n" {
            lines.push(Line::from(std::mem::take(&mut current)));
        }
    }
    lines.push(Line::from(current));
    lines
}

fn visible_expected(grapheme: &str) -> String {
    match grapheme {
        "\n" => "↵".to_string(),
        "\t" => "→".to_string(),
        other => other.to_string(),
    }
}

fn visible_typed(grapheme: &str) -> String {
    match grapheme {
        "\n" => "␤".to_string(),
        "\t" => "⇥".to_string(),
        " " => "␣".to_string(),
        other => other.to_string(),
    }
}

pub fn draw(frame: &mut Frame, area: Rect, title: &str, engine: &Engine, status: &Status) {
    let [title_area, body, status_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);

    let mut heading = Line::from(title.to_string().bold());
    if status.paused {
        heading.push_span(Span::styled(
            "   paused: terminal lost focus",
            Style::new().fg(Color::Yellow),
        ));
    }
    frame.render_widget(Paragraph::new(heading), title_area);

    let lines = styled_lines(engine);
    let width = body.width.saturating_sub(2).clamp(1, MAX_WIDTH);
    let height = lines
        .iter()
        .map(|line| (line.width() as u16).div_ceil(width).max(1))
        .sum::<u16>()
        .clamp(1, body.height.max(1));
    let text_area = centered(body, width, height);
    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        text_area,
    );

    frame.render_widget(Paragraph::new(status_line(status)), status_area);
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let [horizontal] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    let [vertical] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(horizontal);
    vertical
}

fn status_line(status: &Status) -> Line<'static> {
    let mut spans = vec![Span::raw(format!(
        "{:>3.0} cpm   {} {} ({:.1}%)   {}/{}   {}",
        status.cpm,
        status.errors,
        if status.errors == 1 {
            "error"
        } else {
            "errors"
        },
        status.error_rate * 100.0,
        status.done,
        status.total,
        clock(status.active)
    ))];
    if let Some(message) = &status.flash {
        spans.push(Span::styled(
            format!("   {message}"),
            Style::new().fg(Color::Yellow),
        ));
    }
    Line::from(spans)
}

pub fn clock(duration: Duration) -> String {
    let seconds = duration.as_secs();
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Key;
    use std::time::Duration;

    #[test]
    fn wrong_grapheme_replaces_the_expected_one_and_whitespace_is_visible() {
        let mut engine = Engine::new("e n");
        engine.input(Key::Char("e".into()), Duration::ZERO);
        engine.input(Key::Enter, Duration::ZERO);
        let lines = styled_lines(&engine);
        assert_eq!(lines.len(), 1);
        let shown: Vec<&str> = lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert_eq!(shown, ["e", "␤", "n"]);
        assert_eq!(lines[0].spans[1].style.bg, Some(Color::Red));
        assert_eq!(lines[0].spans[2].style.fg, Some(Color::DarkGray));
    }

    #[test]
    fn newlines_split_lines_and_show_a_return_mark() {
        let engine = Engine::new("a\nb");
        let lines = styled_lines(&engine);
        assert_eq!(lines.len(), 2);
        assert_eq!(
            lines[0]
                .spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>(),
            "a↵"
        );
        assert_eq!(lines[1].spans[0].content, "b");
    }

    #[test]
    fn clock_formats_minutes_and_seconds() {
        assert_eq!(clock(Duration::from_secs(0)), "0:00");
        assert_eq!(clock(Duration::from_secs(65)), "1:05");
    }
}
