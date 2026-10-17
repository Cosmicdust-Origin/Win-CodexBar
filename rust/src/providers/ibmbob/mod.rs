//! IBM Bob: monthly Bobcoin usage for every team visible to one API key.
//!
//! Ported from upstream CodexBar v0.73.0 (`IBMBobUsageFetcher.swift`,
//! `IBMBobSettingsReader.swift`, `IBMBobProviderDescriptor.swift`). The key
//! is read from `GET /admin/v1/profile` on the fixed US-East host, then each
//! team's budget from `GET /admin/v1/teams/{team}/users/{user}` on the
//! instance's regional host. Credentials are sent only to HTTPS hosts under
//! `bob.ibm.com`, redirects are never followed, and response bodies are never
//! echoed in errors.

mod model;
#[cfg(test)]
mod tests;

use std::time::Duration;

use async_trait::async_trait;
use base64::Engine;
use reqwest::{Client, StatusCode, Url};

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    SourceMode,
};

const CREDENTIAL_TARGET: &str = "codexbar-ibmbob";
pub(crate) const API_KEY_ENV: &str = "BOBSHELL_API_KEY";
const BASE_URL: &str = "https://api.us-east.bob.ibm.com";
const DASHBOARD_URL: &str = "https://bob.ibm.com";
const STATUS_URL: &str = "https://status.bob.ibm.com";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
/// Upper bound on the regional host echoed in an error message.
const MAX_ECHOED_HOST_CHARS: usize = 253;

pub struct IBMBobProvider {
    metadata: ProviderMetadata,
    client: Option<Client>,
    base_url: String,
    /// Tests send validated regional requests to a local server instead.
    #[cfg(test)]
    regional_override: Option<Url>,
}

impl IBMBobProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::IBMBob,
                display_name: "IBM Bob",
                session_label: "Monthly Bobcoins",
                weekly_label: "Monthly Bobcoins",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some(DASHBOARD_URL),
                status_page_url: Some(STATUS_URL),
                tertiary_label_key: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(REQUEST_TIMEOUT)
                // The credential must never follow a redirect off bob.ibm.com.
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .ok(),
            base_url: BASE_URL.to_owned(),
            #[cfg(test)]
            regional_override: None,
        }
    }

    #[cfg(test)]
    fn with_client(base_url: &str, client: Client) -> Self {
        let mut provider = Self::new();
        provider.client = Some(client);
        provider.base_url = base_url.trim_end_matches('/').to_owned();
        provider.regional_override = Url::parse(base_url).ok();
        provider
    }

    /// The validated regional host, or the fixed host when none is reported.
    fn regional_base(&self, region_domain: Option<&str>) -> Result<String, ProviderError> {
        let Some(url) = regional_base_url(region_domain)? else {
            return Ok(self.base_url.clone());
        };
        let url = self.local_regional_url().unwrap_or(url);
        Ok(url.as_str().trim_end_matches('/').to_owned())
    }

    #[cfg(test)]
    fn local_regional_url(&self) -> Option<Url> {
        self.regional_override.clone()
    }

    #[cfg(not(test))]
    fn local_regional_url(&self) -> Option<Url> {
        None
    }

    async fn get(
        &self,
        client: &Client,
        url: &str,
        token: &str,
        instance_and_team: Option<(&str, &str)>,
        timeout: Duration,
    ) -> Result<Vec<u8>, ProviderError> {
        let mut request = client
            .get(url)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("Authorization", authorization_value(token))
            .header("User-Agent", "CodexBar")
            .timeout(timeout);
        if let Some((instance_id, team_id)) = instance_and_team {
            request = request
                .header("x-instance-id", instance_id)
                .header("x-team-id", team_id);
        }
        let response = request.send().await?;
        check_status(response.status())?;
        crate::providers::read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|error| match error {
                crate::providers::BoundedBodyError::TooLarge => ProviderError::Parse(
                    "Could not parse IBM Bob usage: response is too large".into(),
                ),
                crate::providers::BoundedBodyError::Read(error) => ProviderError::Network(error),
            })
    }

    async fn fetch_api(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        let token = api_key_from(crate::providers::resolve_api_key(
            ctx.api_key.as_deref(),
            CREDENTIAL_TARGET,
            &[API_KEY_ENV],
        ))?;
        let client = self.client.as_ref().ok_or_else(|| {
            ProviderError::Other("Could not create a secure IBM Bob HTTP client.".into())
        })?;
        let timeout = Duration::from_secs(ctx.web_timeout.max(1)).min(REQUEST_TIMEOUT);

        let profile_url = format!("{}/admin/v1/profile", self.base_url);
        let body = self
            .get(client, &profile_url, &token, None, timeout)
            .await?;
        let profile = model::parse_profile(&body)?;

        let mut teams = Vec::new();
        for instance in &profile.instances {
            let Some(user_id) = instance.user_id.as_deref().filter(|id| !id.is_empty()) else {
                continue;
            };
            let base = self.regional_base(instance.region_domain.as_deref())?;
            for team in &instance.teams {
                if team.id.is_empty() {
                    continue;
                }
                let url = format!(
                    "{base}/admin/v1/teams/{}/users/{}",
                    path_segment(&team.id),
                    path_segment(user_id)
                );
                let body = self
                    .get(
                        client,
                        &url,
                        &token,
                        Some((&instance.instance_id, &team.id)),
                        timeout,
                    )
                    .await?;
                let budget = model::parse_team_budget(&body)?;
                teams.push(model::team_usage(instance, team, &budget));
            }
        }
        if teams.is_empty() {
            return Err(ProviderError::Other(
                "IBM Bob returned no subscription instances or teams for this API key.".into(),
            ));
        }
        Ok(model::build_result(&teams))
    }
}

