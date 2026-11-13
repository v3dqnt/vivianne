//! Time in a query: "what did we decide last spring", "in March 2025", "three weeks ago".
//!
//! The idea is Hindsight's (vectorize-io/hindsight, MIT): read a time expression in the question as a window, retrieve what happened in it as
//! its own list, and let a fact that fits the window outrank one that does not. Here the reading is rules only (no LLM call), in UTC, and
//! anything the rules cannot read is simply not a time question: recall behaves exactly as before.
//!
//! "When" for a fact is when it happened (`occurred_at`, from extraction) if known, otherwise when it was learned (`created_at`).

use chrono::{Datelike, Duration, NaiveDate};

/// A span of time, in epoch ms: `start` inclusive, `end` exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    pub start: u64,
    pub end: u64,
}

/// How much a fact inside the window gains: the score is multiplied by `1 + BOOST`. Bounded, so time can reorder close calls but never
/// lift something unrelated over a clearly better match.
pub const BOOST: f32 = 0.25;

const MONTHS: [&str; 12] = ["january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november", "december"];

fn ms(d: NaiveDate) -> u64 {
    u64::try_from(d.and_hms_opt(0, 0, 0).map_or(0, |t| t.and_utc().timestamp_millis())).unwrap_or(0)
}

fn span(a: NaiveDate, b: NaiveDate) -> Option<Window> {
    (a < b).then(|| Window { start: ms(a), end: ms(b) })
}

fn ymd(y: i32, m: u32, d: u32) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(y, m, d)
}

fn month_start(y: i32, m: u32) -> Option<NaiveDate> {
    ymd(y, m, 1)
}

/// First day of the month after `(y, m)`.
fn next_month(y: i32, m: u32) -> Option<NaiveDate> {
    if m == 12 { ymd(y + 1, 1, 1) } else { ymd(y, m + 1, 1) }
}

fn month_window(y: i32, m: u32) -> Option<Window> {
    span(month_start(y, m)?, next_month(y, m)?)
}

fn year_window(y: i32) -> Option<Window> {
    span(ymd(y, 1, 1)?, ymd(y + 1, 1, 1)?)
}

fn number(word: &str) -> Option<i64> {
    const WORDS: [&str; 13] = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve"];
    word.parse().ok().or_else(|| WORDS.iter().position(|w| *w == word).map(|i| i as i64)).filter(|n| (1..=1000).contains(n))
}

/// The window a query talks about, read relative to `today`, or `None` when it names no time.
pub fn window_of(query: &str, today: NaiveDate) -> Option<Window> {
    let q: String = query.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { ' ' }).collect();
    let w: Vec<&str> = q.split_whitespace().collect();
    let has = |s: &str| w.contains(&s);
    let (y, m) = (today.year(), today.month());

    // "3 weeks ago", "two days ago", "a month ago"
    for (i, word) in w.iter().enumerate() {
        if w.get(i + 1).copied() != Some("ago") && w.get(i + 2).copied() != Some("ago") {
            continue;
        }
        let unit = w.get(i + 1).copied().unwrap_or("");
        let n = if *word == "a" || *word == "an" { Some(1) } else { number(word) };
        if let (Some(n), true) = (n, matches!(unit, "day" | "days" | "week" | "weeks" | "month" | "months" | "year" | "years")) {
            return ago(unit, n, today);
        }
    }
    // "last 10 days", "past two weeks"
    for (i, word) in w.iter().enumerate() {
        if matches!(*word, "last" | "past") {
            if let (Some(n), Some(unit)) = (w.get(i + 1).and_then(|x| number(x)), w.get(i + 2)) {
                if matches!(*unit, "days" | "weeks" | "months") {
                    return rolling(unit, n, today);
                }
            }
        }
    }
    if has("today") {
        return span(today, today + Duration::days(1));
    }
    if has("yesterday") {
        return span(today - Duration::days(1), today);
    }
    for (i, word) in w.iter().enumerate() {
        let Some(which) = i.checked_sub(1).map(|j| w[j]).filter(|x| matches!(*x, "last" | "this")) else { continue };
        let back = which == "last";
        match *word {
            "week" => {
                let monday = today - Duration::days(today.weekday().num_days_from_monday() as i64);
                let start = if back { monday - Duration::days(7) } else { monday };
                return span(start, start + Duration::days(7));
            }
            "month" => {
                let (yy, mm) = if back { if m == 1 { (y - 1, 12) } else { (y, m - 1) } } else { (y, m) };
                return month_window(yy, mm);
            }
            "year" => return year_window(if back { y - 1 } else { y }),
            "spring" | "summer" | "autumn" | "fall" | "winter" => return season(word, back, today),
            name if MONTHS.contains(&name) => {
                let mi = MONTHS.iter().position(|n| *n == name)? as u32 + 1;
                let yy = if back { if mi >= m { y - 1 } else { y } } else { y };
                return month_window(yy, mi);
            }
            _ => {}
        }
    }
    // "march 2025", "in 2024", "march of 2025"
    for (i, word) in w.iter().enumerate() {
        let Some(year) = word.parse::<i32>().ok().filter(|n| (1990..=2100).contains(n)) else { continue };
        if i > 0 {
            let prev = w[i - 1];
            let prev2 = if i > 1 && prev == "of" { w[i - 2] } else { prev };
            if let Some(mi) = MONTHS.iter().position(|n| *n == prev2) {
                return month_window(year, mi as u32 + 1);
            }
        }
        return year_window(year);
    }
    // A bare month name: the latest one that has begun ("in march" asked in June is this March; asked in February, last year's).
    for word in &w {
        if let Some(mi) = MONTHS.iter().position(|n| n == word) {
            // "may" is also a verb; a bare "may" is not read as a month.
            if *word == "may" {
                continue;
            }
            let mi = mi as u32 + 1;
            return month_window(if mi > m { y - 1 } else { y }, mi);
        }
    }
    None
}

