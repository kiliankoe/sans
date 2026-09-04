//! Application state: which screen is showing, the stage behind the typing screen, and the
//! translation of terminal events into engine input.

use std::collections::HashSet;
use std::time::{Duration, Instant, SystemTime};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Frame;

use super::typing::HintPane;
use super::{home, results, typing};
use crate::clock::Clock;
use crate::course::{Course, PASS_ERROR_RATE, Progress, StageKind};
use crate::engine::{Engine, Key, Keystroke, Outcome};
use crate::layout;
use crate::stats::{self, Summary};
use crate::store::SessionMeta;
use crate::text::{self, Corpus, StageSpec};

const FLASH: Duration = Duration::from_millis(900);

/// A stage in progress or just finished.
pub struct Active {
    pub lesson: usize,
    pub stage: usize,
    pub kind: StageKind,
    pub seed: u64,
    pub engine: Engine,
    clock: Clock,
    started_at: Option<SystemTime>,
}

pub enum Screen {
    Home {
        selected: usize,
    },
    Typing(Box<Active>),
    Results {
        active: Box<Active>,
        summary: Summary,
        finished: bool,
    },
}

/// A completed or abandoned stage, handed to the store by the event loop.
pub struct SessionEnd {
    pub lesson: String,
    pub stage: u32,
    pub stage_kind: &'static str,
    pub seed: u64,
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
            stage_kind: Some(self.stage_kind),
            seed: Some(self.seed),
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
    course: Course,
    corpus: Corpus,
    progress: Progress,
    screen: Screen,
    flash: Option<(String, Instant)>,
    quit: bool,
}

