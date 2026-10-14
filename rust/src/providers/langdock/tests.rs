use std::io::{Read, Write};
use std::net::TcpListener;

use chrono::{TimeZone, Utc};
use reqwest::redirect::Policy;

use super::model::parse_personal_usage;
use super::*;

/// Upstream `LangdockPluginTests.plan`.
const PLAN: &str = r#"{"sessionUsageLimitsEnabled":true,"sessionUsagePercent":12.5,
 "sessionResetsAt":"2026-09-25T12:00:00.123Z","weeklyUsagePercent":104.2,
 "weeklyResetsAt":"2026-09-28T12:00:00Z"}"#;

fn body(plan: &str) -> String {
    format!(
        r#"[{{"result":{{"data":{{"json":{{"hasIncludedUsageLimits":true,"planUsage":{plan}}}}}}}}}]"#
    )
}

fn parse(body: &str) -> Result<ProviderFetchResult, ProviderError> {
    parse_personal_usage(body.as_bytes())
}

fn rows(result: &ProviderFetchResult) -> Vec<(String, String, String, Option<String>)> {
    result
        .display_details
        .iter()
        .map(|row| {
            (
                row.id().to_owned(),
                row.title().to_owned(),
                row.value().to_owned(),
                row.section_title().map(str::to_owned),
            )
        })
        .collect()
}

#[test]
fn the_batch_input_decodes_to_upstream_json() {
    let url = reqwest::Url::parse(&format!("https://app.langdock.com/?input={USAGE_INPUT}"))
        .expect("static URL");
    let input = url
        .query_pairs()
        .find(|(name, _)| name == "input")
        .map(|(_, value)| value.into_owned());
    assert_eq!(
        input.as_deref(),
        Some(r#"{"0":{"json":null,"meta":{"values":["undefined"],"v":1}}}"#)
    );
}

#[test]
fn session_and_weekly_match_the_contributor_fixture() {
    let result = parse(&body(PLAN)).expect("plan usage");
    let usage = &result.usage;
    assert_eq!(usage.primary.used_percent, 12.5);
    assert_eq!(usage.primary.window_minutes, Some(300));
    assert_eq!(
        usage.primary.resets_at,
        Some(
            Utc.with_ymd_and_hms(2026, 9, 25, 12, 0, 0).unwrap()
                + chrono::Duration::milliseconds(123)
        )
    );
    assert!(usage.primary_label.is_none());
    let weekly = usage.secondary.as_ref().expect("weekly lane");
    // Upstream keeps 104.2% overage; the shared Windows rate window clamps.
    assert_eq!(weekly.used_percent, 100.0);
    assert_eq!(weekly.window_minutes, Some(10_080));
    assert_eq!(
        weekly.resets_at,
        Some(Utc.with_ymd_and_hms(2026, 9, 28, 12, 0, 0).unwrap())
    );
    assert_eq!(usage.login_method.as_deref(), Some("Browser session"));
    assert_eq!(result.source_label, "web");
    assert!(result.display_details.is_empty());
}

#[test]
fn the_parity_pack_payload_maps_to_session_42_and_weekly_63() {
    let result = parse(&body(
        r#"{"sessionUsageLimitsEnabled":true,"sessionUsagePercent":42,
            "sessionResetsAt":"2026-10-11T12:30:00.000Z","weeklyUsagePercent":63,
            "weeklyResetsAt":"2026-10-15T10:00:00.000Z"}"#,
    ))
    .expect("pack usage");
    assert_eq!(result.usage.primary.used_percent, 42.0);
    assert_eq!(
        result.usage.primary.resets_at,
        Some(Utc.with_ymd_and_hms(2026, 10, 11, 12, 30, 0).unwrap())
    );
    let weekly = result.usage.secondary.as_ref().expect("weekly lane");
    assert_eq!(weekly.used_percent, 63.0);
    assert_eq!(
        weekly.resets_at,
        Some(Utc.with_ymd_and_hms(2026, 10, 15, 10, 0, 0).unwrap())
    );
}

#[test]
fn weekly_only_plans_put_the_weekly_lane_first_under_its_own_label() {
    let result = parse(&body(
        r#"{"sessionUsageLimitsEnabled":false,"weeklyUsagePercent":0}"#,
    ))
    .expect("weekly only");
    let usage = &result.usage;
    assert_eq!(usage.primary.used_percent, 0.0);
    assert_eq!(usage.primary.window_minutes, Some(10_080));
    assert!(usage.primary.resets_at.is_none());
    assert!(!usage.primary.is_informational);
    assert_eq!(usage.primary_label.as_deref(), Some("Weekly"));
    assert!(usage.secondary.is_none());
}

