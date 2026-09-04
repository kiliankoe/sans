//! The files list: what you have typed over, and where each one resumes.

use std::path::Path;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};

use crate::store::FileProgress;

pub fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

pub fn draw(frame: &mut Frame, area: Rect, files: &[FileProgress], selected: usize) {
    let [title_area, list_area, help_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("Files".bold()),
            Line::styled(
                "Type over your own files, source code first of all. Add one with `neotype type path/to/file`.",
                Style::new().fg(Color::DarkGray),
            ),
        ]),
        title_area,
    );
    if files.is_empty() {
        frame.render_widget(Paragraph::new("No files yet."), list_area);
    } else {
        let items: Vec<ListItem> = files
            .iter()
            .map(|file| {
                let position = if file.next_chunk >= file.chunks {
                    Span::styled("done".to_string(), Style::new().fg(Color::Green))
                } else {
                    Span::from(format!("chunk {} of {}", file.next_chunk + 1, file.chunks))
                };
                ListItem::new(Line::from(vec![
                    Span::from(format!("{:<28}", file_name(&file.path))),
                    Span::from(format!("{:<16}", position.content)).style(position.style),
                    Span::styled(
                        format!(
                            "{}  {}",
                            &file.updated_at[..10.min(file.updated_at.len())],
                            file.path
                        ),
                        Style::new().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect();
        let list = List::new(items)
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED))
            .highlight_symbol("› ");
        let mut state = ListState::default().with_selected(Some(selected));
        frame.render_stateful_widget(list, list_area, &mut state);
    }
    frame.render_widget(
        Paragraph::new(Line::styled(
            "Enter: continue    j/k: move    Esc: back    q: quit",
            Style::new().fg(Color::DarkGray),
        )),
        help_area,
    );
}
