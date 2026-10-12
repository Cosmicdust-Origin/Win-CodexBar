//! Synthetic (synthetic.new) quota provider.
//!
//! Port of upstream `Resources/Plugins/synthetic.js`: one bearer-authenticated
//! `GET /v2/quotas` whose payload names a rolling five-hour quota, a weekly
//! token quota and an hourly search quota, with a key-guessing fallback for
//! other payload shapes.

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use reqwest::{Client, StatusCode, redirect::Policy};
use serde_json::Value;
use std::time::Duration;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::{BoundedBodyError, read_bounded_response};

const QUOTAS_URL: &str = "https://api.synthetic.new/v2/quotas";
const CREDENTIAL_TARGET: &str = "codexbar-synthetic";
pub(crate) const API_KEY_ENV: &str = "SYNTHETIC_API_KEY";
const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const SEARCH_HOURLY_ID: &str = "synthetic-search-hourly";
const SEARCH_HOURLY_TITLE: &str = "Search hourly";

const PERCENT_USED_KEYS: &[&str] = &[
    "percentUsed",
    "usedPercent",
    "usagePercent",
    "usage_percent",
    "used_percent",
    "percent_used",
    "percent",
];
const PERCENT_REMAINING_KEYS: &[&str] = &[
    "percentRemaining",
    "remainingPercent",
    "remaining_percent",
    "percent_remaining",
];
const LIMIT_KEYS: &[&str] = &[
    "limit",
    "messageLimit",
    "message_limit",
    "messages",
    "maxRequests",
    "max_requests",
    "requestLimit",
    "request_limit",
    "quota",
    "max",
    "total",
    "capacity",
    "allowance",
];
const USED_KEYS: &[&str] = &[
    "used",
    "usage",
    "usedMessages",
    "used_messages",
    "messagesUsed",
    "messages_used",
    "requests",
    "requestCount",
    "request_count",
    "consumed",
    "spent",
];
const REMAINING_KEYS: &[&str] = &["remaining", "left", "available", "balance"];
const RESET_KEYS: &[&str] = &[
    "resetAt",
    "reset_at",
    "resetsAt",
    "resets_at",
    "renewAt",
    "renew_at",
    "renewsAt",
    "renews_at",
    "nextTickAt",
    "next_tick_at",
    "nextRegenAt",
    "next_regen_at",
    "periodEnd",
    "period_end",
    "expiresAt",
    "expires_at",
    "endAt",
    "end_at",
];
const PLAN_KEYS: &[&str] = &[
    "plan",
    "planName",
    "plan_name",
    "subscription",
    "subscriptionPlan",
    "tier",
    "package",
    "packageName",
];
const TICK_PERCENT_KEYS: &[&str] = &[
    "tickPercent",
    "tick_percent",
    "nextTickPercent",
    "next_tick_percent",
];
/// Upstream fallback collections, read from the root and then from `data`.
const FALLBACK_COLLECTIONS: &[&str] = &[
    "quotas",
    "quota",
    "limits",
    "usage",
    "entries",
    "subscription",
];

pub struct SyntheticProvider {
    metadata: ProviderMetadata,
    client: Client,
    quotas_url: String,
}

impl SyntheticProvider {
    pub fn new() -> Self {
        let client = crate::core::credentialed_http_client_builder()
            .redirect(Policy::none())
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("Synthetic HTTP client configuration is valid");
        Self::with_client(QUOTAS_URL, client)
    }

    fn with_client(quotas_url: impl Into<String>, client: Client) -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Synthetic,
                display_name: "Synthetic",
                session_label: "Five-hour quota",
                weekly_label: "Weekly tokens",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: None,
                status_page_url: None,
                tertiary_label_key: None,
            },
            client,
            quotas_url: quotas_url.into(),
        }
    }

    async fn fetch_quotas(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        let key = crate::providers::resolve_api_key(
            ctx.api_key.as_deref(),
            CREDENTIAL_TARGET,
            &[API_KEY_ENV],
        )?;

        let response = self
            .client
            .get(&self.quotas_url)
            .bearer_auth(&key)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await?;
        let status = response.status();
        if status != StatusCode::OK {
            return Err(status_error(status));
        }

        let body = read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|error| match error {
                BoundedBodyError::Read(error) => ProviderError::Network(error),
                BoundedBodyError::TooLarge => ProviderError::Parse(format!(
                    "Synthetic response exceeded {MAX_RESPONSE_BYTES} bytes."
                )),
            })?;
        let payload: Value = serde_json::from_slice(&body)
            .map_err(|_| parse_failure("the response is not JSON."))?;
        Ok(parse_quotas(&payload)?.into_result())
    }
}

