//! Cosmic AI project token usage provider
//!
//! Fetches project AI token usage through the official CLI's dashboard API.
//! Requires Personal Access Token and Project ID.

use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    SourceMode,
};

pub struct CosmicProvider {
    client: Client,
}

impl CosmicProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }
}

impl Default for CosmicProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for CosmicProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Cosmic
    }

    fn metadata(&self) -> &ProviderMetadata {
        &COSMIC_METADATA
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        let project_id = ctx
            .workspace_id
            .as_deref()
            .and_then(|s| {
                let trimmed = s.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            })
            .ok_or_else(|| {
                ProviderError::Other(
                    "Set a Cosmic Project ID in Settings or COSMIC_PROJECT_ID.".to_string(),
                )
            })?;

        let _api_key = ctx.api_key.as_deref().ok_or_else(|| {
            ProviderError::Other(
                "Set a Cosmic Personal Access Token in Settings or COSMIC_TOKEN.".to_string(),
            )
        })?;

        // Build the URL using reqwest's query parameter support
        let url = "https://dapi.cosmicjs.com/v3/projects/usage";

        let response = self
            .client
            .get(url)
            .query(&[("project_id", project_id)])
            .header("Origin", "https://app.cosmicjs.com")
            .send()
            .await
            .map_err(ProviderError::Network)?;

        let status = response.status().as_u16();
        let body = response.text().await.map_err(ProviderError::Network)?;

        // Check HTTP status
        match status {
            200 => {}
            _ => {
                return Err(ProviderError::Other(format!(
                    "Cosmic AI returned HTTP {}.",
                    status
                )));
            }
        }

        // Parse JSON response
        let root: Value = serde_json::from_str(&body).map_err(|_| {
            ProviderError::Other(
                "Cosmic AI returned an unrecognized project AI-token usage response.".to_string(),
            )
        })?;

        parse_cosmic_response(&root)
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Cli]
    }
}

