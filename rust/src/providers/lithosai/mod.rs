//! LithosAI prepaid balance from the signed-in console.
//!
//! Ported from upstream CodexBar v0.73.0 (`Resources/Plugins/lithosai.ts`,
//! `Providers/LithosAI/LithosAIProviderDescriptor.swift`). LithosAI has no API
//! key for billing: the console session cookies `__Host-console_session` and
//! `__Host-console_csrf` read `/api/me`, `/api/billing` and the optional
//! `/api/billing/spend`. The CSRF cookie value is echoed as `X-Console-Csrf`.
//! A pasted or imported Cookie header is the only session tried. Without one,
//! Automatic tries every detected browser holding both console cookies, in
//! order, and moves to the next after a 401 (upstream `rejectCookie`).
//! Response bodies are never echoed in errors and the cookies are never logged.

mod model;
#[cfg(test)]
mod tests;

use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::{
    Client, StatusCode,
    header::{ACCEPT, COOKIE},
};
use serde_json::{Map, Value};

use crate::core::{
    FetchContext, ManualEmptyCookiePolicy, Provider, ProviderError, ProviderFetchResult,
    ProviderId, ProviderMetadata, ProviderStateKind, SourceMode,
};
use crate::providers::{BoundedBodyError, read_bounded_response};

const BASE_URL: &str = "https://console.lithosai.cloud";
const COOKIE_DOMAIN: &str = "console.lithosai.cloud";
const SESSION_COOKIE: &str = "__Host-console_session";
const CSRF_COOKIE: &str = "__Host-console_csrf";
const CSRF_HEADER: &str = "X-Console-Csrf";
const ORGANIZATION_HEADER: &str = "X-Organization-Id";
const SIGN_IN_URL: &str = "https://console.lithosai.cloud";
/// Upstream's fixed web timeout.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_RESPONSE_BYTES: usize = 512 * 1024;

/// Upstream's message after every offered session answered 401.
const SESSION_EXPIRED: &str = "LithosAI session expired. Sign in again or paste fresh cookies.";
const MISSING_SESSION: &str = "Sign in to console.lithosai.cloud in the selected browser, or paste its Cookie header with __Host-console_session and __Host-console_csrf.";
const COOKIES_OFF: &str = "LithosAI cookies are disabled.";

/// Every detected browser's Cookie header for the console, in browser order.
type BrowserCookieHeaders = dyn Fn() -> Result<Vec<(String, String)>, ProviderError> + Sync;

pub struct LithosAIProvider {
    metadata: ProviderMetadata,
    client: Option<Client>,
    base_url: String,
}

impl LithosAIProvider {
    pub fn new() -> Self {
        let client = crate::core::credentialed_http_client_builder()
            .cookie_store(false)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(REQUEST_TIMEOUT)
            .build()
            .ok();
        Self::with_parts(BASE_URL, client)
    }

    #[cfg(test)]
    fn with_client(base_url: &str, client: Client) -> Self {
        Self::with_parts(base_url.trim_end_matches('/'), Some(client))
    }

    fn with_parts(base_url: &str, client: Option<Client>) -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::LithosAI,
                display_name: "LithosAI",
                session_label: "Balance",
                weekly_label: "Spend",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some(SIGN_IN_URL),
                status_page_url: None,
                tertiary_label_key: None,
            },
            client,
            base_url: base_url.to_owned(),
        }
    }

    /// Tries each candidate session in order. Only an expired session moves
    /// on to the next candidate; any other failure ends the fetch.
    async fn fetch_web(
        &self,
        ctx: &FetchContext,
        browser: &BrowserCookieHeaders,
        now: DateTime<Utc>,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let (sessions, browser_error) = sessions(ctx, browser);
        if sessions.is_empty() {
            return Err(browser_error.unwrap_or_else(missing_session));
        }
        let client = self.client.as_ref().ok_or_else(|| {
            ProviderError::Other("Could not create a secure LithosAI HTTP client.".into())
        })?;
        let timeout = Duration::from_secs(ctx.web_timeout.max(1)).min(REQUEST_TIMEOUT);
        let mut last_error = None;
        for session in &sessions {
            let console = Console {
                client,
                base_url: &self.base_url,
                session,
                timeout,
            };
            match fetch_session(&console, now).await {
                Err(error) if is_session_expired(&error) => last_error = Some(error),
                result => return result,
            }
        }
        Err(last_error.unwrap_or_else(missing_session))
    }
}

/// One session's me → billing → spend sequence.
async fn fetch_session(
    console: &Console<'_>,
    now: DateTime<Utc>,
) -> Result<ProviderFetchResult, ProviderError> {
    let me = console.get("/api/me", None).await?;
    let account = model::parse_me(&me)?;
    let billing = console
        .get("/api/billing", Some(&account.organization_id))
        .await?;
    let billing = model::parse_billing(&billing)?;

    // Spend is optional: a broken report must not discard an authenticated
    // balance, but an expired session still rejects this session.
    let (start, end) = model::spend_range(now);
    let spend = match console
        .get(
            &format!("/api/billing/spend?start={start}&end={end}"),
            Some(&account.organization_id),
        )
        .await
    {
        Ok(report) => model::parse_spend(&report, &start, &end).ok(),
        Err(error) if is_session_expired(&error) => return Err(error),
        Err(error) => {
            tracing::debug!(%error, "LithosAI spend report is unavailable");
            None
        }
    };
    Ok(model::result(&account, &billing, spend))
}