impl Default for SyntheticProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for SyntheticProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Synthetic
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => self.fetch_quotas(ctx).await,
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}

fn status_error(status: StatusCode) -> ProviderError {
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            ProviderError::Other("Invalid Synthetic API credentials.".into())
        }
        status => ProviderError::Other(format!("Synthetic API error: HTTP {}.", status.as_u16())),
    }
}

/// One parsed quota object (upstream `parseQuota`).
#[derive(Debug, Clone, PartialEq)]
struct Quota {
    used_percent: f64,
    window_minutes: Option<u32>,
    resets_at: Option<DateTime<Utc>>,
    /// Upstream `windowDescription`, only when there is no reset time.
    window_description: Option<String>,
    /// Upstream `nextRegenPercent`: the share one regen tick restores.
    next_regen_percent: Option<f64>,
    cost: Option<QuotaCost>,
}

#[derive(Debug, Clone, PartialEq)]
struct QuotaCost {
    used: f64,
    limit: f64,
    next_regen_amount: Option<f64>,
}

/// The three card lanes plus plan and the first cost-bearing quota.
#[derive(Debug, Clone, PartialEq)]
struct ParsedQuotas {
    primary: Option<Quota>,
    secondary: Option<Quota>,
    tertiary: Option<Quota>,
    plan: Option<String>,
    cost: Option<QuotaCost>,
}

fn parse_failure(reason: &str) -> ProviderError {
    ProviderError::Parse(format!("Failed to parse Synthetic response: {reason}"))
}

fn parse_quotas(payload: &Value) -> Result<ParsedQuotas, ProviderError> {
    let wrapped;
    let root = match payload {
        Value::Array(_) => {
            wrapped = serde_json::json!({ "quotas": payload });
            &wrapped
        }
        Value::Object(_) => payload,
        _ => return Err(parse_failure("expected an object or array.")),
    };
    let data = root
        .get("data")
        .filter(|data| data.is_object() || data.is_array());

    let slot = |pick: fn(&Value) -> Option<&Value>| -> Option<&Value> {
        pick(root)
            .filter(|value| is_quota(value))
            .or_else(|| data.and_then(pick).filter(|value| is_quota(value)))
    };
    let slots = [
        slot(|value| value.get("rollingFiveHourLimit")),
        slot(|value| value.get("weeklyTokenLimit")),
        slot(|value| value.get("search").and_then(|search| search.get("hourly"))),
    ];

    let parsed: Vec<Option<Quota>> = if slots.iter().any(Option::is_some) {
        slots
            .iter()
            .map(|value| value.and_then(parse_quota))
            .collect()
    } else {
        let candidates = FALLBACK_COLLECTIONS
            .iter()
            .map(|key| root.get(*key))
            .chain(std::iter::once(root.get("data")))
            .chain(
                FALLBACK_COLLECTIONS
                    .iter()
                    .map(|key| data.and_then(|data| data.get(*key))),
            );
        let mut values = Vec::new();
        for candidate in candidates.flatten() {
            collect(candidate, &mut values);
            if !values.is_empty() {
                break;
            }
        }
        values
            .into_iter()
            .filter_map(parse_quota)
            .map(Some)
            .collect()
    };
    if !parsed.iter().any(Option::is_some) {
        return Err(parse_failure("Missing quota data."));
    }

    let plan = first_string(root, PLAN_KEYS)
        .or_else(|| data.and_then(|data| first_string(data, PLAN_KEYS)));
    let cost = parsed.iter().flatten().find_map(|quota| quota.cost.clone());
    let mut lanes = parsed.into_iter();
    Ok(ParsedQuotas {
        primary: lanes.next().flatten(),
        secondary: lanes.next().flatten(),
        tertiary: lanes.next().flatten(),
        plan,
        cost,
    })
}

