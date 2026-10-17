//! `/v1/usage` parsing and card presentation, ported from `clawrouter.js`.

use std::cmp::Ordering;

use chrono::{DateTime, TimeZone, Utc};
use serde_json::{Map, Value};

use crate::core::{
    CostSnapshot, ProviderDisplayDetail, ProviderError, ProviderFetchResult, RateWindow,
    UsageSnapshot,
};
use crate::locale::LocaleKey;

/// Upstream shows at most 20 routed-provider rows.
const MAX_PROVIDER_ROWS: usize = 20;
const USAGE_SECTION: &str = "Usage";
const PROVIDERS_SECTION: &str = "Routed providers";
const COST_PERIOD: &str = "This month";

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Budget {
    pub configured: bool,
    pub ledger: String,
    pub limit: Option<f64>,
    pub spent: Option<f64>,
    pub remaining: Option<f64>,
    pub resets_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Summary {
    pub requests: i64,
    pub successes: i64,
    pub errors: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub actual_cost: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct RoutedProvider {
    pub name: String,
    pub requests: i64,
    pub tokens: i64,
    pub cost: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct ClawRouterUsage {
    pub budget: Budget,
    pub summary: Summary,
    /// Sorted by cost, then requests (both descending), then name.
    pub providers: Vec<RoutedProvider>,
}

fn parse_failure(detail: &str) -> ProviderError {
    ProviderError::Parse(format!("Could not parse ClawRouter usage: {detail}"))
}

/// JavaScript `Number.isInteger` on a JSON number.
#[allow(
    clippy::cast_possible_truncation,
    reason = "the float is integral and checked to be within i64 range"
)]
fn integer(value: Option<&Value>, field: &str) -> Result<i64, ProviderError> {
    let invalid = || parse_failure(&format!("{field} must be an integer"));
    let number = value.and_then(Value::as_number).ok_or_else(invalid)?;
    if let Some(value) = number.as_i64() {
        return Ok(value);
    }
    match number.as_f64() {
        Some(value)
            if value.is_finite() && value.fract() == 0.0 && value.abs() < i64::MAX as f64 =>
        {
            Ok(value as i64)
        }
        _ => Err(invalid()),
    }
}

fn micros(value: Option<&Value>, field: &str) -> Result<f64, ProviderError> {
    integer(value, field).map(|micros| micros as f64 / 1_000_000.0)
}

fn optional_micros(value: Option<&Value>, field: &str) -> Result<Option<f64>, ProviderError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        value => micros(value, field).map(Some),
    }
}

