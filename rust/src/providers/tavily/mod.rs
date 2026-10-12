//! Tavily API usage provider.
//!
//! Fetches account plan, API key, and pay-as-you-go credit usage from
//! `GET https://api.tavily.com/usage` with bearer authentication.
//!
//! Ported from upstream CodexBar v0.74.1 (`Resources/Plugins/tavily.js`).

use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde_json::Value;

use super::format;
use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::read_bounded_response;

const USAGE_URL: &str = "https://api.tavily.com/usage";
const CREDENTIAL_TARGET: &str = "codexbar-tavily";
pub const API_KEY_ENV: &str = "TAVILY_API_KEY";
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

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

        let data: Value = serde_json::from_slice(&body).map_err(|_| unrecognized())?;

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

const UNRECOGNIZED: &str = "Tavily returned an unrecognized credit usage response.";

fn unrecognized() -> ProviderError {
    ProviderError::Other(UNRECOGNIZED.into())
}

/// Upstream `count`: a non-negative safe integer.
fn count(value: Option<&Value>) -> Result<u64, ProviderError> {
    const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
    let Some(Value::Number(n)) = value else {
        return Err(unrecognized());
    };
    if let Some(v) = n.as_u64() {
        return if v <= MAX_SAFE_INTEGER {
            Ok(v)
        } else {
            Err(unrecognized())
        };
    }
    match n.as_f64() {
        // JSON like `5.0` is still a safe integer to JavaScript.
        Some(f) if f.fract() == 0.0 && (0.0..=MAX_SAFE_INTEGER as f64).contains(&f) => {
            // `+ 0.0` turns -0 into 0.
            format!("{:.0}", f + 0.0)
                .parse()
                .map_err(|_| unrecognized())
        }
        _ => Err(unrecognized()),
    }
}

fn credits(value: u64) -> String {
    format!("{} credits", format::number(value as f64, 0))
}

/// One titled section: Used, Limit ("Unlimited" when `cap` is None), and
/// Remaining only for a finite cap.
fn section(
    details: &mut Vec<ProviderDisplayDetail>,
    id: &str,
    title: &str,
    usage: u64,
    cap: Option<u64>,
) {
    let mut rows = vec![
        ("used", "Used", credits(usage)),
        (
            "limit",
            "Limit",
            cap.map_or_else(|| "Unlimited".to_string(), credits),
        ),
    ];
    if let Some(cap) = cap {
        rows.push(("remaining", "Remaining", credits(cap.saturating_sub(usage))));
    }
    for (suffix, label, value) in rows {
        if let Some(detail) = ProviderDisplayDetail::new(format!("{id}-{suffix}"), label, value)
            .and_then(|d| d.with_section_title(title))
        {
            details.push(detail);
        }
    }
}