impl App {
    pub fn new(course: Course, corpus: Corpus, progress: Progress) -> Self {
        let selected = progress.next_index(&course);
        Self {
            course,
            corpus,
            progress,
            screen: Screen::Home { selected },
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
    pub fn active(&self) -> Option<&Active> {
        match &self.screen {
            Screen::Typing(active) | Screen::Results { active, .. } => Some(active),
            Screen::Home { .. } => None,
        }
    }

    #[cfg(test)]
    pub fn progress(&self) -> &Progress {
        &self.progress
    }

    fn start_stage(&mut self, lesson: usize, stage: usize) {
        let kinds = self.course.stages(lesson);
        let kind = kinds[stage.min(kinds.len() - 1)];
        let seed: u64 = rand::random();
        let unlocked = self.course.unlocked_through(lesson);
        let spec = StageSpec {
            kind,
            new: &self.course.lessons()[lesson].new,
            unlocked: &unlocked,
            seed,
        };
        let text = text::generate(&spec, &self.corpus);
        self.screen = Screen::Typing(Box::new(Active {
            lesson,
            stage,
            kind,
            seed,
            engine: Engine::new(&text),
            clock: Clock::new(),
            started_at: None,
        }));
    }

    fn title(&self, active: &Active) -> String {
        let lesson = &self.course.lessons()[active.lesson];
        format!(
            "Lesson {} of {}: {}   {} ({}/{})",
            active.lesson + 1,
            self.course.lessons().len(),
            lesson.title,
            active.kind.name(),
            active.stage + 1,
            self.course.stages(active.lesson).len()
        )
    }

    /// Keyboard and finger hints, shown on intro stages only.
    fn hint_pane(&self, active: &Active) -> Option<HintPane> {
        if active.kind != StageKind::Intro {
            return None;
        }
        let new = &self.course.lessons()[active.lesson].new;
        let lines = if new.iter().all(|key| key.chars().all(char::is_uppercase)) {
            vec!["Capitals: hold Shift with the hand that is not typing the letter".to_string()]
        } else {
            new.iter().filter_map(|key| layout::hint(key)).collect()
        };
        Some(HintPane {
            highlight: new.iter().cloned().collect::<HashSet<_>>(),
            unlocked: self
                .course
                .unlocked_through(active.lesson)
                .into_iter()
                .collect(),
            lines,
        })
    }

    /// Handles one terminal event. Returns a session to store when a stage ends.
    pub fn handle(&mut self, event: Event, now: Instant) -> Option<SessionEnd> {
        match event {
            Event::FocusLost => {
                if let Screen::Typing(active) = &mut self.screen {
                    active.clock.pause(now);
                }
                None
            }
            Event::FocusGained => {
                if let Screen::Typing(active) = &mut self.screen {
                    active.clock.resume(now);
                }
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
                Screen::Typing(_) => self.finish(false),
                _ => None,
            };
        }
        match &self.screen {
            Screen::Home { .. } => {
                self.handle_home_key(key, now);
                None
            }
            Screen::Typing(_) => self.handle_typing_key(key, now),
            Screen::Results { .. } => {
                self.handle_results_key(key);
                None
            }
        }
    }

    fn handle_home_key(&mut self, key: KeyEvent, now: Instant) {
        let Screen::Home { selected } = &mut self.screen else {
            return;
        };
        let last = self.course.lessons().len() - 1;
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => *selected = (*selected + 1).min(last),
            KeyCode::Char('k') | KeyCode::Up => *selected = selected.saturating_sub(1),
            KeyCode::Enter => {
                let selected = *selected;
                if self.progress.available(&self.course, selected) {
                    self.start_stage(selected, 0);
                } else {
                    self.flash("locked: pass the lesson before it first", now);
                }
            }
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            _ => {}
        }
    }

    fn handle_typing_key(&mut self, key: KeyEvent, now: Instant) -> Option<SessionEnd> {
        let Screen::Typing(active) = &mut self.screen else {
            return None;
        };
        if key.code == KeyCode::Esc {
            if active.engine.log().is_empty() {
                self.screen = Screen::Home {
                    selected: active.lesson,
                };
                return None;
            }
            return self.finish(false);
        }
        let input = key_to_input(&key)?;
        if !active.clock.started() {
            active.clock.start(now);
            active.started_at = Some(SystemTime::now());
        }
        let at = active.clock.at(now);
        match active.engine.input(input, at) {
            Outcome::Refused => self.flash("fix the error first (Backspace)", now),
            Outcome::Finished => return self.finish(true),
            _ => {}
        }
        None
    }

    fn handle_results_key(&mut self, key: KeyEvent) {
        let Screen::Results {
            active,
            summary,
            finished,
        } = &self.screen
        else {
            return;
        };
        let (lesson, stage, kind) = (active.lesson, active.stage, active.kind);
        let passed = *finished && summary.error_rate <= PASS_ERROR_RATE;
        match key.code {
            KeyCode::Enter => match (*finished, kind) {
                (true, StageKind::Test) if passed => {
                    if lesson + 1 < self.course.lessons().len() {
                        self.start_stage(lesson + 1, 0);
                    } else {
                        self.screen = Screen::Home { selected: lesson };
                    }
                }
                (true, StageKind::Test) => self.start_stage(lesson, stage),
                (true, _) => self.start_stage(lesson, stage + 1),
                (false, _) => self.start_stage(lesson, stage),
            },
            KeyCode::Char('r') => self.start_stage(lesson, stage),
            KeyCode::Esc | KeyCode::Char('h') => self.screen = Screen::Home { selected: lesson },
            KeyCode::Char('q') => self.quit = true,
            _ => {}
        }
    }

    /// Moves to the results screen. Returns nothing to store when nothing was typed.
    fn finish(&mut self, finished: bool) -> Option<SessionEnd> {
        let Screen::Typing(active) =
            std::mem::replace(&mut self.screen, Screen::Home { selected: 0 })
        else {
            return None;
        };
        let log = active.engine.log().to_vec();
        let summary = stats::summarize(&log);
        let lesson_id = self.course.lessons()[active.lesson].id.clone();
        if finished && active.kind == StageKind::Test {
            self.progress.record(&lesson_id, summary.error_rate);
        }
        let end = (!log.is_empty()).then(|| SessionEnd {
            lesson: lesson_id,
            stage: active.stage as u32 + 1,
            stage_kind: active.kind.name(),
            seed: active.seed,
            started_at: active.started_at.unwrap_or_else(SystemTime::now),
            finished,
            summary: summary.clone(),
            log,
        });
        self.screen = Screen::Results {
            active,
            summary,
            finished,
        };
        end
    }

    fn flash(&mut self, message: &str, now: Instant) {
        self.flash = Some((message.to_string(), now));
    }

    fn flash_text(&self, now: Instant) -> Option<String> {
        self.flash
            .as_ref()
            .filter(|(_, since)| now.duration_since(*since) < FLASH)
            .map(|(message, _)| message.clone())
    }

    fn status(&self, active: &Active, now: Instant) -> Status {
        let log = active.engine.log();
        let summary = stats::summarize(log);
        let elapsed = stats::active_time_until(log, active.clock.at(now));
        Status {
            cpm: stats::rate_per_minute(summary.chars, elapsed),
            errors: summary.errors,
            error_rate: summary.error_rate,
            done: active.engine.cursor(),
            total: active.engine.target().len(),
            active: elapsed,
            paused: active.clock.is_paused(),
            flash: self.flash_text(now),
        }
    }

    pub fn render(&self, frame: &mut Frame, now: Instant) {
        let area = frame.area();
        match &self.screen {
            Screen::Home { selected } => {
                home::draw(frame, area, &self.course, &self.progress, *selected);
                if let Some(message) = self.flash_text(now) {
                    typing::draw_flash(frame, area, &message);
                }
            }
            Screen::Typing(active) => {
                let hints = self.hint_pane(active);
                typing::draw(
                    frame,
                    area,
                    &self.title(active),
                    &active.engine,
                    &self.status(active, now),
                    hints.as_ref(),
                );
            }
            Screen::Results {
                active,
                summary,
                finished,
            } => {
                let passed = *finished && summary.error_rate <= PASS_ERROR_RATE;
                let next = match (*finished, active.kind) {
                    (true, StageKind::Test) if passed => "next lesson",
                    (true, StageKind::Test) => "try the test again",
                    (true, _) => "next stage",
                    (false, _) => "repeat",
                };
                results::draw(
                    frame,
                    area,
                    &self.title(active),
                    summary,
                    *finished,
                    active.kind == StageKind::Test,
                    next,
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
    use ratatui::buffer::Buffer;
    use ratatui::style::Color;

    fn app() -> App {
        App::new(Course::load().unwrap(), Corpus::load(), Progress::new())
    }

    fn press(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(KeyEvent::new(code, modifiers))
    }

    fn ch(c: char) -> Event {
        press(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn enter() -> Event {
        press(KeyCode::Enter, KeyModifiers::NONE)
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Types the whole current stage, `wrong_every` characters preceded by a mistake.
    fn type_stage(app: &mut App, t0: Instant, wrong_every: Option<usize>) -> Option<SessionEnd> {
        let target = app.active().expect("a stage").engine.target().to_vec();
        let mut end = None;
        for (index, grapheme) in target.iter().enumerate() {
            let at = t0 + ms(200 * index as u64);
            if wrong_every.is_some_and(|n| index % n == 0) {
                let wrong = if grapheme == "x" { 'y' } else { 'x' };
                app.handle(ch(wrong), at);
                app.handle(press(KeyCode::Backspace, KeyModifiers::NONE), at + ms(50));
            }
            let event = match grapheme.as_str() {
                "\n" => enter(),
                g => ch(g.chars().next().unwrap()),
            };
            end = app.handle(event, at + ms(100));
        }
        end
    }

    fn stage_of(app: &App) -> (usize, usize, StageKind) {
        let active = app.active().expect("a stage");
        (active.lesson, active.stage, active.kind)
    }

    #[test]
    fn key_mapping_keeps_shifted_characters_and_drops_modified_keys() {
        let key = |code, mods| KeyEvent::new(code, mods);
        assert_eq!(
            key_to_input(&key(KeyCode::Char('E'), KeyModifiers::SHIFT)),
            Some(Key::Char("E".into()))
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
    fn starts_on_the_home_screen_at_the_next_lesson() {
        let app = app();
        assert!(matches!(app.screen(), Screen::Home { selected: 0 }));
        let mut progress = Progress::new();
        progress.record("a01", 0.01);
        let app = App::new(Course::load().unwrap(), Corpus::load(), progress);
        assert!(matches!(app.screen(), Screen::Home { selected: 1 }));
    }

    #[test]
    fn home_navigation_is_bounded_and_locked_lessons_do_not_start() {
        let mut app = app();
        let t0 = Instant::now();
        app.handle(press(KeyCode::Up, KeyModifiers::NONE), t0);
        assert!(matches!(app.screen(), Screen::Home { selected: 0 }));
        app.handle(ch('j'), t0);
        assert!(matches!(app.screen(), Screen::Home { selected: 1 }));
        app.handle(enter(), t0);
        assert!(
            matches!(app.screen(), Screen::Home { selected: 1 }),
            "lesson 2 is locked"
        );
        assert!(app.flash_text(t0).is_some());
        app.handle(ch('k'), t0);
        app.handle(enter(), t0);
        assert_eq!(stage_of(&app), (0, 0, StageKind::Intro));
    }

    #[test]
    fn intro_stage_has_hints_and_only_the_lesson_keys() {
        let mut app = app();
        app.handle(enter(), Instant::now());
        let active = app.active().unwrap();
        let hints = app.hint_pane(active).expect("intro shows hints");
        assert_eq!(
            hints.lines,
            [
                "e  left index finger, home row",
                "n  right index finger, home row"
            ]
        );
        assert!(hints.highlight.contains("e"));
        for grapheme in active.engine.target() {
            assert!(["e", "n", " "].contains(&grapheme.as_str()), "{grapheme:?}");
        }
    }

    #[test]
    fn stages_advance_and_a_passed_test_unlocks_the_next_lesson() {
        let mut app = app();
        let t0 = Instant::now();
        app.handle(enter(), t0);
        for expected in [StageKind::Intro, StageKind::Bigrams, StageKind::Words] {
            assert_eq!(stage_of(&app).2, expected);
            let end = type_stage(&mut app, t0, None).expect("stored");
            assert!(end.finished);
            assert_eq!(end.stage_kind, expected.name());
            assert!(matches!(
                app.screen(),
                Screen::Results { finished: true, .. }
            ));
            app.handle(enter(), t0);
        }
        assert_eq!(stage_of(&app), (0, 3, StageKind::Test));
        assert!(
            app.hint_pane(app.active().unwrap()).is_none(),
            "no hints after the intro"
        );
        let end = type_stage(&mut app, t0, None).unwrap();
        assert_eq!(end.summary.errors, 0);
        assert!(app.progress().passed("a01"));
        app.handle(enter(), t0);
        assert_eq!(
            stage_of(&app),
            (1, 0, StageKind::Intro),
            "next lesson starts"
        );
    }

    #[test]
    fn a_failed_test_is_repeated_with_fresh_text() {
        let mut app = app();
        let t0 = Instant::now();
        app.handle(enter(), t0);
        app.handle(press(KeyCode::Esc, KeyModifiers::NONE), t0);
        assert!(
            matches!(app.screen(), Screen::Home { .. }),
            "Esc before typing goes home"
        );
        app.handle(enter(), t0);
        for _ in 0..3 {
            type_stage(&mut app, t0, None);
            app.handle(enter(), t0);
        }
        assert_eq!(stage_of(&app).2, StageKind::Test);
        let first_text = app.active().unwrap().engine.target().to_vec();
        let end = type_stage(&mut app, t0, Some(10)).unwrap();
        assert!(end.summary.error_rate > PASS_ERROR_RATE);
        assert!(!app.progress().passed("a01"));
        assert_eq!(app.progress().best("a01"), Some(end.summary.error_rate));
        app.handle(enter(), t0);
        assert_eq!(stage_of(&app), (0, 3, StageKind::Test));
        assert_ne!(app.active().unwrap().engine.target(), first_text.as_slice());
    }

    #[test]
    fn escape_after_typing_aborts_and_the_results_screen_can_go_home() {
        let mut app = app();
        let t0 = Instant::now();
        app.handle(enter(), t0);
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
        app.handle(press(KeyCode::Esc, KeyModifiers::NONE), t0);
        assert!(matches!(app.screen(), Screen::Home { selected: 0 }));
    }

    #[test]
    fn focus_events_pause_the_stage_clock() {
        let mut app = app();
        let t0 = Instant::now();
        app.handle(enter(), t0);
        app.handle(ch('e'), t0);
        app.handle(Event::FocusLost, t0 + ms(100));
        app.handle(Event::FocusGained, t0 + ms(10_100));
        let active = app.active().unwrap();
        assert_eq!(active.clock.at(t0 + ms(10_200)), ms(200));
    }

    #[test]
    fn ctrl_c_while_typing_stores_the_partial_session_and_quits() {
        let mut app = app();
        let t0 = Instant::now();
        app.handle(enter(), t0);
        app.handle(ch('e'), t0);
        let end = app.handle(
            press(KeyCode::Char('c'), KeyModifiers::CONTROL),
            t0 + ms(10),
        );
        assert!(end.is_some_and(|end| !end.finished));
        assert!(app.should_quit());
    }

    fn render(app: &App) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(90, 24)).unwrap();
        terminal
            .draw(|frame| app.render(frame, Instant::now()))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    fn row(buffer: &Buffer, y: u16) -> String {
        (0..buffer.area.width)
            .map(|x| buffer.cell((x, y)).unwrap().symbol())
            .collect()
    }

    fn find(buffer: &Buffer, needle: &str) -> Option<(u16, u16)> {
        (0..buffer.area.height).find_map(|y| row(buffer, y).find(needle).map(|x| (x as u16, y)))
    }

    #[test]
    fn home_screen_lists_lessons_with_status() {
        let buffer = render(&app());
        assert!(find(&buffer, "e and n").is_some());
        assert!(find(&buffer, "next").is_some());
        assert!(find(&buffer, "locked").is_some());
        assert!(find(&buffer, "0 of 29 lessons passed").is_some());
    }

    #[test]
    fn intro_screen_shows_keyboard_hints_text_and_marks_a_wrong_character() {
        let mut app = app();
        let t0 = Instant::now();
        app.handle(enter(), t0);
        app.handle(ch('e'), t0);
        app.handle(ch('x'), t0 + ms(10));
        let buffer = render(&app);
        assert!(find(&buffer, "Lesson 1 of 29: e and n").is_some());
        assert!(find(&buffer, "left index finger, home row").is_some());
        assert!(
            find(&buffer, " u  i  a  e  o ").is_some(),
            "keyboard home row"
        );
        let (x, y) = find(&buffer, "exe").expect("typed e then wrong x");
        assert_eq!(buffer.cell((x + 1, y)).unwrap().bg, Color::Red);
        assert!(find(&buffer, "1 error").is_some());
    }

    #[test]
    fn results_screen_shows_verdict_and_next_action() {
        let mut app = app();
        let t0 = Instant::now();
        app.handle(enter(), t0);
        type_stage(&mut app, t0, None);
        let buffer = render(&app);
        assert!(find(&buffer, "Stage complete").is_some());
        assert!(find(&buffer, "Enter: next stage").is_some());
        assert!(find(&buffer, "error rate").is_some());
    }
}
