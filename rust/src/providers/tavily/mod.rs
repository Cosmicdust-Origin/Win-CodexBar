//! Tavily API usage provider.
//!
//! Fetches account plan, API key, and pay-as-you-go credit usage from
//! `GET https://api.tavily.com/usage` with bearer authentication.
//!
//! Ported from upstream CodexBar v0.74.1 (`Resources/Plugins/tavily.js`).

use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use serde_json::Value;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::read_bounded_response;

const USAGE_URL: &str = "https://api.tavily.com/usage";
const CREDENTIAL_TARGET: &str = "codexbar-tavily";
pub const API_KEY_ENV: &str = "TAVILY_API_KEY";
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

#[derive(Debug, Deserialize)]
struct UsageResponse {
    account: Option<AccountInfo>,
    key: Option<KeyInfo>,
}

#[derive(Debug, Deserialize)]
struct AccountInfo {
    plan_usage: Option<i64>,
    plan_limit: Option<i64>,
    paygo_usage: Option<i64>,
    paygo_limit: Option<i64>,
    current_plan: Option<String>,
}

#[derive(Debug, Deserialize)]
struct KeyInfo {
    usage: Option<i64>,
    limit: Option<i64>,
}

pub struct TavilyProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl TavilyProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Tavily,
                display_name: "Tavily",
                session_label: "Account plan",
                weekly_label: "Account plan",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://app.tavily.com"),
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
        let key = crate::providers::resolve_api_key(
            ctx.api_key.as_deref(),
            CREDENTIAL_TARGET,
            &[API_KEY_ENV],
        )?;

        let response = self
            .client
            .get(USAGE_URL)
            .bearer_auth(&key)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await?;

        let status = response.status();
        match status {
            StatusCode::OK => {}
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                return Err(ProviderError::Other(
                    "Tavily API key is invalid or expired. Check your settings.".into(),
                ));
            }
            StatusCode::TOO_MANY_REQUESTS => {
                return Err(ProviderError::Other(
                    "Tavily rate limit exceeded. Keep refreshes at least one minute apart.".into(),
                ));
            }
            status if status.is_server_error() => {
                return Err(ProviderError::Other(format!(
                    "Tavily service error: HTTP {}.",
                    status.as_u16()
                )));
            }
            _ => {
                return Err(ProviderError::Other(format!(
                    "Tavily API error: HTTP {}.",
                    status.as_u16()
                )));
            }
        }

        let body = read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|_| ProviderError::Parse("Failed to read Tavily response.".into()))?;

        let data: Value = serde_json::from_slice(&body)
            .map_err(|_| ProviderError::Parse("Tavily response is not valid JSON.".into()))?;

        let parsed = parse_usage(&data)?;
        let usage = parsed.into_result();
        Ok(ProviderFetchResult::new(usage, "oauth"))
    }
}

impl Default for TavilyProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for TavilyProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Tavily
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
    primary: Option<RateWindow>,
    secondary: Option<RateWindow>,
    plan: Option<String>,
}

impl ParsedUsage {
    fn into_result(self) -> UsageSnapshot {
        let mut result = UsageSnapshot::new(self.primary.unwrap_or_else(|| RateWindow::new(0.0)));
        if let Some(secondary) = self.secondary {
            result = result.with_secondary(secondary);
        }
        if let Some(plan) = self.plan {
            result = result.with_login_method(plan);
        }
        result
    }
}

fn parse_usage(payload: &Value) -> Result<ParsedUsage, ProviderError> {
    let data = payload
        .as_object()
        .ok_or_else(|| ProviderError::Parse("Tavily response is not an object.".into()))?;

    let account = data
        .get("account")
        .and_then(|v| v.as_object())
        .ok_or_else(|| ProviderError::Parse("Tavily response missing account data.".into()))?;

    let key = data
        .get("key")
        .and_then(|v| v.as_object())
        .ok_or_else(|| ProviderError::Parse("Tavily response missing key data.".into()))?;

    // Extract account plan info
    let plan_usage = account
        .get("plan_usage")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| ProviderError::Parse("Invalid plan_usage value.".into()))?;

    let plan_limit = account
        .get("plan_limit")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| ProviderError::Parse("Invalid plan_limit value.".into()))?;

    // Extract API key info
    let key_usage = key
        .get("usage")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| ProviderError::Parse("Invalid key usage value.".into()))?;

    let key_limit = key.get("limit").and_then(|v| v.as_i64());

    // Extract plan name
    let plan = account
        .get("current_plan")
        .and_then(|v| v.as_str())
        .and_then(|s| {
            let trimmed = s.trim();
            if trimmed.is_empty() || trimmed.len() > 120 {
                None
            } else {
                Some(trimmed.to_string())
            }
        });

    // Build primary (account plan) window
    let primary = if plan_limit > 0 {
        Some(RateWindow::new(percent_of(
            plan_usage as f64,
            plan_limit as f64,
        )))
    } else {
        None
    };

    // Build secondary (API key) window
    let secondary = key_limit.and_then(|limit| {
        if limit > 0 {
            Some(RateWindow::new(percent_of(key_usage as f64, limit as f64)))
        } else {
            None
        }
    });

    Ok(ParsedUsage {
        primary,
        secondary,
        plan,
    })
}

