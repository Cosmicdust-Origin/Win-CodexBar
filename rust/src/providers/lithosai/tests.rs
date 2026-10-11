use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread::JoinHandle;

use chrono::{TimeZone, Utc};
use reqwest::redirect::Policy;

use super::*;
use crate::locale::LocaleKey;

const ME: &str = r#"{"user":{"email":"person@example.test"},"activeOrganization":{"id":"org-fixture","name":"Synthetic Organization"}}"#;
const EMPTY_SPEND: &str = r#"{"start":"2027-01-01","end":"2027-01-21","days":[]}"#;
const SESSION_HEADER: &str = "__Host-console_session=fixture-1; __Host-console_csrf=csrf-1";

/// Upstream `LithosAIPluginTests.now`: 2027-01-21 UTC.
fn now() -> DateTime<Utc> {
    Utc.timestamp_opt(1_800_489_600, 0).unwrap()
}

fn billing(balance: &str) -> String {
    format!(r#"{{"balanceNanos":{balance},"hasCard":true,"onHold":false}}"#)
}

/// Serve `responses` in order, one connection each; yields the raw requests.
fn serve(responses: Vec<(u16, String)>) -> (LithosAIProvider, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind local test server");
    let address = listener.local_addr().expect("local server address");
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for (status, body) in responses {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = [0_u8; 8192];
            let read = stream.read(&mut request).expect("read request");
            requests.push(String::from_utf8_lossy(&request[..read]).into_owned());
            write!(
                stream,
                "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("write response");
        }
        requests
    });
    let client = Client::builder()
        .redirect(Policy::none())
        .build()
        .expect("test HTTP client");
    (
        LithosAIProvider::with_client(&format!("http://{address}/"), client),
        server,
    )
}

/// For sessions that must never read a browser.
fn no_browser() -> Result<Vec<(String, String)>, ProviderError> {
    panic!("the browser must not be read")
}

fn context(header: &str) -> FetchContext {
    FetchContext {
        source_mode: SourceMode::Web,
        manual_cookie_header: Some(header.into()),
        ..FetchContext::default()
    }
}

async fn fetch(
    responses: Vec<(u16, String)>,
) -> (Result<ProviderFetchResult, ProviderError>, Vec<String>) {
    let (provider, server) = serve(responses);
    let result = provider
        .fetch_web(&context(SESSION_HEADER), &no_browser, now())
        .await;
    (result, server.join().expect("test server thread"))
}

fn ok_responses(balance: &str, spend: &str) -> Vec<(u16, String)> {
    vec![
        (200, ME.to_owned()),
        (200, billing(balance)),
        (200, spend.to_owned()),
    ]
}

fn rows(result: &ProviderFetchResult) -> Vec<(String, String)> {
    result
        .display_details
        .iter()
        .map(|row| {
            assert_eq!(row.section_title(), Some("Billing"));
            (row.title().to_owned(), row.value().to_owned())
        })
        .collect()
}

fn header<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    request.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name).then(|| value.trim())
    })
}