/// First instant of the month after a `...YYYY-MM` window key, in UTC.
pub(super) fn monthly_reset(window_key: Option<&Value>) -> Option<DateTime<Utc>> {
    let key = window_key?.as_str()?.as_bytes();
    let tail = key.get(key.len().checked_sub(7)?..)?;
    let digits = |range: std::ops::Range<usize>| -> Option<u32> {
        let part = tail.get(range)?;
        part.iter()
            .all(u8::is_ascii_digit)
            .then(|| std::str::from_utf8(part).ok()?.parse().ok())?
    };
    if tail[4] != b'-' {
        return None;
    }
    let year = i32::try_from(digits(0..4)?).ok()?;
    let month = digits(5..7)?;
    if !(1..=12).contains(&month) {
        return None;
    }
    let (year, month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    Utc.with_ymd_and_hms(year, month, 1, 0, 0, 0).single()
}

fn object(value: Option<&Value>) -> Option<&Map<String, Value>> {
    value.and_then(Value::as_object)
}

pub(super) fn parse_usage(body: &[u8]) -> Result<ClawRouterUsage, ProviderError> {
    let payload: Value =
        serde_json::from_slice(body).map_err(|_| parse_failure("response was not valid JSON"))?;
    let shape = || parse_failure("response shape is invalid");
    let root = payload.as_object().ok_or_else(shape)?;
    let budget = object(root.get("budget")).ok_or_else(shape)?;
    let usage = object(root.get("usage")).ok_or_else(shape)?;
    let summary = object(usage.get("summary")).ok_or_else(shape)?;
    let providers = usage
        .get("providers")
        .and_then(Value::as_array)
        .ok_or_else(shape)?;

    let (Some(configured), Some(ledger)) = (
        budget.get("configured").and_then(Value::as_bool),
        budget.get("ledger").and_then(Value::as_str),
    ) else {
        return Err(parse_failure("budget is invalid"));
    };
    let budget = Budget {
        configured,
        ledger: ledger.to_owned(),
        limit: optional_micros(budget.get("limitMicros"), "budget.limitMicros")?,
        spent: optional_micros(budget.get("spentMicros"), "budget.spentMicros")?,
        remaining: optional_micros(budget.get("remainingMicros"), "budget.remainingMicros")?,
        resets_at: monthly_reset(budget.get("windowKey")),
    };
    let summary = Summary {
        requests: integer(summary.get("requestCount"), "summary.requestCount")?,
        successes: integer(summary.get("successCount"), "summary.successCount")?,
        errors: integer(summary.get("errorCount"), "summary.errorCount")?,
        input_tokens: integer(summary.get("inputTokens"), "summary.inputTokens")?,
        output_tokens: integer(summary.get("outputTokens"), "summary.outputTokens")?,
        total_tokens: integer(summary.get("totalTokens"), "summary.totalTokens")?,
        actual_cost: micros(summary.get("actualCostMicros"), "summary.actualCostMicros")?,
    };

    let mut providers = providers
        .iter()
        .map(|item| {
            let name = item
                .get("provider")
                .and_then(Value::as_str)
                .ok_or_else(|| parse_failure("provider name must be a string"))?
                .trim();
            let requests = integer(item.get("requestCount"), "provider.requestCount")?;
            // Success and error counts are validated like upstream even
            // though the card shows only requests, tokens and cost.
            integer(item.get("successCount"), "provider.successCount")?;
            integer(item.get("errorCount"), "provider.errorCount")?;
            Ok(RoutedProvider {
                name: if name.is_empty() { "Unknown" } else { name }.to_owned(),
                requests,
                tokens: integer(item.get("totalTokens"), "provider.totalTokens")?,
                cost: micros(item.get("actualCostMicros"), "provider.actualCostMicros")?,
            })
        })
        .collect::<Result<Vec<_>, ProviderError>>()?;
    providers.sort_by(|a, b| {
        b.cost
            .partial_cmp(&a.cost)
            .unwrap_or(Ordering::Equal)
            .then(b.requests.cmp(&a.requests))
            // Approximates JavaScript `localeCompare`: case-insensitive first.
            .then_with(|| {
                a.name
                    .to_lowercase()
                    .cmp(&b.name.to_lowercase())
                    .then_with(|| a.name.cmp(&b.name))
            })
    });

    Ok(ClawRouterUsage {
        budget,
        summary,
        providers,
    })
}

/// Upstream `ctx.pct`: 100 when the limit is unusable, else clamped to 0..=100.
fn pct(used: f64, limit: f64) -> f64 {
    if !(used.is_finite() && limit.is_finite() && limit > 0.0) {
        return 100.0;
    }
    (used / limit * 100.0).clamp(0.0, 100.0)
}

fn usd6(value: f64) -> String {
    format!("${value:.6}")
}

pub(super) fn build_result(usage: &ClawRouterUsage) -> ProviderFetchResult {
    let budget = &usage.budget;
    let summary = &usage.summary;
    let metered = budget.spent.zip(budget.limit);

    let primary = match metered {
        Some((spent, limit)) if limit > 0.0 => {
            RateWindow::with_details(pct(spent, limit), None, budget.resets_at, None)
        }
        _ if budget.configured => RateWindow::informational("No monthly limit reported"),
        _ => RateWindow::informational("Unmetered"),
    };
    let snapshot = UsageSnapshot::new(primary)
        .with_organization(format!("{} routed providers", usage.providers.len()))
        .with_login_method(if budget.configured {
            "Managed monthly budget"
        } else {
            "Unmetered"
        });

    let mut result = ProviderFetchResult::new(snapshot, "api");
    let cost = match metered {
        Some((spent, limit)) => {
            Some(CostSnapshot::new(spent, "USD", COST_PERIOD).with_limit(limit))
        }
        None if summary.actual_cost > 0.0 => {
            Some(CostSnapshot::new(summary.actual_cost, "USD", COST_PERIOD))
        }
        None => None,
    };
    if let Some(mut cost) = cost {
        let title = cost_card_title(cost.limit);
        cost = cost.with_card_title(title);
        if let Some(resets_at) = budget.resets_at {
            cost = cost.with_resets_at(resets_at);
        }
        result = result.with_cost(cost);
    }
    for detail in details(usage) {
        result = result.with_display_detail(detail);
    }
    result
}

/// Upstream `ProviderCostMenuCardStyle.clawRouter` (`MenuCardView+Costs.swift`):
/// "Monthly budget" when a positive limit is reported, else "ClawRouter spend".
fn cost_card_title(limit: Option<f64>) -> LocaleKey {
    match limit {
        Some(limit) if limit > 0.0 => LocaleKey::CostTitleClawRouterMonthlyBudget,
        _ => LocaleKey::CostTitleClawRouterSpend,
    }
}

fn usage_row(id: &str, title: &str, value: String) -> Option<ProviderDisplayDetail> {
    ProviderDisplayDetail::new(id, title, value)?.with_section_title(USAGE_SECTION)
}

fn details(usage: &ClawRouterUsage) -> Vec<Option<ProviderDisplayDetail>> {
    let budget = &usage.budget;
    let summary = &usage.summary;
    let mut rows = vec![
        usage_row("requests", "Requests", summary.requests.to_string()).and_then(|row| {
            row.with_secondary_value(format!(
                "{} succeeded · {} failed",
                summary.successes, summary.errors
            ))
        }),
        usage_row("tokens", "Tokens", summary.total_tokens.to_string()).and_then(|row| {
            row.with_secondary_value(format!(
                "{} input · {} output",
                summary.input_tokens, summary.output_tokens
            ))
        }),
        usage_row("actual-cost", "Actual cost", usd6(summary.actual_cost)),
        usage_row("ledger", "Budget ledger", budget.ledger.clone()),
    ];
    if let Some((spent, limit)) = budget.spent.zip(budget.limit) {
        let row = usage_row(
            "monthly-budget",
            "Monthly budget",
            format!("{} / ${limit:.2}", usd6(spent)),
        );
        rows.push(match budget.remaining {
            Some(remaining) => row
                .and_then(|row| row.with_secondary_value(format!("{} remaining", usd6(remaining)))),
            None => row,
        });
    }
    rows.extend(
        usage
            .providers
            .iter()
            .take(MAX_PROVIDER_ROWS)
            .enumerate()
            .map(|(index, provider)| {
                ProviderDisplayDetail::new(
                    format!("provider-{index}"),
                    &provider.name,
                    format!("{} requests", provider.requests),
                )?
                .with_secondary_value(format!(
                    "{} · {} tokens",
                    usd6(provider.cost),
                    provider.tokens
                ))?
                .with_section_title(PROVIDERS_SECTION)
            }),
    );
    rows
}
