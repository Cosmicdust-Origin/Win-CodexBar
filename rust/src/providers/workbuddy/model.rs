//! WorkBuddy billing payloads and the card mapping.
//!
//! Ported from upstream CodexBar v0.73.0 `workbuddy.ts`. Amounts arrive as
//! decimal strings ("500"); non-negative numbers are accepted too.

use chrono::{DateTime, Duration, FixedOffset, NaiveDateTime, Utc};
use serde_json::{Map, Value};

use crate::core::{
    ProviderDisplayDetail, ProviderError, ProviderFetchResult, RateWindow, UsageSnapshot,
};
use crate::providers::format;

const SECTION: &str = "Credits";
/// China Standard Time; cycle timestamps carry no zone.
const CYCLE_OFFSET_SECONDS: i32 = 8 * 60 * 60;

/// Upstream `fail(field)`: names the field, never the body.
pub(super) fn invalid(field: &str) -> ProviderError {
    ProviderError::Parse(format!("Invalid WorkBuddy billing response: {field}"))
}

/// JavaScript `String(value)` for the `code` in an API failure message.
fn js_string(value: Option<&Value>) -> String {
    match value {
        None => "undefined".to_owned(),
        Some(Value::Null) => "null".to_owned(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::Number(number)) => match (number.as_i64(), number.as_f64()) {
            (Some(integer), _) => integer.to_string(),
            (None, Some(float)) if float.fract() == 0.0 && float.abs() < 1e21 => {
                format!("{float:.0}")
            }
            _ => number.to_string(),
        },
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::Null => String::new(),
                other => js_string(Some(other)),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".to_owned(),
    }
}

/// The `data` object of a `code: 0` response. A non-zero code is an API
/// failure; anything that is not a JSON object is a parse failure.
pub(super) fn response_data(body: &[u8]) -> Result<Map<String, Value>, ProviderError> {
    let root = match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(root)) => root,
        _ => return Err(invalid("expected a JSON object")),
    };
    let code = root.get("code");
    if code.and_then(Value::as_f64) != Some(0.0) {
        return Err(ProviderError::Other(format!(
            "WorkBuddy billing API returned code {}.",
            js_string(code)
        )));
    }
    match root.get("data") {
        Some(Value::Object(data)) => Ok(data.clone()),
        _ => Err(invalid("data")),
    }
}

/// Upstream `amount`: a non-negative number or a plain decimal string.
fn amount(value: Option<&Value>, field: &str) -> Result<f64, ProviderError> {
    match value {
        Some(Value::Number(number)) => number
            .as_f64()
            .filter(|value| value.is_finite() && *value >= 0.0)
            .ok_or_else(|| invalid(field)),
        Some(Value::String(text)) => {
            let text = text.trim();
            if !is_plain_decimal(text) {
                return Err(invalid(field));
            }
            text.parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .ok_or_else(|| invalid(field))
        }
        _ => Err(invalid(field)),
    }
}

/// `^\d+(?:\.\d+)?$`
fn is_plain_decimal(text: &str) -> bool {
    let (whole, fraction) = match text.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (text, None),
    };
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    digits(whole) && fraction.is_none_or(digits)
}

fn text(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Summary {
    pub total: f64,
    pub remaining: f64,
    pub frozen: f64,
    pub plan: Option<String>,
}

/// Sums the `credits` packages of `get-user-resource-summary`; other units
/// are ignored.
pub(super) fn parse_summary(data: &Map<String, Value>) -> Result<Summary, ProviderError> {
    let Some(Value::Array(packages)) = data.get("Packages") else {
        return Err(invalid("Packages"));
    };
    let mut summary = Summary {
        total: 0.0,
        remaining: 0.0,
        frozen: 0.0,
        plan: text(data.get("SubscriptionPackageName")),
    };
    for raw in packages {
        let Value::Object(item) = raw else {
            return Err(invalid("package"));
        };
        if item.get("CapacityUnit").and_then(Value::as_str) != Some("credits") {
            continue;
        }
        summary.total += amount(item.get("CycleTotalCapacity"), "CycleTotalCapacity")?;
        summary.remaining += amount(item.get("CycleRemainCapacity"), "CycleRemainCapacity")?;
        if let Some(frozen) = item.get("CycleFrozenCapacity") {
            summary.frozen += amount(Some(frozen), "CycleFrozenCapacity")?;
        }
    }
    Ok(summary)
}

/// Upstream `cycleEnd`: "YYYY-MM-DD HH:MM:SS" read as China Standard Time,
/// plus one second because the dashboard refreshes after the inclusive end.
pub(super) fn cycle_end(value: Option<&Value>) -> Option<DateTime<Utc>> {
    let raw = text(value)?;
    let bytes = raw.as_bytes();
    let shaped = bytes.len() == 19
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            10 => *byte == b' ',
            13 | 16 => *byte == b':',
            _ => byte.is_ascii_digit(),
        });
    if !shaped {
        return None;
    }
    let local = NaiveDateTime::parse_from_str(&raw, "%Y-%m-%d %H:%M:%S").ok()?;
    let offset = FixedOffset::east_opt(CYCLE_OFFSET_SECONDS)?;
    let end = local.and_local_timezone(offset).single()?;
    Some(end.with_timezone(&Utc) + Duration::seconds(1))
}

