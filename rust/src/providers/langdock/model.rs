//! Langdock tRPC personal-usage envelope and the card presentation.
//!
//! Ported from upstream CodexBar v0.73.0 `langdock.js` (`fetchUsage`). The
//! payload is read as JSON values so a missing field and an explicit `null`
//! stay distinct, as they are in upstream's JavaScript checks.

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

use crate::core::{ProviderError, ProviderFetchResult, RateWindow, UsageSnapshot};

const SESSION_WINDOW_MINUTES: u32 = 300;
const WEEKLY_WINDOW_MINUTES: u32 = 10_080;
const NO_LIMITS: &str = "No included usage limits available";

/// Upstream `fail()`: one fixed message that never echoes the body.
pub(super) fn unexpected_response() -> ProviderError {
    ProviderError::Parse("Langdock returned an unexpected personal usage response.".into())
}

/// Upstream `denied(code)`: the tRPC error code taxonomy, shared with the
/// HTTP status mapping.
pub(super) fn denied(code: &str) -> ProviderError {
    ProviderError::Other(
        match code {
            "UNAUTHORIZED" => super::SESSION_EXPIRED,
            "FORBIDDEN" => "Langdock denied access to personal usage.",
            "TOO_MANY_REQUESTS" => "Langdock usage requests are rate limited.",
            "INTERNAL_SERVER_ERROR" | "TIMEOUT" => {
                "Langdock personal usage is temporarily unavailable."
            }
            _ => "Langdock rejected the personal usage request.",
        }
        .into(),
    )
}

fn object(value: Option<&Value>) -> Result<&Map<String, Value>, ProviderError> {
    value
        .and_then(Value::as_object)
        .ok_or_else(unexpected_response)
}

/// A finite JSON number. Upstream keeps overage above 100%; the Windows
/// `RateWindow` constructor clamps it to 0-100 when the window is built.
fn percent(value: Option<&Value>) -> Result<f64, ProviderError> {
    value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite())
        .ok_or_else(unexpected_response)
}

/// Missing or `null` means no reset; anything else must be an ISO date.
fn reset(value: Option<&Value>) -> Result<Option<DateTime<Utc>>, ProviderError> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => DateTime::parse_from_rfc3339(text)
            .map(|date| Some(date.with_timezone(&Utc)))
            .map_err(|_| unexpected_response()),
        Some(_) => Err(unexpected_response()),
    }
}

pub(super) fn parse_personal_usage(body: &[u8]) -> Result<ProviderFetchResult, ProviderError> {
    let raw: Value = serde_json::from_slice(body).map_err(|_| unexpected_response())?;
    let [envelope] = raw.as_array().map(Vec::as_slice).unwrap_or_default() else {
        return Err(unexpected_response());
    };
    let envelope = object(Some(envelope))?;
    if let Some(error) = envelope.get("error") {
        let data = object(object(object(Some(error))?.get("json"))?.get("data"))?;
        return match data.get("code").and_then(Value::as_str) {
            Some(code) if !code.is_empty() => Err(denied(code)),
            _ => Err(unexpected_response()),
        };
    }
    let payload = object(object(object(envelope.get("result"))?.get("data"))?.get("json"))?;
    let has_limits = match payload.get("hasIncludedUsageLimits") {
        None => None,
        Some(Value::Bool(value)) => Some(*value),
        Some(_) => return Err(unexpected_response()),
    };
    let plan = match payload.get("planUsage") {
        None | Some(Value::Null) => None,
        plan => Some(plan),
    };
    let Some(plan) = plan.filter(|_| has_limits != Some(false)) else {
        return Ok(no_included_limits());
    };

    let plan = object(plan)?;
    let session_enabled = plan
        .get("sessionUsageLimitsEnabled")
        .and_then(Value::as_bool)
        .ok_or_else(unexpected_response)?;
    let session_reset = reset(plan.get("sessionResetsAt"))?;
    let weekly_reset = reset(plan.get("weeklyResetsAt"))?;
    let session = if session_enabled {
        Some(RateWindow::with_details(
            percent(plan.get("sessionUsagePercent"))?,
            Some(SESSION_WINDOW_MINUTES),
            session_reset,
            None,
        ))
    } else {
        None
    };
    let weekly = RateWindow::with_details(
        percent(plan.get("weeklyUsagePercent"))?,
        Some(WEEKLY_WINDOW_MINUTES),
        weekly_reset,
        None,
    );

    let usage = match session {
        Some(session) => UsageSnapshot::new(session).with_secondary(weekly),
        // Upstream has no primary bar here; Windows needs a primary window,
        // so the weekly lane takes that slot under its own label.
        None => UsageSnapshot::new(weekly).with_primary_label("Weekly"),
    };
    Ok(ProviderFetchResult::new(
        usage.with_login_method("Browser session"),
        "web",
    ))
}

/// Upstream returns only a "Usage" detail row ("Included limits") and no
/// bars. The Windows card always renders the primary window, so that row
/// becomes the informational primary under its upstream label instead of a
/// second copy of the message titled "Session".
fn no_included_limits() -> ProviderFetchResult {
    let usage = UsageSnapshot::new(RateWindow::informational(NO_LIMITS))
        .with_primary_label("Included limits")
        .with_login_method("Browser session");
    ProviderFetchResult::new(usage, "web")
}