#[test]
fn null_resets_stay_unknown() {
    let result = parse(&body(
        r#"{"sessionUsageLimitsEnabled":true,"sessionUsagePercent":0,"sessionResetsAt":null,
            "weeklyUsagePercent":0,"weeklyResetsAt":null}"#,
    ))
    .expect("zero usage");
    assert!(result.usage.primary.resets_at.is_none());
    assert!(result.usage.secondary.as_ref().unwrap().resets_at.is_none());
}

#[test]
fn missing_included_limits_show_one_message_titled_included_limits() {
    for payload in [
        body("null"),
        r#"[{"result":{"data":{"json":{"hasIncludedUsageLimits":false}}}}]"#.to_owned(),
        r#"[{"result":{"data":{"json":{"hasIncludedUsageLimits":false,"planUsage":{"x":1}}}}}]"#
            .to_owned(),
        r#"[{"result":{"data":{"json":{}}}}]"#.to_owned(),
    ] {
        let result = parse(&payload).expect(&payload);
        assert!(result.usage.primary.is_informational, "{payload}");
        assert_eq!(
            result.usage.primary.reset_description.as_deref(),
            Some("No included usage limits available")
        );
        // Upstream's detail row title, so the card never falls back to "Session".
        assert_eq!(
            result.usage.primary_label.as_deref(),
            Some("Included limits"),
            "{payload}"
        );
        assert!(result.usage.secondary.is_none());
        // The primary already carries the message; no second copy as a row.
        assert_eq!(rows(&result), Vec::new(), "{payload}");
        assert_eq!(
            result.usage.login_method.as_deref(),
            Some("Browser session")
        );
    }
}

