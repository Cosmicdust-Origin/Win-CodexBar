use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpListener;

use base64::Engine;
use chrono::{TimeZone, Utc};
use reqwest::redirect::Policy;
use serde_json::json;

use super::model::{
    TeamUsage, bobcoins, build_result, parse_profile, parse_refresh_at, parse_team_budget,
    team_usage,
};
use super::*;

/// The parity pack profile (`scenarios/providers/IBMBob/routes.json`) with
/// `refresh_at` pinned to 2026-10-26T12:00:00Z.
const PROFILE: &str = include_str!("fixtures/profile.json");

fn team(name: &str, used: f64, limit: Option<f64>) -> TeamUsage {
    TeamUsage {
        instance_name: "Parity Labs Bob".into(),
        team_name: name.into(),
        plan_name: Some("Bob Pro".into()),
        used,
        limit,
        resets_at: None,
    }
}

fn rows(result: &ProviderFetchResult) -> Vec<(String, String, String, Option<String>)> {
    result
        .display_details()
        .iter()
        .map(|row| {
            assert_eq!(row.section_title(), Some("Bobcoin usage"));
            (
                row.id().to_owned(),
                row.title().to_owned(),
                row.value().to_owned(),
                row.secondary_value().map(str::to_owned),
            )
        })
        .collect()
}