fn collect<'a>(candidate: &'a Value, out: &mut Vec<&'a Value>) {
    match candidate {
        Value::Array(items) => items.iter().for_each(|item| collect(item, out)),
        Value::Object(_) if is_quota(candidate) => out.push(candidate),
        // serde_json's default map is a BTreeMap, so keys iterate sorted, as
        // upstream's `Object.keys(candidate).sort()` does.
        Value::Object(map) => map.values().for_each(|value| collect(value, out)),
        _ => {}
    }
}

fn is_quota(value: &Value) -> bool {
    value.is_object()
        && [
            LIMIT_KEYS,
            USED_KEYS,
            REMAINING_KEYS,
            PERCENT_USED_KEYS,
            PERCENT_REMAINING_KEYS,
        ]
        .iter()
        .any(|keys| first_number(value, keys).is_some())
}

fn parse_quota(payload: &Value) -> Option<Quota> {
    let mut used_percent = normalized_percent(first_number(payload, PERCENT_USED_KEYS));
    let percent_remaining = normalized_percent(first_number(payload, PERCENT_REMAINING_KEYS));
    if used_percent.is_none()
        && let Some(remaining) = percent_remaining
    {
        used_percent = Some(100.0 - remaining);
    }
    if used_percent.is_none() {
        let mut limit = first_number(payload, LIMIT_KEYS);
        let mut used = first_number(payload, USED_KEYS);
        let remaining = first_number(payload, REMAINING_KEYS);
        if limit.is_none()
            && let (Some(used), Some(remaining)) = (used, remaining)
        {
            limit = Some(used + remaining);
        }
        if used.is_none()
            && let (Some(limit), Some(remaining)) = (limit, remaining)
        {
            used = Some(limit - remaining);
        }
        if let (Some(limit), Some(used)) = (limit, used)
            && limit > 0.0
        {
            used_percent = Some(percent_of(used, limit));
        }
    }
    let used_percent = used_percent?.clamp(0.0, 100.0);

    let window_minutes = window_minutes(payload).and_then(|minutes| u32::try_from(minutes).ok());
    let resets_at = first_date(payload, RESET_KEYS);
    let window_description = if resets_at.is_none() {
        window_minutes.and_then(window_description)
    } else {
        None
    };
    let next_regen_percent = normalized_percent(first_number(payload, TICK_PERCENT_KEYS));

    let cost = first_currency(payload, &["maxCredits", "max_credits"]).map(|limit| {
        let remaining = first_currency(payload, &["remainingCredits", "remaining_credits"]);
        let explicit_used = first_currency(payload, &["usedCredits", "used_credits"]);
        let used = explicit_used
            .or_else(|| remaining.map(|remaining| (limit - remaining).max(0.0)))
            .unwrap_or(used_percent / 100.0 * limit);
        QuotaCost {
            used,
            limit,
            next_regen_amount: first_currency(payload, &["nextRegenCredits", "next_regen_credits"]),
        }
    });

    Some(Quota {
        used_percent,
        window_minutes,
        resets_at,
        window_description,
        next_regen_percent,
        cost,
    })
}

/// Upstream `ctx.pct`: `used / limit * 100`, clamped to 0..=100.
fn percent_of(used: f64, limit: f64) -> f64 {
    if !used.is_finite() || !limit.is_finite() || limit <= 0.0 {
        return 100.0;
    }
    (used / limit * 100.0).clamp(0.0, 100.0)
}

/// Upstream `normalizedPercent`: fractions (<= 1) are scaled to percent.
fn normalized_percent(value: Option<f64>) -> Option<f64> {
    value.map(|value| if value <= 1.0 { value * 100.0 } else { value })
}

fn number_value(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64().filter(|value| value.is_finite()),
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return None;
            }
            trimmed
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
        }
        _ => None,
    }
}

fn first_number(payload: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| payload.get(*key).and_then(number_value))
}

fn first_string(payload: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        payload
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    })
}