#[tokio::test]
async fn console_balance_and_sparse_spend_match_the_upstream_fixture() {
    let (result, requests) = fetch(ok_responses(
        "4707099860",
        r#"{"start":"2027-01-01","end":"2027-01-21","days":[
          {"day":"2027-01-21","modelId":"fixture-model","nanos":260066994},
          {"day":"2027-01-21","modelId":"fixture-other","nanos":100000000},
          {"day":"2027-01-03","modelId":"fixture-model","nanos":2000000000}]}"#,
    ))
    .await;
    let result = result.expect("usage");

    assert_eq!(
        rows(&result),
        [
            ("Balance", "$4.71"),
            ("Payment card", "Added"),
            ("Account status", "Active"),
            ("Today (UTC)", "$0.36"),
            ("This month (UTC)", "$2.36"),
        ]
        .map(|(title, value)| (title.to_owned(), value.to_owned()))
    );
    let cost = result.cost.as_ref().expect("prepaid cost block");
    assert_eq!(cost.balance, Some(4.707_099_86));
    assert_eq!(cost.used, 0.0);
    assert_eq!(cost.limit, None);
    assert_eq!(cost.currency_code, "USD");
    assert_eq!(cost.period, "Prepaid credits");
    // Upstream `.prepaidCredits` titles the block "Credits". The Billing
    // "Balance" row stays too, so the card shows the balance twice, as on the
    // Mac; only the headline bar stays off the card.
    assert_eq!(cost.card_title_key, Some(LocaleKey::CreditsTitle));
    assert_eq!(result.display_details[0].id(), "balance");
    let usage = &result.usage;
    assert!(usage.primary.is_informational);
    assert!(usage.primary.hidden_on_card);
    assert_eq!(
        usage.primary.reset_description.as_deref(),
        Some("Balance: $4.71")
    );
    assert!(usage.secondary.is_none());
    assert_eq!(usage.account_email.as_deref(), Some("person@example.test"));
    assert_eq!(
        usage.account_organization.as_deref(),
        Some("Synthetic Organization")
    );
    assert_eq!(usage.login_method.as_deref(), Some("Browser session"));
    assert_eq!(result.source_label, "web");

    let paths: Vec<&str> = requests
        .iter()
        .map(|request| request.lines().next().unwrap_or_default())
        .collect();
    assert_eq!(
        paths,
        [
            "GET /api/me HTTP/1.1",
            "GET /api/billing HTTP/1.1",
            "GET /api/billing/spend?start=2027-01-01&end=2027-01-21 HTTP/1.1",
        ]
    );
    for request in &requests {
        assert_eq!(header(request, "x-console-csrf"), Some("csrf-1"));
        assert_eq!(header(request, "cookie"), Some(SESSION_HEADER));
    }
    assert_eq!(header(&requests[0], "x-organization-id"), None);
    assert_eq!(
        header(&requests[1], "x-organization-id"),
        Some("org-fixture")
    );
    assert_eq!(
        header(&requests[2], "x-organization-id"),
        Some("org-fixture")
    );
}

#[tokio::test]
async fn the_parity_pack_payload_shows_balance_and_month_spend() {
    let (result, _) = fetch(ok_responses(
        "42370000000",
        r#"{"start":"2027-01-01","end":"2027-01-21","days":[
          {"day":"2027-01-01","nanos":6000000000},{"day":"2027-01-21","nanos":1840000000}]}"#,
    ))
    .await;
    let result = result.expect("usage");
    assert_eq!(
        rows(&result)
            .into_iter()
            .map(|(_, value)| value)
            .collect::<Vec<_>>(),
        ["$42.37", "Added", "Active", "$1.84", "$7.84"]
    );
}

#[tokio::test]
async fn zero_debt_and_subcent_balances_are_not_invented_quotas() {
    for (balance, cost_balance, shown) in [
        ("0", Some(0.0), "$0.00"),
        // The shared cost block floors balances at zero, so a debt has no
        // cost block and stays visible in the primary line and Balance row.
        ("-1000000000", None, "-$1.00"),
        ("1000000", Some(0.001), "Less than $0.01"),
    ] {
        let (result, _) = fetch(ok_responses(balance, EMPTY_SPEND)).await;
        let result = result.expect(balance);
        assert_eq!(
            result.cost.as_ref().map(|cost| cost.balance),
            cost_balance.map(Some),
            "{balance}"
        );
        assert!(result.usage.primary.is_informational);
        assert!(result.usage.primary.hidden_on_card, "{balance}");
        assert_eq!(
            result.usage.primary.reset_description.as_deref(),
            Some(format!("Balance: {shown}").as_str())
        );
        // Every balance with a cost block gets the Credits title; a debt has
        // no block, so the Billing row alone shows it.
        assert_eq!(
            result.cost.as_ref().and_then(|cost| cost.card_title_key),
            cost_balance.map(|_| LocaleKey::CreditsTitle),
            "{balance}"
        );
        let rows = rows(&result);
        assert_eq!(rows[0], ("Balance".to_owned(), shown.to_owned()));
        assert_eq!(rows[3], ("Today (UTC)".to_owned(), "$0.00".to_owned()));
        assert_eq!(rows[4], ("This month (UTC)".to_owned(), "$0.00".to_owned()));
    }
}