#[test]
fn malformed_envelopes_percentages_and_dates_fail_instead_of_fabricating_zero() {
    for payload in [
        "".to_owned(),
        "{}".to_owned(),
        "[]".to_owned(),
        "[{},{}]".to_owned(),
        r#"[{"result":{"data":null}}]"#.to_owned(),
        r#"[{"error":{}}]"#.to_owned(),
        r#"[{"error":{"json":{"data":{"code":403}}}}]"#.to_owned(),
        r#"[{"error":{"json":{"data":{"code":""}}}}]"#.to_owned(),
        r#"[{"result":{"data":{"json":{"hasIncludedUsageLimits":null,"planUsage":null}}}}]"#
            .to_owned(),
        r#"[{"result":{"data":{"json":{"hasIncludedUsageLimits":true,"planUsage":[]}}}}]"#
            .to_owned(),
        body(r#"{"weeklyUsagePercent":4}"#),
        body(r#"{"sessionUsageLimitsEnabled":true,"weeklyUsagePercent":4}"#),
        body(
            r#"{"sessionUsageLimitsEnabled":true,"sessionUsagePercent":"5","weeklyUsagePercent":4}"#,
        ),
        body(r#"{"sessionUsageLimitsEnabled":false,"weeklyUsagePercent":"4"}"#),
        body(r#"{"sessionUsageLimitsEnabled":false}"#),
        body(
            r#"{"sessionUsageLimitsEnabled":false,"weeklyUsagePercent":4,"weeklyResetsAt":"tomorrow"}"#,
        ),
        body(r#"{"sessionUsageLimitsEnabled":false,"weeklyUsagePercent":4,"sessionResetsAt":5}"#),
    ] {
        let error = parse(&payload).expect_err(&payload);
        assert_eq!(
            error.to_string(),
            "Parse error: Langdock returned an unexpected personal usage response.",
            "{payload}"
        );
    }
}

#[test]
fn trpc_error_codes_map_to_upstream_messages() {
    for (code, message) in [
        (
            "UNAUTHORIZED",
            "The selected browser profile is no longer signed in to Langdock.",
        ),
        ("FORBIDDEN", "Langdock denied access to personal usage."),
        (
            "TOO_MANY_REQUESTS",
            "Langdock usage requests are rate limited.",
        ),
        (
            "INTERNAL_SERVER_ERROR",
            "Langdock personal usage is temporarily unavailable.",
        ),
        (
            "TIMEOUT",
            "Langdock personal usage is temporarily unavailable.",
        ),
        (
            "BAD_REQUEST",
            "Langdock rejected the personal usage request.",
        ),
    ] {
        let payload = format!(r#"[{{"error":{{"json":{{"data":{{"code":"{code}"}}}}}}}}]"#);
        assert_eq!(parse(&payload).expect_err(code).to_string(), message);
    }
}

#[test]
fn http_statuses_map_to_upstream_messages() {
    for (status, message) in [
        (
            401,
            "The selected browser profile is no longer signed in to Langdock.",
        ),
        (403, "Langdock denied access to personal usage."),
        (429, "Langdock usage requests are rate limited."),
        (500, "Langdock personal usage is temporarily unavailable."),
        (503, "Langdock personal usage is temporarily unavailable."),
        (400, "Langdock usage request failed with HTTP 400."),
        (302, "Langdock usage request failed with HTTP 302."),
    ] {
        let status = StatusCode::from_u16(status).unwrap();
        assert_eq!(check_status(status).unwrap_err().to_string(), message);
    }
    assert!(check_status(StatusCode::OK).is_ok());
    assert!(check_status(StatusCode::NO_CONTENT).is_ok());
}

#[test]
fn an_expired_session_is_classified_as_expired() {
    let provider = LangdockProvider::new();
    assert_eq!(
        provider.error_state_kind(&model::denied("UNAUTHORIZED")),
        ProviderStateKind::ExpiredSession
    );
    assert_eq!(
        provider.error_state_kind(&model::denied("FORBIDDEN")),
        ProviderStateKind::Unknown
    );
}

#[test]
fn metadata_matches_the_upstream_descriptor() {
    let provider = LangdockProvider::new();
    let metadata = provider.metadata();
    assert_eq!(metadata.display_name, "Langdock");
    assert_eq!(metadata.session_label, "Session");
    assert_eq!(metadata.weekly_label, "Weekly");
    assert_eq!(
        metadata.dashboard_url,
        Some("https://app.langdock.com/settings/account/usage")
    );
    assert!(!metadata.default_enabled);
    assert_eq!(
        provider.available_sources(),
        vec![SourceMode::Auto, SourceMode::Web]
    );
    assert!(provider.supports_web());
}

/// Serve one canned response; the handle yields the lowercased request.
fn provider_serving(
    status: &'static str,
    body: String,
) -> (LangdockProvider, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind local test server");
    let address = listener.local_addr().expect("local server address");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut request = [0_u8; 8192];
        let read = stream.read(&mut request).expect("read request");
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .expect("write response");
        String::from_utf8_lossy(&request[..read]).into_owned()
    });
    let client = Client::builder()
        .redirect(Policy::none())
        .build()
        .expect("test HTTP client");
    (
        LangdockProvider::with_client(&format!("http://{address}/"), client),
        server,
    )
}

fn cookie_context(header: &str) -> FetchContext {
    FetchContext {
        source_mode: SourceMode::Web,
        manual_cookie_header: Some(header.into()),
        ..FetchContext::default()
    }
}

#[tokio::test]
async fn sends_the_trpc_request_with_the_session_cookie() {
    let (provider, server) = provider_serving("200 OK", body(PLAN));
    let result = provider
        .fetch_usage(&cookie_context("Cookie: auth_token=synthetic-account-a"))
        .await
        .expect("usage fetch");
    let request = server.join().expect("test server thread");
    let first_line = request.lines().next().unwrap_or_default();
    assert_eq!(
        first_line,
        format!("GET {USAGE_PATH}?batch=1&input={USAGE_INPUT} HTTP/1.1")
    );
    let lower = request.to_ascii_lowercase();
    assert!(
        lower.contains("\r\ncookie: auth_token=synthetic-account-a\r\n"),
        "{request}"
    );
    assert!(lower.contains("\r\naccept: application/json\r\n"));
    assert!(lower.contains("\r\nreferer: https://app.langdock.com/settings/account/usage\r\n"));
    assert_eq!(result.usage.primary.used_percent, 12.5);
}

#[tokio::test]
async fn a_cookie_header_without_auth_token_never_sends_a_request() {
    let provider = LangdockProvider::with_client("http://127.0.0.1:9/", Client::new());
    for header in ["session=other", "   ", "auth_token="] {
        let error = provider
            .fetch_usage(&cookie_context(header))
            .await
            .expect_err(header);
        assert_eq!(
            error.to_string(),
            "No Langdock session was found in the selected browser. Sign in at app.langdock.com or paste a Cookie header in Settings.",
            "{header}"
        );
        assert_eq!(error.state_kind(), ProviderStateKind::NeedsAuthentication);
    }
}

#[tokio::test]
async fn a_manual_source_without_a_cookie_does_not_read_a_browser() {
    let provider = LangdockProvider::with_client("http://127.0.0.1:9/", Client::new());
    let ctx = FetchContext {
        source_mode: SourceMode::Web,
        manual_cookie_missing: true,
        ..FetchContext::default()
    };
    let error = provider.fetch_usage(&ctx).await.expect_err("no session");
    assert!(matches!(error, ProviderError::BrowserSignInRequired { .. }));
}

fn browser_context() -> FetchContext {
    FetchContext {
        source_mode: SourceMode::Auto,
        ..FetchContext::default()
    }
}

#[test]
fn the_provider_owns_browser_cookie_resolution() {
    assert!(LangdockProvider::new().owns_browser_cookie_resolution());
}

#[tokio::test]
async fn automatic_skips_a_browser_without_auth_token() {
    let (provider, server) = provider_serving("200 OK", body(PLAN));
    let browser = || {
        Ok(vec![
            ("Chrome".to_owned(), "_ga=1".to_owned()),
            ("Edge".to_owned(), "auth_token=x".to_owned()),
        ])
    };
    let result = provider
        .fetch_routed(&browser_context(), &browser)
        .await
        .expect("usage fetch");
    let request = server.join().expect("test server thread");
    let cookies: Vec<&str> = request
        .lines()
        .filter(|line| line.to_ascii_lowercase().starts_with("cookie:"))
        .collect();
    assert_eq!(cookies, vec!["cookie: auth_token=x"]);
    assert_eq!(result.usage.primary.used_percent, 12.5);
}

#[tokio::test]
async fn automatic_without_any_auth_token_reports_the_missing_session() {
    let provider = LangdockProvider::with_client("http://127.0.0.1:9/", Client::new());
    let browser = || {
        Ok(vec![
            ("Chrome".to_owned(), "_ga=1".to_owned()),
            ("Edge".to_owned(), "auth_token=; _gid=2".to_owned()),
        ])
    };
    let error = provider
        .fetch_routed(&browser_context(), &browser)
        .await
        .expect_err("no session");
    assert_eq!(
        error.to_string(),
        "No Langdock session was found in the selected browser. Sign in at app.langdock.com or paste a Cookie header in Settings."
    );
    assert_eq!(error.state_kind(), ProviderStateKind::NeedsAuthentication);
}

#[tokio::test]
async fn a_manual_header_or_empty_manual_source_never_reads_a_browser() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    let counter = std::sync::Arc::clone(&calls);
    let browser = move || {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok(vec![("Edge".to_owned(), "auth_token=browser".to_owned())])
    };

    let (provider, server) = provider_serving("200 OK", body(PLAN));
    let manual = FetchContext {
        source_mode: SourceMode::Web,
        manual_cookie_header: Some("auth_token=manual".into()),
        ..FetchContext::default()
    };
    provider
        .fetch_routed(&manual, &browser)
        .await
        .expect("manual usage");
    let request = server.join().expect("test server thread");
    assert!(
        request
            .to_ascii_lowercase()
            .contains("\r\ncookie: auth_token=manual\r\n")
    );

    let offline = LangdockProvider::with_client("http://127.0.0.1:9/", Client::new());
    let empty_manual = FetchContext {
        source_mode: SourceMode::Web,
        manual_cookie_missing: true,
        ..FetchContext::default()
    };
    assert!(matches!(
        offline.fetch_routed(&empty_manual, &browser).await,
        Err(ProviderError::BrowserSignInRequired { .. })
    ));
    let manual_without_token = FetchContext {
        source_mode: SourceMode::Web,
        manual_cookie_header: Some("_ga=1".into()),
        ..FetchContext::default()
    };
    assert!(matches!(
        offline.fetch_routed(&manual_without_token, &browser).await,
        Err(ProviderError::BrowserSignInRequired { .. })
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn errors_do_not_echo_the_response_body() {
    let (provider, server) =
        provider_serving("200 OK", r#"{"secret":"synthetic-leak-0001"}"#.to_owned());
    let error = provider
        .fetch_usage(&cookie_context("auth_token=synthetic-account-a"))
        .await
        .expect_err("not an array");
    server.join().expect("test server thread");
    assert!(!error.to_string().contains("synthetic-leak-0001"));
}

#[tokio::test]
async fn cookies_off_and_oauth_do_not_fetch() {
    let provider = LangdockProvider::with_client("http://127.0.0.1:9/", Client::new());
    let off = FetchContext {
        source_mode: SourceMode::Cli,
        ..FetchContext::default()
    };
    assert_eq!(
        provider.fetch_usage(&off).await.unwrap_err().to_string(),
        "Langdock reads usage from a browser session. Turn on browser cookies or paste a Cookie header in Settings."
    );
    let oauth = FetchContext {
        source_mode: SourceMode::OAuth,
        ..FetchContext::default()
    };
    assert!(matches!(
        provider.fetch_usage(&oauth).await,
        Err(ProviderError::UnsupportedSource(SourceMode::OAuth))
    ));
}