/// Upstream `currencyValue`: strings may carry `$` and thousands separators.
fn currency_value(value: &Value) -> Option<f64> {
    match value {
        Value::String(text) => {
            let cleaned: String = text
                .trim()
                .chars()
                .filter(|ch| *ch != '$' && *ch != ',')
                .collect();
            let cleaned = cleaned.trim();
            // JavaScript `Number("")` is 0.
            if cleaned.is_empty() {
                return Some(0.0);
            }
            cleaned
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
        }
        other => number_value(other),
    }
}

fn first_currency(payload: &Value, keys: &[&str]) -> Option<f64> {
    keys.iter()
        .find_map(|key| payload.get(*key).and_then(currency_value))
}

/// Upstream `parseDate`: epoch milliseconds above 1e12, epoch seconds above
/// 1e9, otherwise an ISO-8601 string.
#[allow(
    clippy::cast_possible_truncation,
    reason = "finite epoch values; `as` saturates and chrono rejects out-of-range instants"
)]
fn parse_date(value: &Value) -> Option<DateTime<Utc>> {
    if let Some(number) = number_value(value) {
        if number > 1_000_000_000_000.0 {
            return Utc.timestamp_millis_opt(number as i64).single();
        }
        if number > 1_000_000_000.0 {
            return Utc.timestamp_millis_opt((number * 1000.0) as i64).single();
        }
    }
    value
        .as_str()
        .and_then(|text| DateTime::parse_from_rfc3339(text.trim()).ok())
        .map(|date| date.with_timezone(&Utc))
}

fn first_date(payload: &Value, keys: &[&str]) -> Option<DateTime<Utc>> {
    keys.iter().find_map(|key| match payload.get(*key) {
        None | Some(Value::Null) => None,
        Some(value) => parse_date(value),
    })
}

/// JavaScript `Math.round`: halves round toward positive infinity.
#[allow(
    clippy::cast_possible_truncation,
    reason = "display counts are small; `as` saturates on overflow"
)]
fn js_round(value: f64) -> i64 {
    (value + 0.5).floor() as i64
}

fn window_minutes(payload: &Value) -> Option<i64> {
    let scaled = [
        (
            &[
                "windowMinutes",
                "window_minutes",
                "periodMinutes",
                "period_minutes",
            ],
            1.0,
        ),
        (
            &["windowHours", "window_hours", "periodHours", "period_hours"],
            60.0,
        ),
        (
            &["windowDays", "window_days", "periodDays", "period_days"],
            1440.0,
        ),
        (
            &[
                "windowSeconds",
                "window_seconds",
                "periodSeconds",
                "period_seconds",
            ],
            1.0 / 60.0,
        ),
    ];
    for (keys, factor) in scaled {
        if let Some(value) = first_number(payload, keys) {
            return Some(js_round(value * factor));
        }
    }
    let text = first_string(
        payload,
        &[
            "window",
            "windowLabel",
            "window_label",
            "period",
            "periodLabel",
            "period_label",
        ],
    )?;
    window_text_minutes(&text)
}

/// Upstream regex `^([0-9]*\.?[0-9]+)(minutes?|mins?|m|hours?|hrs?|hr|h|days?|d)$`
/// over the lowercased text with all whitespace removed.
fn window_text_minutes(text: &str) -> Option<i64> {
    let compact: String = text
        .to_lowercase()
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .collect();
    let split = compact
        .find(|ch: char| !(ch.is_ascii_digit() || ch == '.'))
        .unwrap_or(compact.len());
    let (number, unit) = compact.split_at(split);
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    let valid_number = match number.split_once('.') {
        Some((whole, fraction)) => (whole.is_empty() || digits(whole)) && digits(fraction),
        None => digits(number),
    };
    if !valid_number {
        return None;
    }
    let multiplier = match unit {
        "m" | "min" | "mins" | "minute" | "minutes" => 1.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => 60.0,
        "d" | "day" | "days" => 1440.0,
        _ => return None,
    };
    let value: f64 = number.parse().ok()?;
    Some(js_round(value * multiplier))
}

/// Upstream `windowDescription`.
fn window_description(minutes: u32) -> Option<String> {
    let plural = |count: u32, unit: &str| {
        format!("{count} {unit}{} window", if count == 1 { "" } else { "s" })
    };
    match minutes {
        0 => None,
        m if m % 1440 == 0 => Some(plural(m / 1440, "day")),
        m if m % 60 == 0 => Some(plural(m / 60, "hour")),
        m => Some(plural(m, "minute")),
    }
}