fn ago(unit: &str, n: i64, today: NaiveDate) -> Option<Window> {
    match unit {
        "day" | "days" => span(today - Duration::days(n), today - Duration::days(n - 1)),
        // A week or a month ago is a fuzzy point, not a day: a window of the same unit around it.
        "week" | "weeks" => span(today - Duration::days(7 * n + 3), today - Duration::days(7 * n - 4)),
        "month" | "months" => {
            let total = today.year() as i64 * 12 + today.month0() as i64 - n;
            month_window((total.div_euclid(12)) as i32, total.rem_euclid(12) as u32 + 1)
        }
        _ => year_window(today.year() - n as i32),
    }
}

fn rolling(unit: &str, n: i64, today: NaiveDate) -> Option<Window> {
    let days = match unit {
        "days" => n,
        "weeks" => 7 * n,
        _ => 30 * n,
    };
    span(today - Duration::days(days), today + Duration::days(1))
}

/// Meteorological seasons, northern hemisphere. "This" is the one under way or most recently begun; "last" is the one before it, or that one
/// when it has already ended.
fn season(name: &str, back: bool, today: NaiveDate) -> Option<Window> {
    let start_month = match name {
        "spring" => 3,
        "summer" => 6,
        "autumn" | "fall" => 9,
        _ => 12,
    };
    // The season that begins in `year`; winter runs from December into March of the next year.
    let build = |year: i32| span(month_start(year, start_month)?, if start_month == 12 { month_start(year + 1, 3)? } else { month_start(year, start_month + 3)? });
    let now = ms(today);
    let (start_year, current) = {
        let this = build(today.year())?;
        if this.start <= now { (today.year(), this) } else { (today.year() - 1, build(today.year() - 1)?) }
    };
    if !back {
        return Some(current);
    }
    if current.end > now { build(start_year - 1) } else { Some(current) }
}

/// How well a fact's time fits a window: 1 inside, falling to 0 within one window's length outside.
pub fn fit(at_ms: u64, w: Window) -> f32 {
    if at_ms >= w.start && at_ms < w.end {
        return 1.0;
    }
    let len = (w.end - w.start).max(1) as f64;
    let gap = if at_ms < w.start { w.start - at_ms } else { at_ms - w.end } as f64;
    (1.0 - gap / len).max(0.0) as f32
}