fn parse_cosmic_response(root: &Value) -> Result<ProviderFetchResult, ProviderError> {
    use crate::core::{ProviderDisplayDetail, RateWindow, UsageSnapshot};

    let fail = || {
        ProviderError::Other(
            "Cosmic AI returned an unrecognized project AI-token usage response.".to_string(),
        )
    };

    // Extract usage and allowance objects
    let usage = root
        .get("usage")
        .and_then(|u| u.get("ai"))
        .and_then(|a| a.as_object())
        .ok_or_else(fail)?;

    let allowance = root
        .get("plan_info")
        .and_then(|p| p.get("ai_tokens"))
        .and_then(|t| t.as_object())
        .ok_or_else(fail)?;

    // Parse input tokens
    let input_used = parse_number(usage.get("input_tokens"))?;
    let input_limit = parse_limit_value(allowance.get("max_input"))?;

    // Parse output tokens
    let output_used = parse_number(usage.get("output_tokens"))?;
    let output_limit = parse_limit_value(allowance.get("max_output"))?;

    // Both dimensions must have at least some data
    if input_used.is_none()
        && input_limit.is_none()
        && output_used.is_none()
        && output_limit.is_none()
    {
        return Err(fail());
    }

    let mut display_details = Vec::new();

    // Input tokens section
    if let Some(used) = input_used
        && let Some(d) =
            ProviderDisplayDetail::new("input_tokens_used", "Used", format_number(used))
                .and_then(|row| row.with_section_title("Input tokens"))
    {
        display_details.push(d);
    }
    if let Some(limit) = input_limit {
        if let Some(d) =
            ProviderDisplayDetail::new("input_tokens_allowance", "Allowance", format_number(limit))
                .and_then(|row| row.with_section_title("Input tokens"))
        {
            display_details.push(d);
        }
    } else if let Some(raw_limit) = allowance.get("max_input")
        && let Some(s) = raw_limit.as_str()
    {
        let s = s.trim();
        if !s.is_empty() {
            let label = if s.eq_ignore_ascii_case("unlimited") {
                "Unlimited"
            } else {
                "Not reported"
            };
            if let Some(d) =
                ProviderDisplayDetail::new("input_tokens_allowance", "Allowance", label)
                    .and_then(|row| row.with_section_title("Input tokens"))
            {
                display_details.push(d);
            }
        }
    }
    if let (Some(used), Some(limit)) = (input_used, input_limit) {
        let remaining = (limit - used).max(0.0);
        if let Some(d) = ProviderDisplayDetail::new(
            "input_tokens_remaining",
            "Remaining",
            format_number(remaining),
        )
        .and_then(|row| row.with_section_title("Input tokens"))
        {
            display_details.push(d);
        }
        if used > limit
            && let Some(d) = ProviderDisplayDetail::new(
                "input_tokens_above",
                "Above allowance",
                format_number(used - limit),
            )
            .and_then(|row| row.with_section_title("Input tokens"))
        {
            display_details.push(d);
        }
    }

    // Output tokens section
    if let Some(used) = output_used
        && let Some(d) =
            ProviderDisplayDetail::new("output_tokens_used", "Used", format_number(used))
                .and_then(|row| row.with_section_title("Output tokens"))
    {
        display_details.push(d);
    }
    if let Some(limit) = output_limit {
        if let Some(d) =
            ProviderDisplayDetail::new("output_tokens_allowance", "Allowance", format_number(limit))
                .and_then(|row| row.with_section_title("Output tokens"))
        {
            display_details.push(d);
        }
    } else if let Some(raw_limit) = allowance.get("max_output")
        && let Some(s) = raw_limit.as_str()
    {
        let s = s.trim();
        if !s.is_empty() {
            let label = if s.eq_ignore_ascii_case("unlimited") {
                "Unlimited"
            } else {
                "Not reported"
            };
            if let Some(d) =
                ProviderDisplayDetail::new("output_tokens_allowance", "Allowance", label)
                    .and_then(|row| row.with_section_title("Output tokens"))
            {
                display_details.push(d);
            }
        }
    }
    if let (Some(used), Some(limit)) = (output_used, output_limit) {
        let remaining = (limit - used).max(0.0);
        if let Some(d) = ProviderDisplayDetail::new(
            "output_tokens_remaining",
            "Remaining",
            format_number(remaining),
        )
        .and_then(|row| row.with_section_title("Output tokens"))
        {
            display_details.push(d);
        }
        if used > limit
            && let Some(d) = ProviderDisplayDetail::new(
                "output_tokens_above",
                "Above allowance",
                format_number(used - limit),
            )
            .and_then(|row| row.with_section_title("Output tokens"))
        {
            display_details.push(d);
        }
    }

    if display_details.is_empty() {
        return Err(fail());
    }

    // Calculate percentages for primary window (only if we have both used and a valid numeric limit > 0)
    // Matches upstream: return window only if used !== undefined && limit !== undefined && limit > 0
    let primary = if let (Some(used), Some(limit)) = (input_used, input_limit) {
        if limit > 0.0 {
            RateWindow::new(((used / limit) * 100.0).clamp(0.0, 100.0))
        } else {
            RateWindow::new(0.0)
        }
    } else {
        RateWindow::new(0.0)
    };

    // Calculate percentages for secondary window (only if we have both used and a valid numeric limit > 0)
    let secondary = if let (Some(used), Some(limit)) = (output_used, output_limit) {
        if limit > 0.0 {
            Some(RateWindow::new(((used / limit) * 100.0).clamp(0.0, 100.0)))
        } else {
            None
        }
    } else {
        None
    };

    let mut snapshot = UsageSnapshot::new(primary).with_secondary_label("Output tokens");

    if let Some(sec) = secondary {
        snapshot = snapshot.with_secondary(sec);
    }

    let mut result = ProviderFetchResult::new(snapshot, "api");
    result.display_details = display_details;

    Ok(result)
}

/// Parse a numeric value, rejecting invalid types
fn parse_number(value: Option<&Value>) -> Result<Option<f64>, ProviderError> {
    match value {
        None => Ok(None),
        Some(v) => match v {
            Value::Null => Ok(None),
            Value::Number(n) => {
                let f = n.as_f64().ok_or_else(|| {
                    ProviderError::Other(
                        "Cosmic AI returned an unrecognized project AI-token usage response."
                            .to_string(),
                    )
                })?;
                if f.is_finite() && f >= 0.0 && f.fract() == 0.0 && f < 9007199254740992.0 {
                    Ok(Some(f))
                } else {
                    Err(ProviderError::Other(
                        "Cosmic AI returned an unrecognized project AI-token usage response."
                            .to_string(),
                    ))
                }
            }
            _ => Err(ProviderError::Other(
                "Cosmic AI returned an unrecognized project AI-token usage response.".to_string(),
            )),
        },
    }
}

