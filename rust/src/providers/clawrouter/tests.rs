use std::io::{Read, Write};
use std::net::TcpListener;

use chrono::{TimeZone, Utc};
use reqwest::redirect::Policy;
use serde_json::{Value, json};

use super::model::{build_result, monthly_reset, parse_usage};
use super::*;
use crate::locale::LocaleKey;

/// The parity pack payload (`scenarios/providers/ClawRouter/routes.json`)
/// with `{{month:+0}}` resolved to `window_key`.
fn pack_payload(window_key: &str) -> Value {
    json!({
        "budget": {
            "configured": true,
            "ledger": "parity-ledger",
            "limitMicros": 50_000_000,
            "spentMicros": 18_420_000,
            "remainingMicros": 31_580_000,
            "windowKey": window_key
        },
        "usage": {
            "summary": {
                "requestCount": 1284,
                "successCount": 1261,
                "errorCount": 23,
                "inputTokens": 3_120_450,
                "outputTokens": 812_300,
                "totalTokens": 3_932_750,
                "actualCostMicros": 18_420_000
            },
            "providers": [
                {"provider": "Anthropic", "requestCount": 402, "successCount": 395, "errorCount": 7, "totalTokens": 1_520_400, "actualCostMicros": 9_120_000},
                {"provider": "OpenAI", "requestCount": 511, "successCount": 503, "errorCount": 8, "totalTokens": 1_230_100, "actualCostMicros": 5_300_000},
                {"provider": "DeepSeek", "requestCount": 233, "successCount": 225, "errorCount": 8, "totalTokens": 790_250, "actualCostMicros": 2_700_000},
                {"provider": "Mistral", "requestCount": 138, "successCount": 138, "errorCount": 0, "totalTokens": 392_000, "actualCostMicros": 1_300_000}
            ]
        }
    })
}

fn parse(value: &Value) -> Result<super::model::ClawRouterUsage, ProviderError> {
    parse_usage(value.to_string().as_bytes())
}

/// (section, title, value, secondary) for every display row, in order.
fn rows(result: &ProviderFetchResult) -> Vec<(String, String, String, Option<String>)> {
    result
        .display_details()
        .iter()
        .map(|row| {
            (
                row.section_title().unwrap_or_default().to_owned(),
                row.title().to_owned(),
                row.value().to_owned(),
                row.secondary_value().map(str::to_owned),
            )
        })
        .collect()
}

fn row(
    section: &str,
    title: &str,
    value: &str,
    secondary: Option<&str>,
) -> (String, String, String, Option<String>) {
    (
        section.to_owned(),
        title.to_owned(),
        value.to_owned(),
        secondary.map(str::to_owned),
    )
}

#[test]
fn pack_result_matches_the_mac_card() {
    let usage = parse(&pack_payload("parity-2026-10")).expect("pack payload parses");
    let result = build_result(&usage);
    let reset = Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap();

    // "Monthly budget 63% left", resets at the first of next month.
    let primary = &result.usage.primary;
    assert!((primary.used_percent - 36.84).abs() < 1e-9);
    assert_eq!(primary.remaining_percent().round(), 63.0);
    assert_eq!(primary.resets_at, Some(reset));
    assert_eq!(primary.window_minutes, None);
    assert!(!primary.is_informational);
    assert!(result.usage.secondary.is_none());

    // Header: "Managed monthly budget"; organization counts routed providers.
    assert_eq!(
        result.usage.login_method.as_deref(),
        Some("Managed monthly budget")
    );
    assert_eq!(
        result.usage.account_organization.as_deref(),
        Some("4 routed providers")
    );

    // "This month: $18.42 / $50.00".
    let cost = result.cost.as_ref().expect("monthly cost");
    assert!((cost.used - 18.42).abs() < 1e-9);
    assert_eq!(cost.limit, Some(50.0));
    assert_eq!(cost.currency_code, "USD");
    assert_eq!(cost.period, "This month");
    assert_eq!(cost.resets_at, Some(reset));
    // Upstream `.clawRouter` with a positive limit titles the block "Monthly budget".
    assert_eq!(
        cost.card_title_key,
        Some(LocaleKey::CostTitleClawRouterMonthlyBudget)
    );

    assert_eq!(
        rows(&result),
        vec![
            row(
                "Usage",
                "Requests",
                "1284",
                Some("1261 succeeded · 23 failed")
            ),
            row(
                "Usage",
                "Tokens",
                "3932750",
                Some("3120450 input · 812300 output")
            ),
            row("Usage", "Actual cost", "$18.420000", None),
            row("Usage", "Budget ledger", "parity-ledger", None),
            row(
                "Usage",
                "Monthly budget",
                "$18.420000 / $50.00",
                Some("$31.580000 remaining")
            ),
            row(
                "Routed providers",
                "Anthropic",
                "402 requests",
                Some("$9.120000 · 1520400 tokens")
            ),
            row(
                "Routed providers",
                "OpenAI",
                "511 requests",
                Some("$5.300000 · 1230100 tokens")
            ),
            row(
                "Routed providers",
                "DeepSeek",
                "233 requests",
                Some("$2.700000 · 790250 tokens")
            ),
            row(
                "Routed providers",
                "Mistral",
                "138 requests",
                Some("$1.300000 · 392000 tokens")
            ),
        ]
    );
}

