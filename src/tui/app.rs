//! Application state: which screen is showing, the stage or chunk behind the typing screen,
//! and the translation of terminal events into engine input.

use std::collections::HashSet;
use std::time::{Duration, Instant, SystemTime};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::Frame;

use super::stats as stats_screen;
use super::stats::{Range, StatsView};
use super::typing::HintPane;
use super::{files as files_screen, home, results, typing};
use crate::clock::Clock;
use crate::course::{Course, PASS_ERROR_RATE, Progress, StageKind};
use crate::engine::{Engine, Key, Keystroke, Outcome};
use crate::files::FileSession;
use crate::layout::{self, Layout};
use crate::stats::{self, Snapshot, Summary};
use crate::store::{FileProgress, SessionMeta};
use crate::text::file::{self as file_text, Indent};

/// Settings the app needs from the config file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    pub indent: Indent,
    pub layout: Layout,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            indent: Indent::Skip,
            layout: Layout::Bone,
        }
    }
}
use crate::text::{self, Corpus, StageSpec, Weakness};

const FLASH: Duration = Duration::from_millis(1500);
/// How long a first Esc stays armed, waiting for the second one that aborts.
const ABORT_CONFIRM: Duration = Duration::from_secs(2);
/// Lesson text wraps at this width, code gets more room.
const LESSON_WIDTH: u16 = 60;
const FILE_WIDTH: u16 = 100;
const PRACTICE_WIDTH: u16 = 80;
const RECENT_FILES: usize = 20;

/// What a typing session is about.
pub enum Source {
    Lesson {
        lesson: usize,
        stage: usize,
        kind: StageKind,
        seed: u64,
    },
    File {
        session: FileSession,
        chunk: usize,
    },
    /// A round over every unlocked key (or `restrict`), favouring the weakest.
    Practice {
        seed: u64,
        restrict: Option<Vec<String>>,
    },
}

/// A stage or chunk in progress or just finished.
pub struct Active {
    pub source: Source,
    pub engine: Engine,
    clock: Clock,
    started_at: Option<SystemTime>,
    /// When Esc was pressed without a second press following it yet.
    abort_asked_at: Option<Instant>,
}

pub enum Screen {
    Home {
        selected: usize,
    },
    Files {
        selected: usize,
    },
    Stats(StatsView),
    Typing(Box<Active>),
    Results {
        active: Box<Active>,
        summary: Summary,
        finished: bool,
    },
}

/// Where a file resumes after a finished chunk.
pub struct FileProgressUpdate {
    pub path: String,
    pub content_hash: String,
    pub next_chunk: usize,
    pub chunks: usize,
}

/// A completed or abandoned session, handed to the store by the event loop.
pub struct SessionEnd {
    pub kind: &'static str,
    pub lesson: Option<String>,
    pub file: Option<String>,
    pub stage: u32,
    pub stage_kind: &'static str,
    pub seed: Option<u64>,
    pub started_at: SystemTime,
    pub finished: bool,
    pub summary: Summary,
    /// Empty when nothing was typed; the event loop then stores no session.
    pub log: Vec<Keystroke>,
    pub file_progress: Option<FileProgressUpdate>,
}

impl SessionEnd {
    pub fn meta(&self) -> SessionMeta<'_> {
        SessionMeta {
            kind: self.kind,
            lesson: self.lesson.as_deref(),
            file: self.file.as_deref(),
            stage: Some(self.stage),
            stage_kind: Some(self.stage_kind),
            seed: self.seed,
            started_at: self.started_at,
            finished: self.finished,
        }
    }
}

/// Something the event loop has to do for the app, because it needs the store or the disk.
pub enum Effect {
    Store(Box<SessionEnd>),
    OpenFile(String),
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
    snapshot: Snapshot,
    files: Vec<FileProgress>,
    settings: Settings,
    screen: Screen,
    flash: Option<(String, Instant)>,
    quit: bool,
}

impl App {
    pub fn new(
        course: Course,
        corpus: Corpus,
        progress: Progress,
        snapshot: Snapshot,
        files: Vec<FileProgress>,
        settings: Settings,
    ) -> Self {
        let selected = progress.next_index(&course);
        Self {
            course,
            corpus,
            progress,
            snapshot,
            files,
            settings,
            screen: Screen::Home { selected },
            flash: None,
            quit: false,
        }
    }