/// The factor a fact's score is multiplied by: `1 + BOOST·(fit − ½)`, so between `1 − BOOST/2` (far from the window) and `1 + BOOST/2` (inside it).
pub fn multiplier(at_ms: u64, w: Window) -> f32 {
    1.0 + BOOST * (fit(at_ms, w) - 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }
    fn range(w: Option<Window>) -> Option<(NaiveDate, NaiveDate)> {
        w.map(|w| {
            let f = |t: u64| chrono::DateTime::from_timestamp_millis(t as i64).unwrap().date_naive();
            (f(w.start), f(w.end))
        })
    }
    /// Thursday 2026-11-12.
    fn today() -> NaiveDate {
        d(2026, 11, 12)
    }

    #[test]
    fn days_weeks_months_and_years_relative_to_today() {
        assert_eq!(range(window_of("what did we do yesterday", today())), Some((d(2026, 11, 11), d(2026, 11, 12))));
        assert_eq!(range(window_of("today's plan", today())), Some((d(2026, 11, 12), d(2026, 11, 13))));
        assert_eq!(range(window_of("last week", today())), Some((d(2026, 11, 2), d(2026, 11, 9))), "weeks run Monday to Sunday");
        assert_eq!(range(window_of("this week", today())), Some((d(2026, 11, 9), d(2026, 11, 16))));
        assert_eq!(range(window_of("last month", today())), Some((d(2026, 10, 1), d(2026, 11, 1))));
        assert_eq!(range(window_of("this year", today())), Some((d(2026, 1, 1), d(2027, 1, 1))));
        assert_eq!(range(window_of("last year", d(2026, 1, 5))), Some((d(2025, 1, 1), d(2026, 1, 1))));
        assert_eq!(range(window_of("last month", d(2026, 1, 5))), Some((d(2025, 12, 1), d(2026, 1, 1))), "across a year boundary");
    }

    #[test]
    fn counted_expressions() {
        assert_eq!(range(window_of("three days ago", today())), Some((d(2026, 11, 9), d(2026, 11, 10))));
        assert_eq!(range(window_of("a month ago", today())), Some((d(2026, 10, 1), d(2026, 11, 1))));
        assert_eq!(range(window_of("2 months ago", today())), Some((d(2026, 9, 1), d(2026, 10, 1))));
        assert_eq!(range(window_of("14 months ago", d(2026, 3, 1))), Some((d(2025, 1, 1), d(2025, 2, 1))));
        assert_eq!(range(window_of("two years ago", today())), Some((d(2024, 1, 1), d(2025, 1, 1))));
        assert_eq!(range(window_of("a week ago", today())), Some((d(2026, 11, 2), d(2026, 11, 9))), "a week ago is a week around that day");
        assert_eq!(range(window_of("last 10 days", today())), Some((d(2026, 11, 2), d(2026, 11, 13))));
        assert_eq!(range(window_of("past two weeks", today())), Some((d(2026, 10, 29), d(2026, 11, 13))));
    }

    #[test]
    fn named_months_and_years() {
        assert_eq!(range(window_of("in 2024", today())), Some((d(2024, 1, 1), d(2025, 1, 1))));
        assert_eq!(range(window_of("March 2025", today())), Some((d(2025, 3, 1), d(2025, 4, 1))));
        assert_eq!(range(window_of("in march of 2025", today())), Some((d(2025, 3, 1), d(2025, 4, 1))));
        assert_eq!(range(window_of("what happened in March", today())), Some((d(2026, 3, 1), d(2026, 4, 1))), "the latest March that has begun");
        assert_eq!(range(window_of("in december", today())), Some((d(2025, 12, 1), d(2026, 1, 1))), "December has not come yet this year");
        assert_eq!(range(window_of("last march", today())), Some((d(2026, 3, 1), d(2026, 4, 1))));
        assert_eq!(range(window_of("last november", today())), Some((d(2025, 11, 1), d(2025, 12, 1))), "last means before this one");
    }

    #[test]
    fn seasons() {
        assert_eq!(range(window_of("last spring", today())), Some((d(2026, 3, 1), d(2026, 6, 1))));
        assert_eq!(range(window_of("last summer", today())), Some((d(2026, 6, 1), d(2026, 9, 1))));
        assert_eq!(range(window_of("last autumn", today())), Some((d(2025, 9, 1), d(2025, 12, 1))), "this autumn is still under way");
        assert_eq!(range(window_of("this autumn", today())), Some((d(2026, 9, 1), d(2026, 12, 1))));
        assert_eq!(range(window_of("last winter", today())), Some((d(2025, 12, 1), d(2026, 3, 1))));
    }

    #[test]
    fn what_names_no_time_is_not_a_time_question() {
        for q in ["how do I deploy the app", "may I ask about the build", "what is the plan for 5000 users", "remember the 1234 port", "the last time we spoke", "this is fine"] {
            assert_eq!(window_of(q, today()), None, "{q}");
        }
    }

    #[test]
    fn fit_is_one_inside_and_fades_outside() {
        let w = Window { start: 1_000, end: 2_000 };
        assert_eq!(fit(1_000, w), 1.0);
        assert_eq!(fit(1_999, w), 1.0);
        assert_eq!(fit(2_000, w), 1.0, "the end is the first moment outside, at no distance");
        assert!(fit(2_500, w) > 0.4 && fit(2_500, w) < 0.6);
        assert_eq!(fit(9_000, w), 0.0);
        assert!(fit(0, w) < 1.0);
    }

    #[test]
    fn the_boost_is_bounded_and_centred() {
        let w = Window { start: 1_000, end: 2_000 };
        assert!((multiplier(1_500, w) - 1.125).abs() < 1e-6);
        assert!((multiplier(9_000, w) - 0.875).abs() < 1e-6);
        assert!((0.875..=1.125).contains(&multiplier(2_300, w)));
    }
}