#[test]
fn monthly_reset_rolls_into_the_next_month_and_year() {
    let key = |value: &str| json!(value);
    assert_eq!(
        monthly_reset(Some(&key("2026-01"))),
        Utc.with_ymd_and_hms(2026, 2, 1, 0, 0, 0).single()
    );
    assert_eq!(
        monthly_reset(Some(&key("ledger:2026-12"))),
        Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).single()
    );
    for invalid in [
        "2026-13", "2026-00", "2026-1", "26-10", "2026/10", "2026-10x", "",
    ] {
        assert_eq!(monthly_reset(Some(&key(invalid))), None, "{invalid}");
    }
    assert_eq!(monthly_reset(Some(&json!(202610))), None);
    assert_eq!(monthly_reset(None), None);
}

#[test]
fn routed_providers_sort_by_cost_then_requests_then_name_and_cap_at_twenty() {
    let mut payload = pack_payload("2026-10");
    let providers: Vec<Value> = (0..25)
        .map(|index| {
            json!({
                "provider": if index == 3 { "  ".to_owned() } else { format!("p{index:02}") },
                "requestCount": index % 3,
                "successCount": 0,
                "errorCount": 0,
                "totalTokens": 10,
                "actualCostMicros": if index < 6 { 1_000_000 } else { 0 }
            })
        })
        .collect();
    payload["usage"]["providers"] = Value::Array(providers);
    let usage = parse(&payload).expect("parses");

    let names: Vec<&str> = usage.providers.iter().map(|p| p.name.as_str()).collect();
    // $1 rows first, by requests desc (2, 1, 0) then case-insensitive name;
    // blank names become "Unknown".
    assert_eq!(
        &names[..6],
        ["p02", "p05", "p01", "p04", "p00", "Unknown"].as_slice()
    );

    let result = build_result(&usage);
    let routed = rows(&result)
        .into_iter()
        .filter(|(section, ..)| section == "Routed providers")
        .count();
    assert_eq!(routed, 20);
    assert_eq!(
        result.usage.account_organization.as_deref(),
        Some("25 routed providers")
    );
}

#[test]
fn unmetered_policy_reports_actual_cost_without_a_limit() {
    let mut payload = pack_payload("2026-10");
    payload["budget"] = json!({"configured": false, "ledger": "none", "windowKey": "2026-10"});
    let result = build_result(&parse(&payload).expect("parses"));

    assert!(result.usage.primary.is_informational);
    assert_eq!(
        result.usage.primary.reset_description.as_deref(),
        Some("Unmetered")
    );
    assert_eq!(result.usage.login_method.as_deref(), Some("Unmetered"));
    let cost = result.cost.as_ref().expect("actual cost");
    assert!((cost.used - 18.42).abs() < 1e-9);
    assert_eq!(cost.limit, None);
    assert_eq!(
        cost.card_title_key,
        Some(LocaleKey::CostTitleClawRouterSpend)
    );
    assert_eq!(
        cost.resets_at,
        Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).single()
    );
    assert!(
        rows(&result)
            .iter()
            .all(|(_, title, ..)| title != "Monthly budget")
    );

    // No spend at all: no cost block.
    payload["usage"]["summary"]["actualCostMicros"] = json!(0);
    assert!(
        build_result(&parse(&payload).expect("parses"))
            .cost
            .is_none()
    );
}