    pub fn should_quit(&self) -> bool {
        self.quit
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    /// The event loop refreshes this after every stored session.
    pub fn set_snapshot(&mut self, snapshot: Snapshot) {
        self.snapshot = snapshot;
    }

    pub fn set_files(&mut self, files: Vec<FileProgress>) {
        self.files = files;
        if let Screen::Files { selected } = &mut self.screen {
            *selected = (*selected).min(self.files.len().saturating_sub(1));
        }
    }

    pub fn recent_files_limit() -> usize {
        RECENT_FILES
    }

    /// A message on the current screen's status line for a moment.
    pub fn notify(&mut self, message: &str, now: Instant) {
        self.flash = Some((message.to_string(), now));
    }

    #[cfg(test)]
    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    #[cfg(test)]
    pub fn active(&self) -> Option<&Active> {
        match &self.screen {
            Screen::Typing(active) | Screen::Results { active, .. } => Some(active),
            Screen::Home { .. } | Screen::Files { .. } | Screen::Stats(_) => None,
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
        let definition = &self.course.lessons()[lesson];
        let weakness = self.weakness();
        let spec = StageSpec {
            kind,
            new: &definition.new,
            unlocked: &unlocked,
            code: definition.code,
            weakness: &weakness,
            seed,
        };
        let text = text::generate(&spec, &self.corpus);
        self.screen = Screen::Typing(Box::new(Active {
            source: Source::Lesson {
                lesson,
                stage,
                kind,
                seed,
            },
            engine: Engine::new(&text),
            clock: Clock::new(),
            started_at: None,
            abort_asked_at: None,
        }));
    }

    fn weakness(&self) -> Weakness {
        Weakness::from_stats(&self.snapshot.keys, &self.snapshot.bigrams)
    }

    /// A practice round over every key unlocked so far, or over `restrict`.
    pub fn start_practice(&mut self, restrict: Option<Vec<String>>) {
        let unlocked = match &restrict {
            Some(keys) => keys.clone(),
            None => {
                let next = self.progress.next_index(&self.course);
                let learnt = self.course.unlocked_before(next);
                if learnt.is_empty() {
                    self.course.unlocked_through(0)
                } else {
                    learnt
                }
            }
        };
        let seed: u64 = rand::random();
        let text = text::practice(&unlocked, &self.weakness(), &self.corpus, seed);
        self.screen = Screen::Typing(Box::new(Active {
            source: Source::Practice { seed, restrict },
            engine: Engine::new(&text),
            clock: Clock::new(),
            started_at: None,
            abort_asked_at: None,
        }));
    }

    /// Opens a file at the chunk it resumes from.
    pub fn start_file(&mut self, session: FileSession, now: Instant) {
        if let Some(note) = session.note {
            self.notify(note, now);
        }
        let chunk = session.next_chunk;
        self.start_chunk(session, chunk);
    }

    fn start_chunk(&mut self, session: FileSession, chunk: usize) {
        let prepared = file_text::prepare(&session.chunks[chunk], self.settings.indent);
        self.screen = Screen::Typing(Box::new(Active {
            source: Source::File { session, chunk },
            engine: Engine::with_given(prepared.target, prepared.given),
            clock: Clock::new(),
            started_at: None,
            abort_asked_at: None,
        }));
    }

    fn title(&self, active: &Active) -> String {
        match &active.source {
            Source::Lesson {
                lesson,
                stage,
                kind,
                ..
            } => {
                let definition = &self.course.lessons()[*lesson];
                format!(
                    "Lesson {} of {}: {}   {} ({}/{})",
                    lesson + 1,
                    self.course.lessons().len(),
                    definition.title,
                    kind.label(definition.code),
                    stage + 1,
                    self.course.stages(*lesson).len()
                )
            }
            Source::Practice { .. } => {
                let weakness = self.weakness();
                let mut weakest: Vec<String> = weakness
                    .weakest_keys(3)
                    .into_iter()
                    .map(|(c, f)| format!("{c} {f:.1}x"))
                    .chain(
                        weakness
                            .weakest_bigrams(2)
                            .into_iter()
                            .map(|(b, f)| format!("{b} {f:.1}x")),
                    )
                    .collect();
                if weakest.is_empty() {
                    weakest.push("nothing stands out yet".to_string());
                }
                format!("Practice   weakest: {}", weakest.join(", "))
            }
            Source::File { session, chunk } => {
                let part = &session.chunks[*chunk];
                format!(
                    "{}   chunk {} of {}   lines {} to {}",
                    files_screen::file_name(&session.path),
                    chunk + 1,
                    session.chunks.len(),
                    part.first_line + 1,
                    part.first_line + part.lines.len()
                )
            }
        }
    }

    /// Keyboard and finger hints, shown on intro stages only.
    fn hint_pane(&self, active: &Active) -> Option<HintPane> {
        let Source::Lesson {
            lesson,
            kind: StageKind::Intro,
            ..
        } = &active.source
        else {
            return None;
        };
        let new = &self.course.lessons()[*lesson].new;
        let all_capitals = new.iter().all(|key| key.chars().all(char::is_uppercase));
        let all_digits = new
            .iter()
            .all(|key| key.chars().all(|c| c.is_ascii_digit()));
        let lines = if all_capitals {
            vec!["Capitals: hold Shift with the hand that is not typing the letter".to_string()]
        } else if all_digits {
            vec![
                "Digits sit on the number row: 1 to 5 for the left hand, 6 to 0 for the right, \
                 pinky outward to index inward"
                    .to_string(),
                "With Mod4 the right hand also has a numpad: 7 8 9 on h g f, 4 5 6 on n r t, \
                 1 2 3 on m , . and 0 on space"
                    .to_string(),
            ]
        } else {
            new.iter()
                .filter_map(|key| layout::hint(self.settings.layout, key))
                .collect()
        };
        let layer = match new
            .iter()
            .filter_map(|key| layout::primary(self.settings.layout, key))
            .map(|p| p.layer)
            .max()
        {
            Some(3) => 3,
            _ => 1,
        };
        Some(HintPane {
            layout: self.settings.layout,
            layer,
            highlight: new.iter().cloned().collect::<HashSet<_>>(),
            unlocked: self.course.unlocked_through(*lesson).into_iter().collect(),
            lines,
        })
    }

    /// Handles one terminal event.
    pub fn handle(&mut self, event: Event, now: Instant) -> Option<Effect> {
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
                self.notify("pasting is not typing", now);
                None
            }
            Event::Key(key) if key.kind == KeyEventKind::Press => self.handle_key(key, now),
            _ => None,
        }
    }