fn percent_of(used: f64, limit: f64) -> f64 {
    if limit <= 0.0 {
        0.0
    } else {
        ((used / limit) * 100.0).clamp(0.0, 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_valid_usage() {
        let json = r#"{
            "account": {
                "plan_usage": 50,
                "plan_limit": 100,
                "current_plan": "Pro"
            },
            "key": {
                "usage": 25,
                "limit": 200
            }
        }"#;
        let data: Value = serde_json::from_str(json).unwrap();
        let result = parse_usage(&data).unwrap();

        assert!(result.primary.is_some());
        assert!(result.secondary.is_some());
        assert_eq!(result.plan, Some("Pro".to_string()));
    }

    #[test]
    fn test_parse_unlimited_key() {
        let json = r#"{
            "account": {
                "plan_usage": 50,
                "plan_limit": 100,
                "current_plan": null
            },
            "key": {
                "usage": 25,
                "limit": null
            }
        }"#;
        let data: Value = serde_json::from_str(json).unwrap();
        let result = parse_usage(&data).unwrap();

        assert!(result.primary.is_some());
        assert!(result.secondary.is_none());
        assert!(result.plan.is_none());
    }

    #[test]
    fn test_percent_of() {
        assert_eq!(percent_of(50.0, 100.0), 50.0);
        assert_eq!(percent_of(0.0, 100.0), 0.0);
        assert_eq!(percent_of(100.0, 100.0), 100.0);
        assert_eq!(percent_of(1.0, 100.0), 1.0);
        assert_eq!(percent_of(50.0, 0.0), 0.0);
    }

    #[test]
    fn test_parse_with_paygo() {
        let json = r#"{
            "account": {
                "plan_usage": 50,
                "plan_limit": 100,
                "paygo_usage": 10,
                "paygo_limit": 50,
                "current_plan": "Pro"
            },
            "key": {
                "usage": 25,
                "limit": 200
            }
        }"#;
        let data: Value = serde_json::from_str(json).unwrap();
        let result = parse_usage(&data).unwrap();

        assert!(result.primary.is_some());
        assert!(result.secondary.is_some());
        assert_eq!(result.plan, Some("Pro".to_string()));
    }

    #[test]
    fn test_parse_rejects_missing_account() {
        let json = r#"{
            "key": {
                "usage": 25,
                "limit": 200
            }
        }"#;
        let data: Value = serde_json::from_str(json).unwrap();
        let result = parse_usage(&data);

        assert!(result.is_err());
        assert!(matches!(result, Err(ProviderError::Parse(_))));
    }

    #[test]
    fn test_parse_rejects_missing_key() {
        let json = r#"{
            "account": {
                "plan_usage": 50,
                "plan_limit": 100
            }
        }"#;
        let data: Value = serde_json::from_str(json).unwrap();
        let result = parse_usage(&data);

        assert!(result.is_err());
        assert!(matches!(result, Err(ProviderError::Parse(_))));
    }

    #[test]
    fn test_parse_rejects_non_object() {
        let data: Value = serde_json::json!([1, 2, 3]);
        let result = parse_usage(&data);

        assert!(result.is_err());
        assert!(matches!(result, Err(ProviderError::Parse(_))));
    }

    #[test]
    fn test_parse_rejects_invalid_plan_usage() {
        let json = r#"{
            "account": {
                "plan_usage": "fifty",
                "plan_limit": 100
            },
            "key": {
                "usage": 25,
                "limit": 200
            }
        }"#;
        let data: Value = serde_json::from_str(json).unwrap();
        let result = parse_usage(&data);

        assert!(result.is_err());
    }

    #[test]
    fn test_parse_empty_plan_name() {
        let json = r#"{
            "account": {
                "plan_usage": 50,
                "plan_limit": 100,
                "current_plan": "   "
            },
            "key": {
                "usage": 25,
                "limit": 200
            }
        }"#;
        let data: Value = serde_json::from_str(json).unwrap();
        let result = parse_usage(&data).unwrap();

        // Trimmed empty string is rejected
        assert!(result.plan.is_none());
    }

    #[test]
    fn test_parse_oversized_plan_name() {
        let json = format!(
            r#"{{
            "account": {{
                "plan_usage": 50,
                "plan_limit": 100,
                "current_plan": "{}"
            }},
            "key": {{
                "usage": 25,
                "limit": 200
            }}
        }}"#,
            "x".repeat(121)
        );
        let data: Value = serde_json::from_str(&json).unwrap();
        let result = parse_usage(&data).unwrap();

        // Plan name > 120 chars is rejected
        assert!(result.plan.is_none());
    }
}
