//! Aerostack monthly AI-token usage provider
//!
//! Fetches account billing AI-token usage through the official account billing API.
//! Requires an account JWT token for authentication.

use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    SourceMode,
};

pub struct AerostackProvider {
    client: Client,
}

impl AerostackProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }
}

impl Default for AerostackProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for AerostackProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Aerostack
    }

    fn metadata(&self) -> &ProviderMetadata {
        &AEROSTACK_METADATA
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        let _api_key = ctx.api_key.as_deref().ok_or_else(|| {
            ProviderError::Other(
                "Set an Aerostack account JWT in Settings or AEROSTACK_TOKEN.".to_string(),
            )
        })?;

        let response = self
            .client
            .get("https://api.aerostack.dev/api/billing/usage")
            .send()
            .await
            .map_err(ProviderError::Network)?;

        let status = response.status().as_u16();
        let body = response.text().await.map_err(ProviderError::Network)?;

        // Check HTTP status
        match status {
            401 => {
                return Err(ProviderError::Other(
                    "Aerostack account JWT expired or was rejected.".to_string(),
                ));
            }
            403 => {
                return Err(ProviderError::Other(format!(
                    "Aerostack returned HTTP 403 (permission denied)."
                )));
            }
            429 => {
                return Err(ProviderError::Other(format!(
                    "Aerostack returned HTTP 429 (rate limited)."
                )));
            }
            500..=599 => {
                return Err(ProviderError::Other(format!(
                    "Aerostack returned HTTP {}.",
                    status
                )));
            }
            200 => {}
            _ => {
                return Err(ProviderError::Other(format!(
                    "Aerostack returned HTTP {}.",
                    status
                )));
            }
        }

        // Parse JSON response
        let root: Value = serde_json::from_str(&body).map_err(|_| {
            ProviderError::Other(
                "Aerostack returned an unrecognized account AI-token usage response.".to_string(),
            )
        })?;

        parse_aerostack_response(&root)
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Cli]
    }
}

fn parse_aerostack_response(root: &Value) -> Result<ProviderFetchResult, ProviderError> {
    use crate::core::{RateWindow, UsageSnapshot};

    let fail_msg = "Aerostack returned an unrecognized account AI-token usage response.";

    // Extract the nested tokens object
    let tokens_obj = root
        .get("usage")
        .and_then(|u| u.get("ai_tokens"))
        .and_then(|v| v.as_object())
        .ok_or_else(|| ProviderError::Other(fail_msg.to_string()))?;

    // Helper to parse counter values
    let get_counter = |key: &str| -> Result<Option<f64>, ProviderError> {
        match tokens_obj.get(key) {
            None => Ok(None),
            Some(Value::Null) => Ok(None),
            Some(Value::Number(n)) => {
                let f = n
                    .as_f64()
                    .ok_or_else(|| ProviderError::Other(fail_msg.to_string()))?;
                if f.is_finite() && f >= 0.0 && f.fract() == 0.0 {
                    Ok(Some(f))
                } else {
                    Err(ProviderError::Other(fail_msg.to_string()))
                }
            }
            Some(_) => Err(ProviderError::Other(fail_msg.to_string())),
        }
    };

    let used = get_counter("used")?;
    let limit = get_counter("limit")?;

    // At least one counter must be present
    if used.is_none() && limit.is_none() {
        return Err(ProviderError::Other(fail_msg.to_string()));
    }

    // Calculate percentages for primary window
    // Only show percentage if both used and limit are present, and limit > 0
    let primary = if let (Some(u), Some(l)) = (used, limit) {
        if l > 0.0 {
            RateWindow::new(((u / l) * 100.0).clamp(0.0, 100.0))
        } else {
            RateWindow::new(0.0)
        }
    } else {
        RateWindow::new(0.0)
    };

    let snapshot = UsageSnapshot::new(primary);
    let result = ProviderFetchResult::new(snapshot, "api");

    Ok(result)
}

/// Validate YYYY-MM format
fn is_valid_period(period: &str) -> bool {
    // Match YYYY-MM format
    let parts: Vec<&str> = period.split('-').collect();
    if parts.len() != 2 {
        return false;
    }

    let year = parts[0];
    let month = parts[1];

    if year.len() != 4 || month.len() != 2 {
        return false;
    }

    if !year.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }

    if !month.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }

    if let Ok(m) = month.parse::<u32>() {
        m >= 1 && m <= 12
    } else {
        false
    }
}

/// Format a number with thousands separators
fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.is_finite() {
        format!("{:.0}", n)
    } else {
        format!("{}", n)
    }
}

const AEROSTACK_METADATA: ProviderMetadata = ProviderMetadata {
    id: ProviderId::Aerostack,
    display_name: "Aerostack",
    session_label: "Monthly AI tokens",
    weekly_label: "Weekly",
    supports_opus: false,
    supports_credits: false,
    default_enabled: false,
    is_primary: false,
    dashboard_url: Some("https://app.aerostack.dev/settings/billing"),
    status_page_url: None,
    tertiary_label_key: None,
};

#[cfg(test)]
mod tests;