/// The earliest future `credits` cycle end in one package listing.
pub(super) fn earliest_reset(
    data: &Map<String, Value>,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let Some(Value::Array(accounts)) = data.get("Accounts") else {
        return None;
    };
    accounts
        .iter()
        .filter_map(Value::as_object)
        .filter(|account| account.get("CapacityUnit").and_then(Value::as_str) == Some("credits"))
        .filter_map(|account| cycle_end(account.get("CycleEndTime")))
        .filter(|end| *end > now)
        .min()
}

/// `ctx.format.number(value, { maximumFractionDigits: 2 })`.
pub(super) fn credits(value: f64) -> String {
    format::number(value, 2)
}

/// Upstream `ctx.pct(used, total)`.
fn percent(used: f64, total: f64) -> f64 {
    (used / total * 100.0).clamp(0.0, 100.0)
}

fn row(id: &str, title: &str, value: String) -> Option<ProviderDisplayDetail> {
    ProviderDisplayDetail::new(id, title, value).and_then(|row| row.with_section_title(SECTION))
}

/// One Credits meter when the allowance is above zero, else Left and Total
/// rows. Reserved (frozen) credits are listed as reported.
pub(super) fn result(summary: &Summary, resets_at: Option<DateTime<Utc>>) -> ProviderFetchResult {
    let left = credits(summary.remaining);
    let primary = if summary.total > 0.0 {
        let used = (summary.total - summary.remaining).max(0.0);
        RateWindow::with_details(
            percent(used, summary.total),
            None,
            resets_at,
            Some(format!("{left} / {} credits left", credits(summary.total))),
        )
        .with_description_as_detail()
    } else {
        // Upstream has no meter here; the Windows card always has a primary
        // lane, so it carries the balance as an informational value.
        RateWindow::informational(format!("{left} credits left"))
    };

    let mut rows = Vec::new();
    if summary.total <= 0.0 {
        rows.push(row("left", "Left", left));
        rows.push(row("total", "Total", credits(summary.total)));
    }
    if summary.frozen > 0.0 {
        rows.push(row("reserved", "Reserved", credits(summary.frozen)));
    }

    let mut usage = UsageSnapshot::new(primary);
    if let Some(plan) = &summary.plan {
        usage = usage.with_login_method(plan.clone());
    }
    ProviderFetchResult::new(usage, "web").with_display_details(rows.into_iter().flatten())
}

/// The major version of a Chrome version string ("143.0.7499.170" → 143),
/// when it is above 1 as upstream requires.
pub(super) fn chrome_major(version: &str) -> Option<u32> {
    let mut parts = version.trim().split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let rest_ok =
        parts.all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()));
    (major > 1 && rest_ok).then_some(major)
}

/// Upstream `userAgents`: the installed major, then the previous one (an
/// update Chrome has not relaunched into). Windows sends the Windows form of
/// Chrome's reduced User-Agent; without a version there is none.
pub(super) fn user_agents(major: Option<u32>) -> Vec<Option<String>> {
    let agent = |version: u32| {
        Some(format!(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{version}.0.0.0 Safari/537.36"
        ))
    };
    match major {
        Some(major) if major > 1 => vec![agent(major), agent(major - 1)],
        _ => vec![None],
    }
}
