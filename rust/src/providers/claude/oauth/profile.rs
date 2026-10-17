//! Best-effort Claude OAuth profile lookup for the card header email.
//!
//! Upstream reads the account email from `GET /api/oauth/profile` next to the
//! usage call. The email is display metadata only: a failed, slow or
//! malformed profile response never fails the usage fetch, and the result is
//! remembered per token fingerprint so a refresh loop does not re-ask.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use reqwest::Client;

pub(super) const PROFILE_URL: &str = "https://api.anthropic.com/api/oauth/profile";
const PROFILE_TIMEOUT: Duration = Duration::from_secs(5);
/// A failed lookup is retried after this long; a found email is kept for the
/// life of the token.
const FAILURE_RETRY: Duration = Duration::from_secs(15 * 60);

enum CachedProfile {
    Found(String),
    Failed(Instant),
}

static PROFILE_CACHE: LazyLock<Mutex<HashMap<String, CachedProfile>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The cached answer for `fingerprint`: `Some(Some(email))` when known,
/// `Some(None)` while a recent failure is cooling down, `None` to ask again.
fn cached(fingerprint: &str, now: Instant) -> Option<Option<String>> {
    let cache = PROFILE_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    match cache.get(fingerprint)? {
        CachedProfile::Found(email) => Some(Some(email.clone())),
        CachedProfile::Failed(at) if now.duration_since(*at) < FAILURE_RETRY => Some(None),
        CachedProfile::Failed(_) => None,
    }
}

fn remember(fingerprint: &str, email: Option<&str>, now: Instant) {
    let entry = match email {
        Some(email) => CachedProfile::Found(email.to_string()),
        None => CachedProfile::Failed(now),
    };
    PROFILE_CACHE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(fingerprint.to_string(), entry);
}

/// The account email for `access_token`, or `None` on any failure.
pub(super) async fn fetch_email(
    client: &Client,
    access_token: &str,
    fingerprint: &str,
) -> Option<String> {
    if let Some(answer) = cached(fingerprint, Instant::now()) {
        return answer;
    }
    let email = request_email(client, access_token).await;
    remember(fingerprint, email.as_deref(), Instant::now());
    email
}

async fn request_email(client: &Client, access_token: &str) -> Option<String> {
    let response = client
        .get(PROFILE_URL)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("Accept", "application/json")
        .header("anthropic-beta", "oauth-2025-04-20")
        .timeout(PROFILE_TIMEOUT)
        .send()
        .await;
    let response = match response {
        Ok(response) if response.status().is_success() => response,
        Ok(response) => {
            tracing::debug!(status = %response.status(), "Claude OAuth profile unavailable");
            return None;
        }
        Err(error) => {
            tracing::debug!(%error, "Claude OAuth profile request failed");
            return None;
        }
    };
    let body = response.text().await.ok()?;
    let email = parse_email(&body);
    if email.is_none() {
        tracing::debug!("Claude OAuth profile has no account email");
    }
    email
}

/// `account.email_address` (also `emailAddress` / `email`), falling back to
/// the same keys at the top level. Blank or non-email values are ignored.
pub(super) fn parse_email(body: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    let read = |object: &serde_json::Value| {
        ["email_address", "emailAddress", "email"]
            .iter()
            .filter_map(|key| object.get(*key).and_then(|v| v.as_str()))
            .map(str::trim)
            .find(|value| value.contains('@'))
            .map(str::to_string)
    };
    json.get("account").and_then(read).or_else(|| read(&json))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shape of the upstream profile response (synthetic values).
    const PROFILE_FIXTURE: &str = r#"{
        "account": {
            "uuid": "00000000-0000-4000-8000-000000000001",
            "full_name": "Parity User",
            "display_name": "Parity",
            "email_address": "parity.user@example.com",
            "has_claude_max": true,
            "has_claude_pro": false
        },
        "organization": {
            "uuid": "00000000-0000-4000-8000-000000000002",
            "name": "Parity Labs",
            "organization_type": "claude_max",
            "billing_type": "stripe_subscription",
            "rate_limit_tier": "default_claude_max_20x"
        }
    }"#;

    #[test]
    fn parses_the_account_email_from_the_profile_fixture() {
        assert_eq!(
            parse_email(PROFILE_FIXTURE).as_deref(),
            Some("parity.user@example.com")
        );
    }

    #[test]
    fn accepts_alternate_keys_and_top_level_email() {
        assert_eq!(
            parse_email(r#"{"account":{"emailAddress":" a@example.com "}}"#).as_deref(),
            Some("a@example.com")
        );
        assert_eq!(
            parse_email(r#"{"email":"top@example.com"}"#).as_deref(),
            Some("top@example.com")
        );
    }

    #[test]
    fn ignores_missing_blank_or_malformed_values() {
        assert!(parse_email("").is_none());
        assert!(parse_email("not json").is_none());
        assert!(parse_email(r#"{"account":{"email_address":"   "}}"#).is_none());
        assert!(parse_email(r#"{"account":{"email_address":42}}"#).is_none());
        assert!(parse_email(r#"{"organization":{"name":"Parity Labs"}}"#).is_none());
    }

    #[test]
    fn cache_keeps_found_emails_and_cools_down_failures() {
        let now = Instant::now();
        remember("fp-found-test", Some("parity.user@example.com"), now);
        assert_eq!(
            cached("fp-found-test", now + Duration::from_secs(86_400)),
            Some(Some("parity.user@example.com".to_string()))
        );

        remember("fp-failed-test", None, now);
        assert_eq!(cached("fp-failed-test", now), Some(None));
        assert_eq!(cached("fp-failed-test", now + FAILURE_RETRY), None);
        assert_eq!(cached("fp-unknown-test", now), None);
    }
}
