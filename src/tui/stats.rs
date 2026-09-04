//! The stats screen: trend charts per day, keys and bigrams, lessons.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::symbols::Marker;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Axis, Block, Chart, Dataset, GraphType, Paragraph, Row, Table};

use super::keyboard;
use crate::course::Course;
use crate::stats::{self, KeyStat, Snapshot};
use crate::summary::habit_line;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Trend,
    Keys,
    Lessons,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    Days30,
    Days90,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatsView {
    pub page: Page,
    pub range: Range,
}

impl StatsView {
    pub fn new() -> Self {
        Self {
            page: Page::Trend,
            range: Range::Days30,
        }
    }

    pub fn next_page(&mut self) {
        self.page = match self.page {
            Page::Trend => Page::Keys,
            Page::Keys => Page::Lessons,
            Page::Lessons => Page::Trend,
        };
    }

    pub fn prev_page(&mut self) {
        self.page = match self.page {
            Page::Trend => Page::Lessons,
            Page::Keys => Page::Trend,
            Page::Lessons => Page::Keys,
        };
    }
}

/// Keys and bigrams need at least this many attempts before they are called weak.
const MIN_ATTEMPTS: u32 = 10;

pub fn draw(frame: &mut Frame, area: Rect, snapshot: &Snapshot, view: &StatsView, course: &Course) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(vec![
            tabs(view.page),
            Line::from(habit_line(&snapshot.habit)),
        ]),
        header,
    );
    match view.page {
        Page::Trend => draw_trend(frame, body, snapshot, view.range),
        Page::Keys => draw_keys(frame, body, snapshot),
        Page::Lessons => draw_lessons(frame, body, snapshot, course),
    }
    frame.render_widget(
        Paragraph::new(Line::styled(
            "Tab: next page    1/2/3: 30 / 90 / all days    Esc: back    q: quit",
            Style::new().fg(Color::DarkGray),
        )),
        footer,
    );
}

fn tabs(current: Page) -> Line<'static> {
    let mut spans = vec![Span::from("Stats   ").bold()];
    for (page, label) in [
        (Page::Trend, "Trend"),
        (Page::Keys, "Keys"),
        (Page::Lessons, "Lessons"),
    ] {
        let style = if page == current {
            Style::new()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(Color::DarkGray)
        };
        spans.push(Span::styled(format!(" {label} "), style));
        spans.push(Span::from("  "));
    }
    Line::from(spans)
}

/// Day numbers covered by a range ending today.
fn bounds(snapshot: &Snapshot, range: Range) -> (i64, i64) {
    let end = stats::day_number(&snapshot.today).unwrap_or(0);
    let first = snapshot
        .days
        .first()
        .and_then(|d| stats::day_number(&d.date))
        .unwrap_or(end);
    let start = match range {
        Range::Days30 => end - 29,
        Range::Days90 => end - 89,
        Range::All => first.min(end),
    };
    (start, end)
}

fn draw_trend(frame: &mut Frame, area: Rect, snapshot: &Snapshot, range: Range) {
    let (start, end) = bounds(snapshot, range);
    let points: Vec<(f64, f64, f64)> = snapshot
        .days
        .iter()
        .filter_map(|day| {
            let number = stats::day_number(&day.date)?;
            (start..=end).contains(&number).then_some((
                (number - start) as f64,
                day.error_rate * 100.0,
                day.cpm,
            ))
        })
        .collect();
    if points.is_empty() {
        frame.render_widget(Paragraph::new("No practice in this range yet."), area);
        return;
    }
    let [errors_area, speed_area] =
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(area);
    let span = (end - start).max(1) as f64;
    let x_labels = vec![stats::date_string(start), stats::date_string(end)];
    let errors: Vec<(f64, f64)> = points.iter().map(|&(x, rate, _)| (x, rate)).collect();
    let max_rate = errors.iter().map(|&(_, y)| y).fold(5.0_f64, f64::max) * 1.1;
    chart(
        frame,
        errors_area,
        "error rate %",
        &errors,
        Color::Red,
        span,
        &x_labels,
        max_rate,
    );
    let speed: Vec<(f64, f64)> = points.iter().map(|&(x, _, cpm)| (x, cpm)).collect();
    let max_speed = speed.iter().map(|&(_, y)| y).fold(100.0_f64, f64::max) * 1.1;
    chart(
        frame,
        speed_area,
        "speed cpm",
        &speed,
        Color::Cyan,
        span,
        &x_labels,
        max_speed,
    );
}