fn pack_teams() -> Vec<TeamUsage> {
    let profile = parse_profile(PROFILE.as_bytes()).expect("pack profile");
    let instance = &profile.instances[0];
    let budgets = [
        parse_team_budget(br#"{"usage": 312.5, "budget_limit": 500}"#).expect("budget"),
        parse_team_budget(br#"{"usage": 96.25, "budget_limit": 250}"#).expect("budget"),
    ];
    instance
        .teams
        .iter()
        .zip(&budgets)
        .map(|(team, budget)| team_usage(instance, team, budget))
        .collect()
}

#[test]
fn pack_payload_matches_the_mac_card() {
    let result = build_result(&pack_teams());
    let primary = &result.usage.primary;
    // 408.75 of 750 Bobcoins: 54.5% used, "45% left" on the card.
    assert!((primary.used_percent - 54.5).abs() < 1e-9);
    assert!(!primary.is_informational);
    assert_eq!(primary.window_minutes, Some(43_200));
    assert_eq!(
        primary.resets_at,
        Some(Utc.with_ymd_and_hms(2026, 10, 26, 12, 0, 0).unwrap())
    );
    assert_eq!(
        primary.reset_description.as_deref(),
        Some("408.75 / 750 Bobcoins")
    );
    assert!(result.usage.secondary.is_none());
    assert_eq!(result.usage.login_method.as_deref(), Some("API key"));
    assert_eq!(
        result.usage.account_organization.as_deref(),
        Some("Bob Pro")
    );
    assert_eq!(result.source_label, "api");
    assert!(result.cost.is_none());
    assert_eq!(
        rows(&result),
        vec![
            (
                "team-0".into(),
                "Parity Labs Bob · Platform".into(),
                "312.50 / 500 Bobcoins".into(),
                Some("Bob Pro".into())
            ),
            (
                "team-1".into(),
                "Parity Labs Bob · Docs".into(),
                "96.25 / 250 Bobcoins".into(),
                Some("Bob Pro".into())
            ),
        ]
    );
}

#[test]
fn a_team_without_a_limit_makes_the_total_usage_only() {
    let result = build_result(&[
        team("Platform", 312.5, Some(500.0)),
        team("Docs", 96.0, None),
    ]);
    let primary = &result.usage.primary;
    assert!(primary.is_informational);
    assert_eq!(primary.window_minutes, Some(43_200));
    assert_eq!(
        primary.reset_description.as_deref(),
        Some("408.50 Bobcoins used")
    );
    assert_eq!(rows(&result)[1].2, "96 Bobcoins used");
}

#[test]
fn a_zero_total_limit_is_usage_only_and_overspend_clamps() {
    let zero = build_result(&[team("Platform", 5.0, Some(0.0))]);
    assert!(zero.usage.primary.is_informational);
    assert_eq!(
        zero.usage.primary.reset_description.as_deref(),
        Some("5 / 0 Bobcoins")
    );

    let over = build_result(&[team("Platform", 900.0, Some(500.0))]);
    assert_eq!(over.usage.primary.used_percent, 100.0);
}

#[test]
fn rows_and_plans_follow_upstream_naming() {
    let mut same = team("Parity Labs Bob", 1.0, Some(2.0));
    same.plan_name = None;
    let mut other_plan = team("Docs", 1.0, Some(2.0));
    other_plan.plan_name = Some("Bob Enterprise".into());
    let result = build_result(&[same, other_plan, team("Platform", 1.0, Some(2.0))]);

    let rows = rows(&result);
    assert_eq!(rows[0].1, "Parity Labs Bob");
    assert_eq!(rows[0].3, None);
    assert_eq!(
        result.usage.account_organization.as_deref(),
        Some("Bob Enterprise, Bob Pro")
    );
    // The earliest refresh date wins.
    let early = Utc.with_ymd_and_hms(2026, 10, 20, 0, 0, 0).unwrap();
    let mut a = team("A", 1.0, Some(2.0));
    a.resets_at = Some(Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap());
    let mut b = team("B", 1.0, Some(2.0));
    b.resets_at = Some(early);
    assert_eq!(build_result(&[a, b]).usage.primary.resets_at, Some(early));
}

#[test]
fn team_usage_applies_upstream_fallbacks() {
    let profile = parse_profile(
        json!({"instances": [
            {"instance_id": "inst-1", "instance_name": "  ", "name": "Legacy",
             "user_id": "u", "plan_name": " ", "teams": [{"id": "team-1", "budget_limit": 40}]},
            {"instance_id": "inst-2", "name": "Legacy Two", "user_id": "u",
             "teams": [{"id": "team-2", "name": "Docs"}]}
        ]})
        .to_string()
        .as_bytes(),
    )
    .expect("profile");
    let first = &profile.instances[0];
    let second = &profile.instances[1];

    // An empty instance_name does not fall back to the legacy name.
    let usage = team_usage(
        first,
        &first.teams[0],
        &parse_team_budget(br#"{"usage": -3}"#).unwrap(),
    );
    assert_eq!(usage.instance_name, "inst-1");
    assert_eq!(usage.team_name, "team-1");
    assert_eq!(usage.plan_name, None);
    assert_eq!(usage.used, 0.0);
    assert_eq!(usage.limit, Some(40.0), "profile limit fills a missing one");

    // A missing instance_name uses the legacy name.
    let usage = team_usage(
        second,
        &second.teams[0],
        &parse_team_budget(br#"{"usage": 2, "budget_limit": -1}"#).unwrap(),
    );
    assert_eq!(usage.instance_name, "Legacy Two");
    assert_eq!(usage.team_name, "Docs");
    assert_eq!(usage.limit, None, "a negative limit is dropped");
}

#[test]
fn refresh_at_accepts_seconds_and_iso_strings() {
    let parse = |value: serde_json::Value| {
        let profile = parse_profile(
            json!({"instances": [{"instance_id": "i", "refresh_at": value, "teams": []}]})
                .to_string()
                .as_bytes(),
        )
        .expect("profile");
        parse_refresh_at(profile.instances[0].refresh_at.as_ref())
    };
    let expected = Utc.with_ymd_and_hms(2026, 10, 26, 12, 0, 0).unwrap();
    assert_eq!(parse(json!(1_793_016_000)), Some(expected));
    assert_eq!(
        parse(json!(1_793_016_000.5)),
        Some(expected + chrono::Duration::milliseconds(500))
    );
    assert_eq!(parse(json!("2026-10-26T12:00:00Z")), Some(expected));
    assert_eq!(
        parse(json!(" 2026-10-26T14:00:00.000+02:00 ")),
        Some(expected)
    );
    assert_eq!(parse(json!("next month")), None);
    assert_eq!(parse(json!("")), None);
    assert_eq!(parse(json!(0)), None);
    assert_eq!(parse(json!(-5)), None);
    assert_eq!(parse(json!(null)), None);
}

#[test]
fn malformed_payloads_fail_without_echoing_the_body() {
    for body in [
        "not json private-body",
        r#"{"instances": "private-body"}"#,
        r#"{"instances": [{"instance_id": 7, "teams": []}]}"#,
        r#"{"instances": [{"instance_id": "i", "refresh_at": true, "teams": []}]}"#,
        r#"{"instances": [{"instance_id": "i", "teams": [{"name": "private-body"}]}]}"#,
    ] {
        let error = parse_profile(body.as_bytes()).expect_err(body);
        assert_eq!(
            error.to_string(),
            "Parse error: Could not parse IBM Bob usage: profile response is invalid"
        );
    }
    for body in [r#"{"budget_limit": 5}"#, r#"{"usage": "private-body"}"#] {
        assert_eq!(
            parse_team_budget(body.as_bytes()).unwrap_err().to_string(),
            "Parse error: Could not parse IBM Bob usage: team budget response is invalid"
        );
    }
}

#[test]
fn bobcoins_format_like_upstream() {
    assert_eq!(bobcoins(500.0), "500");
    assert_eq!(bobcoins(312.5), "312.50");
    assert_eq!(bobcoins(96.25), "96.25");
    assert_eq!(bobcoins(0.004), "0.00");
}

fn jwt(payload: &str) -> String {
    let encode = |text: &str| base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(text);
    format!("{}.{}.sig", encode(r#"{"alg":"none"}"#), encode(payload))
}

#[test]
fn jwt_session_tokens_use_bearer_and_api_keys_use_apikey() {
    let session = jwt(r#"{"sub":"parity-user","exp":1792929600}"#);
    assert_eq!(authorization_value(&session), format!("Bearer {session}"));
    assert_eq!(
        authorization_value("sk-parity-synthetic-0001"),
        "Apikey sk-parity-synthetic-0001"
    );
    assert!(!is_jwt(&jwt("[1,2]")), "payload must be a JSON object");
    assert!(!is_jwt("a.not-base64!.c"));
    assert!(!is_jwt("only.two"));
    assert!(!is_jwt(&format!("{session}.extra")));
}

#[test]
fn api_keys_are_trimmed_and_unquoted() {
    assert_eq!(normalize_api_key("  key  ").as_deref(), Some("key"));
    assert_eq!(normalize_api_key("\" key \"").as_deref(), Some("key"));
    assert_eq!(normalize_api_key("'key'").as_deref(), Some("key"));
    assert_eq!(normalize_api_key("\"key'").as_deref(), Some("\"key'"));
    assert_eq!(normalize_api_key("\""), None);
    assert_eq!(normalize_api_key("''"), None);
    assert_eq!(normalize_api_key("   "), None);
}

#[test]
fn a_missing_key_uses_the_ibm_bob_message() {
    let missing = "Provider not installed: Missing IBM Bob API key. Add one in Settings or set BOBSHELL_API_KEY.";
    let not_found = ProviderError::NotInstalled(
        "API key not found. Set BOBSHELL_API_KEY in Preferences or environment.".into(),
    );
    assert_eq!(
        api_key_from(Err(not_found)).unwrap_err().to_string(),
        missing
    );
    assert_eq!(
        api_key_from(Ok("''".into())).unwrap_err().to_string(),
        missing
    );
    assert_eq!(api_key_from(Ok("  'k'  ".into())).unwrap(), "k");
    assert_eq!(
        api_key_from(Err(ProviderError::Other("boom".into())))
            .unwrap_err()
            .to_string(),
        "boom"
    );
}

#[test]
fn regional_hosts_must_be_https_under_bob_ibm_com() {
    let host = |domain: &str| {
        regional_base_url(Some(domain))
            .expect(domain)
            .map(|url| url.to_string())
    };
    assert_eq!(regional_base_url(None).unwrap(), None);
    assert_eq!(regional_base_url(Some("  ")).unwrap(), None);
    assert_eq!(
        host("us-east.bob.ibm.com").as_deref(),
        Some("https://api.us-east.bob.ibm.com/")
    );
    assert_eq!(
        host("API.eu-de.bob.ibm.com").as_deref(),
        Some("https://api.eu-de.bob.ibm.com/")
    );
    assert_eq!(
        host("bob.ibm.com").as_deref(),
        Some("https://api.bob.ibm.com/")
    );

    for (domain, shown) in [
        ("evil.example.com", "api.evil.example.com"),
        ("evilbob.ibm.com", "api.evilbob.ibm.com"),
        ("bob.ibm.com.evil.com", "api.bob.ibm.com.evil.com"),
        ("us-east.bob.ibm.com:8443", "api.us-east.bob.ibm.com:8443"),
        ("us-east.bob.ibm.com/admin", "api.us-east.bob.ibm.com/admin"),
        ("user@us-east.bob.ibm.com", "api.user@us-east.bob.ibm.com"),
        ("us-east.bob.ibm.com?x=1", "api.us-east.bob.ibm.com?x=1"),
        ("us-east.bob.ibm.com.", "api.us-east.bob.ibm.com."),
    ] {
        assert_eq!(
            regional_base_url(Some(domain)).unwrap_err().to_string(),
            format!("IBM Bob returned an untrusted regional API host: {shown}."),
            "{domain}"
        );
    }
}

#[test]
fn path_segments_are_percent_encoded() {
    assert_eq!(path_segment("team-parity-0001"), "team-parity-0001");
    assert_eq!(path_segment("a/b c"), "a%2Fb%20c");
}

#[test]
fn status_codes_map_to_upstream_messages() {
    for status in [StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN] {
        assert_eq!(
            check_status(status).unwrap_err().to_string(),
            "IBM Bob rejected the API key. Check that it is active and can read subscription usage."
        );
    }
    assert_eq!(
        check_status(StatusCode::TOO_MANY_REQUESTS)
            .unwrap_err()
            .to_string(),
        "IBM Bob API returned HTTP 429."
    );
    assert!(check_status(StatusCode::OK).is_ok());
}

/// Serve canned responses by request path; the handle yields every request
/// (lowercased) in arrival order.
fn provider_serving(
    routes: Vec<(&'static str, &'static str, String)>,
    expected_requests: usize,
) -> (IBMBobProvider, std::thread::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind local test server");
    let address = listener.local_addr().expect("local server address");
    let routes: HashMap<_, _> = routes
        .into_iter()
        .map(|(path, status, body)| (path, (status, body)))
        .collect();
    let server = std::thread::spawn(move || {
        let mut requests = Vec::new();
        for _ in 0..expected_requests {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = [0_u8; 8192];
            let read = stream.read(&mut request).expect("read request");
            let request = String::from_utf8_lossy(&request[..read]).to_ascii_lowercase();
            let path = request.split_whitespace().nth(1).unwrap_or_default();
            let (status, body) = routes
                .iter()
                .find(|(route, _)| route.eq_ignore_ascii_case(path))
                .map(|(_, response)| response.clone())
                .unwrap_or(("404 Not Found", String::new()));
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            )
            .expect("write response");
            requests.push(request);
        }
        requests
    });
    let client = Client::builder()
        .redirect(Policy::none())
        .build()
        .expect("test HTTP client");
    let provider = IBMBobProvider::with_client(&format!("http://{address}/"), client);
    (provider, server)
}

fn keyed_context() -> FetchContext {
    FetchContext {
        api_key: Some("sk-parity-synthetic-0001".into()),
        ..FetchContext::default()
    }
}

const TEAM_1: &str = "/admin/v1/teams/team-parity-0001/users/user-parity-0001";
const TEAM_2: &str = "/admin/v1/teams/team-parity-0002/users/user-parity-0001";

#[tokio::test]
async fn fetches_the_profile_then_each_team_budget() {
    let (provider, server) = provider_serving(
        vec![
            ("/admin/v1/profile", "200 OK", PROFILE.to_owned()),
            (
                TEAM_1,
                "200 OK",
                r#"{"usage": 312.5, "budget_limit": 500}"#.into(),
            ),
            (
                TEAM_2,
                "200 OK",
                r#"{"usage": 96.25, "budget_limit": 250}"#.into(),
            ),
        ],
        3,
    );

    let result = provider
        .fetch_usage(&keyed_context())
        .await
        .expect("usage fetch");
    let requests = server.join().expect("test server thread");

    assert!(
        requests[0].starts_with("get /admin/v1/profile "),
        "{}",
        requests[0]
    );
    assert!(!requests[0].contains("x-instance-id"));
    assert!(
        requests[1].starts_with(&format!("get {TEAM_1} ")),
        "{}",
        requests[1]
    );
    assert!(requests[1].contains("x-instance-id: inst-parity-0001"));
    assert!(requests[1].contains("x-team-id: team-parity-0001"));
    assert!(
        requests[2].starts_with(&format!("get {TEAM_2} ")),
        "{}",
        requests[2]
    );
    for request in &requests {
        assert!(request.contains("authorization: apikey sk-parity-synthetic-0001"));
        assert!(request.contains("user-agent: codexbar"));
        assert!(request.contains("accept: application/json"));
    }
    assert!((result.usage.primary.used_percent - 54.5).abs() < 1e-9);
    assert_eq!(rows(&result).len(), 2);
}

#[tokio::test]
async fn an_untrusted_region_fails_before_any_team_request() {
    let profile = PROFILE.replace("us-east.bob.ibm.com", "bob.evil.example.com");
    let (provider, server) = provider_serving(vec![("/admin/v1/profile", "200 OK", profile)], 1);

    let error = provider
        .fetch_usage(&keyed_context())
        .await
        .expect_err("untrusted region");
    assert_eq!(server.join().expect("test server thread").len(), 1);
    assert_eq!(
        error.to_string(),
        "IBM Bob returned an untrusted regional API host: api.bob.evil.example.com."
    );
}

#[tokio::test]
async fn no_usable_instance_or_team_is_a_no_subscription_error() {
    let profile = json!({"instances": [
        {"instance_id": "no-user", "teams": [{"id": "t"}]},
        {"instance_id": "no-teams", "user_id": "u", "teams": [{"id": ""}]}
    ]})
    .to_string();
    let (provider, server) = provider_serving(vec![("/admin/v1/profile", "200 OK", profile)], 1);

    let error = provider
        .fetch_usage(&keyed_context())
        .await
        .expect_err("no teams");
    server.join().expect("test server thread");
    assert_eq!(
        error.to_string(),
        "IBM Bob returned no subscription instances or teams for this API key."
    );
}

#[tokio::test]
async fn errors_do_not_echo_the_response_body() {
    for status in ["200 OK", "401 Unauthorized", "503 Service Unavailable"] {
        let (provider, server) = provider_serving(
            vec![("/admin/v1/profile", status, "private-response".into())],
            1,
        );
        let error = provider
            .fetch_usage(&keyed_context())
            .await
            .expect_err(status);
        server.join().expect("test server thread");
        assert!(!error.to_string().contains("private-response"), "{status}");
    }
}

#[tokio::test]
async fn a_quoted_blank_key_is_missing() {
    let provider = IBMBobProvider::new();
    let ctx = FetchContext {
        api_key: Some("''".into()),
        ..FetchContext::default()
    };
    assert_eq!(
        provider.fetch_usage(&ctx).await.unwrap_err().to_string(),
        "Provider not installed: Missing IBM Bob API key. Add one in Settings or set BOBSHELL_API_KEY."
    );
}

#[tokio::test]
async fn web_and_cli_sources_are_unsupported() {
    let provider = IBMBobProvider::new();
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