    fn handle_key(&mut self, key: KeyEvent, now: Instant) -> Option<Effect> {
        if is_ctrl_c(&key) {
            self.quit = true;
            return match self.screen {
                Screen::Typing(_) => self.finish(false).map(|end| Effect::Store(Box::new(end))),
                _ => None,
            };
        }
        match &self.screen {
            Screen::Home { .. } => {
                self.handle_home_key(key, now);
                None
            }
            Screen::Files { .. } => self.handle_files_key(key),
            Screen::Typing(_) => self
                .handle_typing_key(key, now)
                .map(|end| Effect::Store(Box::new(end))),
            Screen::Results { .. } => {
                self.handle_results_key(key);
                None
            }
            Screen::Stats(_) => {
                self.handle_stats_key(key);
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
            KeyCode::Down => *selected = (*selected + 1).min(last),
            KeyCode::Up => *selected = selected.saturating_sub(1),
            KeyCode::Enter | KeyCode::Char(' ') => {
                let selected = *selected;
                if self.progress.available(&self.course, selected) {
                    self.start_stage(selected, 0);
                } else {
                    self.notify("locked: pass the lesson before it first", now);
                }
            }
            KeyCode::Char('s') => self.screen = Screen::Stats(StatsView::new()),
            KeyCode::Char('f') => self.screen = Screen::Files { selected: 0 },
            KeyCode::Char('p') => self.start_practice(None),
            KeyCode::Char('q') => self.quit = true,
            _ => {}
        }
    }

    fn handle_files_key(&mut self, key: KeyEvent) -> Option<Effect> {
        let Screen::Files { selected } = &mut self.screen else {
            return None;
        };
        let last = self.files.len().saturating_sub(1);
        match key.code {
            KeyCode::Down => *selected = (*selected + 1).min(last),
            KeyCode::Up => *selected = selected.saturating_sub(1),
            KeyCode::Enter => {
                return self
                    .files
                    .get(*selected)
                    .map(|file| Effect::OpenFile(file.path.clone()));
            }
            KeyCode::Esc | KeyCode::Char('f') => self.go_home(),
            KeyCode::Char('q') => self.quit = true,
            _ => {}
        }
        None
    }

    fn handle_stats_key(&mut self, key: KeyEvent) {
        let Screen::Stats(view) = &mut self.screen else {
            return;
        };
        match key.code {
            KeyCode::Tab | KeyCode::Right => view.next_page(),
            KeyCode::BackTab | KeyCode::Left => view.prev_page(),
            KeyCode::Char('1') => view.range = Range::Days30,
            KeyCode::Char('2') => view.range = Range::Days90,
            KeyCode::Char('3') => view.range = Range::All,
            KeyCode::Esc | KeyCode::Char('s') => self.go_home(),
            KeyCode::Char('q') => self.quit = true,
            _ => {}
        }
    }

    fn go_home(&mut self) {
        self.screen = Screen::Home {
            selected: self.progress.next_index(&self.course),
        };
    }

    /// Back to the list the active session came from.
    fn go_back(&mut self, source: &Source) {
        self.screen = match source {
            Source::Lesson { lesson, .. } => Screen::Home { selected: *lesson },
            Source::File { session, .. } => Screen::Files {
                selected: self
                    .files
                    .iter()
                    .position(|f| f.path == session.path)
                    .unwrap_or(0),
            },
            Source::Practice { .. } => Screen::Home {
                selected: self.progress.next_index(&self.course),
            },
        };
    }

    fn handle_typing_key(&mut self, key: KeyEvent, now: Instant) -> Option<SessionEnd> {
        let Screen::Typing(active) = &mut self.screen else {
            return None;
        };
        if key.code == KeyCode::Esc {
            // Aborting takes two presses, so one stray Esc never throws a session away.
            let asked_at = active.abort_asked_at.take();
            if !asked_at.is_some_and(|at| now.duration_since(at) < ABORT_CONFIRM) {
                active.abort_asked_at = Some(now);
                return None;
            }
            if active.engine.log().is_empty() {
                let Screen::Typing(active) =
                    std::mem::replace(&mut self.screen, Screen::Home { selected: 0 })
                else {
                    return None;
                };
                self.go_back(&active.source);
                return None;
            }
            return self.finish(false);
        }
        active.abort_asked_at = None;
        let input = key_to_input(&key)?;
        if !active.clock.started() {
            active.clock.start(now);
            active.started_at = Some(SystemTime::now());
        }
        let at = active.clock.at(now);
        match active.engine.input(input, at) {
            Outcome::Refused => self.notify("fix the error first (Backspace)", now),
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
        let finished = *finished;
        let passed = finished && summary.error_rate <= PASS_ERROR_RATE;
        match key.code {
            KeyCode::Enter | KeyCode::Char(' ') => match &active.source {
                Source::Lesson {
                    lesson,
                    stage,
                    kind,
                    ..
                } => {
                    let (lesson, stage, kind) = (*lesson, *stage, *kind);
                    match (finished, kind) {
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
                    }
                }
                Source::File { session, chunk } => {
                    let (session, chunk) = (session.clone(), *chunk);
                    match finished {
                        true if chunk + 1 < session.chunks.len() => {
                            self.start_chunk(session, chunk + 1)
                        }
                        true => self.go_back(&Source::File { session, chunk }),
                        false => self.start_chunk(session, chunk),
                    }
                }
                Source::Practice { restrict, .. } => {
                    let restrict = restrict.clone();
                    self.start_practice(restrict);
                }
            },
            KeyCode::Char('p') => self.start_practice(None),
            KeyCode::Char('r') => match &active.source {
                Source::Lesson { lesson, stage, .. } => {
                    let (lesson, stage) = (*lesson, *stage);
                    self.start_stage(lesson, stage);
                }
                Source::File { session, chunk } => {
                    let (session, chunk) = (session.clone(), *chunk);
                    self.start_chunk(session, chunk);
                }
                Source::Practice { restrict, .. } => {
                    let restrict = restrict.clone();
                    self.start_practice(restrict);
                }
            },
            KeyCode::Esc => {
                let Screen::Results { active, .. } =
                    std::mem::replace(&mut self.screen, Screen::Home { selected: 0 })
                else {
                    return;
                };
                self.go_back(&active.source);
            }
            KeyCode::Char('q') => self.quit = true,
            _ => {}
        }
    }

    /// Moves to the results screen and describes what to store.
    fn finish(&mut self, finished: bool) -> Option<SessionEnd> {
        let Screen::Typing(active) =
            std::mem::replace(&mut self.screen, Screen::Home { selected: 0 })
        else {
            return None;
        };
        let log = active.engine.log().to_vec();
        let summary = stats::summarize(&log);
        let started_at = active.started_at.unwrap_or_else(SystemTime::now);
        let end = match &active.source {
            Source::Lesson {
                lesson,
                stage,
                kind,
                seed,
            } => {
                let lesson_id = self.course.lessons()[*lesson].id.clone();
                if finished && *kind == StageKind::Test {
                    self.progress.record(&lesson_id, summary.error_rate);
                }
                SessionEnd {
                    kind: "lesson",
                    lesson: Some(lesson_id),
                    file: None,
                    stage: *stage as u32 + 1,
                    stage_kind: kind.name(),
                    seed: Some(*seed),
                    started_at,
                    finished,
                    summary: summary.clone(),
                    log,
                    file_progress: None,
                }
            }
            Source::Practice { seed, .. } => SessionEnd {
                kind: "practice",
                lesson: None,
                file: None,
                stage: 1,
                stage_kind: "practice",
                seed: Some(*seed),
                started_at,
                finished,
                summary: summary.clone(),
                log,
                file_progress: None,
            },
            Source::File { session, chunk } => SessionEnd {
                kind: "file",
                lesson: None,
                file: Some(session.path.clone()),
                stage: *chunk as u32 + 1,
                stage_kind: "chunk",
                seed: None,
                started_at,
                finished,
                summary: summary.clone(),
                log,
                file_progress: finished.then(|| FileProgressUpdate {
                    path: session.path.clone(),
                    content_hash: session.content_hash.clone(),
                    next_chunk: chunk + 1,
                    chunks: session.chunks.len(),
                }),
            },
        };
        self.screen = Screen::Results {
            active,
            summary,
            finished,
        };
        (!end.log.is_empty() || end.file_progress.is_some()).then_some(end)
    }

    /// The pending abort question, which outranks any other status message.
    fn abort_question(active: &Active, now: Instant) -> Option<String> {
        let what = match active.source {
            Source::Lesson { .. } => "lesson",
            Source::File { .. } => "chunk",
            Source::Practice { .. } => "round",
        };
        active
            .abort_asked_at
            .filter(|at| now.duration_since(*at) < ABORT_CONFIRM)
            .map(|_| format!("abort this {what}? press Esc again to confirm"))
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
            flash: Self::abort_question(active, now).or_else(|| self.flash_text(now)),
        }
    }

    pub fn render(&self, frame: &mut Frame, now: Instant) {
        let area = frame.area();
        match &self.screen {
            Screen::Home { selected } => {
                home::draw(
                    frame,
                    area,
                    &self.course,
                    &self.progress,
                    &self.snapshot.habit,
                    *selected,
                    self.settings.layout,
                );
                if let Some(message) = self.flash_text(now) {
                    typing::draw_flash(frame, area, &message);
                }
            }
            Screen::Files { selected } => {
                files_screen::draw(frame, area, &self.files, *selected);
                if let Some(message) = self.flash_text(now) {
                    typing::draw_flash(frame, area, &message);
                }
            }
            Screen::Stats(view) => stats_screen::draw(
                frame,
                area,
                &self.snapshot,
                view,
                &self.course,
                self.settings.layout,
            ),
            Screen::Typing(active) => {
                let hints = self.hint_pane(active);
                let width = match active.source {
                    Source::Lesson { .. } => LESSON_WIDTH,
                    Source::File { .. } => FILE_WIDTH,
                    Source::Practice { .. } => PRACTICE_WIDTH,
                };
                typing::draw(
                    frame,
                    area,
                    &self.title(active),
                    &active.engine,
                    &self.status(active, now),
                    hints.as_ref(),
                    width,
                );
            }
            Screen::Results {
                active,
                summary,
                finished,
            } => {
                let passed = *finished && summary.error_rate <= PASS_ERROR_RATE;
                let (what, is_test, next) = match &active.source {
                    Source::Lesson { kind, .. } => {
                        let next = match (*finished, kind) {
                            (true, StageKind::Test) if passed => "next lesson",
                            (true, StageKind::Test) => "try the test again",
                            (true, _) => "next stage",
                            (false, _) => "repeat",
                        };
                        ("Stage", *kind == StageKind::Test, next)
                    }
                    Source::File { session, chunk } => {
                        let next = match *finished {
                            true if chunk + 1 < session.chunks.len() => "next chunk",
                            true => "back to files",
                            false => "repeat",
                        };
                        ("Chunk", false, next)
                    }
                    Source::Practice { .. } => ("Round", false, "another round"),
                };
                let view = results::View {
                    title: &self.title(active),
                    summary,
                    finished: *finished,
                    what,
                    is_test,
                    next_label: next,
                };
                results::draw(frame, area, &view);
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
    use super::super::stats::Page;
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::{Color, Modifier};

    fn app() -> App {
        App::new(
            Course::load(Layout::Bone).unwrap(),
            Corpus::load(),
            Progress::new(),
            Snapshot::empty("2026-09-04"),
            Vec::new(),
            Settings::default(),
        )
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

    /// The two Esc presses that abort an active session.
    fn abort(app: &mut App, at: Instant) -> Option<Effect> {
        app.handle(press(KeyCode::Esc, KeyModifiers::NONE), at);
        app.handle(press(KeyCode::Esc, KeyModifiers::NONE), at + ms(10))
    }

    fn stored(effect: Option<Effect>) -> Option<SessionEnd> {
        match effect {
            Some(Effect::Store(end)) => Some(*end),
            _ => None,
        }
    }

    /// Types the whole current stage, `wrong_every` characters preceded by a mistake.
    fn type_stage(app: &mut App, t0: Instant, wrong_every: Option<usize>) -> Option<SessionEnd> {
        let active = app.active().expect("a stage");
        let target: Vec<(String, bool)> = active
            .engine
            .target()
            .iter()
            .enumerate()
            .map(|(i, g)| (g.clone(), active.engine.is_given(i)))
            .collect();
        let mut end = None;
        for (index, (grapheme, given)) in target.iter().enumerate() {
            if *given {
                continue;
            }
            let at = t0 + ms(200 * index as u64);
            if wrong_every.is_some_and(|n| index % n == 0) {
                let wrong = if grapheme == "x" { 'y' } else { 'x' };
                app.handle(ch(wrong), at);
                app.handle(press(KeyCode::Backspace, KeyModifiers::NONE), at + ms(50));
            }
            let event = match grapheme.as_str() {
                "\n" => enter(),
                "\t" => press(KeyCode::Tab, KeyModifiers::NONE),
                g => ch(g.chars().next().unwrap()),
            };
            end = stored(app.handle(event, at + ms(100)));
        }
        end
    }

    fn stage_of(app: &App) -> (usize, usize, StageKind) {
        match &app.active().expect("a stage").source {
            Source::Lesson {
                lesson,
                stage,
                kind,
                ..
            } => (*lesson, *stage, *kind),
            Source::File { .. } | Source::Practice { .. } => panic!("not a lesson"),
        }
    }

    fn is_practice(app: &App) -> bool {
        matches!(
            app.active().map(|a| &a.source),
            Some(Source::Practice { .. })
        )
    }

    #[test]
    fn practice_rounds_start_from_home_and_from_results_and_repeat() {
        let mut app = app();
        let t0 = Instant::now();
        app.handle(ch('p'), t0);
        assert!(is_practice(&app));
        assert!(
            app.title(app.active().unwrap())
                .starts_with("Practice   weakest: nothing stands out yet")
        );
        for grapheme in app.active().unwrap().engine.target() {
            assert!(
                ["e", "n", " "].contains(&grapheme.as_str()),
                "nothing passed yet: {grapheme:?}"
            );
        }
        let end = type_stage(&mut app, t0, None).expect("stored");
        assert_eq!((end.kind, end.stage_kind), ("practice", "practice"));
        assert!(end.lesson.is_none() && end.file.is_none() && end.seed.is_some());
        let first = app.active().unwrap().engine.target().to_vec();
        let buffer = render(&app);
        assert!(find(&buffer, "Round complete").is_some());
        assert!(find(&buffer, "Enter/Space: another round").is_some());
        app.handle(enter(), t0);
        assert!(is_practice(&app));
        assert_ne!(app.active().unwrap().engine.target(), first.as_slice());
        abort(&mut app, t0);
        assert!(matches!(app.screen(), Screen::Home { .. }));
        app.handle(enter(), t0);
        app.handle(ch('e'), t0);
        abort(&mut app, t0 + ms(10));
        app.handle(ch('p'), t0);
        assert!(is_practice(&app), "p on a results screen starts practice");
    }

    #[test]
    fn practice_uses_the_learnt_keys_and_names_the_weak_ones() {
        use crate::stats::KeyStat;
        let mut app = app_with_track_a_passed();
        let mut snapshot = Snapshot::empty("2026-09-05");
        snapshot.keys.push(KeyStat {
            key: "k".into(),
            attempts: 40,
            errors: 8,
            error_rate: 0.2,
            median_ms: Some(300),
        });
        app.set_snapshot(snapshot);
        app.handle(ch('p'), Instant::now());
        let title = app.title(app.active().unwrap());
        assert!(title.contains("k 3.0x"), "{title}");
        let target: String = app.active().unwrap().engine.target().concat();
        assert!(target.contains('k') && !target.contains('('), "{target}");
        let mut app = app_with_track_a_passed();
        app.start_practice(Some(vec!["e".into(), "n".into()]));
        for grapheme in app.active().unwrap().engine.target() {
            assert!(
                ["e", "n", " "].contains(&grapheme.as_str()),
                "restricted: {grapheme:?}"
            );
        }
    }

    fn chunk_of(app: &App) -> usize {
        match &app.active().expect("a chunk").source {
            Source::File { chunk, .. } => *chunk,
            Source::Lesson { .. } | Source::Practice { .. } => panic!("not a file"),
        }
    }

    fn file_session() -> FileSession {
        let text: String = (0..15)
            .map(|i| format!("fn f{i}() {{\n    let x = {i};\n}}\n"))
            .collect();
        let lines = file_text::normalise(&text);
        FileSession {
            path: "/tmp/somewhere/main.rs".into(),
            content_hash: file_text::content_hash(&lines),
            chunks: file_text::chunks(&lines),
            next_chunk: 0,
            note: None,
        }
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
        progress.record("bone-a01", 0.01);
        let app = App::new(
            Course::load(Layout::Bone).unwrap(),
            Corpus::load(),
            progress,
            Snapshot::empty("2026-09-04"),
            Vec::new(),
            Settings::default(),
        );
        assert!(matches!(app.screen(), Screen::Home { selected: 1 }));
    }

    #[test]
    fn stats_screen_opens_from_home_pages_and_returns() {
        let mut app = app();
        let t0 = Instant::now();
        app.handle(ch('s'), t0);
        assert!(matches!(app.screen(), Screen::Stats(view) if view.page == Page::Trend));
        app.handle(press(KeyCode::Tab, KeyModifiers::NONE), t0);
        app.handle(ch('3'), t0);
        assert!(
            matches!(app.screen(), Screen::Stats(view) if view.page == Page::Keys && view.range == Range::All)
        );
        let buffer = render(&app);
        assert!(find(&buffer, "worst bigrams").is_some());
        app.handle(press(KeyCode::Esc, KeyModifiers::NONE), t0);
        assert!(matches!(app.screen(), Screen::Home { selected: 0 }));
        let buffer = render(&app);
        assert!(
            find(&buffer, "streak 0 days").is_some(),
            "habit line on the home screen"
        );
    }

    #[test]
    fn home_navigation_is_bounded_and_locked_lessons_do_not_start() {
        let mut app = app();
        let t0 = Instant::now();
        app.handle(press(KeyCode::Up, KeyModifiers::NONE), t0);
        assert!(matches!(app.screen(), Screen::Home { selected: 0 }));
        app.handle(press(KeyCode::Down, KeyModifiers::NONE), t0);
        assert!(matches!(app.screen(), Screen::Home { selected: 1 }));
        app.handle(enter(), t0);
        assert!(
            matches!(app.screen(), Screen::Home { selected: 1 }),
            "lesson 2 is locked"
        );
        assert!(app.flash_text(t0).is_some());
        app.handle(press(KeyCode::Up, KeyModifiers::NONE), t0);
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
        assert!(app.progress().passed("bone-a01"));
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
        abort(&mut app, t0);
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
        assert!(!app.progress().passed("bone-a01"));
        assert_eq!(
            app.progress().best("bone-a01"),
            Some(end.summary.error_rate)
        );
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
        let end = stored(abort(&mut app, t0 + ms(100))).expect("stored");
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
        let end = stored(app.handle(
            press(KeyCode::Char('c'), KeyModifiers::CONTROL),
            t0 + ms(10),
        ));
        assert!(end.is_some_and(|end| !end.finished));
        assert!(app.should_quit());
    }

    #[test]
    fn a_file_is_typed_chunk_by_chunk_with_indentation_given() {
        let mut app = app();
        let t0 = Instant::now();
        let session = file_session();
        assert_eq!(session.chunks.len(), 4, "45 lines in chunks of 12");
        app.start_file(session, t0);
        assert_eq!(chunk_of(&app), 0);
        assert!(
            app.title(app.active().unwrap())
                .contains("main.rs   chunk 1 of 4   lines 1 to 12")
        );
        let engine = &app.active().unwrap().engine;
        let indent_index = engine.target().iter().position(|g| g == "\n").unwrap() + 1;
        assert!(
            engine.is_given(indent_index),
            "the second line's indentation is given"
        );
        assert!(!engine.is_given(indent_index + 4));
        let end = type_stage(&mut app, t0, None).expect("stored");
        assert_eq!((end.kind, end.stage_kind, end.stage), ("file", "chunk", 1));
        assert_eq!(end.file.as_deref(), Some("/tmp/somewhere/main.rs"));
        let progress = end.file_progress.expect("progress after a finished chunk");
        assert_eq!((progress.next_chunk, progress.chunks), (1, 4));
        app.handle(enter(), t0);
        assert_eq!(chunk_of(&app), 1);
        app.handle(ch('f'), t0);
        let end = stored(abort(&mut app, t0)).expect("aborted chunk stored");
        assert!(!end.finished && end.file_progress.is_none());
        app.handle(press(KeyCode::Esc, KeyModifiers::NONE), t0);
        assert!(
            matches!(app.screen(), Screen::Files { .. }),
            "a file session goes back to the files list"
        );
    }

    #[test]
    fn the_last_chunk_returns_to_the_files_list() {
        let mut app = app();
        let t0 = Instant::now();
        let mut session = file_session();
        session.next_chunk = 3;
        app.start_file(session, t0);
        assert_eq!(chunk_of(&app), 3);
        let end = type_stage(&mut app, t0, None).unwrap();
        assert_eq!(end.file_progress.unwrap().next_chunk, 4);
        let buffer = render(&app);
        assert!(find(&buffer, "Chunk complete").is_some());
        assert!(find(&buffer, "Enter/Space: back to files").is_some());
        app.handle(enter(), t0);
        assert!(matches!(app.screen(), Screen::Files { .. }));
    }

    #[test]
    fn files_screen_lists_files_and_asks_the_loop_to_open_one() {
        let mut app = app();
        let t0 = Instant::now();
        app.handle(ch('f'), t0);
        assert!(matches!(app.screen(), Screen::Files { selected: 0 }));
        assert!(app.handle(enter(), t0).is_none(), "nothing to open");
        app.set_files(vec![FileProgress {
            path: "/tmp/somewhere/main.rs".into(),
            content_hash: "h".into(),
            next_chunk: 2,
            chunks: 4,
            updated_at: "2026-09-05T10:00:00Z".into(),
        }]);
        let buffer = render(&app);
        assert!(find(&buffer, "main.rs").is_some());
        assert!(find(&buffer, "chunk 3 of 4").is_some());
        assert!(
            matches!(app.handle(enter(), t0), Some(Effect::OpenFile(path)) if path == "/tmp/somewhere/main.rs")
        );
        app.handle(press(KeyCode::Esc, KeyModifiers::NONE), t0);
        assert!(matches!(app.screen(), Screen::Home { .. }));
    }

    #[test]
    fn escape_on_the_home_screen_does_not_quit() {
        let mut app = app();
        let t0 = Instant::now();
        app.handle(press(KeyCode::Esc, KeyModifiers::NONE), t0);
        assert!(!app.should_quit(), "only q and ctrl+c leave the program");
        assert!(matches!(app.screen(), Screen::Home { .. }));
        app.handle(ch('q'), t0);
        assert!(app.should_quit());
    }

    #[test]
    fn aborting_a_stage_needs_a_second_escape_within_two_seconds() {
        let mut app = app();
        let t0 = Instant::now();
        app.handle(enter(), t0);
        app.handle(ch('e'), t0);
        app.handle(press(KeyCode::Esc, KeyModifiers::NONE), t0 + ms(100));
        assert!(
            matches!(app.screen(), Screen::Typing(_)),
            "the first Esc only asks"
        );
        let buffer = render_at(&app, t0 + ms(150));
        assert!(find(&buffer, "abort this lesson?").is_some());
        let buffer = render_at(&app, t0 + ms(2_150));
        assert!(
            find(&buffer, "abort this lesson?").is_none(),
            "asked for 2s"
        );
        app.handle(press(KeyCode::Esc, KeyModifiers::NONE), t0 + ms(2_200));
        assert!(
            matches!(app.screen(), Screen::Typing(_)),
            "too late to confirm, so it asks again"
        );
        let end = stored(app.handle(press(KeyCode::Esc, KeyModifiers::NONE), t0 + ms(2_300)))
            .expect("stored");
        assert!(!end.finished);
        assert!(matches!(app.screen(), Screen::Results { .. }));
    }

    #[test]
    fn typing_on_takes_back_the_abort_question() {
        let mut app = app();
        let t0 = Instant::now();
        app.handle(enter(), t0);
        app.handle(press(KeyCode::Esc, KeyModifiers::NONE), t0);
        app.handle(ch('e'), t0 + ms(50));
        let buffer = render_at(&app, t0 + ms(100));
        assert!(find(&buffer, "abort this lesson?").is_none());
        app.handle(press(KeyCode::Esc, KeyModifiers::NONE), t0 + ms(150));
        assert!(
            matches!(app.screen(), Screen::Typing(_)),
            "the next Esc asks from scratch"
        );
    }

    fn render(app: &App) -> Buffer {
        render_at(app, Instant::now())
    }

    fn render_at(app: &App, now: Instant) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
        terminal.draw(|frame| app.render(frame, now)).unwrap();
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
        assert!(find(&buffer, "0 of 35 lessons passed").is_some());
        assert!(find(&buffer, "sans   Bone").is_some());
    }

    #[test]
    fn intro_screen_shows_keyboard_hints_text_and_marks_a_wrong_character() {
        let mut app = app();
        let t0 = Instant::now();
        app.handle(enter(), t0);
        app.handle(ch('e'), t0);
        app.handle(ch('x'), t0 + ms(10));
        let buffer = render(&app);
        assert!(find(&buffer, "Lesson 1 of 35: e and n").is_some());
        assert!(find(&buffer, "left index finger, home row").is_some());
        assert!(
            find(&buffer, " c  t  i  e  o ").is_some(),
            "keyboard home row"
        );
        let (x, y) = find(&buffer, "exe").expect("typed e then wrong x");
        assert_eq!(buffer.cell((x + 1, y)).unwrap().bg, Color::Red);
        assert!(find(&buffer, "1 error").is_some());
    }

    #[test]
    fn file_screen_renders_given_indentation_dim() {
        let mut app = app();
        let t0 = Instant::now();
        app.start_file(file_session(), t0);
        let buffer = render(&app);
        let (x, y) = find(&buffer, "    let x = 0;").expect("indented line");
        let cell = buffer.cell((x, y)).unwrap();
        assert!(
            cell.modifier.contains(Modifier::DIM),
            "given indentation is dim"
        );
        assert!(find(&buffer, "main.rs   chunk 1 of 4").is_some());
    }

    fn app_with_track_a_passed() -> App {
        let course = Course::load(Layout::Bone).unwrap();
        let mut progress = Progress::new();
        for lesson in course.lessons().iter().filter(|l| !l.code) {
            progress.record(&lesson.id, 0.0);
        }
        App::new(
            course,
            Corpus::load(),
            progress,
            Snapshot::empty("2026-09-05"),
            Vec::new(),
            Settings::default(),
        )
    }

    #[test]
    fn symbol_lessons_show_the_layer_three_keyboard_with_modifier_hints() {
        let mut app = app_with_track_a_passed();
        let t0 = Instant::now();
        assert!(
            matches!(app.screen(), Screen::Home { selected: 22 }),
            "track B is next"
        );
        app.handle(enter(), t0);
        let active = app.active().unwrap();
        let hints = app.hint_pane(active).expect("intro hints");
        assert_eq!(hints.layer, 3);
        assert_eq!(hints.lines.len(), 2);
        assert!(
            hints.lines[0].contains("Mod3 with the left pinky (Caps Lock), then n"),
            "{:?}",
            hints.lines
        );
        let buffer = render(&app);
        assert!(find(&buffer, "Lesson 23 of 35: Parentheses   intro (1/4)").is_some());
        assert!(
            find(&buffer, " \\  /  {  }  *  ?  (  )  -  :  @ ").is_some(),
            "layer 3 home row"
        );
        let (x, y) = find(&buffer, " ( ").expect("open paren key");
        assert_eq!(
            buffer.cell((x + 1, y)).unwrap().bg,
            Color::Yellow,
            "new symbol lit"
        );
        type_stage(&mut app, t0, None);
        app.handle(enter(), t0);
        assert!(app.title(app.active().unwrap()).contains("tokens (2/4)"));
        let target: String = app.active().unwrap().engine.target().concat();
        assert!(target.contains('(') && !target.contains('='), "{target}");
    }

    #[test]
    fn digits_lesson_mentions_the_numpad() {
        let mut app = app_with_track_a_passed();
        let t0 = Instant::now();
        for _ in 0..11 {
            app.handle(press(KeyCode::Down, KeyModifiers::NONE), t0);
        }
        assert!(matches!(app.screen(), Screen::Home { selected: 33 }));
        app.handle(enter(), t0);
        assert!(
            matches!(app.screen(), Screen::Home { .. }),
            "b12 is locked until b11 passes"
        );
        let course = Course::load(Layout::Bone).unwrap();
        let mut progress = Progress::new();
        for lesson in course.lessons().iter().take(33) {
            progress.record(&lesson.id, 0.0);
        }
        let mut app = App::new(
            course,
            Corpus::load(),
            progress,
            Snapshot::empty("2026-09-05"),
            Vec::new(),
            Settings::default(),
        );
        app.handle(enter(), t0);
        let hints = app.hint_pane(app.active().unwrap()).unwrap();
        assert_eq!(hints.layer, 1);
        assert!(
            hints.lines.iter().any(|l| l.contains("numpad")),
            "{:?}",
            hints.lines
        );
    }

    #[test]
    fn home_screen_shows_the_track_b_heading() {
        let mut app = app_with_track_a_passed();
        let buffer = render(&app);
        assert!(find(&buffer, "Track B: symbols (layer 3)").is_some());
        app.handle(press(KeyCode::Up, KeyModifiers::NONE), Instant::now());
        assert!(matches!(app.screen(), Screen::Home { selected: 21 }));
    }

    #[test]
    fn space_starts_a_stage_and_advances_like_enter() {
        let mut app = app();
        let t0 = Instant::now();
        app.handle(ch(' '), t0);
        assert_eq!(stage_of(&app), (0, 0, StageKind::Intro));
        type_stage(&mut app, t0, None);
        app.handle(ch(' '), t0);
        assert_eq!(stage_of(&app), (0, 1, StageKind::Bigrams));
    }

    #[test]
    fn results_screen_shows_verdict_and_next_action() {
        let mut app = app();
        let t0 = Instant::now();
        app.handle(enter(), t0);
        type_stage(&mut app, t0, None);
        let buffer = render(&app);
        assert!(find(&buffer, "Stage complete").is_some());
        assert!(find(&buffer, "Enter/Space: next stage").is_some());
        assert!(find(&buffer, "error rate").is_some());
    }
}
