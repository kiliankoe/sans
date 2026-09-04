//! Application state: which screen is showing, the engine and clock behind the typing
//! screen, and the translation of terminal events into engine input.

use std::time::{Duration, Instant, SystemTime};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Frame;

use super::{results, typing};
use crate::clock::Clock;
use crate::engine::{Engine, Key, Keystroke, Outcome};
use crate::stats::{self, Summary};
use crate::store::SessionMeta;

/// Error rate at or below which a stage counts as passed.
pub const PASS_ERROR_RATE: f64 = 0.03;

const FLASH: Duration = Duration::from_millis(700);

/// Something to type. Phase 2 replaces the hard-coded one with course stages.
#[derive(Debug, Clone)]
pub struct Drill {
    pub lesson: String,
    pub stage: u32,
    pub title: String,
    pub text: String,
}

pub enum Screen {
    Typing,
    Results { summary: Summary, finished: bool },
}

/// A completed or abandoned stage, handed to the store by the event loop.
pub struct SessionEnd {
    pub lesson: String,
    pub stage: u32,
    pub started_at: SystemTime,
    pub finished: bool,
    pub summary: Summary,
    pub log: Vec<Keystroke>,
}

impl SessionEnd {
    pub fn meta(&self) -> SessionMeta<'_> {
        SessionMeta {
            kind: "lesson",
            lesson: Some(&self.lesson),
            stage: Some(self.stage),
            seed: None,
            started_at: self.started_at,
            finished: self.finished,
        }
    }
}

/// Live numbers for the status line.
pub struct Status {
    pub cpm: f64,
    pub errors: usize,
    pub error_rate: f64,
    pub done: usize,
    pub total: usize,
    pub active: Duration,
    pub paused: bool,
    pub flash: Option<String>,
}

pub struct App {
    drill: Drill,
    engine: Engine,
    clock: Clock,
    started_at: Option<SystemTime>,
    screen: Screen,
    flash: Option<(String, Instant)>,
    quit: bool,
}

impl App {
    pub fn new(drill: Drill) -> Self {
        let engine = Engine::new(&drill.text);
        Self {
            drill,
            engine,
            clock: Clock::new(),
            started_at: None,
            screen: Screen::Typing,
            flash: None,
            quit: false,
        }
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    #[cfg(test)]
    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    #[cfg(test)]
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    #[cfg(test)]
    pub fn is_paused(&self) -> bool {
        self.clock.is_paused()
    }

    fn restart(&mut self) {
        self.engine = Engine::new(&self.drill.text);
        self.clock = Clock::new();
        self.started_at = None;
        self.flash = None;
        self.screen = Screen::Typing;
    }

    /// Handles one terminal event. Returns a session to store when a stage ends.
    pub fn handle(&mut self, event: Event, now: Instant) -> Option<SessionEnd> {
        match event {
            Event::FocusLost => {
                self.clock.pause(now);
                None
            }
            Event::FocusGained => {
                self.clock.resume(now);
                None
            }
            Event::Paste(_) => {
                self.flash("pasting is not typing", now);
                None
            }
            Event::Key(key) if key.kind == KeyEventKind::Press => self.handle_key(key, now),
            _ => None,
        }
    }

    fn handle_key(&mut self, key: KeyEvent, now: Instant) -> Option<SessionEnd> {
        if is_ctrl_c(&key) {
            self.quit = true;
            return match self.screen {
                Screen::Typing => self.finish(false),
                Screen::Results { .. } => None,
            };
        }
        match self.screen {
            Screen::Typing => self.handle_typing_key(key, now),
            Screen::Results { .. } => {
                match key.code {
                    KeyCode::Enter | KeyCode::Char('r') => self.restart(),
                    KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
                    _ => {}
                }
                None
            }
        }
    }

    fn handle_typing_key(&mut self, key: KeyEvent, now: Instant) -> Option<SessionEnd> {
        if key.code == KeyCode::Esc {
            if self.engine.log().is_empty() {
                self.quit = true;
                return None;
            }
            return self.finish(false);
        }
        let input = key_to_input(&key)?;
        if !self.clock.started() {
            self.clock.start(now);
            self.started_at = Some(SystemTime::now());
        }
        let at = self.clock.at(now);
        match self.engine.input(input, at) {
            Outcome::Refused => self.flash("fix the error first (Backspace)", now),
            Outcome::Finished => return self.finish(true),
            _ => {}
        }
        None
    }

    /// Moves to the results screen. Returns nothing to store when nothing was typed.
    fn finish(&mut self, finished: bool) -> Option<SessionEnd> {
        let log = self.engine.log().to_vec();
        let summary = stats::summarize(&log);
        self.screen = Screen::Results {
            summary: summary.clone(),
            finished,
        };
        if log.is_empty() {
            return None;
        }
        Some(SessionEnd {
            lesson: self.drill.lesson.clone(),
            stage: self.drill.stage,
            started_at: self.started_at.unwrap_or_else(SystemTime::now),
            finished,
            summary,
            log,
        })
    }

    fn flash(&mut self, message: &str, now: Instant) {
        self.flash = Some((message.to_string(), now));
    }

    fn status(&self, now: Instant) -> Status {
        let log = self.engine.log();
        let summary = stats::summarize(log);
        let active = stats::active_time_until(log, self.clock.at(now));
        Status {
            cpm: stats::rate_per_minute(summary.chars, active),
            errors: summary.errors,
            error_rate: summary.error_rate,
            done: self.engine.cursor(),
            total: self.engine.target().len(),
            active,
            paused: self.clock.is_paused(),
            flash: self
                .flash
                .as_ref()
                .filter(|(_, since)| now.duration_since(*since) < FLASH)
                .map(|(message, _)| message.clone()),
        }
    }

    pub fn render(&self, frame: &mut Frame, now: Instant) {
        let area = frame.area();
        match &self.screen {
            Screen::Typing => {
                typing::draw(
                    frame,
                    area,
                    &self.drill.title,
                    &self.engine,
                    &self.status(now),
                );
            }
            Screen::Results { summary, finished } => {
                results::draw(
                    frame,
                    area,
                    &self.drill.title,
                    summary,
                    *finished,
                    PASS_ERROR_RATE,
                );
            }
        }
    }
}

/// Terminal key to engine input. Shift is part of a character; Alt, Control and Super are
/// never typing (Karabiner's shortcut shield sends Shift+Alt+PageUp before every layer 3
/// symbol, and that must not reach the engine).
pub fn key_to_input(key: &KeyEvent) -> Option<Key> {
    if key.kind != KeyEventKind::Press
        || key
            .modifiers
            .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL | KeyModifiers::SUPER)
    {
        return None;
    }
    match key.code {
        KeyCode::Char(c) => Some(Key::Char(c.to_string())),
        KeyCode::Backspace => Some(Key::Backspace),
        KeyCode::Enter => Some(Key::Enter),
        KeyCode::Tab => Some(Key::Tab),
        _ => None,
    }
}