#[tokio::test]
async fn billing_flags_render_their_negative_labels() {
    let (result, _) = fetch(vec![
        (200, ME.to_owned()),
        (
            200,
            r#"{"balanceNanos":1500000000,"hasCard":false,"onHold":true}"#.to_owned(),
        ),
        (200, EMPTY_SPEND.to_owned()),
    ])
    .await;
    let rows = rows(&result.expect("usage"));
    assert_eq!(rows[1].1, "Not added");
    assert_eq!(rows[2].1, "On hold");
}

#[tokio::test]
async fn malformed_required_balance_fails_closed() {
    for balance in ["null", "\"4707099860\"", "0.5", "1e99", "9007199254740992"] {
        let (result, _) = fetch(vec![(200, ME.to_owned()), (200, billing(balance))]).await;
        assert_eq!(
            result.expect_err(balance).to_string(),
            "Parse error: Could not parse LithosAI integer amount.",
            "{balance}"
        );
    }
    let (result, _) = fetch(vec![
        (200, ME.to_owned()),
        (
            200,
            r#"{"balanceNanos":1,"hasCard":"yes","onHold":false}"#.to_owned(),
        ),
    ])
    .await;
    assert_eq!(
        result.unwrap_err().to_string(),
        "Parse error: Could not parse LithosAI hasCard."
    );
}

#[tokio::test]
async fn an_integral_float_balance_is_a_safe_integer() {
    let (result, _) = fetch(ok_responses("2000000000.0", EMPTY_SPEND)).await;
    assert_eq!(result.expect("usage").cost.unwrap().balance, Some(2.0));
}

#[tokio::test]
async fn optional_malformed_spend_preserves_balance_without_reporting_false_zero() {
    for spend in [
        "not-json",
        "{}",
        r#"{"days":null}"#,
        r#"{"start":"2027-01-01","end":"2027-01-21","days":[{"day":"2027-01-22","nanos":1}]}"#,
        r#"{"start":"2027-01-01","end":"2027-01-21","days":[{"day":"2026-12-31","nanos":1}]}"#,
        r#"{"start":"2027-01-01","end":"2027-01-21","days":[{"day":"2027-1-05","nanos":1}]}"#,
        r#"{"start":"2027-01-01","end":"2027-01-21","days":[{"day":"2027-01-05","nanos":-1}]}"#,
        r#"{"start":"2027-01-01","end":"2027-01-21","days":[{"day":"2027-01-05","nanos":0.5}]}"#,
        r#"{"start":"2027-01-01","end":"2027-01-21","days":[{"day":"2027-01-05","nanos":9007199254740991},{"day":"2027-01-06","nanos":1}]}"#,
        r#"{"start":"2027-01-02","end":"2027-01-21","days":[]}"#,
        r#"{"start":"2027-01-01","end":"2027-01-20","days":[]}"#,
    ] {
        let (result, _) = fetch(ok_responses("4707099860", spend)).await;
        let result = result.expect(spend);
        assert_eq!(result.cost.as_ref().unwrap().balance, Some(4.707_099_86));
        let rows = rows(&result);
        assert_eq!(rows.len(), 4, "{spend}");
        assert_eq!(
            rows[3],
            ("Spend".to_owned(), "Unavailable".to_owned()),
            "{spend}"
        );
    }
}

#[tokio::test]
async fn spend_http_failures_keep_the_balance_except_an_expired_session() {
    for status in [403, 404, 429, 500] {
        let (result, _) = fetch(vec![
            (200, ME.to_owned()),
            (200, billing("4707099860")),
            (status, "{}".to_owned()),
        ])
        .await;
        let result = result.expect("balance survives");
        assert_eq!(rows(&result)[3].1, "Unavailable");
    }
    let (result, _) = fetch(vec![
        (200, ME.to_owned()),
        (200, billing("4707099860")),
        (401, "{}".to_owned()),
    ])
    .await;
    assert_eq!(
        result.unwrap_err().to_string(),
        "LithosAI session expired. Sign in again or paste fresh cookies."
    );
}