impl Default for LithosAIProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for LithosAIProvider {
    fn id(&self) -> ProviderId {
        ProviderId::LithosAI
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => {
                self.fetch_web(
                    ctx,
                    &|| crate::providers::browser_cookie_headers_for_domain(COOKIE_DOMAIN),
                    Utc::now(),
                )
                .await
            }
            // The shell maps an Off cookie source to Cli for web-only
            // providers; upstream reports that cookies are disabled.
            SourceMode::Cli => Err(ProviderError::Other(COOKIES_OFF.into())),
            SourceMode::OAuth => Err(ProviderError::UnsupportedSource(SourceMode::OAuth)),
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web]
    }

    fn supports_web(&self) -> bool {
        true
    }

    /// Upstream sends only the pasted header in Manual mode, so an empty
    /// manual source must not fall back to a browser session.
    fn manual_empty_cookie_policy(&self) -> ManualEmptyCookiePolicy {
        ManualEmptyCookiePolicy::FailClosedWeb
    }

    /// The shell would import only the first browser's cookies; the provider
    /// reads every browser so an expired session can move to the next one.
    fn owns_browser_cookie_resolution(&self) -> bool {
        true
    }

    fn error_state_kind(&self, error: &ProviderError) -> ProviderStateKind {
        if is_session_expired(error) {
            ProviderStateKind::ExpiredSession
        } else {
            error.state_kind()
        }
    }
}

fn is_session_expired(error: &ProviderError) -> bool {
    matches!(error, ProviderError::Other(message) if message == SESSION_EXPIRED)
}

/// A console session: the full Cookie header plus the CSRF value it echoes.
struct Session {
    cookie_header: String,
    csrf: String,
}

fn missing_session() -> ProviderError {
    ProviderError::BrowserSignInRequired {
        message: MISSING_SESSION.into(),
        sign_in_url: SIGN_IN_URL.into(),
    }
}

/// Candidate sessions in upstream's order, plus a browser read failure worth
/// reporting when no candidate is usable. A pasted or shell-resolved Cookie
/// header is used alone, an empty Manual source offers nothing, and otherwise
/// every browser holding both console cookies is a candidate.
fn sessions(
    ctx: &FetchContext,
    browser: &BrowserCookieHeaders,
) -> (Vec<Session>, Option<ProviderError>) {
    if let Some(header) = ctx.manual_cookie_header.as_deref() {
        let session = crate::providers::normalize_cookie_header(header)
            .and_then(|header| session_from_header(&header));
        return (session.into_iter().collect(), None);
    }
    if ctx.manual_cookie_missing {
        return (Vec::new(), None);
    }
    match browser() {
        Ok(headers) => (
            headers
                .iter()
                .filter_map(|(_, header)| crate::providers::normalize_cookie_header(header))
                .filter_map(|header| session_from_header(&header))
                .collect(),
            None,
        ),
        // No browser holds console cookies: the sign-in hint is clearer.
        Err(ProviderError::NoCookies) => (Vec::new(), None),
        Err(error) => {
            tracing::debug!(%error, "LithosAI browser sessions are unavailable");
            (Vec::new(), Some(error))
        }
    }
}

fn session_from_header(header: &str) -> Option<Session> {
    let first = |name: &str| {
        crate::providers::cookie_values(header, name)
            .into_iter()
            .find(|value| !value.is_empty())
            .map(str::to_owned)
    };
    first(SESSION_COOKIE)?;
    let csrf = first(CSRF_COOKIE)?;
    Some(Session {
        cookie_header: header.to_owned(),
        csrf,
    })
}

struct Console<'a> {
    client: &'a Client,
    base_url: &'a str,
    session: &'a Session,
    timeout: Duration,
}

impl Console<'_> {
    async fn get(
        &self,
        path: &str,
        organization_id: Option<&str>,
    ) -> Result<Map<String, Value>, ProviderError> {
        let mut request = self
            .client
            .get(format!("{}{path}", self.base_url))
            .header(ACCEPT, "application/json")
            .header(COOKIE, &self.session.cookie_header)
            .header(CSRF_HEADER, &self.session.csrf)
            .timeout(self.timeout);
        if let Some(organization_id) = organization_id {
            request = request.header(ORGANIZATION_HEADER, organization_id);
        }
        let response = request.send().await?;
        check_status(response.status())?;
        let body = read_bounded_response(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|error| match error {
                BoundedBodyError::TooLarge => model::invalid("JSON response"),
                BoundedBodyError::Read(error) => ProviderError::Network(error),
            })?;
        model::parse_object(&body)
    }
}

/// Upstream's HTTP status taxonomy; only 200 continues to the body.
fn check_status(status: StatusCode) -> Result<(), ProviderError> {
    let message = match status.as_u16() {
        200 => return Ok(()),
        401 => SESSION_EXPIRED.to_owned(),
        403 => "LithosAI console access was denied.".to_owned(),
        429 => "LithosAI console requests are rate limited.".to_owned(),
        500.. => "LithosAI console is unavailable.".to_owned(),
        code => format!("LithosAI console returned HTTP {code}."),
    };
    Err(ProviderError::Other(message))
}
