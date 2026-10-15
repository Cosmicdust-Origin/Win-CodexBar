//! Year of per-day token totals for the Usage & Spend token activity grid
//! (upstream `SpendDashboardModel.tokenActivity`).
//!
//! Each day keeps two facts apart: whether the scan reached it at all
//! (`is_scanned`) and whether its total is known (`total_tokens`). A scanned
//! day without a total is a real gap, never an implied zero.

use chrono::{Duration, NaiveDate, Utc};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;

use super::claude_incomplete::ClaudeIncompleteTracker;
use super::claude_pricing::ClaudeScanPricingResolver;
use super::today::day_key;
use super::{
    ClaudeFileScanResult, CostScanner, CostSummary, add_claude_record_to_daily_tokens,
    add_codex_days_map_to_summary, scan_claude_file_with_pricing,
};
use crate::core::CostUsageDayRange;
use crate::cost_reporting_period::cost_bucket_zone;

/// Days the annual grid covers, today included.
pub const TOKEN_ACTIVITY_DAYS: u32 = 365;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenActivityDay {
    pub day: String,
    /// `None` is unknown: the day was not scanned, or it was scanned and
    /// still cannot be totalled.
    pub total_tokens: Option<u64>,
    pub is_scanned: bool,
}

/// Per-day token activity for one local-log provider, oldest day first.
/// Providers without a supported local history get no days.
pub fn daily_token_activity(provider: &str, days: u32) -> Vec<TokenActivityDay> {
    let today = cost_bucket_zone().date(Utc::now());
    let keys = window_keys(today, days);
    match provider {
        "codex" => codex_token_activity(days, &keys),
        "claude" => claude_token_activity(days, &keys),
        _ => Vec::new(),
    }
}

fn window_keys(today: NaiveDate, days: u32) -> Vec<String> {
    (0..i64::from(days))
        .rev()
        .map(|days_ago| day_key(today - Duration::days(days_ago)))
        .collect()
}

fn codex_token_activity(days: u32, keys: &[String]) -> Vec<TokenActivityDay> {
    let (_summary, _stats, cache) = CostScanner::new(days).scan_codex_detailed_with_cache(None);
    let mut totals = HashMap::new();
    for (key, models) in &cache.days {
        let Some(day) = CostUsageDayRange::parse_day_key(key) else {
            continue;
        };
        let mut one_day = HashMap::new();
        one_day.insert(key.clone(), models.clone());
        let mut scratch = CostSummary::default();
        add_codex_days_map_to_summary(&mut scratch, &one_day, &CostUsageDayRange::new(day, day));
        totals.insert(key.clone(), scratch.total_tokens_for_provider("codex"));
    }
    // A stale or still-catching-up cache proves nothing about the days it
    // has not reached, so it covers no day at all.
    let established = cache.previous_report.is_none() && !cache.codex_scan_incomplete;
    let coverage = established
        .then(|| {
            cache
                .scan_since_key
                .clone()
                .zip(cache.scan_until_key.clone())
        })
        .flatten();
    codex_days(keys, &totals, coverage.as_ref())
}

fn codex_days(
    keys: &[String],
    totals: &HashMap<String, u64>,
    coverage: Option<&(String, String)>,
) -> Vec<TokenActivityDay> {
    keys.iter()
        .map(|key| {
            let scanned = coverage.is_some_and(|(since, until)| key >= since && key <= until);
            TokenActivityDay {
                day: key.clone(),
                total_tokens: scanned.then(|| totals.get(key).copied().unwrap_or(0)),
                is_scanned: scanned,
            }
        })
        .collect()
}