#[tokio::test]
async fn an_expired_session_at_any_request_is_reported_as_expired() {
    for at in 1..=2 {
        let mut responses = ok_responses("4707099860", EMPTY_SPEND);
        responses.truncate(at);
        responses[at - 1] = (401, "{}".to_owned());
        let (provider, server) = serve(responses);
        let error = provider
            .fetch_web(&context(SESSION_HEADER), &no_browser, now())
            .await
            .expect_err("expired");
        server.join().expect("test server thread");
        assert_eq!(
            error.to_string(),
            "LithosAI session expired. Sign in again or paste fresh cookies."
        );
        assert_eq!(
            provider.error_state_kind(&error),
            ProviderStateKind::ExpiredSession
        );
    }
}

#[tokio::test]
async fn non_authentication_failures_map_to_upstream_messages() {
    for (status, message) in [
        (403, "LithosAI console access was denied."),
        (429, "LithosAI console requests are rate limited."),
        (500, "LithosAI console is unavailable."),
        (503, "LithosAI console is unavailable."),
        (404, "LithosAI console returned HTTP 404."),
        (302, "LithosAI console returned HTTP 302."),
    ] {
        let (provider, server) = serve(vec![(status, "{}".to_owned())]);
        let error = provider
            .fetch_web(&context(SESSION_HEADER), &no_browser, now())
            .await
            .expect_err(message);
        server.join().expect("test server thread");
        assert_eq!(error.to_string(), message);
        assert_ne!(
            provider.error_state_kind(&error),
            ProviderStateKind::ExpiredSession
        );
    }
}

