//! Exa API usage provider.
//!
//! Fetches selected API key monthly spend (UTC calendar month to date) from
//! `GET https://admin-api.exa.ai/team-management/api-keys/{id}/usage` with
//! x-api-key (Team Management service key) authentication.
//!
//! Ported from upstream CodexBar v0.74.1 (`Resources/Plugins/exa.js`).

use async_trait::async_trait;
use chrono::{Datelike, Timelike, Utc};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::read_bounded_response;

const USAGE_URL_BASE: &str = "https://admin-api.exa.ai/team-management/api-keys";
const CREDENTIAL_TARGET: &str = "codexbar-exa";
pub const SERVICE_KEY_ENV: &str = "EXA_SERVICE_KEY";
pub const API_KEY_ID_ENV: &str = "EXA_API_KEY_ID";
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

#[derive(Debug, Deserialize, Serialize)]
struct UsageResponse {
    total_cost_usd: f64,
    period: PeriodInfo,
}

#[derive(Debug, Deserialize, Serialize)]
struct PeriodInfo {
    start: String,
    end: String,
}

pub struct ExaProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl ExaProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Exa,
                display_name: "Exa",
                session_label: "Spend",
                weekly_label: "Spend",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://dashboard.exa.ai/api-keys"),
                status_page_url: None,
                tertiary_label_key: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(REQUEST_TIMEOUT)
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    async fn fetch_usage_impl(
        &self,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        // Get service key (for header auth)
        let service_key = crate::providers::resolve_api_key(
            ctx.api_key.as_deref(),
            CREDENTIAL_TARGET,
            &[SERVICE_KEY_ENV],
        )?;

        // Get API key ID (scope/workspace)
        let key_id = ctx
            .workspace_id
            .as_deref()
            .and_then(|s| {
                let trimmed = s.trim();
                if trimmed.is_empty() || trimmed == "." || trimmed == ".." {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            })
            .ok_or_else(|| {
                ProviderError::Other(
                    "Set the Exa API key ID in Scope or EXA_API_KEY_ID; this is not the service key.".into(),
                )
            })?;

        // Calculate date range: start of month to current time (in UTC)
        let now = Utc::now();
        let month_start = Utc::now()
            .with_day(1)
            .unwrap_or(now)
            .with_hour(0)
            .unwrap_or(now)
            .with_minute(0)
            .unwrap_or(now)
            .with_second(0)
            .unwrap_or(now)
            .with_nanosecond(0)
            .unwrap_or(now);

        // Round current time down to whole seconds
        let end_time = now.with_nanosecond(0).unwrap_or(now);

        let start_date = month_start.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
        let end_date = end_time.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();

        // Build URL with encoded key ID and query parameters
        let encoded_key_id = key_id
            .chars()
            .map(|c| match c {
                'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '~' | '.' => c.to_string(),
                _ => format!("%{:02X}", c as u8),
            })
            .collect::<String>();

        let url = format!(
            "{}/{}/usage?start_date={}&end_date={}",
            USAGE_URL_BASE, encoded_key_id, start_date, end_date
        );

        let response = self
            .client
            .get(&url)
            .header("x-api-key", &service_key)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await?;

        let status = response.status();
        match status {
            StatusCode::OK => {}
            StatusCode::UNAUTHORIZED => {
                return Err(ProviderError::Other(
                    "Exa rejected the service key. Ordinary search API keys are not supported."
                        .into(),
                ));
            }
            StatusCode::FORBIDDEN => {
                return Err(ProviderError::Other(
                    "Exa Team Management must be enabled and both keys must belong to the same team.".into(),
                ));
            }
            StatusCode::TOO_MANY_REQUESTS => {
                return Err(ProviderError::Other("Exa rate limit exceeded.".into()));
            }
            status if status.is_server_error() => {
                return Err(ProviderError::Other(format!(
                    "Exa service error: HTTP {}.",
                    status.as_u16()
                )));
            }
            _ => {
                return Err(ProviderError::Other(format!(
                    "Exa API error: HTTP {}.",
                    status.as_u16()
                )));
            }
        }

        let body = read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|_| ProviderError::Parse("Failed to read Exa response.".into()))?;

        let data: Value = serde_json::from_slice(&body)
            .map_err(|_| ProviderError::Parse("Exa response is not valid JSON.".into()))?;

        let parsed = parse_usage(&data, month_start, end_time)?;
        let usage = parsed.usage;
        let mut result = ProviderFetchResult::new(usage, "api");
        for detail in parsed.details {
            result = result.with_display_detail(Some(detail));
        }
        Ok(result)
    }
}

impl Default for ExaProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for ExaProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Exa
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => self.fetch_usage_impl(ctx).await,
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}

#[derive(Debug, Clone)]
struct ParsedUsage {
    usage: UsageSnapshot,
    details: Vec<ProviderDisplayDetail>,
}