#[test]
fn zero_limit_keeps_the_cost_but_drops_the_meter() {
    let mut payload = pack_payload("2026-10");
    payload["budget"]["limitMicros"] = json!(0);
    payload["budget"]["remainingMicros"] = Value::Null;
    let result = build_result(&parse(&payload).expect("parses"));

    assert!(result.usage.primary.is_informational);
    assert_eq!(
        result.usage.primary.reset_description.as_deref(),
        Some("No monthly limit reported")
    );
    assert_eq!(result.cost.as_ref().and_then(|cost| cost.limit), Some(0.0));
    // Upstream `.clawRouter` with `limit <= 0` is "ClawRouter spend".
    assert_eq!(
        result.cost.as_ref().and_then(|cost| cost.card_title_key),
        Some(LocaleKey::CostTitleClawRouterSpend)
    );
    assert!(rows(&result).contains(&row("Usage", "Monthly budget", "$18.420000 / $0.00", None)));
}

#[test]
fn overspent_budget_clamps_to_full() {
    let mut payload = pack_payload("2026-10");
    payload["budget"]["spentMicros"] = json!(75_000_000);
    let result = build_result(&parse(&payload).expect("parses"));
    assert_eq!(result.usage.primary.used_percent, 100.0);
}

#[test]
fn rejects_invalid_payloads_with_upstream_messages() {
    let cases: Vec<(Value, &str)> = vec![
        (json!([]), "response shape is invalid"),
        (json!({"budget": {}}), "response shape is invalid"),
        (
            {
                let mut p = pack_payload("2026-10");
                p["usage"]["providers"] = json!({});
                p
            },
            "response shape is invalid",
        ),
        (
            {
                let mut p = pack_payload("2026-10");
                p["budget"]["configured"] = json!("yes");
                p
            },
            "budget is invalid",
        ),
        (
            {
                let mut p = pack_payload("2026-10");
                p["budget"]["limitMicros"] = json!(1.5);
                p
            },
            "budget.limitMicros must be an integer",
        ),
        (
            {
                let mut p = pack_payload("2026-10");
                p["usage"]["summary"]["totalTokens"] = json!("3932750");
                p
            },
            "summary.totalTokens must be an integer",
        ),
        (
            {
                let mut p = pack_payload("2026-10");
                p["usage"]["providers"][1]["provider"] = json!(7);
                p
            },
            "provider name must be a string",
        ),
        (
            {
                let mut p = pack_payload("2026-10");
                p["usage"]["providers"][0]["errorCount"] = Value::Null;
                p
            },
            "provider.errorCount must be an integer",
        ),
    ];
    for (payload, detail) in cases {
        let error = parse(&payload).expect_err(detail);
        assert_eq!(
            error.to_string(),
            format!("Parse error: Could not parse ClawRouter usage: {detail}")
        );
    }
    assert_eq!(
        parse_usage(b"<html>").unwrap_err().to_string(),
        "Parse error: Could not parse ClawRouter usage: response was not valid JSON"
    );
}

#[test]
fn integral_floats_count_as_integers() {
    let mut payload = pack_payload("2026-10");
    payload["usage"]["summary"]["requestCount"] = json!(1284.0);
    assert_eq!(parse(&payload).expect("parses").summary.requests, 1284);
}

#[test]
fn base_url_overrides_normalize_to_v1_usage_over_https() {
    for (raw, expected) in [
        (
            "https://router.example.com",
            "https://router.example.com/v1/usage",
        ),
        (
            "https://router.example.com/",
            "https://router.example.com/v1/usage",
        ),
        (
            "https://router.example.com/v1",
            "https://router.example.com/v1/usage",
        ),
        (
            "https://router.example.com/v1/",
            "https://router.example.com/v1/usage",
        ),
        (
            "https://router.example.com/team/v1",
            "https://router.example.com/team/v1/usage",
        ),
        ("router.example.com", "https://router.example.com/v1/usage"),
        (
            "  router.example.com:8443  ",
            "https://router.example.com:8443/v1/usage",
        ),
    ] {
        assert_eq!(usage_url(raw).expect(raw).as_str(), expected, "{raw}");
    }
}

#[test]
fn base_url_overrides_reject_non_https_and_credentials() {
    for raw in [
        "http://router.example.com",
        "http://localhost:8080",
        "ftp://router.example.com",
        "https://user:pass@router.example.com",
        "https://router.example.com/?x=1",
        "https://router.example.com/#frag",
        "https://",
        "https://router example.com",
        "/",
    ] {
        let error = usage_url(raw).expect_err(raw);
        assert_eq!(
            error.to_string(),
            "ClawRouter Base URL is invalid. Use an HTTPS URL without embedded credentials.",
            "{raw}"
        );
        assert!(validate_gateway_url(raw).is_err(), "{raw}");
    }
    assert!(validate_gateway_url("").is_ok());
    assert!(validate_gateway_url("   ").is_ok());
    assert!(validate_gateway_url("clawrouter.openclaw.ai").is_ok());
}