/// Parse limit value which can be a number or a string with k/M suffix
fn parse_limit_value(value: Option<&Value>) -> Result<Option<f64>, ProviderError> {
    match value {
        None => Ok(None),
        Some(v) => match v {
            Value::Null => Ok(None),
            Value::String(s) => parse_limit_string(s),
            Value::Number(n) => {
                let f = n.as_f64().ok_or_else(|| {
                    ProviderError::Other(
                        "Cosmic AI returned an unrecognized project AI-token usage response."
                            .to_string(),
                    )
                })?;
                if f.is_finite() && f >= 0.0 && f.fract() == 0.0 && f < 9007199254740992.0 {
                    Ok(Some(f))
                } else {
                    Err(ProviderError::Other(
                        "Cosmic AI returned an unrecognized project AI-token usage response."
                            .to_string(),
                    ))
                }
            }
            _ => Err(ProviderError::Other(
                "Cosmic AI returned an unrecognized project AI-token usage response.".to_string(),
            )),
        },
    }
}

/// Parse limit string, handling k/M suffixes
fn parse_limit_string(s: &str) -> Result<Option<f64>, ProviderError> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }

    // Try to parse with regex-like logic
    let mut chars = s.chars().peekable();
    let mut num_str = String::new();
    let mut multiplier = 1.0;

    // Read numeric part (including decimal point)
    while let Some(&ch) = chars.peek() {
        if ch.is_ascii_digit() || ch == '.' {
            num_str.push(ch);
            chars.next();
        } else {
            break;
        }
    }

    // Read suffix
    if let Some(suffix) = chars.next() {
        match suffix {
            'k' | 'K' => multiplier = 1000.0,
            'm' | 'M' => multiplier = 1000000.0,
            _ => return Ok(None), // Unrecognized suffix
        }
    }

    // Ensure suffix consumed everything
    if chars.next().is_some() {
        return Ok(None);
    }

    // Parse the number
    let num: f64 = num_str.parse().map_err(|_| {
        ProviderError::Other(
            "Cosmic AI returned an unrecognized project AI-token usage response.".to_string(),
        )
    })?;

    if !num.is_finite() || num < 0.0 {
        return Err(ProviderError::Other(
            "Cosmic AI returned an unrecognized project AI-token usage response.".to_string(),
        ));
    }

    Ok(Some(num * multiplier))
}

fn format_number(n: f64) -> String {
    format!("{:.0}", n)
}

const COSMIC_METADATA: ProviderMetadata = ProviderMetadata {
    id: ProviderId::Cosmic,
    display_name: "Cosmic AI",
    session_label: "Input tokens",
    weekly_label: "",
    supports_opus: false,
    supports_credits: false,
    default_enabled: false,
    is_primary: false,
    dashboard_url: Some("https://app.cosmicjs.com"),
    status_page_url: None,
    tertiary_label_key: None,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_limit_string_with_k_suffix() {
        assert!(matches!(parse_limit_string("300k"), Ok(Some(300000.0))));
        assert!(matches!(parse_limit_string("300K"), Ok(Some(300000.0))));
    }

    #[test]
    fn test_parse_limit_string_with_m_suffix() {
        assert!(matches!(parse_limit_string("1.5M"), Ok(Some(1500000.0))));
        assert!(matches!(parse_limit_string("1.5m"), Ok(Some(1500000.0))));
    }

    #[test]
    fn test_parse_limit_string_plain_number() {
        assert!(matches!(parse_limit_string("1000"), Ok(Some(1000.0))));
    }

    #[test]
    fn test_parse_limit_string_empty() {
        assert!(matches!(parse_limit_string(""), Ok(None)));
    }

    #[test]
    fn test_parse_limit_string_unlimited() {
        // "Unlimited" is not a valid numeric format, returns None
        assert!(matches!(parse_limit_string("unlimited"), Ok(None)));
    }

    #[tokio::test]
    async fn cosmic_requires_project_id() {
        let provider = CosmicProvider::new();
        let ctx = FetchContext {
            api_key: Some("cos_test_token".to_string()),
            workspace_id: None,
            ..Default::default()
        };
        let result = provider.fetch_usage(&ctx).await;
        assert!(matches!(result, Err(ProviderError::Other(msg)) if msg.contains("Project ID")));
    }

    #[tokio::test]
    async fn cosmic_requires_api_key() {
        let provider = CosmicProvider::new();
        let ctx = FetchContext {
            api_key: None,
            workspace_id: Some("project_123".to_string()),
            ..Default::default()
        };
        let result = provider.fetch_usage(&ctx).await;
        assert!(matches!(result, Err(ProviderError::Other(msg)) if msg.contains("Token")));
    }
}