fn is_ctrl_c(key: &KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color;

    fn drill(text: &str) -> Drill {
        Drill {
            lesson: "a01".into(),
            stage: 1,
            title: "Lesson 1: e and n".into(),
            text: text.into(),
        }
    }

    fn press(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new(code, modifiers))
    }

    fn ch(c: char) -> Event {
        press(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn key_mapping_keeps_shifted_characters_and_drops_modified_keys() {
        let key = |code, mods| KeyEvent::new(code, mods);
        assert_eq!(
            key_to_input(&key(KeyCode::Char('E'), KeyModifiers::SHIFT)),
            Some(Key::Char("E".into()))
        );
        assert_eq!(
            key_to_input(&key(KeyCode::Char('{'), KeyModifiers::NONE)),
            Some(Key::Char("{".into()))
        );
        assert_eq!(
            key_to_input(&key(KeyCode::Backspace, KeyModifiers::NONE)),
            Some(Key::Backspace)
        );
        assert_eq!(
            key_to_input(&key(KeyCode::Enter, KeyModifiers::NONE)),
            Some(Key::Enter)
        );
        assert_eq!(
            key_to_input(&key(KeyCode::Tab, KeyModifiers::NONE)),
            Some(Key::Tab)
        );
        assert_eq!(
            key_to_input(&key(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(
            key_to_input(&key(KeyCode::Char('a'), KeyModifiers::ALT)),
            None
        );
        assert_eq!(
            key_to_input(&key(
                KeyCode::PageUp,
                KeyModifiers::SHIFT | KeyModifiers::ALT
            )),
            None
        );
        assert_eq!(key_to_input(&key(KeyCode::Left, KeyModifiers::NONE)), None);
    }

    #[test]
    fn typing_the_whole_drill_ends_the_session_and_shows_results() {
        let mut app = App::new(drill("en"));
        let t0 = Instant::now();
        assert!(app.handle(ch('e'), t0).is_none());
        let end = app.handle(ch('n'), t0 + ms(500)).expect("session ends");
        assert!(end.finished);
        assert_eq!(end.summary.chars, 2);
        assert_eq!(end.summary.active, ms(500));
        assert_eq!(end.log.len(), 2);
        assert_eq!(end.meta().lesson, Some("a01"));
        assert!(matches!(
            app.screen(),
            Screen::Results { finished: true, .. }
        ));
        assert!(!app.should_quit());
    }

    #[test]
    fn escape_after_typing_aborts_and_stores_an_unfinished_session() {
        let mut app = App::new(drill("en"));
        let t0 = Instant::now();
        app.handle(ch('e'), t0);
        let end = app
            .handle(press(KeyCode::Esc, KeyModifiers::NONE), t0 + ms(100))
            .expect("stored");
        assert!(!end.finished);
        assert!(matches!(
            app.screen(),
            Screen::Results {
                finished: false,
                ..
            }
        ));
    }

    #[test]
    fn escape_before_typing_just_quits() {
        let mut app = App::new(drill("en"));
        assert!(
            app.handle(press(KeyCode::Esc, KeyModifiers::NONE), Instant::now())
                .is_none()
        );
        assert!(app.should_quit());
    }

    #[test]
    fn stray_keys_from_the_shortcut_shield_are_ignored() {
        let mut app = App::new(drill("{"));
        let t0 = Instant::now();
        app.handle(
            press(KeyCode::PageUp, KeyModifiers::SHIFT | KeyModifiers::ALT),
            t0,
        );
        assert!(app.engine().log().is_empty());
        assert!(app.handle(ch('{'), t0 + ms(10)).is_some());
    }

    #[test]
    fn focus_events_pause_and_resume_the_clock() {
        let mut app = App::new(drill("en"));
        let t0 = Instant::now();
        app.handle(ch('e'), t0);
        app.handle(Event::FocusLost, t0 + ms(100));
        assert!(app.is_paused());
        app.handle(Event::FocusGained, t0 + ms(10_100));
        assert!(!app.is_paused());
        let end = app.handle(ch('n'), t0 + ms(10_200)).unwrap();
        assert_eq!(
            end.summary.active,
            ms(200),
            "the ten seconds away do not count"
        );
    }

    #[test]
    fn refused_key_sets_a_flash_message() {
        let mut app = App::new(drill("en"));
        let t0 = Instant::now();
        app.handle(ch('n'), t0);
        app.handle(ch('e'), t0 + ms(50));
        assert_eq!(
            app.status(t0 + ms(100)).flash.as_deref(),
            Some("fix the error first (Backspace)")
        );
        assert_eq!(app.status(t0 + ms(2000)).flash, None, "flash fades");
    }

    #[test]
    fn enter_on_results_restarts_the_drill_and_q_quits() {
        let mut app = App::new(drill("e"));
        let t0 = Instant::now();
        app.handle(ch('e'), t0);
        app.handle(press(KeyCode::Enter, KeyModifiers::NONE), t0 + ms(10));
        assert!(matches!(app.screen(), Screen::Typing));
        assert_eq!(app.engine().cursor(), 0);
        app.handle(ch('e'), t0 + ms(20));
        app.handle(ch('q'), t0 + ms(30));
        assert!(app.should_quit());
    }

    #[test]
    fn ctrl_c_while_typing_stores_the_partial_session_and_quits() {
        let mut app = App::new(drill("en"));
        let t0 = Instant::now();
        app.handle(ch('e'), t0);
        let end = app.handle(
            press(KeyCode::Char('c'), KeyModifiers::CONTROL),
            t0 + ms(10),
        );
        assert!(end.is_some_and(|end| !end.finished));
        assert!(app.should_quit());
    }

    fn render(app: &App) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
        terminal
            .draw(|frame| app.render(frame, Instant::now()))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn row(buffer: &ratatui::buffer::Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer.cell((x, y)).unwrap().symbol())
            .collect::<String>()
    }

    fn find(buffer: &ratatui::buffer::Buffer, needle: &str) -> Option<(u16, u16)> {
        (0..buffer.area.height).find_map(|y| row(buffer, y).find(needle).map(|x| (x as u16, y)))
    }

    #[test]
    fn typing_screen_shows_title_text_and_marks_the_wrong_character() {
        let mut app = App::new(drill("ene"));
        let t0 = Instant::now();
        app.handle(ch('e'), t0);
        app.handle(ch('x'), t0 + ms(10));
        let buffer = render(&app);
        assert!(find(&buffer, "Lesson 1: e and n").is_some());
        let (x, y) = find(&buffer, "exe").expect("typed char, wrong char, pending char");
        assert_eq!(buffer.cell((x + 1, y)).unwrap().bg, Color::Red);
        assert!(find(&buffer, "1 error").is_some(), "{}", row(&buffer, 11));
    }

    #[test]
    fn results_screen_shows_the_numbers() {
        let mut app = App::new(drill("e"));
        app.handle(ch('e'), Instant::now());
        let buffer = render(&app);
        assert!(find(&buffer, "passed").is_some());
        assert!(find(&buffer, "error rate").is_some());
        assert!(find(&buffer, "cpm").is_some());
    }
}