#[tokio::test]
async fn the_active_organization_is_validated_before_billing() {
    for (me, message) in [
        (
            r#"{"user":{},"activeOrganization":{"id":"org/../x"}}"#,
            "organization ID",
        ),
        (
            r#"{"user":{},"activeOrganization":{"id":"   "}}"#,
            "active organization",
        ),
        (
            r#"{"user":{},"activeOrganization":{"id":7}}"#,
            "active organization",
        ),
        (r#"{"user":{}}"#, "response object"),
        (
            r#"{"activeOrganization":{"id":"org-fixture"}}"#,
            "response object",
        ),
        ("[]", "response object"),
        ("not-json", "JSON response"),
    ] {
        let (result, requests) = fetch(vec![(200, me.to_owned())]).await;
        assert_eq!(requests.len(), 1);
        assert_eq!(
            result.expect_err(me).to_string(),
            format!("Parse error: Could not parse LithosAI {message}."),
            "{me}"
        );
    }
    let long = "a".repeat(129);
    let (result, _) = fetch(vec![(
        200,
        format!(r#"{{"user":{{}},"activeOrganization":{{"id":"{long}"}}}}"#),
    )])
    .await;
    assert!(result.unwrap_err().to_string().contains("organization ID"));
}

#[tokio::test]
async fn missing_identity_fields_stay_empty() {
    let (result, _) = fetch(vec![
        (
            200,
            r#"{"user":{"email":"  "},"activeOrganization":{"id":"org_1"}}"#.to_owned(),
        ),
        (200, billing("1")),
        (200, EMPTY_SPEND.to_owned()),
    ])
    .await;
    let usage = result.expect("usage").usage;
    assert!(usage.account_email.is_none());
    assert!(usage.account_organization.is_none());
}

#[tokio::test]
async fn both_console_cookies_are_required_before_any_request() {
    let provider = LithosAIProvider::with_client("http://127.0.0.1:9/", Client::new());
    for header in [
        "__Host-console_session=fixture-1",
        "__Host-console_csrf=csrf-1",
        "__Host-console_session=; __Host-console_csrf=csrf-1",
        "session=other",
    ] {
        let error = provider
            .fetch_web(&context(header), &no_browser, now())
            .await
            .expect_err(header);
        assert!(
            matches!(error, ProviderError::BrowserSignInRequired { .. }),
            "{header}"
        );
        assert_eq!(error.state_kind(), ProviderStateKind::NeedsAuthentication);
    }
    let ctx = FetchContext {
        source_mode: SourceMode::Web,
        manual_cookie_missing: true,
        ..FetchContext::default()
    };
    assert!(matches!(
        provider.fetch_web(&ctx, &no_browser, now()).await,
        Err(ProviderError::BrowserSignInRequired { .. })
    ));
}

const SESSION_A: &str = "__Host-console_session=fixture-a; __Host-console_csrf=csrf-a";
const SESSION_B: &str = "__Host-console_session=fixture-b; __Host-console_csrf=csrf-b";

fn browser_context() -> FetchContext {
    FetchContext {
        source_mode: SourceMode::Auto,
        ..FetchContext::default()
    }
}

fn two_browsers() -> Result<Vec<(String, String)>, ProviderError> {
    Ok(vec![
        ("Chrome".to_owned(), SESSION_A.to_owned()),
        ("Edge".to_owned(), SESSION_B.to_owned()),
    ])
}

#[tokio::test]
async fn automatic_import_skips_a_browser_without_both_console_cookies() {
    let (provider, server) = serve(ok_responses("4707099860", EMPTY_SPEND));
    let browser = || {
        Ok(vec![
            (
                "Chrome".to_owned(),
                "__Host-console_session=fixture-a".to_owned(),
            ),
            ("Edge".to_owned(), SESSION_B.to_owned()),
        ])
    };
    let result = provider
        .fetch_web(&browser_context(), &browser, now())
        .await;
    let requests = server.join().expect("test server thread");
    assert_eq!(rows(&result.expect("usage"))[0].1, "$4.71");
    assert_eq!(requests.len(), 3);
    for request in &requests {
        assert_eq!(header(request, "cookie"), Some(SESSION_B));
        assert_eq!(header(request, "x-console-csrf"), Some("csrf-b"));
    }
}

#[tokio::test]
async fn an_expired_browser_session_advances_to_the_next_browser() {
    // Upstream `expired session advances to the next profile`: a 401 at any
    // of the three requests rejects that session.
    for at in 0..3 {
        let mut responses = vec![(200, ME.to_owned()), (200, billing("4707099860"))];
        responses.truncate(at);
        responses.push((401, "{}".to_owned()));
        responses.extend(ok_responses("4707099860", EMPTY_SPEND));
        let (provider, server) = serve(responses);
        let result = provider
            .fetch_web(&browser_context(), &two_browsers, now())
            .await;
        let requests = server.join().expect("test server thread");
        assert_eq!(rows(&result.expect("usage"))[0].1, "$4.71", "{at}");
        assert_eq!(requests.len(), at + 4, "{at}");
        for request in &requests[..=at] {
            assert_eq!(header(request, "x-console-csrf"), Some("csrf-a"), "{at}");
        }
        assert_eq!(
            requests[at + 1].lines().next(),
            Some("GET /api/me HTTP/1.1")
        );
        for request in &requests[at + 1..] {
            assert_eq!(header(request, "x-console-csrf"), Some("csrf-b"), "{at}");
            assert_eq!(header(request, "cookie"), Some(SESSION_B), "{at}");
        }
    }
}

#[tokio::test]
async fn every_expired_browser_session_reports_an_expired_session() {
    let (provider, server) = serve(vec![(401, "{}".to_owned()), (401, "{}".to_owned())]);
    let error = provider
        .fetch_web(&browser_context(), &two_browsers, now())
        .await
        .expect_err("expired");
    assert_eq!(server.join().expect("test server thread").len(), 2);
    assert_eq!(
        error.to_string(),
        "LithosAI session expired. Sign in again or paste fresh cookies."
    );
    assert_eq!(
        provider.error_state_kind(&error),
        ProviderStateKind::ExpiredSession
    );
}

#[tokio::test]
async fn a_non_authentication_failure_does_not_try_the_next_browser() {
    let (provider, server) = serve(vec![(403, "{}".to_owned())]);
    let error = provider
        .fetch_web(&browser_context(), &two_browsers, now())
        .await
        .expect_err("denied");
    assert_eq!(server.join().expect("test server thread").len(), 1);
    assert_eq!(error.to_string(), "LithosAI console access was denied.");
}

#[tokio::test]
async fn an_expired_manual_header_never_reads_a_browser() {
    let (provider, server) = serve(vec![(401, "{}".to_owned())]);
    let error = provider
        .fetch_web(&context(SESSION_HEADER), &no_browser, now())
        .await
        .expect_err("expired");
    assert_eq!(server.join().expect("test server thread").len(), 1);
    assert_eq!(
        error.to_string(),
        "LithosAI session expired. Sign in again or paste fresh cookies."
    );
}

#[tokio::test]
async fn a_browser_read_failure_is_reported_when_no_session_is_usable() {
    let provider = LithosAIProvider::with_client("http://127.0.0.1:9/", Client::new());
    let blocked = || {
        Err(ProviderError::Other(
            "Failed to read browser cookies: synthetic app-bound encryption".to_owned(),
        ))
    };
    assert_eq!(
        provider
            .fetch_web(&browser_context(), &blocked, now())
            .await
            .unwrap_err()
            .to_string(),
        "Failed to read browser cookies: synthetic app-bound encryption"
    );
    let no_cookies = || Err(ProviderError::NoCookies);
    let csrf_only = || {
        Ok(vec![(
            "Chrome".to_owned(),
            "__Host-console_csrf=csrf-a".to_owned(),
        )])
    };
    for browser in [&no_cookies as &BrowserCookieHeaders, &csrf_only] {
        let error = provider
            .fetch_web(&browser_context(), browser, now())
            .await
            .expect_err("no usable session");
        assert!(matches!(error, ProviderError::BrowserSignInRequired { .. }));
    }
}

#[tokio::test]
async fn disabled_cookies_and_oauth_never_fetch() {
    let provider = LithosAIProvider::with_client("http://127.0.0.1:9/", Client::new());
    let off = FetchContext {
        source_mode: SourceMode::Cli,
        manual_cookie_header: Some(SESSION_HEADER.into()),
        ..FetchContext::default()
    };
    assert_eq!(
        provider.fetch_usage(&off).await.unwrap_err().to_string(),
        "LithosAI cookies are disabled."
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

#[tokio::test]
async fn errors_do_not_echo_cookies_or_bodies() {
    let (result, _) = fetch(vec![(200, r#"{"leak":"synthetic-leak-0001"}"#.to_owned())]).await;
    let message = result.unwrap_err().to_string();
    assert!(!message.contains("synthetic-leak-0001"));
    assert!(!message.contains("csrf-1"));
}

#[test]
fn spend_range_is_the_utc_month_to_date() {
    assert_eq!(
        model::spend_range(now()),
        ("2027-01-01".to_owned(), "2027-01-21".to_owned())
    );
    let first = Utc.with_ymd_and_hms(2026, 10, 1, 23, 59, 59).unwrap();
    assert_eq!(
        model::spend_range(first),
        ("2026-10-01".to_owned(), "2026-10-01".to_owned())
    );
}

#[test]
fn money_matches_upstream_formatting() {
    assert_eq!(model::money(1234.5), "$1,234.50");
    assert_eq!(model::money(0.004), "Less than $0.01");
    assert_eq!(model::money(0.0), "$0.00");
}

#[test]
fn metadata_matches_the_upstream_descriptor() {
    let provider = LithosAIProvider::new();
    let metadata = provider.metadata();
    assert_eq!(metadata.display_name, "LithosAI");
    assert_eq!(metadata.session_label, "Balance");
    assert_eq!(metadata.weekly_label, "Spend");
    assert_eq!(
        metadata.dashboard_url,
        Some("https://console.lithosai.cloud")
    );
    assert!(!metadata.default_enabled);
    assert!(provider.owns_browser_cookie_resolution());
    assert_eq!(
        provider.available_sources(),
        vec![SourceMode::Auto, SourceMode::Web]
    );
}