/// Upstream `syntheticRollingRegenDetail` / `syntheticRegenDetail` text in
/// the default "% left" form: `"<n>% after next regen · <full in>"`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "regen tick counts are small; `as` saturates on overflow"
)]
fn regen_detail(used_percent: f64, next_regen_percent: f64, ticks_to_full: f64) -> String {
    let remaining = (100.0 - used_percent).max(0.0);
    let after_next = (remaining + next_regen_percent).min(100.0);
    let right = if ticks_to_full <= 0.1 {
        "Near full".to_string()
    } else if ticks_to_full < 1.5 {
        "Full in ~1 regen".to_string()
    } else {
        format!("Full in ~{} regens", ticks_to_full.ceil() as i64)
    };
    format!("{after_next:.0}% after next regen · {right}")
}

impl Quota {
    /// A plain window: reset time, or the "N hours window" text without one.
    fn rate_window(&self) -> RateWindow {
        RateWindow::with_details(
            self.used_percent,
            self.window_minutes,
            self.resets_at,
            self.window_description.clone(),
        )
    }

    /// A window whose next regen tick replaces the pace line.
    ///
    /// The window keeps the payload length, as upstream synthetic.js does, so
    /// the tray can still score its pace. The card hides the pace line for a
    /// regenerating window (`with_regenerating_reset`) and shows the regen
    /// line instead, as upstream `syntheticRollingRegenDetail` and
    /// `syntheticRegenDetail` do.
    fn regen_window(&self, next_regen_percent: f64, ticks_to_full: f64) -> RateWindow {
        RateWindow::with_details(
            self.used_percent,
            self.window_minutes,
            self.resets_at,
            Some(regen_detail(
                self.used_percent,
                next_regen_percent,
                ticks_to_full,
            )),
        )
        .with_description_as_detail()
        // Upstream `syntheticRegenDetail`: the card counts down to the regen.
        .with_regenerating_reset()
    }

    /// Five-hour lane: regen line when the payload has a tick percent.
    fn rolling_window(&self) -> RateWindow {
        match (self.resets_at, self.next_regen_percent) {
            (Some(_), Some(tick)) if tick > 0.0 => {
                self.regen_window(tick, self.used_percent.max(0.0) / tick)
            }
            _ => self.rate_window(),
        }
    }

    /// Weekly lane: regen line when the cost carries a next-regen amount.
    fn weekly_window(&self, cost: Option<&QuotaCost>) -> RateWindow {
        let regen = cost.and_then(|cost| {
            let amount = cost.next_regen_amount.filter(|amount| *amount > 0.0)?;
            (cost.limit > 0.0 && self.resets_at.is_some())
                .then(|| (amount / cost.limit * 100.0, cost.used.max(0.0) / amount))
        });
        match regen {
            Some((percent, ticks)) => self.regen_window(percent, ticks),
            None => self.rate_window(),
        }
    }
}

impl ParsedQuotas {
    fn into_result(self) -> ProviderFetchResult {
        let primary = self
            .primary
            .as_ref()
            .map(Quota::rolling_window)
            .unwrap_or_else(|| RateWindow::informational("No five-hour quota reported"));
        let mut usage = UsageSnapshot::new(primary);
        if let Some(weekly) = &self.secondary {
            usage = usage.with_secondary(weekly.weekly_window(self.cost.as_ref()));
        }
        if let Some(search) = &self.tertiary {
            usage = usage.with_extra_rate_window(
                SEARCH_HOURLY_ID,
                SEARCH_HOURLY_TITLE,
                search.rate_window(),
            );
        }
        if let Some(plan) = &self.plan {
            usage = usage.with_login_method(plan.clone());
        }
        // The weekly credits only drive the weekly regen line above. Upstream
        // hides the cost block on the Synthetic card
        // (`ProviderCostPresentation(menuCardStyle: .hidden)`), so no cost is
        // attached here.
        ProviderFetchResult::new(usage, "api")
    }
}

#[cfg(test)]
mod tests;
