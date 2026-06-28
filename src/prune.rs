//! Retention planning: which snapshots to keep and which to delete.
//!
//! GFS style ("grandfather-father-son"): keep the N most recent, plus the
//! newest per day/week/month for N periods. The newest snapshot is always kept.
//! The logic is pure (no I/O) and is tested in `tests` below.

use crate::config::Retention;
use chrono::{Datelike, NaiveDateTime};
use std::collections::HashSet;

/// The format that snapshot directories are named with (see [`crate::snapshot::timestamp`]).
const FMT: &str = "%Y-%m-%dT%H-%M-%S";

/// The result of a planning run.
pub struct Plan {
    pub keep: Vec<String>,
    pub delete: Vec<String>,
}

/// Decides which timestamps to keep and which to delete according to the policy.
/// Preserves the input order in the output lists. Timestamps that cannot be
/// parsed are always kept (safer not to delete what we don't understand).
pub fn plan(timestamps: &[String], policy: &Retention) -> Plan {
    if policy.is_empty() {
        return Plan {
            keep: timestamps.to_vec(),
            delete: Vec::new(),
        };
    }

    let mut keep: HashSet<String> = HashSet::new();

    // Parse and sort valid timestamps newest first.
    let mut valid: Vec<(String, NaiveDateTime)> = Vec::new();
    for ts in timestamps {
        match NaiveDateTime::parse_from_str(ts, FMT) {
            Ok(dt) => valid.push((ts.clone(), dt)),
            Err(_) => {
                keep.insert(ts.clone()); // unparseable → keep
            }
        }
    }
    valid.sort_by_key(|b| std::cmp::Reverse(b.1));

    // Always keep the newest (protects, among other things, the `latest` symlink).
    if let Some((ts, _)) = valid.first() {
        keep.insert(ts.clone());
    }

    keep_last(&valid, policy.keep_last as usize, &mut keep);
    keep_tier(&valid, policy.keep_daily as usize, day_key, &mut keep);
    keep_tier(&valid, policy.keep_weekly as usize, week_key, &mut keep);
    keep_tier(&valid, policy.keep_monthly as usize, month_key, &mut keep);

    Plan {
        keep: timestamps
            .iter()
            .filter(|t| keep.contains(*t))
            .cloned()
            .collect(),
        delete: timestamps
            .iter()
            .filter(|t| !keep.contains(*t))
            .cloned()
            .collect(),
    }
}

fn keep_last(valid: &[(String, NaiveDateTime)], n: usize, keep: &mut HashSet<String>) {
    for (ts, _) in valid.iter().take(n) {
        keep.insert(ts.clone());
    }
}

/// Keep the newest snapshot per period (day/week/month), for `limit` periods.
fn keep_tier(
    valid: &[(String, NaiveDateTime)],
    limit: usize,
    key_fn: fn(&NaiveDateTime) -> String,
    keep: &mut HashSet<String>,
) {
    if limit == 0 {
        return;
    }
    let mut seen: HashSet<String> = HashSet::new();
    for (ts, dt) in valid {
        let k = key_fn(dt);
        if seen.contains(&k) {
            continue; // already kept the newest for that period
        }
        if seen.len() >= limit {
            break; // all periods filled
        }
        seen.insert(k);
        keep.insert(ts.clone());
    }
}

fn day_key(d: &NaiveDateTime) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn week_key(d: &NaiveDateTime) -> String {
    let w = d.iso_week();
    format!("{}-W{:02}", w.year(), w.week())
}

fn month_key(d: &NaiveDateTime) -> String {
    d.format("%Y-%m").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ret(last: u32, daily: u32, weekly: u32, monthly: u32) -> Retention {
        Retention {
            keep_last: last,
            keep_daily: daily,
            keep_weekly: weekly,
            keep_monthly: monthly,
        }
    }

    /// Helper: a timestamp for a date at 12:00.
    fn ts(date: &str) -> String {
        format!("{date}T12-00-00")
    }

    #[test]
    fn empty_policy_keeps_everything() {
        let snaps = vec![ts("2026-01-01"), ts("2026-01-02")];
        let p = plan(&snaps, &ret(0, 0, 0, 0));
        assert_eq!(p.delete.len(), 0);
        assert_eq!(p.keep.len(), 2);
    }

    #[test]
    fn keep_last_n_only() {
        let snaps = vec![
            ts("2026-01-01"),
            ts("2026-01-02"),
            ts("2026-01-03"),
            ts("2026-01-04"),
        ];
        let p = plan(&snaps, &ret(2, 0, 0, 0));
        // keeps the two newest
        assert_eq!(p.keep, vec![ts("2026-01-03"), ts("2026-01-04")]);
        assert_eq!(p.delete, vec![ts("2026-01-01"), ts("2026-01-02")]);
    }

    #[test]
    fn daily_keeps_newest_per_day() {
        // two snapshots on the same day + one more day
        let snaps = vec![
            "2026-01-01T08-00-00".to_string(),
            "2026-01-01T20-00-00".to_string(), // newest on the 1st
            "2026-01-02T09-00-00".to_string(),
        ];
        let p = plan(&snaps, &ret(0, 2, 0, 0));
        assert!(p.keep.contains(&"2026-01-02T09-00-00".to_string()));
        assert!(p.keep.contains(&"2026-01-01T20-00-00".to_string()));
        // the earlier one on the same day is deleted
        assert_eq!(p.delete, vec!["2026-01-01T08-00-00".to_string()]);
    }

    #[test]
    fn always_keeps_newest_even_if_policy_smaller() {
        let snaps = vec![ts("2026-01-01"), ts("2026-06-01")];
        // keep_daily=1 → only the newest day; the newest is kept anyway
        let p = plan(&snaps, &ret(0, 1, 0, 0));
        assert!(p.keep.contains(&ts("2026-06-01")));
        assert_eq!(p.delete, vec![ts("2026-01-01")]);
    }

    #[test]
    fn unparseable_timestamps_are_kept() {
        let snaps = vec![
            "not-a-timestamp".to_string(),
            ts("2026-01-01"),
            ts("2026-01-02"),
        ];
        let p = plan(&snaps, &ret(1, 0, 0, 0));
        assert!(p.keep.contains(&"not-a-timestamp".to_string()));
    }

    #[test]
    fn monthly_keeps_one_per_month() {
        let snaps = vec![
            ts("2026-01-10"),
            ts("2026-01-20"),
            ts("2026-02-05"),
            ts("2026-03-05"),
        ];
        let p = plan(&snaps, &ret(0, 0, 0, 3));
        // newest per month: jan-20, feb-05, mar-05
        assert!(p.keep.contains(&ts("2026-01-20")));
        assert!(p.keep.contains(&ts("2026-02-05")));
        assert!(p.keep.contains(&ts("2026-03-05")));
        assert_eq!(p.delete, vec![ts("2026-01-10")]);
    }
}