fn parse_usage(
    payload: &Value,
    month_start: chrono::DateTime<Utc>,
    end_time: chrono::DateTime<Utc>,
) -> Result<ParsedUsage, ProviderError> {
    let data = payload
        .as_object()
        .ok_or_else(|| ProviderError::Parse("Exa response is not an object.".into()))?;

    // Extract total_cost_usd
    let total_cost_usd = data
        .get("total_cost_usd")
        .and_then(|v| v.as_f64())
        .ok_or_else(|| ProviderError::Parse("Missing or invalid total_cost_usd.".into()))?;

    // Validate finitude and non-negativity
    if !total_cost_usd.is_finite() || total_cost_usd < 0.0 {
        return Err(ProviderError::Parse(
            "Exa returned an invalid spend value.".into(),
        ));
    }

    // Validate period
    let period = data
        .get("period")
        .and_then(|v| v.as_object())
        .ok_or_else(|| ProviderError::Parse("Missing period data.".into()))?;

    let period_start = period
        .get("start")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ProviderError::Parse("Missing period.start.".into()))?;

    let period_end = period
        .get("end")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ProviderError::Parse("Missing period.end.".into()))?;

    // Validate period matches request (UTC month start to request end)
    let expected_start = month_start.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let expected_end = end_time.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();

    if period_start != expected_start.as_str() || period_end != expected_end.as_str() {
        return Err(ProviderError::Parse(
            "Exa returned mismatched period dates.".into(),
        ));
    }

    // Build usage snapshot - Exa only has spend, no quota
    let usage = UsageSnapshot::new(RateWindow::new(0.0)) // placeholder, not displayed
        .with_login_method("Service key");

    // Build display details
    let mut details = Vec::new();

    if let Some(detail) =
        ProviderDisplayDetail::new("spend", "Spend", format!("${:.2}", total_cost_usd))
            .and_then(|d| d.with_section_title("API key this month (UTC)"))
    {
        details.push(detail);
    }

    Ok(ParsedUsage { usage, details })
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests_module;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_usage() {
        let json = r#"{
            "total_cost_usd": 45.67,
            "period": {
                "start": "2026-10-01T00:00:00.000Z",
                "end": "2026-10-08T12:31:36.000Z"
            }
        }"#;

        let month_start = chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        let end_time = chrono::DateTime::parse_from_rfc3339("2026-10-08T12:31:36+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let data: Value = serde_json::from_str(json).unwrap();
        let result = parse_usage(&data, month_start, end_time).unwrap();

        assert_eq!(result.usage.login_method, Some("Service key".to_string()));
        assert!(!result.details.is_empty());
        assert_eq!(result.details[0].title(), "Spend");
        assert!(result.details[0].value().contains("$45.67"));
    }

    #[test]
    fn test_parse_zero_spend() {
        let json = r#"{
            "total_cost_usd": 0.0,
            "period": {
                "start": "2026-10-01T00:00:00.000Z",
                "end": "2026-10-08T12:31:36.000Z"
            }
        }"#;

        let month_start = chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        let end_time = chrono::DateTime::parse_from_rfc3339("2026-10-08T12:31:36+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let data: Value = serde_json::from_str(json).unwrap();
        let result = parse_usage(&data, month_start, end_time).unwrap();

        assert_eq!(result.details[0].value(), "$0.00");
    }

    #[test]
    fn test_parse_rejects_missing_cost() {
        let json = r#"{
            "period": {
                "start": "2026-10-01T00:00:00.000Z",
                "end": "2026-10-08T12:31:36.000Z"
            }
        }"#;

        let month_start = chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        let end_time = chrono::DateTime::parse_from_rfc3339("2026-10-08T12:31:36+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let data: Value = serde_json::from_str(json).unwrap();
        assert!(parse_usage(&data, month_start, end_time).is_err());
    }

    #[test]
    fn test_parse_rejects_negative_cost() {
        let json = r#"{
            "total_cost_usd": -0.01,
            "period": {
                "start": "2026-10-01T00:00:00.000Z",
                "end": "2026-10-08T12:31:36.000Z"
            }
        }"#;

        let month_start = chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        let end_time = chrono::DateTime::parse_from_rfc3339("2026-10-08T12:31:36+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let data: Value = serde_json::from_str(json).unwrap();
        assert!(parse_usage(&data, month_start, end_time).is_err());
    }

    #[test]
    fn test_parse_rejects_nonfinite_cost() {
        // Test non-numeric and invalid JSON values (note: serde_json doesn't support 1e400 as infinity)
        for json_val in ["null", "true", "\"string\"", "[]", "{}"] {
            let json = format!(
                r#"{{
                "total_cost_usd": {},
                "period": {{
                    "start": "2026-10-01T00:00:00.000Z",
                    "end": "2026-10-08T12:31:36.000Z"
                }}
            }}"#,
                json_val
            );

            let month_start = chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:00+00:00")
                .unwrap()
                .with_timezone(&Utc);
            let end_time = chrono::DateTime::parse_from_rfc3339("2026-10-08T12:31:36+00:00")
                .unwrap()
                .with_timezone(&Utc);

            let data: Value = serde_json::from_str(&json).unwrap();
            assert!(parse_usage(&data, month_start, end_time).is_err());
        }
    }

    #[test]
    fn test_parse_rejects_mismatched_period() {
        let json = r#"{
            "total_cost_usd": 45.67,
            "period": {
                "start": "2026-09-01T00:00:00.000Z",
                "end": "2026-10-08T12:31:36.000Z"
            }
        }"#;

        let month_start = chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc);
        let end_time = chrono::DateTime::parse_from_rfc3339("2026-10-08T12:31:36+00:00")
            .unwrap()
            .with_timezone(&Utc);

        let data: Value = serde_json::from_str(json).unwrap();
        assert!(parse_usage(&data, month_start, end_time).is_err());
    }
}