#[test]
fn maps_statuses_to_upstream_messages() {
    for status in [StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN] {
        assert_eq!(
            check_status(status).unwrap_err().to_string(),
            "ClawRouter rejected the API key. Check the key and its policy status."
        );
    }
    for (status, code) in [
        (StatusCode::TOO_MANY_REQUESTS, 429),
        (StatusCode::BAD_GATEWAY, 502),
        (StatusCode::NOT_FOUND, 404),
    ] {
        assert_eq!(
            check_status(status).unwrap_err().to_string(),
            format!("ClawRouter API returned HTTP {code}.")
        );
    }
    assert!(check_status(StatusCode::OK).is_ok());
}

#[test]
fn rejected_keys_classify_as_needs_authentication() {
    let provider = ClawRouterProvider::new();
    let kind = |status: StatusCode| provider.error_state_kind(&check_status(status).unwrap_err());
    assert_eq!(
        kind(StatusCode::UNAUTHORIZED),
        ProviderStateKind::NeedsAuthentication
    );
    assert_eq!(
        kind(StatusCode::FORBIDDEN),
        ProviderStateKind::NeedsAuthentication
    );
    for status in [
        StatusCode::INTERNAL_SERVER_ERROR,
        StatusCode::TOO_MANY_REQUESTS,
        StatusCode::NOT_FOUND,
    ] {
        assert_eq!(kind(status), ProviderStateKind::Unknown, "{status}");
    }
    // The guard is message-scoped: other `Other` errors keep the default.
    assert_eq!(
        provider.error_state_kind(&usage_url("http://router.example.com").unwrap_err()),
        ProviderStateKind::Unknown
    );
    // A missing key still reaches the shared sign-in mapping.
    assert_eq!(
        provider.error_state_kind(&ProviderError::NotInstalled("API key not found.".into())),
        ProviderStateKind::NeedsAuthentication
    );
}

#[test]
fn saved_base_url_wins_over_the_environment_fallback() {
    fn env_router() -> Option<String> {
        Some("https://env.example.com".into())
    }
    fn env_blank() -> Option<String> {
        Some("   ".into())
    }
    fn env_unset() -> Option<String> {
        None
    }
    let with_gateway = |gateway: Option<&str>| FetchContext {
        gateway_url: gateway.map(str::to_owned),
        ..FetchContext::default()
    };

    assert_eq!(
        configured_base_url(&with_gateway(Some("https://saved.example.com")), env_router),
        Some("https://saved.example.com".to_owned())
    );
    for gateway in [None, Some(""), Some("  ")] {
        assert_eq!(
            configured_base_url(&with_gateway(gateway), env_router),
            Some("https://env.example.com".to_owned()),
            "{gateway:?}"
        );
        assert_eq!(configured_base_url(&with_gateway(gateway), env_blank), None);
        assert_eq!(configured_base_url(&with_gateway(gateway), env_unset), None);
    }
}

/// Upstream `ClawRouterPluginGoldenTests.budgetedResponse` (v0.73.0), verbatim.
const UPSTREAM_BUDGETED_RESPONSE: &str = r#"{
  "policyId": "openclaw-smoke",
  "budget": {
    "configured": true,
    "ledger": "durable_object",
    "windowKey": "openclaw/openclaw-smoke/2026-07",
    "limitMicros": 25000000,
    "spentMicros": 6000,
    "remainingMicros": 24994000
  },
  "usage": {
    "ledger": "ready",
    "summary": {
      "requestCount": 6,
      "successCount": 5,
      "errorCount": 1,
      "inputTokens": 50000,
      "outputTokens": 4191,
      "totalTokens": 54191,
      "actualCostMicros": 6000
    },
    "providers": [
      {
        "provider": "anthropic",
        "requestCount": 2,
        "successCount": 2,
        "errorCount": 0,
        "totalTokens": 12191,
        "actualCostMicros": 2000
      },
      {
        "provider": "openai",
        "requestCount": 4,
        "successCount": 3,
        "errorCount": 1,
        "totalTokens": 42000,
        "actualCostMicros": 4000
      }
    ],
    "events": []
  }
}"#;

