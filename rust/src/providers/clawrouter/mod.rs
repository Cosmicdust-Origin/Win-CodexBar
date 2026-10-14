//! ClawRouter: monthly budget, spend and routed-provider usage for one policy key.
//!
//! Ported from upstream CodexBar v0.73.0 (`Resources/Plugins/clawrouter.js`,
//! `ClawRouterSettingsReader.swift`). The key is sent only as a Bearer token
//! to `GET {base}/v1/usage`; the configured base URL is validated before the
//! credential is resolved, redirects are never followed, and response bodies
//! are never echoed in errors.

mod model;
#[cfg(test)]
mod tests;

use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, StatusCode, Url};

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    ProviderStateKind, SourceMode,
};

const CREDENTIAL_TARGET: &str = "codexbar-clawrouter";
pub(crate) const API_KEY_ENV: &str = "CLAWROUTER_API_KEY";
const BASE_URL_ENV: &str = "CLAWROUTER_BASE_URL";
const DEFAULT_BASE_URL: &str = "https://clawrouter.openclaw.ai";
const DASHBOARD_URL: &str = "https://clawrouter.openclaw.ai/dashboard/access";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
/// Upstream `ctx.fail.authenticationExpired` text for HTTP 401/403.
const REJECTED_KEY_MESSAGE: &str =
    "ClawRouter rejected the API key. Check the key and its policy status.";

pub struct ClawRouterProvider {
    metadata: ProviderMetadata,
    client: Option<Client>,
    default_base_url: String,
    /// Reads the `CLAWROUTER_BASE_URL` fallback; tests inject a fixed lookup.
    base_url_env: fn() -> Option<String>,
}

impl ClawRouterProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::ClawRouter,
                display_name: "ClawRouter",
                session_label: "Monthly budget",
                weekly_label: "Requests",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some(DASHBOARD_URL),
                status_page_url: None,
                tertiary_label_key: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(REQUEST_TIMEOUT)
                // The Bearer credential must never follow a redirect to
                // another origin.
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .ok(),
            default_base_url: DEFAULT_BASE_URL.to_owned(),
            base_url_env: base_url_from_env,
        }
    }

    #[cfg(test)]
    fn with_client(default_base_url: &str, client: Client) -> Self {
        let mut provider = Self::new();
        provider.client = Some(client);
        provider.default_base_url = default_base_url.to_owned();
        provider.base_url_env = || None;
        provider
    }

    async fn fetch_api(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        // Validate the endpoint before touching the keyring or environment so
        // a malformed or plain-HTTP override never receives the key.
        let url = match configured_base_url(ctx, self.base_url_env) {
            Some(raw) => usage_url(&raw)?,
            None => default_usage_url(&self.default_base_url)?,
        };
        let api_key = crate::providers::resolve_api_key(
            ctx.api_key.as_deref(),
            CREDENTIAL_TARGET,
            &[API_KEY_ENV],
        )?;
        let client = self.client.as_ref().ok_or_else(|| {
            ProviderError::Other("Could not create a secure ClawRouter HTTP client.".into())
        })?;

        let timeout = Duration::from_secs(ctx.web_timeout.max(1)).min(REQUEST_TIMEOUT);
        let response = client
            .get(url)
            .bearer_auth(api_key)
            .header("Accept", "application/json")
            .timeout(timeout)
            .send()
            .await?;
        check_status(response.status())?;

        let bytes = crate::providers::read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|error| match error {
                crate::providers::BoundedBodyError::TooLarge => ProviderError::Parse(
                    "Could not parse ClawRouter usage: response is too large".into(),
                ),
                crate::providers::BoundedBodyError::Read(error) => ProviderError::Network(error),
            })?;
        let usage = model::parse_usage(&bytes)?;
        Ok(model::build_result(&usage))
    }
}

impl Default for ClawRouterProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for ClawRouterProvider {
    fn id(&self) -> ProviderId {
        ProviderId::ClawRouter
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => self.fetch_api(ctx).await,
            source => Err(ProviderError::UnsupportedSource(source)),
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }

    /// A rejected key (HTTP 401/403) is a sign-in gate, matching upstream's
    /// `authenticationExpired`; other `Other` errors keep the default mapping.
    fn error_state_kind(&self, error: &ProviderError) -> ProviderStateKind {
        match error {
            ProviderError::Other(message) if message == REJECTED_KEY_MESSAGE => {
                ProviderStateKind::NeedsAuthentication
            }
            _ => error.state_kind(),
        }
    }
}

/// The saved Base URL, then `CLAWROUTER_BASE_URL`; `None` selects the hosted service.
fn configured_base_url(ctx: &FetchContext, env: fn() -> Option<String>) -> Option<String> {
    ctx.gateway_url
        .clone()
        .filter(|url| !url.trim().is_empty())
        .or_else(|| env().filter(|url| !url.trim().is_empty()))
}

fn base_url_from_env() -> Option<String> {
    std::env::var(BASE_URL_ENV).ok()
}

/// Classify a non-success status without reading or echoing the body.
fn check_status(status: StatusCode) -> Result<(), ProviderError> {
    let code = status.as_u16();
    match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            Err(ProviderError::Other(REJECTED_KEY_MESSAGE.into()))
        }
        _ if status.is_success() => Ok(()),
        _ => Err(ProviderError::Other(format!(
            "ClawRouter API returned HTTP {code}."
        ))),
    }
}

/// Build `{root}/v1/usage` from a configured Base URL override.
///
/// The value may point at the service root or `/v1`; trailing slashes are
/// dropped and a scheme-less value is treated as HTTPS. Only HTTPS URLs
/// without embedded credentials, queries or fragments are accepted.
fn usage_url(raw: &str) -> Result<Url, ProviderError> {
    let invalid = || {
        ProviderError::Other(
            "ClawRouter Base URL is invalid. Use an HTTPS URL without embedded credentials.".into(),
        )
    };
    let raw = raw.trim();
    // Decide on the scheme before trimming slashes, so "https://" stays a
    // URL with no host instead of becoming the host name "https".
    let has_scheme = raw.contains("://");
    let raw = raw.trim_end_matches('/');
    if raw.is_empty() || raw.contains(['?', '#']) || raw.chars().any(char::is_whitespace) {
        return Err(invalid());
    }
    let candidate = if has_scheme {
        raw.to_owned()
    } else {
        format!("https://{raw}")
    };
    let mut url = Url::parse(&candidate).map_err(|_| invalid())?;
    if url.scheme() != "https"
        || url.host_str().is_none_or(str::is_empty)
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(invalid());
    }
    let path = url.path().trim_end_matches('/');
    let path = if path.ends_with("/v1") {
        format!("{path}/usage")
    } else {
        format!("{path}/v1/usage")
    };
    url.set_path(&path);
    Ok(url)
}

fn default_usage_url(base: &str) -> Result<Url, ProviderError> {
    let base = base.trim_end_matches('/');
    Url::parse(&format!("{base}/v1/usage"))
        .map_err(|_| ProviderError::Other("ClawRouter default URL is invalid.".into()))
}

/// Validate a configured Base URL before saving it to settings. An empty value
/// is valid and selects the hosted service.
pub fn validate_gateway_url(raw: &str) -> Result<(), ProviderError> {
    if raw.trim().is_empty() {
        return Ok(());
    }
    usage_url(raw).map(|_| ())
}
