//! X API prepaid credits from the signed-in console.
//!
//! Ported from upstream CodexBar v0.74.0 (`Resources/Plugins/xapi.js`,
//! `Providers/XAPI/XAPIProviderDescriptor.swift`). X API has no API key for
//! prepaid credits: the console session cookies `auth_token` and `ct0` read
//! `/api/me` and `/api/accounts/{account_id}/credits` endpoints. The CSRF
//! cookie value is echoed as `X-CSRF-Token`. A pasted or imported Cookie
//! header is the only session tried. Without one, Automatic tries every
//! detected browser holding both cookies for console.x.com, in order, and
//! moves to the next after a 401 (upstream `rejectCookie`). Response bodies
//! are never echoed in errors and the cookies are never logged.

#[cfg(test)]
mod tests;

use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, StatusCode, header::ACCEPT};
use serde_json::{Map, Value};

use crate::core::{
    CostSnapshot, FetchContext, ManualEmptyCookiePolicy, ProviderDisplayDetail, ProviderError,
    ProviderFetchResult, ProviderId, ProviderMetadata, ProviderStateKind, RateWindow, SourceMode,
    UsageSnapshot,
};
use crate::locale::LocaleKey;
use crate::providers::{BoundedBodyError, read_bounded_response};

const BASE_URL: &str = "https://console.x.com";
const COOKIE_DOMAIN: &str = "x.com";
const AUTH_COOKIE: &str = "auth_token";
const CSRF_COOKIE: &str = "ct0";
const CSRF_HEADER: &str = "X-CSRF-Token";
const SIGN_IN_URL: &str = "https://console.x.com";
/// Upstream's fixed web timeout.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_RESPONSE_BYTES: usize = 512 * 1024;

/// Upstream's message when every offered session answered 401.
const SESSION_EXPIRED: &str = "X API session expired. Sign in to console.x.com again.";
const MISSING_SESSION: &str = "Sign in to console.x.com in the selected browser, or paste its Cookie header with auth_token and ct0.";
const COOKIES_OFF: &str = "X API cookies are disabled.";

/// Every detected browser's Cookie header for the console, in browser order.
type BrowserCookieHeaders = dyn Fn() -> Result<Vec<(String, String)>, ProviderError> + Sync;

pub struct XapiProvider {
    metadata: ProviderMetadata,
    client: Option<Client>,
}

impl XapiProvider {
    pub fn new() -> Self {
        let client = crate::core::credentialed_http_client_builder()
            .cookie_store(false)
            .redirect(reqwest::redirect::Policy::none())
            .timeout(REQUEST_TIMEOUT)
            .build()
            .ok();
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Xapi,
                display_name: "X API",
                session_label: "Balance",
                weekly_label: "Balance",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some(SIGN_IN_URL),
                status_page_url: None,
                tertiary_label_key: None,
            },
            client,
        }
    }

    /// Tries each candidate session in order. Only an expired session moves
    /// on to the next candidate; any other failure ends the fetch.
    async fn fetch_web(
        &self,
        ctx: &FetchContext,
        browser: &BrowserCookieHeaders,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let (sessions, browser_error) = sessions(ctx, browser);
        if sessions.is_empty() {
            return Err(browser_error.unwrap_or_else(missing_session));
        }
        let client = self.client.as_ref().ok_or_else(|| {
            ProviderError::Other("Could not create a secure X API HTTP client.".into())
        })?;
        let timeout = Duration::from_secs(ctx.web_timeout.max(1)).min(REQUEST_TIMEOUT);
        let mut last_error = None;
        for session in &sessions {
            match fetch_session(client, session, timeout).await {
                Err(error) if is_session_expired(&error) => last_error = Some(error),
                result => return result,
            }
        }
        Err(last_error.unwrap_or_else(missing_session))
    }
}

impl Default for XapiProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl crate::core::Provider for XapiProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Xapi
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => {
                self.fetch_web(ctx, &|| {
                    crate::providers::browser_cookie_headers_for_domain(COOKIE_DOMAIN)
                })
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

/// One session's me → credits sequence.
async fn fetch_session(
    client: &Client,
    session: &Session,
    timeout: Duration,
) -> Result<ProviderFetchResult, ProviderError> {
    let me = get_json(client, "/api/me", session, timeout).await?;
    let account_id = parse_account_id(&me)?;
    let credits = get_json(
        client,
        &format!("/api/accounts/{}/credits", account_id),
        session,
        timeout,
    )
    .await?;
    parse_credits(&account_id, &credits)
}

fn is_session_expired(error: &ProviderError) -> bool {
    matches!(error, ProviderError::Other(message) if message == SESSION_EXPIRED)
}

/// A console session: the full Cookie header plus the CSRF value it echoes.
#[derive(Clone)]
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
            tracing::debug!(%error, "X API browser sessions are unavailable");
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
    first(AUTH_COOKIE)?;
    let csrf = first(CSRF_COOKIE)?;
    Some(Session {
        cookie_header: header.to_owned(),
        csrf,
    })
}

