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
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::read_bounded_response;
use super::format;

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
                session_label: "Plan credits",
                weekly_label: "API key",
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
            _ => {
                return Err(ProviderError::Other(format!(
                    "Tavily returned HTTP {}.",
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
        let usage = parsed.usage;
        let mut result = ProviderFetchResult::new(usage, "api");
        for detail in parsed.details {
            result = result.with_display_detail(Some(detail));
        }
        Ok(result)
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
    usage: UsageSnapshot,
    details: Vec<ProviderDisplayDetail>,
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

    // Extract PAYG info (optional)
    let paygo_usage = account.get("paygo_usage").and_then(|v| v.as_i64());
    let paygo_limit = account.get("paygo_limit").and_then(|v| v.as_i64());

    // Validate that if either PAYG field exists, both must be valid numbers
    if (paygo_usage.is_some() || paygo_limit.is_some())
        && !(paygo_usage.is_some() && paygo_limit.is_some())
    {
        return Err(ProviderError::Parse(
            "Tavily pay-as-you-go usage and limit must both be present together.".into(),
        ));
    }

    // Extract plan name (optional)
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
        RateWindow::new(percent_of(plan_usage as f64, plan_limit as f64))
    } else {
        return Err(ProviderError::Parse("Plan limit must be positive.".into()));
    };

    // Build secondary (API key) window - null limit means unlimited
    let secondary = if let Some(limit) = key_limit {
        if limit > 0 {
            Some(RateWindow::new(percent_of(key_usage as f64, limit as f64)))
        } else {
            None
        }
    } else {
        // Null key limit = unlimited, no quota window displayed
        None
    };

    // Build usage snapshot
    let mut usage = UsageSnapshot::new(primary);
    if let Some(secondary) = secondary {
        usage = usage
            .with_secondary(secondary)
            .with_secondary_label("API key");
    }
    if let Some(plan_name) = plan {
        usage = usage.with_login_method(plan_name);
    }

    // Build display details (used for Settings panel expandable section)
    let mut details = Vec::new();

    // Account plan section
    if let Some(detail) = ProviderDisplayDetail::new(
        "account-plan-used",
        "Used",
        format!("{} credits", format::number(plan_usage as f64, 0)),
    )
    .and_then(|d| d.with_section_title("Account plan"))
    {
        details.push(detail);
    }
    if let Some(detail) = ProviderDisplayDetail::new(
        "account-plan-limit",
        "Limit",
        format!("{} credits", format::number(plan_limit as f64, 0)),
    )
    .and_then(|d| d.with_section_title("Account plan"))
    {
        details.push(detail);
    }
    if let Some(detail) = ProviderDisplayDetail::new(
        "account-plan-remaining",
        "Remaining",
        format!("{} credits", format::number((plan_limit - plan_usage).max(0) as f64, 0)),
    )
    .and_then(|d| d.with_section_title("Account plan"))
    {
        details.push(detail);
    }

    // API key section
    if let Some(detail) =
        ProviderDisplayDetail::new("api-key-used", "Used", format!("{} credits", format::number(key_usage as f64, 0)))
            .and_then(|d| d.with_section_title("API key"))
    {
        details.push(detail);
    }
    if let Some(limit) = key_limit {
        if let Some(detail) =
            ProviderDisplayDetail::new("api-key-limit", "Limit", format!("{} credits", format::number(limit as f64, 0)))
                .and_then(|d| d.with_section_title("API key"))
        {
            details.push(detail);
        }
        if let Some(detail) = ProviderDisplayDetail::new(
            "api-key-remaining",
            "Remaining",
            format!("{} credits", format::number((limit - key_usage).max(0) as f64, 0)),
        )
        .and_then(|d| d.with_section_title("API key"))
        {
            details.push(detail);
        }
    } else {
        // Unlimited key
        if let Some(detail) =
            ProviderDisplayDetail::new("api-key-limit", "Limit", "Unlimited".to_string())
                .and_then(|d| d.with_section_title("API key"))
        {
            details.push(detail);
        }
    }

    // Pay-as-you-go section (if present)
    if let (Some(paygo_u), Some(paygo_l)) = (paygo_usage, paygo_limit) {
        if let Some(detail) =
            ProviderDisplayDetail::new("paygo-used", "Used", format!("{} credits", format::number(paygo_u as f64, 0)))
                .and_then(|d| d.with_section_title("Pay as you go"))
        {
            details.push(detail);
        }
        if let Some(detail) =
            ProviderDisplayDetail::new("paygo-limit", "Limit", format!("{} credits", format::number(paygo_l as f64, 0)))
                .and_then(|d| d.with_section_title("Pay as you go"))
        {
            details.push(detail);
        }
        if let Some(detail) = ProviderDisplayDetail::new(
            "paygo-remaining",
            "Remaining",
            format!("{} credits", format::number((paygo_l - paygo_u).max(0) as f64, 0)),
        )
        .and_then(|d| d.with_section_title("Pay as you go"))
        {
            details.push(detail);
        }
    }

    Ok(ParsedUsage { usage, details })
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

        assert_eq!(result.usage.primary.used_percent, 50.0);
        assert!(result.usage.secondary.is_some());
        assert_eq!(result.usage.secondary_label, Some("API key".to_string()));
        assert_eq!(result.usage.login_method, Some("Pro".to_string()));
        assert!(!result.details.is_empty());
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

        assert_eq!(result.usage.primary.used_percent, 50.0);
        assert!(result.usage.secondary.is_none());
        assert!(result.usage.login_method.is_none());
        assert!(!result.details.is_empty());
        // Should have api-key-limit detail with "Unlimited" label
        let api_key_detail = result.details.iter().find(|d| d.id() == "api-key-limit");
        assert!(api_key_detail.is_some());
        assert!(api_key_detail.unwrap().value().contains("Unlimited"));
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

        assert_eq!(result.usage.primary.used_percent, 50.0);
        assert!(result.usage.secondary.is_some());
        assert_eq!(result.usage.login_method, Some("Pro".to_string()));
        // Should have details for account-plan (3 rows), api-key (3 rows), and paygo (3 rows) = 9 total
        assert_eq!(result.details.len(), 9);
        let paygo_detail = result.details.iter().find(|d| d.id() == "paygo-used");
        assert!(paygo_detail.is_some());
    }

    #[test]
    fn test_parse_rejects_partial_paygo() {
        let json = r#"{
            "account": {
                "plan_usage": 50,
                "plan_limit": 100,
                "paygo_usage": 10
            },
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
        assert!(result.usage.login_method.is_none());
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
        assert!(result.usage.login_method.is_none());
    }

    #[test]
    fn test_parse_rejects_zero_plan_limit() {
        let json = r#"{
            "account": {
                "plan_usage": 50,
                "plan_limit": 0
            },
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
}