#[test]
fn upstream_budgeted_golden_fixture() {
    let usage = parse_usage(UPSTREAM_BUDGETED_RESPONSE.as_bytes()).expect("golden parses");
    let result = build_result(&usage);
    let reset = Utc.with_ymd_and_hms(2026, 8, 1, 0, 0, 0).unwrap();

    let primary = &result.usage.primary;
    assert!(
        (primary.used_percent - 0.024).abs() < 1e-12,
        "{}",
        primary.used_percent
    );
    assert_eq!(primary.resets_at, Some(reset));
    assert!(!primary.is_informational);
    assert!(result.usage.secondary.is_none());

    let cost = result.cost.as_ref().expect("monthly cost");
    assert!((cost.used - 0.006).abs() < 1e-12, "{}", cost.used);
    assert_eq!(cost.limit, Some(25.0));
    assert_eq!(cost.resets_at, Some(reset));

    let routed: Vec<String> = rows(&result)
        .into_iter()
        .filter(|(section, ..)| section == "Routed providers")
        .map(|(_, title, ..)| title)
        .collect();
    assert_eq!(routed, ["openai", "anthropic"]);
    assert_eq!(
        rows(&result)
            .into_iter()
            .find(|(_, title, ..)| title == "Monthly budget"),
        Some(row(
            "Usage",
            "Monthly budget",
            "$0.006000 / $25.00",
            Some("$24.994000 remaining")
        ))
    );
}

/// Serve one canned HTTP response; the handle yields the lowercased request.
fn provider_serving(
    status_line: &'static str,
    body: String,
) -> (ClawRouterProvider, std::thread::JoinHandle<String>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind local test server");
    let address = listener.local_addr().expect("local server address");
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut request = [0_u8; 4096];
        let read = stream.read(&mut request).expect("read request");
        let request = String::from_utf8_lossy(&request[..read]).to_ascii_lowercase();
        write!(
            stream,
            "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .expect("write response");
        request
    });
    let client = Client::builder()
        .redirect(Policy::none())
        .build()
        .expect("test HTTP client");
    let provider = ClawRouterProvider::with_client(&format!("http://{address}/"), client);
    (provider, server)
}

fn keyed_context() -> FetchContext {
    FetchContext {
        api_key: Some("test-clawrouter-key".into()),
        ..FetchContext::default()
    }
}

#[tokio::test]
async fn sends_a_bearer_request_to_v1_usage() {
    let (provider, server) = provider_serving("200 OK", pack_payload("2026-10").to_string());

    let result = provider
        .fetch_usage(&keyed_context())
        .await
        .expect("usage fetch");
    let request = server.join().expect("test server thread");
    assert!(request.starts_with("get /v1/usage "), "{request}");
    assert!(request.contains("authorization: bearer test-clawrouter-key"));
    assert_eq!(result.source_label, "api");
    assert_eq!(result.usage.primary.remaining_percent().round(), 63.0);
}

#[tokio::test]
async fn errors_do_not_echo_the_response_body() {
    for status_line in ["200 OK", "401 Unauthorized", "500 Internal Server Error"] {
        let (provider, server) = provider_serving(status_line, "private-response".into());
        let error = provider
            .fetch_usage(&keyed_context())
            .await
            .expect_err(status_line);
        server.join().expect("test server thread");
        assert!(
            !error.to_string().contains("private-response"),
            "{status_line}"
        );
        let expected = if status_line.starts_with("401") {
            ProviderStateKind::NeedsAuthentication
        } else {
            ProviderStateKind::Unknown
        };
        assert_eq!(provider.error_state_kind(&error), expected, "{status_line}");
    }
}

#[tokio::test]
async fn invalid_saved_base_url_fails_before_any_request() {
    let provider = ClawRouterProvider::new();
    let ctx = FetchContext {
        gateway_url: Some("http://router.example.com".into()),
        ..keyed_context()
    };
    let error = provider.fetch_usage(&ctx).await.expect_err("http override");
    assert_eq!(
        error.to_string(),
        "ClawRouter Base URL is invalid. Use an HTTPS URL without embedded credentials."
    );
}

#[tokio::test]
async fn web_and_cli_sources_are_unsupported() {
    let provider = ClawRouterProvider::new();
    for source_mode in [SourceMode::Web, SourceMode::Cli] {
        let ctx = FetchContext {
            source_mode,
            ..keyed_context()
        };
        assert!(matches!(
            provider.fetch_usage(&ctx).await,
            Err(ProviderError::UnsupportedSource(_))
        ));
    }
}