#[allow(clippy::too_many_arguments)]
fn chart(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    points: &[(f64, f64)],
    color: Color,
    span: f64,
    x_labels: &[String],
    y_max: f64,
) {
    let graph_type = if points.len() > 1 {
        GraphType::Line
    } else {
        GraphType::Scatter
    };
    let dataset = Dataset::default()
        .marker(Marker::Braille)
        .graph_type(graph_type)
        .style(Style::new().fg(color))
        .data(points);
    let chart = Chart::new(vec![dataset])
        .block(Block::bordered().title(title.to_string()))
        .x_axis(
            Axis::default()
                .bounds([0.0, span])
                .labels(x_labels.to_vec()),
        )
        .y_axis(
            Axis::default()
                .bounds([0.0, y_max])
                .labels(vec!["0".to_string(), format!("{y_max:.0}")]),
        );
    frame.render_widget(chart, area);
}

fn heat(rate: f64) -> Color {
    if rate <= 0.02 {
        Color::Green
    } else if rate <= 0.05 {
        Color::Yellow
    } else {
        Color::Red
    }
}

/// Attempts and errors of a key and its capital together.
fn key_rate(keys: &[KeyStat], label: &str) -> Option<f64> {
    let upper = label.to_uppercase();
    let (attempts, errors) = keys
        .iter()
        .filter(|k| k.key == label || k.key == upper)
        .fold((0, 0), |(a, e), k| (a + k.attempts, e + k.errors));
    (attempts > 0).then(|| errors as f64 / attempts as f64)
}

fn draw_keys(frame: &mut Frame, area: Rect, snapshot: &Snapshot) {
    let [keyboard_area, _, lists] = Layout::vertical([
        Constraint::Length(keyboard::HEIGHT + 1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .areas(area);
    keyboard::draw_styled(frame, keyboard_area, |label| {
        match key_rate(&snapshot.keys, label) {
            Some(rate) => Style::new().fg(heat(rate)).add_modifier(Modifier::BOLD),
            None => Style::new().fg(Color::DarkGray),
        }
    });
    let legend = Rect {
        y: keyboard_area.y + keyboard::HEIGHT,
        height: 1,
        ..keyboard_area
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("green", Style::new().fg(Color::Green)),
            Span::from(" 2% or less   "),
            Span::styled("yellow", Style::new().fg(Color::Yellow)),
            Span::from(" up to 5%   "),
            Span::styled("red", Style::new().fg(Color::Red)),
            Span::from(" above   "),
            Span::styled("grey", Style::new().fg(Color::DarkGray)),
            Span::from(" not typed yet"),
        ])),
        legend,
    );

    let [keys_area, bigrams_area] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(lists);
    let mut key_lines = vec![Line::from("worst keys".bold())];
    key_lines.extend(
        snapshot
            .keys
            .iter()
            .filter(|k| k.attempts >= MIN_ATTEMPTS)
            .take(8)
            .map(|k| stat_line(&k.key, k.error_rate, k.median_ms, k.attempts)),
    );
    let mut bigram_lines = vec![Line::from("worst bigrams".bold())];
    bigram_lines.extend(
        snapshot
            .bigrams
            .iter()
            .filter(|b| b.attempts >= MIN_ATTEMPTS)
            .take(8)
            .map(|b| stat_line(&b.bigram, b.error_rate, b.median_ms, b.attempts)),
    );
    for lines in [&mut key_lines, &mut bigram_lines] {
        if lines.len() == 1 {
            lines.push(Line::styled(
                format!("not enough data yet ({MIN_ATTEMPTS} attempts each)"),
                Style::new().fg(Color::DarkGray),
            ));
        }
    }
    frame.render_widget(Paragraph::new(Text::from(key_lines)), keys_area);
    frame.render_widget(Paragraph::new(Text::from(bigram_lines)), bigrams_area);
}

fn stat_line(name: &str, rate: f64, median_ms: Option<u64>, attempts: u32) -> Line<'static> {
    let median = median_ms.map_or("    -".to_string(), |ms| format!("{ms:>4} ms"));
    Line::from(vec![
        Span::styled(
            format!("{name:<4}"),
            Style::new().fg(heat(rate)).add_modifier(Modifier::BOLD),
        ),
        Span::from(format!(
            "{:>5.1}%   {median}   {attempts} attempts",
            rate * 100.0
        )),
    ])
}