fn parse_usage(payload: &Value) -> Result<ParsedUsage, ProviderError> {
    let data = payload.as_object().ok_or_else(unrecognized)?;
    let account = data
        .get("account")
        .and_then(Value::as_object)
        .ok_or_else(unrecognized)?;
    let key = data
        .get("key")
        .and_then(Value::as_object)
        .ok_or_else(unrecognized)?;

    let used = count(account.get("plan_usage"))?;
    let limit = count(account.get("plan_limit"))?;
    let key_used = count(key.get("usage"))?;
    let key_limit = match key.get("limit") {
        Some(Value::Null) => None,
        other => Some(count(other)?),
    };

    let mut details = Vec::new();
    section(
        &mut details,
        "account-plan",
        "Account plan",
        used,
        Some(limit),
    );
    section(&mut details, "api-key", "API key", key_used, key_limit);
    if account.contains_key("paygo_usage") || account.contains_key("paygo_limit") {
        let paygo_used = count(account.get("paygo_usage"))?;
        let paygo_limit = count(account.get("paygo_limit"))?;
        section(
            &mut details,
            "paygo",
            "Pay as you go",
            paygo_used,
            Some(paygo_limit),
        );
    }

    // JavaScript `plan.length` counts UTF-16 units of the untrimmed string.
    let login_method = match account.get("current_plan") {
        None => "API key".to_string(),
        Some(Value::String(plan))
            if !plan.trim().is_empty() && plan.encode_utf16().count() <= 120 =>
        {
            plan.trim().to_string()
        }
        Some(_) => return Err(unrecognized()),
    };

    // Upstream omits a window whose limit is zero; the card then draws no bar.
    let primary = if limit > 0 {
        RateWindow::new(percent_of(used as f64, limit as f64))
    } else {
        RateWindow::informational("Account plan").hidden_on_card()
    };
    let mut usage = UsageSnapshot::new(primary).with_login_method(login_method);
    if let Some(key_limit) = key_limit.filter(|limit| *limit > 0) {
        usage = usage
            .with_secondary(RateWindow::new(percent_of(
                key_used as f64,
                key_limit as f64,
            )))
            .with_secondary_label("API key");
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
    use serde_json::json;

    fn rows(result: &ParsedUsage) -> Vec<(String, String, String)> {
        result
            .details
            .iter()
            .map(|d| {
                (
                    d.section_title().unwrap_or_default().to_string(),
                    d.title().to_string(),
                    d.value().to_string(),
                )
            })
            .collect()
    }

    fn row(section: &str, label: &str, value: &str) -> (String, String, String) {
        (section.to_string(), label.to_string(), value.to_string())
    }

    fn payload(account: Value, key: Value) -> Value {
        json!({ "account": account, "key": key })
    }

    fn assert_unrecognized(result: Result<ParsedUsage, ProviderError>) {
        match result {
            Err(ProviderError::Other(message)) => assert_eq!(message, UNRECOGNIZED),
            other => panic!("expected unrecognized response, got {other:?}"),
        }
    }

    #[test]
    fn full_payload_matches_upstream_sections() {
        let result = parse_usage(&payload(
            json!({ "plan_usage": 1200, "plan_limit": 4000, "paygo_usage": 10, "paygo_limit": 50, "current_plan": " Pro " }),
            json!({ "usage": 800, "limit": 2000 }),
        ))
        .unwrap();

        assert_eq!(result.usage.primary.used_percent, 30.0);
        assert!(!result.usage.primary.hidden_on_card);
        assert_eq!(
            result.usage.secondary.as_ref().map(|w| w.used_percent),
            Some(40.0)
        );
        assert_eq!(result.usage.secondary_label.as_deref(), Some("API key"));
        assert_eq!(result.usage.login_method.as_deref(), Some("Pro"));
        assert_eq!(
            rows(&result),
            [
                row("Account plan", "Used", "1,200 credits"),
                row("Account plan", "Limit", "4,000 credits"),
                row("Account plan", "Remaining", "2,800 credits"),
                row("API key", "Used", "800 credits"),
                row("API key", "Limit", "2,000 credits"),
                row("API key", "Remaining", "1,200 credits"),
                row("Pay as you go", "Used", "10 credits"),
                row("Pay as you go", "Limit", "50 credits"),
                row("Pay as you go", "Remaining", "40 credits"),
            ]
        );
    }

    #[test]
    fn unlimited_key_has_no_secondary_and_no_remaining_row() {
        let result = parse_usage(&payload(
            json!({ "plan_usage": 5, "plan_limit": 10 }),
            json!({ "usage": 3, "limit": null }),
        ))
        .unwrap();

        assert!(result.usage.secondary.is_none());
        assert_eq!(result.usage.login_method.as_deref(), Some("API key"));
        assert_eq!(
            rows(&result)[3..],
            [
                row("API key", "Used", "3 credits"),
                row("API key", "Limit", "Unlimited")
            ]
        );
    }

    #[test]
    fn zero_limits_keep_the_sections_but_draw_no_bars() {
        let result = parse_usage(&payload(
            json!({ "plan_usage": 7, "plan_limit": 0 }),
            json!({ "usage": 2, "limit": 0 }),
        ))
        .unwrap();

        assert!(result.usage.primary.hidden_on_card);
        assert!(result.usage.primary.is_informational);
        assert!(result.usage.secondary.is_none());
        assert_eq!(
            rows(&result)[2],
            row("Account plan", "Remaining", "0 credits")
        );
    }

    #[test]
    fn rejects_shapes_upstream_rejects() {
        let ok_account = json!({ "plan_usage": 1, "plan_limit": 2 });
        let ok_key = json!({ "usage": 1, "limit": 2 });
        assert_unrecognized(parse_usage(&json!([1, 2])));
        assert_unrecognized(parse_usage(&json!({ "key": ok_key })));
        assert_unrecognized(parse_usage(&json!({ "account": ok_account })));
        assert_unrecognized(parse_usage(&payload(
            json!({ "plan_usage": -1, "plan_limit": 2 }),
            ok_key.clone(),
        )));
        assert_unrecognized(parse_usage(&payload(
            json!({ "plan_usage": 1.5, "plan_limit": 2 }),
            ok_key.clone(),
        )));
        assert_unrecognized(parse_usage(&payload(
            json!({ "plan_usage": "1", "plan_limit": 2 }),
            ok_key.clone(),
        )));
        assert_unrecognized(parse_usage(&payload(
            ok_account.clone(),
            json!({ "usage": 1 }),
        )));
        assert_unrecognized(parse_usage(&payload(
            ok_account.clone(),
            json!({ "usage": 1, "limit": "2" }),
        )));
        assert_unrecognized(parse_usage(&payload(
            json!({ "plan_usage": 1, "plan_limit": 2, "paygo_usage": 1 }),
            ok_key.clone(),
        )));
    }

    #[test]
    fn plan_name_must_be_a_short_non_blank_string_when_present() {
        let key = json!({ "usage": 1, "limit": 2 });
        let with_plan = |plan: Value| {
            parse_usage(&payload(
                json!({ "plan_usage": 1, "plan_limit": 2, "current_plan": plan }),
                key.clone(),
            ))
        };
        assert_unrecognized(with_plan(json!(null)));
        assert_unrecognized(with_plan(json!("   ")));
        assert_unrecognized(with_plan(json!(5)));
        assert_unrecognized(with_plan(json!("x".repeat(121))));
        assert_eq!(
            with_plan(json!("x".repeat(120)))
                .unwrap()
                .usage
                .login_method
                .map(|m| m.len()),
            Some(120)
        );
    }

    #[test]
    fn percent_is_clamped() {
        assert_eq!(percent_of(50.0, 100.0), 50.0);
        assert_eq!(percent_of(150.0, 100.0), 100.0);
        assert_eq!(percent_of(50.0, 0.0), 0.0);
    }
}
