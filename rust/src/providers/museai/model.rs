//! muse.ai server-action discovery and the subscription mapping.
//!
//! Ported from upstream CodexBar v0.73.0 `museai.js`. The page, chunk and
//! React flight formats are matched with the same patterns upstream uses.

use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use regex_lite::Regex;
use serde_json::Value;

use crate::core::{
    ProviderDisplayDetail, ProviderError, ProviderFetchResult, RateWindow, SubscriptionMetadata,
    UsageSnapshot,
};
use crate::providers::format;

/// Upstream `7 * 24 * 60`.
const WEEK_MINUTES: u32 = 7 * 24 * 60;
const MICRO_DOLLARS_PER_USD: f64 = 1e6;
const UNEXPECTED_RESPONSE: &str = "Unexpected muse.ai response.";
pub(super) const DISCOVERY_FAILURE: &str = "Could not find a working muse.ai subscription action. muse.ai may have changed. Refresh to retry.";

static CHUNK_PATH: LazyLock<Regex> = LazyLock::new(|| compile(r"static/chunks/[\w~.-]+\.js"));
static SETTINGS_MODULE: LazyLock<Regex> =
    LazyLock::new(|| compile(r"\.A\((\d+)\)\.then\(\(\{[^}]*Settings"));
static ACTION_ID: LazyLock<Regex> =
    LazyLock::new(|| compile(r#""([0-9a-f]{40,})",[^"]{0,200}"fetchSubscriptionAction""#));
static TOKENS_LEFT: LazyLock<Regex> = LazyLock::new(|| compile(r"\(([^()]+ tokens left)\)"));

fn compile(pattern: &str) -> Regex {
    Regex::new(pattern).expect("static muse.ai pattern compiles")
}

pub(super) fn discovery_failure() -> ProviderError {
    ProviderError::Parse(DISCOVERY_FAILURE.into())
}

fn unexpected_response() -> ProviderError {
    ProviderError::Parse(UNEXPECTED_RESPONSE.into())
}

/// Upstream `chunkPaths`: unique `/_next/static/chunks/*.js` paths in page
/// order.
pub(super) fn chunk_paths(text: &str) -> Vec<String> {
    let mut paths: Vec<String> = Vec::new();
    for found in CHUNK_PATH.find_iter(text) {
        let path = format!("/_next/{}", found.as_str());
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

/// The chunk list of the lazy module that preloads the settings dialog:
/// the app chunk names the module (`e.A(<module>).then(({…Settings…`) and
/// the loader chunk lists its files (`,<module>,s=>{s.v(t=>Promise.all([…`).
pub(super) fn settings_chunk_paths(text: &str) -> Option<Vec<String>> {
    let module = SETTINGS_MODULE.captures(text)?.get(1)?.as_str();
    // `module` is all digits, so it needs no escaping.
    let loader = Regex::new(&format!(
        r"[,{{\[]{module},\w+=>\{{\w+\.v\(\w+=>Promise\.all\(\[([^\]]*)"
    ))
    .ok()?;
    let files = loader.captures(text)?.get(1)?.as_str();
    Some(chunk_paths(files))
}

/// The `fetchSubscriptionAction` server reference ID in a settings chunk.
pub(super) fn action_id(body: &str) -> Option<String> {
    ACTION_ID
        .captures(body)
        .and_then(|captures| captures.get(1))
        .map(|id| id.as_str().to_owned())
}

/// React flight rows look like `1:{...}`; the subscription is the last row
/// whose `success` is `true`.
fn flight_subscription(body: &str) -> Option<Value> {
    let mut subscription = None;
    for line in body.split('\n') {
        let Some((id, row)) = line.split_once(':') else {
            continue;
        };
        let row = row.trim_end();
        let is_row = !id.is_empty()
            && id
                .bytes()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            && row.starts_with('{')
            && row.ends_with('}');
        if !is_row {
            continue;
        }
        let Ok(Value::Object(row)) = serde_json::from_str::<Value>(row) else {
            continue;
        };
        if row.get("success") == Some(&Value::Bool(true)) {
            subscription = row.get("subscription").cloned();
        }
    }
    subscription
}

/// Upstream `tokensLeft`: "3% used (2.8B tokens left)" → "2.8B tokens left".
pub(super) fn tokens_left(label: Option<&str>) -> Option<String> {
    TOKENS_LEFT
        .captures(label?)
        .and_then(|captures| captures.get(1))
        .map(|text| text.as_str().to_owned())
}

/// Upstream `ctx.date.unixSeconds` for numeric values only.
fn unix_seconds(value: Option<&Value>) -> Option<DateTime<Utc>> {
    let seconds = value?.as_f64()?;
    let millis = (seconds * 1000.0).round();
    if !millis.is_finite() || millis.abs() > 8.64e15 {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "the value is finite, integral and within the JavaScript Date range"
    )]
    let millis = millis as i64;
    DateTime::from_timestamp_millis(millis)
}

fn string<'a>(object: &'a Value, key: &str) -> Option<&'a str> {
    object.get(key).and_then(Value::as_str)
}

/// The top-up balance (purchased or referral tokens outside the weekly
/// allowance), when muse.ai reports a positive total. Upstream returns it as
/// a detail row with a progress bar, not as a quota window.
fn top_up(subscription: &Value) -> Option<ProviderDisplayDetail> {
    let balance = subscription.get("topupBalance")?.as_f64()?;
    let total = subscription.get("topupTotal")?.as_f64()?;
    if total <= 0.0 {
        return None;
    }
    let title = string(subscription, "topupRowLabel").unwrap_or("Additional tokens");
    let label = string(subscription, "topupRowValueLabel");
    let value = tokens_left(label)
        .or_else(|| label.map(str::to_owned))
        .unwrap_or_else(|| format!("{} left", format::usd(balance / MICRO_DOLLARS_PER_USD)));
    let used = ((total - balance) / total).clamp(0.0, 1.0);
    ProviderDisplayDetail::new("topup", title, value)?.with_progress(used, 1.0)
}

/// Maps the subscription server-action response to the card.
pub(super) fn result(body: &str) -> Result<ProviderFetchResult, ProviderError> {
    let subscription = flight_subscription(body).ok_or_else(unexpected_response)?;
    let percent = subscription
        .get("usage")
        .and_then(|usage| usage.get("percentUsed"))
        .and_then(Value::as_f64)
        .ok_or_else(unexpected_response)?;

    let usage_row = tokens_left(string(&subscription, "usageRowValueLabel"));
    let resets_at = unix_seconds(
        subscription
            .get("usage")
            .and_then(|usage| usage.get("resetsAt")),
    );
    let has_tokens_left = usage_row.is_some();
    let mut primary = RateWindow::with_details(
        percent.clamp(0.0, 100.0),
        Some(WEEK_MINUTES),
        resets_at,
        usage_row,
    );
    // Upstream shows "2.8B tokens left" beside the reset countdown.
    if has_tokens_left {
        primary = primary.with_description_as_detail();
    }

    let mut usage = UsageSnapshot::new(primary);
    let plan = subscription
        .get("tier")
        .and_then(|tier| string(tier, "name"))
        .or_else(|| string(&subscription, "usageRowLabel"));
    if let Some(plan) = plan {
        usage = usage.with_login_method(plan);
    }
    let renews_at = unix_seconds(
        subscription
            .get("agreement")
            .and_then(|agreement| agreement.get("currentPeriodEndTime")),
    );
    if let Some(renews_at) = renews_at {
        usage =
            usage.with_subscription(Some(SubscriptionMetadata::new(None, None, Some(renews_at))));
    }
    Ok(ProviderFetchResult::new(usage, "web").with_display_detail(top_up(&subscription)))
}
