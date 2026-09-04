//! Metrics derived from a keystroke log. Nothing here is stored; the log is the truth.

use std::time::Duration;

use crate::engine::{Keystroke, KeystrokeKind};

/// Pauses longer than this are not counted as active time.
pub const MAX_GAP: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq)]
pub struct Summary {
    /// Correctly typed characters.
    pub chars: usize,
    /// Wrong keystrokes.
    pub errors: usize,
    pub active: Duration,
    /// Characters per minute of active time (Anschläge pro Minute).
    pub cpm: f64,
    /// cpm / 5.
    pub wpm: f64,
    /// errors / (chars + errors), 0.0 when nothing was typed.
    pub error_rate: f64,
}

/// Active time up to the last keystroke, gaps over `MAX_GAP` excluded.
pub fn active_time(log: &[Keystroke]) -> Duration {
    log.windows(2)
        .map(|pair| counted_gap(pair[0].at, pair[1].at))
        .sum()
}

/// Active time as of `now`, for the live display: like `active_time` plus the gap since the
/// last keystroke if it is still within `MAX_GAP`.
pub fn active_time_until(log: &[Keystroke], now: Duration) -> Duration {
    match log.last() {
        Some(last) => active_time(log) + counted_gap(last.at, now),
        None => Duration::ZERO,
    }
}

fn counted_gap(from: Duration, to: Duration) -> Duration {
    let gap = to.saturating_sub(from);
    if gap > MAX_GAP { Duration::ZERO } else { gap }
}

pub fn summarize(log: &[Keystroke]) -> Summary {
    let chars = log
        .iter()
        .filter(|k| k.kind == KeystrokeKind::Correct)
        .count();
    let errors = log
        .iter()
        .filter(|k| k.kind == KeystrokeKind::Wrong)
        .count();
    let active = active_time(log);
    let cpm = rate_per_minute(chars, active);
    Summary {
        chars,
        errors,
        active,
        cpm,
        wpm: cpm / 5.0,
        error_rate: ratio(errors, chars + errors),
    }
}

/// `count` per minute of `active`, 0 when no time has passed.
pub fn rate_per_minute(count: usize, active: Duration) -> f64 {
    if active.is_zero() {
        return 0.0;
    }
    count as f64 / (active.as_secs_f64() / 60.0)
}

fn ratio(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        0.0
    } else {
        part as f64 / whole as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stroke(ms: u64, kind: KeystrokeKind) -> Keystroke {
        Keystroke {
            at: Duration::from_millis(ms),
            expected: "e".into(),
            typed: "e".into(),
            kind,
        }
    }

    fn correct_every(ms: u64, count: u64) -> Vec<Keystroke> {
        (0..count)
            .map(|i| stroke(i * ms, KeystrokeKind::Correct))
            .collect()
    }

    #[test]
    fn empty_log_is_all_zeros() {
        let s = summarize(&[]);
        assert_eq!(s.chars, 0);
        assert_eq!(s.errors, 0);
        assert_eq!(s.active, Duration::ZERO);
        assert_eq!(s.cpm, 0.0);
        assert_eq!(s.error_rate, 0.0);
    }

    #[test]
    fn cpm_counts_correct_characters_over_active_minutes() {
        let s = summarize(&correct_every(500, 10));
        assert_eq!(s.chars, 10);
        assert_eq!(s.active, Duration::from_millis(4500));
        assert!((s.cpm - 133.333).abs() < 0.01, "{}", s.cpm);
        assert!((s.wpm - 26.666).abs() < 0.01, "{}", s.wpm);
    }

    #[test]
    fn gaps_over_five_seconds_are_not_active_time() {
        let log = [
            stroke(0, KeystrokeKind::Correct),
            stroke(1000, KeystrokeKind::Correct),
            stroke(20_000, KeystrokeKind::Correct),
            stroke(21_000, KeystrokeKind::Correct),
        ];
        assert_eq!(active_time(&log), Duration::from_secs(2));
    }

    #[test]
    fn error_rate_is_errors_over_all_counted_keystrokes() {
        let mut log = correct_every(100, 8);
        log.push(stroke(800, KeystrokeKind::Wrong));
        log.push(stroke(900, KeystrokeKind::Backspace));
        log.push(stroke(1000, KeystrokeKind::Wrong));
        let s = summarize(&log);
        assert_eq!(s.errors, 2);
        assert_eq!(s.chars, 8);
        assert!((s.error_rate - 0.2).abs() < 1e-9, "{}", s.error_rate);
    }

    #[test]
    fn live_active_time_includes_the_trailing_gap_only_while_short() {
        let log = correct_every(1000, 3);
        assert_eq!(
            active_time_until(&log, Duration::from_millis(2500)),
            Duration::from_millis(2500)
        );
        assert_eq!(
            active_time_until(&log, Duration::from_secs(30)),
            Duration::from_secs(2)
        );
        assert_eq!(
            active_time_until(&[], Duration::from_secs(30)),
            Duration::ZERO
        );
    }
}