fn draw_lessons(frame: &mut Frame, area: Rect, snapshot: &Snapshot, course: &Course) {
    let header = Row::new([
        "#",
        "lesson",
        "attempts",
        "tests",
        "best error",
        "best cpm",
        "passed on",
    ])
    .bold();
    let rows = course.lessons().iter().enumerate().map(|(index, lesson)| {
        let stat = snapshot.lessons.iter().find(|s| s.lesson == lesson.id);
        let passed = stat
            .and_then(|s| s.passed_at.as_deref())
            .map(|at| at[..10.min(at.len())].to_string());
        let cells = [
            format!("{:>2}", index + 1),
            lesson.title.clone(),
            stat.map_or(String::new(), |s| s.attempts.to_string()),
            stat.map_or(String::new(), |s| s.tests.to_string()),
            stat.and_then(|s| s.best_error_rate)
                .map_or(String::new(), |r| format!("{:.1}%", r * 100.0)),
            stat.and_then(|s| s.best_cpm)
                .map_or(String::new(), |c| format!("{c:.0}")),
            passed.unwrap_or_default(),
        ];
        let style = if stat.is_some() {
            Style::new()
        } else {
            Style::new().fg(Color::DarkGray)
        };
        Row::new(cells).style(style)
    });
    let widths = [
        Constraint::Length(2),
        Constraint::Length(22),
        Constraint::Length(8),
        Constraint::Length(5),
        Constraint::Length(10),
        Constraint::Length(8),
        Constraint::Length(10),
    ];
    frame.render_widget(
        Table::new(rows, widths).header(header).column_spacing(2),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::{BigramStat, DayStat, LessonStat};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn snapshot() -> Snapshot {
        let days = vec![
            DayStat::new("2026-09-03".into(), 3, 600, 12, 300_000),
            DayStat::new("2026-09-04".into(), 2, 500, 5, 200_000),
        ];
        Snapshot {
            today: "2026-09-04".into(),
            habit: stats::habit(&days, "2026-09-04", 15),
            days,
            lessons: vec![LessonStat {
                lesson: "a01".into(),
                attempts: 5,
                tests: 1,
                best_error_rate: Some(0.012),
                best_cpm: Some(180.0),
                passed_at: Some("2026-09-04T17:00:00Z".into()),
            }],
            keys: vec![KeyStat {
                key: "n".into(),
                attempts: 40,
                errors: 3,
                error_rate: 0.075,
                median_ms: Some(310),
            }],
            bigrams: vec![BigramStat {
                bigram: "en".into(),
                attempts: 20,
                errors: 2,
                error_rate: 0.1,
                median_ms: Some(280),
            }],
        }
    }

    fn render(snapshot: &Snapshot, view: &StatsView) -> String {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        let course = Course::load().unwrap();
        terminal
            .draw(|frame| draw(frame, frame.area(), snapshot, view, &course))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer.cell((x, y)).unwrap().symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn trend_page_shows_both_charts_and_the_habit_line() {
        let text = render(&snapshot(), &StatsView::new());
        assert!(text.contains("error rate %"), "{text}");
        assert!(text.contains("speed cpm"), "{text}");
        assert!(text.contains("2026-08-06"), "range start label: {text}");
        assert!(text.contains("streak 2 days"), "{text}");
    }

    #[test]
    fn empty_snapshot_says_so() {
        let text = render(&Snapshot::empty("2026-09-04"), &StatsView::new());
        assert!(text.contains("No practice in this range yet."), "{text}");
    }

    #[test]
    fn keys_page_lists_weak_keys_and_bigrams() {
        let view = StatsView {
            page: Page::Keys,
            range: Range::All,
        };
        let text = render(&snapshot(), &view);
        assert!(text.contains("worst keys"), "{text}");
        assert!(
            text.contains("n     7.5%    310 ms   40 attempts"),
            "{text}"
        );
        assert!(
            text.contains("en   10.0%    280 ms   20 attempts"),
            "{text}"
        );
        assert!(text.contains(" u  i  a  e  o "), "{text}");
    }

    #[test]
    fn lessons_page_is_a_table() {
        let view = StatsView {
            page: Page::Lessons,
            range: Range::All,
        };
        let text = render(&snapshot(), &view);
        assert!(text.contains("passed on"), "{text}");
        assert!(text.contains("1.2%"), "{text}");
        assert!(text.contains("2026-09-04"), "{text}");
        assert!(
            text.contains("a and r"),
            "unstarted lessons are listed too: {text}"
        );
    }

    #[test]
    fn pages_cycle_both_ways() {
        let mut view = StatsView::new();
        view.next_page();
        assert_eq!(view.page, Page::Keys);
        view.prev_page();
        view.prev_page();
        assert_eq!(view.page, Page::Lessons);
    }
}