fn claude_token_activity(days: u32, keys: &[String]) -> Vec<TokenActivityDay> {
    let scanner = CostScanner::new(days);
    let roots = scanner.claude_projects_roots();
    if roots.paths.is_empty() {
        return claude_days(keys, &HashMap::new(), &HashSet::new(), false);
    }
    let cutoff = Utc::now() - Duration::days(i64::from(days));
    let mut totals: HashMap<String, u64> = keys.iter().map(|key| (key.clone(), 0)).collect();
    let mut seen = HashSet::new();
    let mut pricing = ClaudeScanPricingResolver::default();
    let mut incomplete = ClaudeIncompleteTracker::default();
    let mut scan = ClaudeFileScanResult::default();
    let traversal_read_failures = {
        let mut handle_file = |path: &Path| {
            let mut aggregation_failures = 0u32;
            let mut result = scan_claude_file_with_pricing(
                path,
                &cutoff,
                &mut seen,
                None,
                &mut pricing,
                &mut incomplete,
                |record| {
                    if record.timestamp.is_none()
                        || !add_claude_record_to_daily_tokens(&mut totals, record)
                    {
                        aggregation_failures = aggregation_failures.saturating_add(1);
                    }
                },
            );
            result.aggregation_failures = result
                .aggregation_failures
                .saturating_add(aggregation_failures);
            scan.absorb(result);
        };
        scanner.walk_claude_roots(&roots.paths, &cutoff, None, &mut handle_file)
    };
    scan.read_failures = scan
        .read_failures
        .saturating_add(roots.read_failures)
        .saturating_add(traversal_read_failures);
    // Preliminary rows only invalidate the days they fall on, so they are
    // judged per day below instead of failing the whole walk.
    scan.incomplete_requests = 0;
    let incomplete_days: HashSet<String> = incomplete.resolve(&seen).by_day.into_keys().collect();
    claude_days(keys, &totals, &incomplete_days, scan.is_complete())
}

fn claude_days(
    keys: &[String],
    totals: &HashMap<String, u64>,
    incomplete_days: &HashSet<String>,
    complete: bool,
) -> Vec<TokenActivityDay> {
    keys.iter()
        .map(|key| TokenActivityDay {
            day: key.clone(),
            total_tokens: (complete && !incomplete_days.contains(key))
                .then(|| totals.get(key).copied().unwrap_or(0)),
            is_scanned: complete,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).unwrap()
    }

    #[test]
    fn window_keys_run_oldest_first_and_end_today() {
        let keys = window_keys(date(2024, 3, 1), 365);
        assert_eq!(keys.len(), 365);
        assert_eq!(keys.last().map(String::as_str), Some("2024-03-01"));
        // 2024 is a leap year: 364 days before 1 March is 3 March 2023.
        assert_eq!(keys.first().map(String::as_str), Some("2023-03-03"));
        assert!(keys.contains(&"2024-02-29".to_string()));
    }

    #[test]
    fn codex_days_outside_the_scan_window_are_unscanned_and_unknown() {
        let keys = window_keys(date(2026, 1, 5), 5);
        let totals = HashMap::from([("2026-01-04".to_string(), 42u64)]);
        let coverage = ("2026-01-03".to_string(), "2026-01-05".to_string());
        let days = codex_days(&keys, &totals, Some(&coverage));
        let summary: Vec<_> = days
            .iter()
            .map(|day| (day.day.as_str(), day.total_tokens, day.is_scanned))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("2026-01-01", None, false),
                ("2026-01-02", None, false),
                ("2026-01-03", Some(0), true),
                ("2026-01-04", Some(42), true),
                ("2026-01-05", Some(0), true),
            ]
        );
    }

    #[test]
    fn codex_without_established_coverage_reports_no_known_day() {
        let keys = window_keys(date(2026, 1, 5), 3);
        let totals = HashMap::from([("2026-01-04".to_string(), 42u64)]);
        let days = codex_days(&keys, &totals, None);
        assert!(
            days.iter()
                .all(|day| day.total_tokens.is_none() && !day.is_scanned)
        );
    }

    #[test]
    fn claude_incomplete_days_stay_scanned_but_unknown() {
        let keys = window_keys(date(2026, 1, 3), 3);
        let totals = HashMap::from([
            ("2026-01-01".to_string(), 10u64),
            ("2026-01-02".to_string(), 20u64),
        ]);
        let incomplete = HashSet::from(["2026-01-02".to_string()]);
        let days = claude_days(&keys, &totals, &incomplete, true);
        let summary: Vec<_> = days
            .iter()
            .map(|day| (day.total_tokens, day.is_scanned))
            .collect();
        assert_eq!(
            summary,
            vec![(Some(10), true), (None, true), (Some(0), true)]
        );
    }

    #[test]
    fn claude_failed_walk_scans_no_day() {
        let keys = window_keys(date(2026, 1, 3), 3);
        let days = claude_days(&keys, &HashMap::new(), &HashSet::new(), false);
        assert!(
            days.iter()
                .all(|day| day.total_tokens.is_none() && !day.is_scanned)
        );
    }

    #[test]
    fn serializes_camel_case_with_explicit_null() {
        let json = serde_json::to_value(TokenActivityDay {
            day: "2026-01-01".to_string(),
            total_tokens: None,
            is_scanned: true,
        })
        .unwrap();
        assert_eq!(
            json,
            serde_json::json!({"day": "2026-01-01", "totalTokens": null, "isScanned": true})
        );
    }
}
