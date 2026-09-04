//! Metrics derived from a keystroke log. Nothing here is stored; the log is the truth.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use serde::Serialize;

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

/// One day of practice, every session kind included.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DayStat {
    /// Local date, `YYYY-MM-DD`.
    pub date: String,
    pub sessions: u32,
    pub chars: u64,
    pub errors: u64,
    pub active_ms: u64,
    pub error_rate: f64,
    pub cpm: f64,
}

impl DayStat {
    pub fn new(date: String, sessions: u32, chars: u64, errors: u64, active_ms: u64) -> Self {
        let active = Duration::from_millis(active_ms);
        Self {
            date,
            sessions,
            chars,
            errors,
            active_ms,
            error_rate: ratio(errors as usize, (chars + errors) as usize),
            cpm: rate_per_minute(chars as usize, active),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LessonStat {
    pub lesson: String,
    pub attempts: u32,
    pub tests: u32,
    pub best_error_rate: Option<f64>,
    pub best_cpm: Option<f64>,
    pub passed_at: Option<String>,
}

/// A keystroke read back with its predecessor in the same session, for aggregation.
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    pub expected: String,
    pub kind: KeystrokeKind,
    pub prev_expected: Option<String>,
    pub prev_kind: Option<KeystrokeKind>,
    pub interval_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct KeyStat {
    pub key: String,
    pub attempts: u32,
    pub errors: u32,
    pub error_rate: f64,
    /// Median time before a correct keystroke of this key whose predecessor was also
    /// correct, so corrections never count as transitions.
    pub median_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BigramStat {
    pub bigram: String,
    pub attempts: u32,
    pub errors: u32,
    pub error_rate: f64,
    pub median_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Habit {
    pub streak_days: u32,
    pub today_minutes: f64,
    pub target_minutes: u32,
    pub total_hours: f64,
    pub sessions: u32,
}

/// Everything the stats screen and `stats --json` show.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub today: String,
    pub days: Vec<DayStat>,
    pub lessons: Vec<LessonStat>,
    pub keys: Vec<KeyStat>,
    pub bigrams: Vec<BigramStat>,
    pub habit: Habit,
}

/// Per expected key: attempts, errors, median interval. Worst error rate first.
pub fn aggregate_keys(strokes: &[Stroke]) -> Vec<KeyStat> {
    let mut groups: HashMap<&str, Group> = HashMap::new();
    for stroke in strokes
        .iter()
        .filter(|s| s.kind != KeystrokeKind::Backspace)
    {
        groups.entry(&stroke.expected).or_default().add(stroke);
    }
    let mut keys: Vec<KeyStat> = groups
        .into_iter()
        .map(|(key, group)| KeyStat {
            key: key.to_string(),
            attempts: group.attempts,
            errors: group.errors,
            error_rate: ratio(group.errors as usize, group.attempts as usize),
            median_ms: median(group.intervals),
        })
        .collect();
    keys.sort_by(|a, b| {
        worst_first(
            a.error_rate,
            a.attempts,
            &a.key,
            b.error_rate,
            b.attempts,
            &b.key,
        )
    });
    keys
}

/// Per transition into a key, counting only strokes whose predecessor was correct, so a
/// correction sequence never counts as a transition. Worst error rate first.
pub fn aggregate_bigrams(strokes: &[Stroke]) -> Vec<BigramStat> {
    let mut groups: HashMap<String, Group> = HashMap::new();
    for stroke in strokes
        .iter()
        .filter(|s| s.kind != KeystrokeKind::Backspace)
    {
        if stroke.prev_kind != Some(KeystrokeKind::Correct) {
            continue;
        }
        let Some(prev) = &stroke.prev_expected else {
            continue;
        };
        groups
            .entry(format!("{prev}{}", stroke.expected))
            .or_default()
            .add(stroke);
    }
    let mut bigrams: Vec<BigramStat> = groups
        .into_iter()
        .map(|(bigram, group)| BigramStat {
            bigram,
            attempts: group.attempts,
            errors: group.errors,
            error_rate: ratio(group.errors as usize, group.attempts as usize),
            median_ms: median(group.intervals),
        })
        .collect();
    bigrams.sort_by(|a, b| {
        worst_first(
            a.error_rate,
            a.attempts,
            &a.bigram,
            b.error_rate,
            b.attempts,
            &b.bigram,
        )
    });
    bigrams
}

#[derive(Default)]
struct Group {
    attempts: u32,
    errors: u32,
    intervals: Vec<u64>,
}

impl Group {
    fn add(&mut self, stroke: &Stroke) {
        self.attempts += 1;
        match stroke.kind {
            KeystrokeKind::Wrong => self.errors += 1,
            KeystrokeKind::Correct if stroke.prev_kind == Some(KeystrokeKind::Correct) => {
                if let Some(interval) = stroke
                    .interval_ms
                    .filter(|&ms| ms <= MAX_GAP.as_millis() as u64)
                {
                    self.intervals.push(interval);
                }
            }
            _ => {}
        }
    }
}

fn median(mut values: Vec<u64>) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    let middle = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2
    } else {
        values[middle]
    })
}

fn worst_first(
    rate_a: f64,
    attempts_a: u32,
    name_a: &str,
    rate_b: f64,
    attempts_b: u32,
    name_b: &str,
) -> std::cmp::Ordering {
    rate_b
        .total_cmp(&rate_a)
        .then(attempts_b.cmp(&attempts_a))
        .then(name_a.cmp(name_b))
}

pub fn habit(days: &[DayStat], today: &str, target_minutes: u32) -> Habit {
    let today_ms: u64 = days
        .iter()
        .filter(|d| d.date == today)
        .map(|d| d.active_ms)
        .sum();
    let total_ms: u64 = days.iter().map(|d| d.active_ms).sum();
    Habit {
        streak_days: streak(days, today),
        today_minutes: today_ms as f64 / 60_000.0,
        target_minutes,
        total_hours: total_ms as f64 / 3_600_000.0,
        sessions: days.iter().map(|d| d.sessions).sum(),
    }
}

/// Consecutive practised days ending today or yesterday.
pub fn streak(days: &[DayStat], today: &str) -> u32 {
    let practised: HashSet<i64> = days
        .iter()
        .filter(|d| d.sessions > 0)
        .filter_map(|d| day_number(&d.date))
        .collect();
    let Some(today) = day_number(today) else {
        return 0;
    };
    let mut day = if practised.contains(&today) {
        today
    } else if practised.contains(&(today - 1)) {
        today - 1
    } else {
        return 0;
    };
    let mut count = 0;
    while practised.contains(&day) {
        count += 1;
        day -= 1;
    }
    count
}

impl Snapshot {
    #[cfg(test)]
    pub fn empty(today: &str) -> Self {
        Self {
            today: today.to_string(),
            days: Vec::new(),
            lessons: Vec::new(),
            keys: Vec::new(),
            bigrams: Vec::new(),
            habit: habit(&[], today, 0),
        }
    }
}

/// The inverse of `day_number`.
pub fn date_string(days: i64) -> String {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month = (5 * day_of_year + 2) / 153;
    let d = day_of_year - (153 * month + 2) / 5 + 1;
    let m = if month < 10 { month + 3 } else { month - 9 };
    let y = year_of_era + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Days since 1970-01-01 for a `YYYY-MM-DD` string, so dates can be compared and subtracted.
pub fn day_number(date: &str) -> Option<i64> {
    let mut parts = date.split('-').map(|part| part.parse::<i64>().ok());
    let (Some(Some(y)), Some(Some(m)), Some(Some(d)), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Howard Hinnant's days_from_civil: years start in March so leap days come last.
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let year_of_era = y - era * 400;
    let month = (m + 9) % 12;
    let day_of_year = (153 * month + 2) / 5 + d - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    Some(era * 146_097 + day_of_era - 719_468)
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

    fn day(date: &str, sessions: u32, active_ms: u64) -> DayStat {
        DayStat::new(date.into(), sessions, 100, 2, active_ms)
    }

    #[test]
    fn day_numbers_are_consecutive_across_month_and_year_ends() {
        assert_eq!(day_number("1970-01-01"), Some(0));
        assert_eq!(
            day_number("2026-03-01").unwrap() - day_number("2026-02-28").unwrap(),
            1
        );
        assert_eq!(
            day_number("2027-01-01").unwrap() - day_number("2026-12-31").unwrap(),
            1
        );
        assert_eq!(
            day_number("2024-03-01").unwrap() - day_number("2024-02-28").unwrap(),
            2,
            "leap day"
        );
        assert_eq!(day_number("garbage"), None);
        for date in [
            "1970-01-01",
            "2024-02-29",
            "2026-09-04",
            "2026-12-31",
            "2100-03-01",
        ] {
            assert_eq!(date_string(day_number(date).unwrap()), date);
        }
    }

    #[test]
    fn streak_counts_back_from_today_or_yesterday() {
        let days = [
            day("2026-09-01", 1, 0),
            day("2026-09-02", 1, 0),
            day("2026-09-03", 2, 0),
        ];
        assert_eq!(streak(&days, "2026-09-03"), 3);
        assert_eq!(
            streak(&days, "2026-09-04"),
            3,
            "yesterday keeps the streak alive"
        );
        assert_eq!(streak(&days, "2026-09-05"), 0);
        assert_eq!(
            streak(
                &[day("2026-09-01", 1, 0), day("2026-09-03", 1, 0)],
                "2026-09-03"
            ),
            1
        );
        assert_eq!(streak(&[], "2026-09-03"), 0);
    }

    #[test]
    fn habit_sums_today_and_total() {
        let days = [day("2026-09-03", 3, 600_000), day("2026-09-04", 2, 480_000)];
        let habit = habit(&days, "2026-09-04", 15);
        assert_eq!(habit.streak_days, 2);
        assert!((habit.today_minutes - 8.0).abs() < 1e-9);
        assert_eq!(habit.target_minutes, 15);
        assert!((habit.total_hours - 0.3).abs() < 1e-9);
        assert_eq!(habit.sessions, 5);
    }

    fn strokes_for(sequence: &[(&str, KeystrokeKind, u64)]) -> Vec<Stroke> {
        let mut out = Vec::new();
        let mut prev: Option<(&str, KeystrokeKind, u64)> = None;
        for &(expected, kind, at) in sequence {
            out.push(Stroke {
                expected: expected.into(),
                kind,
                prev_expected: prev.map(|p| p.0.into()),
                prev_kind: prev.map(|p| p.1),
                interval_ms: prev.map(|p| at - p.2),
            });
            prev = Some((expected, kind, at));
        }
        out
    }

    #[test]
    fn key_aggregation_counts_attempts_errors_and_clean_medians() {
        use KeystrokeKind::*;
        let strokes = strokes_for(&[
            ("e", Correct, 0),
            ("n", Wrong, 300),
            ("n", Backspace, 500),
            ("n", Correct, 700),
            ("e", Correct, 900),
            ("n", Correct, 1300),
            ("e", Correct, 1500),
        ]);
        let keys = aggregate_keys(&strokes);
        let n = keys.iter().find(|k| k.key == "n").unwrap();
        assert_eq!((n.attempts, n.errors), (3, 1));
        assert!((n.error_rate - 1.0 / 3.0).abs() < 1e-9);
        assert_eq!(
            n.median_ms,
            Some(400),
            "only the correct stroke after a correct one counts"
        );
        let e = keys.iter().find(|k| k.key == "e").unwrap();
        assert_eq!((e.attempts, e.errors, e.median_ms), (3, 0, Some(200)));
        assert_eq!(keys[0].key, "n", "worst first");
    }

    #[test]
    fn bigram_aggregation_only_counts_clean_transitions() {
        use KeystrokeKind::*;
        let strokes = strokes_for(&[
            ("e", Correct, 0),
            ("n", Wrong, 300),
            ("n", Backspace, 500),
            ("n", Correct, 700),
            ("e", Correct, 900),
            ("n", Correct, 1300),
        ]);
        let bigrams = aggregate_bigrams(&strokes);
        let en = bigrams.iter().find(|b| b.bigram == "en").unwrap();
        assert_eq!((en.attempts, en.errors, en.median_ms), (2, 1, Some(400)));
        let ne = bigrams.iter().find(|b| b.bigram == "ne").unwrap();
        assert_eq!((ne.attempts, ne.errors, ne.median_ms), (1, 0, Some(200)));
        assert!(
            bigrams.iter().all(|b| b.bigram != "nn"),
            "after a backspace nothing is a transition"
        );
    }
}
