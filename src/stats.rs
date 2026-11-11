//! Lifetime insights. Kept apart from the history so that clearing history (or a
//! short "keep history" setting) doesn't wipe the numbers on Home.

use chrono::{Datelike, Duration, Local, NaiveDate, TimeZone};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Stats {
    pub words: u64,
    pub seconds: f64,
    pub sessions: u64,
    /// "YYYY-MM-DD" -> number of dictations that day.
    pub days: BTreeMap<String, u32>,
}

impl Stats {
    pub fn load() -> Self {
        std::fs::read_to_string(crate::settings::data_dir().join("stats.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Ok(s) = serde_json::to_string(self) {
            let _ = std::fs::write(crate::settings::data_dir().join("stats.json"), s);
        }
    }

    pub fn record(&mut self, when: u64, words: usize, seconds: f32) {
        self.words += words as u64;
        self.seconds += seconds as f64;
        self.sessions += 1;
        *self.days.entry(day_key(local_date(when))).or_default() += 1;
        self.save();
    }

    pub fn wpm(&self) -> f64 {
        if self.seconds < 1.0 { 0.0 } else { self.words as f64 / (self.seconds / 60.0) }
    }

    /// Minutes saved compared with typing at `typing_wpm`.
    pub fn minutes_saved(&self, typing_wpm: f32) -> f64 {
        let typing = self.words as f64 / typing_wpm.max(1.0) as f64;
        (typing - self.seconds / 60.0).max(0.0)
    }

    pub fn active_days(&self) -> usize {
        self.days.len()
    }

    pub fn count_on(&self, d: NaiveDate) -> u32 {
        self.days.get(&day_key(d)).copied().unwrap_or(0)
    }

    pub fn first_day(&self) -> Option<NaiveDate> {
        self.days.keys().next().and_then(|k| NaiveDate::parse_from_str(k, "%Y-%m-%d").ok())
    }

    pub fn current_streak(&self) -> usize {
        let today = Local::now().date_naive();
        let mut d = if self.count_on(today) > 0 { today } else { today - Duration::days(1) };
        let mut n = 0;
        while self.count_on(d) > 0 {
            n += 1;
            d -= Duration::days(1);
        }
        n
    }

    pub fn longest_streak(&self) -> usize {
        let mut best = 0;
        let mut run = 0;
        let mut prev: Option<NaiveDate> = None;
        for k in self.days.keys() {
            let Ok(d) = NaiveDate::parse_from_str(k, "%Y-%m-%d") else { continue };
            run = match prev {
                Some(p) if d - p == Duration::days(1) => run + 1,
                _ => 1,
            };
            best = best.max(run);
            prev = Some(d);
        }
        best
    }

    pub fn busiest_day(&self) -> u32 {
        self.days.values().copied().max().unwrap_or(0)
    }
}

pub fn local_date(secs: u64) -> NaiveDate {
    Local
        .timestamp_opt(secs as i64, 0)
        .single()
        .map(|t| t.date_naive())
        .unwrap_or_else(|| Local::now().date_naive())
}

/// "Mon 10 Nov 2026, 15:48" - the exact moment something happened.
///
/// Spelled out, with the year, because this is the record of when: a list
/// that says only "Monday" is no help a fortnight later, which is what the
/// meeting titles used to say.
pub fn stamp(secs: u64) -> String {
    Local
        .timestamp_opt(secs as i64, 0)
        .single()
        .map(|t| t.format("%a %-d %b %Y, %H:%M").to_string())
        .unwrap_or_default()
}

pub fn day_key(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

pub fn clock(secs: u64) -> String {
    Local
        .timestamp_opt(secs as i64, 0)
        .single()
        .map(|t| t.format("%I:%M %p").to_string())
        .unwrap_or_default()
}

pub fn day_heading(d: NaiveDate) -> String {
    let today = Local::now().date_naive();
    if d == today {
        "Today".into()
    } else if d == today - Duration::days(1) {
        "Yesterday".into()
    } else {
        format!("{} {}, {}", d.format("%B"), d.day(), d.year())
    }
}