impl Default for IBMBobProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for IBMBobProvider {
    fn id(&self) -> ProviderId {
        ProviderId::IBMBob
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
}

/// Normalize a resolved key; a missing or blank key gets upstream's
/// `IBMBobUsageFetcher` message instead of the shared generic one.
fn api_key_from(resolved: Result<String, ProviderError>) -> Result<String, ProviderError> {
    let missing = || {
        ProviderError::NotInstalled(
            "Missing IBM Bob API key. Add one in Settings or set BOBSHELL_API_KEY.".into(),
        )
    };
    match resolved {
        Ok(raw) => normalize_api_key(&raw).ok_or_else(missing),
        Err(ProviderError::NotInstalled(_)) => Err(missing()),
        Err(error) => Err(error),
    }
}

/// Upstream `IBMBobSettingsReader.apiKey`: trim, then drop one pair of
/// matching surrounding quotes.
fn normalize_api_key(raw: &str) -> Option<String> {
    let value = raw.trim();
    let quoted = ['"', '\'']
        .iter()
        .any(|quote| value.starts_with(*quote) && value.ends_with(*quote));
    let value = if quoted {
        // A lone quote character unwraps to nothing, like upstream.
        value.get(1..value.len() - 1).unwrap_or("").trim()
    } else {
        value
    };
    (!value.is_empty()).then(|| value.to_owned())
}

/// Bob Shell's formats: `Bearer` for a JWT session token, else `Apikey`.
fn authorization_value(token: &str) -> String {
    if is_jwt(token) {
        format!("Bearer {token}")
    } else {
        format!("Apikey {token}")
    }
}

/// Three dot-separated parts whose middle part is base64url JSON object.
fn is_jwt(token: &str) -> bool {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return false;
    }
    let mut payload = parts[1].replace('-', "+").replace('_', "/");
    payload.push_str(&"=".repeat((4 - payload.len() % 4) % 4));
    base64::engine::general_purpose::STANDARD
        .decode(payload)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|value| value.is_object())
}

/// Validate the instance's `region_domain`. `None` selects the fixed host.
///
/// The host gets an `api.` prefix when missing and must be `bob.ibm.com` or
/// a subdomain of it, reachable over HTTPS with no port, path, credentials,
/// query or fragment.
fn regional_base_url(region_domain: Option<&str>) -> Result<Option<Url>, ProviderError> {
    let Some(domain) = model::non_empty(region_domain) else {
        return Ok(None);
    };
    let host = if domain.to_ascii_lowercase().starts_with("api.") {
        domain.to_owned()
    } else {
        format!("api.{domain}")
    };
    let untrusted = || {
        let shown: String = host
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_ECHOED_HOST_CHARS)
            .collect();
        ProviderError::Other(format!(
            "IBM Bob returned an untrusted regional API host: {shown}."
        ))
    };
    // Plain host names only: anything that could carry a port, path,
    // credentials, query or fragment is rejected before parsing.
    if !host
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return Err(untrusted());
    }
    let url = Url::parse(&format!("https://{host}")).map_err(|_| untrusted())?;
    let trusted = url.scheme() == "https"
        && url.port().is_none()
        && url.path() == "/"
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url
            .host_str()
            .is_some_and(|host| host == "bob.ibm.com" || host.ends_with(".bob.ibm.com"));
    if trusted {
        Ok(Some(url))
    } else {
        Err(untrusted())
    }
}

/// Percent-encode one path segment of an id returned by the API.
fn path_segment(value: &str) -> String {
    let mut url = Url::parse("https://bob.ibm.com/").expect("static URL");
    url.path_segments_mut()
        .expect("https URL has path segments")
        .push(value);
    url.path().trim_start_matches('/').to_owned()
}

/// Classify a non-success status without reading or echoing the body.
fn check_status(status: StatusCode) -> Result<(), ProviderError> {
    match status {
        _ if status.is_success() => Ok(()),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(ProviderError::Other(
            "IBM Bob rejected the API key. Check that it is active and can read subscription usage."
                .into(),
        )),
        _ => Err(ProviderError::Other(format!(
            "IBM Bob API returned HTTP {}.",
            status.as_u16()
        ))),
    }
}
