//! The session clock: elapsed time since the first keystroke, minus time spent paused.
//!
//! The app pauses it on `FocusLost` and resumes it on `FocusGained`, so time in another
//! window never counts as typing time, however short.

use std::time::{Duration, Instant};

#[derive(Debug, Default)]
pub struct Clock {
    start: Option<Instant>,
    paused_at: Option<Instant>,
    paused_total: Duration,
}

impl Clock {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn started(&self) -> bool {
        self.start.is_some()
    }

    pub fn is_paused(&self) -> bool {
        self.paused_at.is_some()
    }

    /// Starts the clock at `now` unless it is already running.
    pub fn start(&mut self, now: Instant) {
        if self.start.is_none() {
            self.start = Some(now);
        }
    }

    /// Session time at `now`: elapsed since start, paused time excluded. Zero before start.
    pub fn at(&self, now: Instant) -> Duration {
        let Some(start) = self.start else {
            return Duration::ZERO;
        };
        let end = self.paused_at.unwrap_or(now);
        end.saturating_duration_since(start)
            .saturating_sub(self.paused_total)
    }

    pub fn pause(&mut self, now: Instant) {
        if self.started() && self.paused_at.is_none() {
            self.paused_at = Some(now);
        }
    }

    pub fn resume(&mut self, now: Instant) {
        if let Some(paused_at) = self.paused_at.take() {
            self.paused_total += now.saturating_duration_since(paused_at);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn zero_before_start() {
        let base = Instant::now();
        let clock = Clock::new();
        assert!(!clock.started());
        assert_eq!(clock.at(base + secs(10)), Duration::ZERO);
    }

    #[test]
    fn counts_from_the_first_start_only() {
        let base = Instant::now();
        let mut clock = Clock::new();
        clock.start(base);
        clock.start(base + secs(5));
        assert_eq!(clock.at(base + secs(10)), secs(10));
    }

    #[test]
    fn paused_time_is_excluded() {
        let base = Instant::now();
        let mut clock = Clock::new();
        clock.start(base);
        clock.pause(base + secs(2));
        assert!(clock.is_paused());
        assert_eq!(clock.at(base + secs(60)), secs(2), "frozen while paused");
        clock.resume(base + secs(62));
        assert!(!clock.is_paused());
        assert_eq!(clock.at(base + secs(65)), secs(5));
    }

    #[test]
    fn pause_and_resume_are_idempotent() {
        let base = Instant::now();
        let mut clock = Clock::new();
        clock.start(base);
        clock.pause(base + secs(1));
        clock.pause(base + secs(2));
        clock.resume(base + secs(3));
        clock.resume(base + secs(4));
        assert_eq!(clock.at(base + secs(5)), secs(3));
    }

    #[test]
    fn pausing_before_start_is_a_no_op() {
        let base = Instant::now();
        let mut clock = Clock::new();
        clock.pause(base);
        clock.resume(base + secs(1));
        clock.start(base + secs(2));
        assert_eq!(clock.at(base + secs(3)), secs(1));
    }
}