async fn get_json(
    client: &Client,
    path: &str,
    session: &Session,
    timeout: Duration,
) -> Result<Map<String, Value>, ProviderError> {
    let response = client
        .get(format!("{}{}", BASE_URL, path))
        .header(ACCEPT, "application/json")
        .header("Cookie", &session.cookie_header)
        .header(CSRF_HEADER, &session.csrf)
        .timeout(timeout)
        .send()
        .await?;

    check_status(response.status())?;
    let body_bytes = read_bounded_response(response, MAX_RESPONSE_BYTES)
        .await
        .map_err(|error| match error {
            BoundedBodyError::TooLarge => invalid("JSON response"),
            BoundedBodyError::Read(error) => ProviderError::Network(error),
        })?;

    let body = std::str::from_utf8(&body_bytes).map_err(|_| invalid("JSON response"))?;
    parse_object(body)
}

fn parse_object(text: &str) -> Result<Map<String, Value>, ProviderError> {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| v.as_object().cloned())
        .ok_or_else(|| invalid("JSON object"))
}

fn parse_account_id(me: &Map<String, Value>) -> Result<String, ProviderError> {
    let id = me
        .get("account")
        .and_then(|v| v.get("id"))
        .ok_or_else(|| invalid("account.id"))?;

    // Handle both string and integer IDs
    let account_id = match id {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => return Err(invalid("account.id type")),
    };

    // Validate account ID format
    if account_id.is_empty()
        || account_id.len() > 128
        || !account_id
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Err(invalid("account.id format"));
    }

    Ok(account_id)
}

fn parse_credits(
    _account_id: &str,
    credits: &Map<String, Value>,
) -> Result<ProviderFetchResult, ProviderError> {
    let get_balance = |key: &str| -> Result<f64, ProviderError> {
        credits
            .get(key)
            .and_then(|v| v.get("balance"))
            .and_then(|v| v.as_f64())
            .ok_or_else(|| invalid("balance amount"))
            .and_then(|n| {
                if n.is_finite() {
                    Ok(n)
                } else {
                    Err(invalid("balance amount"))
                }
            })
    };

    let paid = get_balance("credits")?;
    let free = credits
        .get("freeCredits")
        .and_then(|v| v.get("balance"))
        .and_then(|v| v.as_f64())
        .filter(|n| n.is_finite())
        .unwrap_or(0.0);

    let balance = paid + free;
    let balance_text = format!("${:.2}", balance);

    // Build the usage snapshot with balance as informational
    let usage = UsageSnapshot::new(
        RateWindow::informational(balance_text.clone()),
    )
    .with_login_method("Browser session");

    let mut result = ProviderFetchResult::new(usage, "web");

    // Add cost block with balance
    result = result.with_cost(
        CostSnapshot::new(0.0, "USD", "Prepaid credits")
            .with_balance(balance)
            .with_card_title(LocaleKey::CreditsTitle),
    );

    // Add display details rows
    let mut rows = vec![];
    if let Some(row) = ProviderDisplayDetail::new("balance", "Balance", balance_text) {
        rows.push(row);
    }
    if let Some(row) = ProviderDisplayDetail::new(
        "purchased-credits",
        "Purchased credits",
        format!("${:.2}", paid),
    ) {
        rows.push(row);
    }
    if let Some(row) =
        ProviderDisplayDetail::new("free-credits", "Free credits", format!("${:.2}", free))
    {
        rows.push(row);
    }

    result = result.with_display_details(rows);
    Ok(result)
}

fn invalid(field: &str) -> ProviderError {
    ProviderError::Other(format!("X API returned an unrecognized {}.", field))
}

/// Upstream's HTTP status taxonomy; only 200 continues to the body.
fn check_status(status: StatusCode) -> Result<(), ProviderError> {
    let message = match status.as_u16() {
        200 => return Ok(()),
        400 => {
            // 400 might be an authentication error (code 215)
            SESSION_EXPIRED.to_owned()
        }
        401 => SESSION_EXPIRED.to_owned(),
        403 => "X developer console access was denied.".to_owned(),
        429 => "X API console is rate limited. Wait before refreshing.".to_owned(),
        500.. => "X developer console is unavailable.".to_owned(),
        code => format!("X developer console returned HTTP {}.", code),
    };
    Err(ProviderError::Other(message))
}
