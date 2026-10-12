use super::*;
use std::io::{Read, Write};
use std::net::TcpListener;

/// The parity pack payload (`scenarios/providers/Synthetic/routes.json`) with
/// its `{{epoch:+N}}` templates resolved against `now`.
fn pack_payload(now: i64) -> Value {
    serde_json::json!({
        "plan": "Synthetic Standard",
        "rollingFiveHourLimit": {
            "limit": 600,
            "used": 214,
            "remaining": 386,
            "windowMinutes": 300,
            "nextTickAt": now + 2700,
            "tickPercent": 0.12
        },
        "weeklyTokenLimit": {
            "percentUsed": 0.58,
            "windowDays": 7,
            "resetAt": now + 388_800
        },
        "search": {
            "hourly": {
                "limit": 250,
                "used": 61,
                "remaining": 189,
                "windowMinutes": 60,
                "resetAt": now + 1500
            }
        }
    })
}

const NOW: i64 = 1_790_000_000;

fn at(seconds: i64) -> Option<DateTime<Utc>> {
    Utc.timestamp_opt(seconds, 0).single()
}

#[test]
fn parses_the_pack_payload_into_three_lanes_and_a_plan() {
    let parsed = parse_quotas(&pack_payload(NOW)).expect("pack payload parses");

    let five_hour = parsed.primary.as_ref().expect("five-hour lane");
    assert!((five_hour.used_percent - 214.0 / 600.0 * 100.0).abs() < 1e-9);
    assert_eq!(five_hour.window_minutes, Some(300));
    assert_eq!(five_hour.resets_at, at(NOW + 2700));
    assert_eq!(five_hour.next_regen_percent, Some(12.0));
    assert_eq!(five_hour.window_description, None);

    let weekly = parsed.secondary.as_ref().expect("weekly lane");
    assert!((weekly.used_percent - 58.0).abs() < 1e-9);
    assert_eq!(weekly.window_minutes, Some(10_080));
    assert_eq!(weekly.resets_at, at(NOW + 388_800));

    let search = parsed.tertiary.as_ref().expect("search lane");
    assert!((search.used_percent - 24.4).abs() < 1e-9);
    assert_eq!(search.window_minutes, Some(60));
    assert_eq!(search.resets_at, at(NOW + 1500));

    assert_eq!(parsed.plan.as_deref(), Some("Synthetic Standard"));
    assert_eq!(parsed.cost, None);
}

#[test]
fn pack_result_matches_the_mac_card() {
    let result = parse_quotas(&pack_payload(NOW))
        .expect("pack payload parses")
        .into_result();
    let usage = &result.usage;

    // "Five-hour quota 64% left", "Regenerates in 45m",
    // "76% after next regen · Full in ~3 regens".
    assert_eq!(usage.primary.remaining_percent().round(), 64.0);
    assert_eq!(usage.primary.resets_at, at(NOW + 2700));
    assert_eq!(
        usage.primary.reset_description.as_deref(),
        Some("76% after next regen · Full in ~3 regens")
    );
    assert!(usage.primary.description_is_detail);
    // The card says "Regenerates in 45m", not "Resets in 45m".
    assert!(usage.primary.reset_regenerates);
    // The regen lane keeps its five-hour length (upstream synthetic.js); the
    // card hides its pace line because the reset regenerates.
    assert_eq!(usage.primary.window_minutes, Some(300));

    // "Weekly tokens 42% left", "Resets in 4d 12h", pace from the 7-day window.
    let weekly = usage.secondary.as_ref().expect("weekly lane");
    assert!((weekly.remaining_percent() - 42.0).abs() < 1e-9);
    assert_eq!(weekly.window_minutes, Some(10_080));
    assert_eq!(weekly.resets_at, at(NOW + 388_800));
    assert!(!weekly.description_is_detail);
    assert!(!weekly.reset_regenerates);

    // "Search hourly 76% left", "Resets in 25m".
    assert_eq!(usage.extra_rate_windows.len(), 1);
    let search = &usage.extra_rate_windows[0];
    assert_eq!(search.id, "synthetic-search-hourly");
    assert_eq!(search.title, "Search hourly");
    assert_eq!(search.window.remaining_percent().round(), 76.0);
    assert_eq!(search.window.resets_at, at(NOW + 1500));
    assert!(!search.window.reset_regenerates);
    assert!(usage.tertiary.is_none());

    assert_eq!(usage.login_method.as_deref(), Some("Synthetic Standard"));
    assert!(result.cost.is_none());
}

#[test]
fn accepts_epoch_seconds_millis_numeric_strings_and_iso_dates() {
    for reset in [
        serde_json::json!(NOW + 60),
        serde_json::json!((NOW + 60) * 1000),
        serde_json::json!(format!("{}", NOW + 60)),
        serde_json::json!(
            at(NOW + 60)
                .expect("valid date")
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        ),
    ] {
        let payload = serde_json::json!({ "used": 1, "limit": 4, "resetAt": reset });
        let quota = parse_quota(&payload).expect("quota parses");
        assert_eq!(quota.resets_at, at(NOW + 60), "{payload}");
    }
    // Small numbers are not epochs, and unparseable strings are skipped.
    let payload = serde_json::json!({ "used": 1, "limit": 4, "resetAt": 12, "endAt": "soon" });
    assert_eq!(parse_quota(&payload).expect("quota parses").resets_at, None);
}

#[test]
fn derives_used_percent_like_upstream() {
    let cases = [
        (serde_json::json!({ "usedPercent": 0.25 }), 25.0),
        (serde_json::json!({ "percent": 40 }), 40.0),
        // Exactly 1 is a fraction upstream, so it means 100%.
        (serde_json::json!({ "percent_used": 1 }), 100.0),
        (serde_json::json!({ "percentRemaining": 0.3 }), 70.0),
        (serde_json::json!({ "remaining_percent": "80" }), 20.0),
        (serde_json::json!({ "used": 30, "remaining": 70 }), 30.0),
        (serde_json::json!({ "quota": 50, "left": 10 }), 80.0),
        (serde_json::json!({ "limit": 10, "used": 15 }), 100.0),
        (serde_json::json!({ "usedPercent": 150 }), 100.0),
    ];
    for (payload, expected) in cases {
        let quota = parse_quota(&payload).expect("quota parses");
        assert!(
            (quota.used_percent - expected).abs() < 1e-9,
            "{payload}: {}",
            quota.used_percent
        );
    }
    // A limit of zero gives no percentage, so the object is not a lane.
    assert_eq!(
        parse_quota(&serde_json::json!({ "limit": 0, "used": 0 })),
        None
    );
}

#[test]
fn reads_window_length_from_numbers_and_text() {
    let cases = [
        (serde_json::json!({ "windowHours": 5 }), 300),
        (serde_json::json!({ "period_days": 1 }), 1440),
        (serde_json::json!({ "windowSeconds": 3600 }), 60),
        (serde_json::json!({ "window": "5h" }), 300),
        (serde_json::json!({ "period": "7 Days" }), 10_080),
        (serde_json::json!({ "windowLabel": "1.5 hrs" }), 90),
        (serde_json::json!({ "period_label": ".5d" }), 720),
        (serde_json::json!({ "window": "30mins" }), 30),
    ];
    for (payload, expected) in cases {
        assert_eq!(window_minutes(&payload), Some(expected), "{payload}");
    }
    for text in ["weekly", "5", "h", "5w", "1.h", "5.5.5h"] {
        assert_eq!(window_text_minutes(text), None, "{text}");
    }
}

#[test]
fn describes_windows_without_a_reset_time() {
    assert_eq!(window_description(300).as_deref(), Some("5 hours window"));
    assert_eq!(window_description(60).as_deref(), Some("1 hour window"));
    assert_eq!(window_description(1440).as_deref(), Some("1 day window"));
    assert_eq!(window_description(10_080).as_deref(), Some("7 days window"));
    assert_eq!(window_description(45).as_deref(), Some("45 minutes window"));
    assert_eq!(window_description(1).as_deref(), Some("1 minute window"));
    assert_eq!(window_description(0), None);

    let quota = parse_quota(&serde_json::json!({ "used": 1, "limit": 2, "window": "5h" }))
        .expect("quota parses");
    assert_eq!(quota.window_description.as_deref(), Some("5 hours window"));
    assert_eq!(
        quota.rate_window().reset_description.as_deref(),
        Some("5 hours window")
    );
    assert!(!quota.rate_window().description_is_detail);
}

#[test]
fn regen_detail_text_matches_upstream_thresholds() {
    assert_eq!(
        regen_detail(214.0 / 6.0, 12.0, 214.0 / 6.0 / 12.0),
        "76% after next regen · Full in ~3 regens"
    );
    assert_eq!(
        regen_detail(1.0, 12.0, 1.0 / 12.0),
        "100% after next regen · Near full"
    );
    assert_eq!(
        regen_detail(14.0, 12.0, 14.0 / 12.0),
        "98% after next regen · Full in ~1 regen"
    );
    assert_eq!(
        regen_detail(18.0, 12.0, 1.5),
        "94% after next regen · Full in ~2 regens"
    );
}

#[test]
fn weekly_cost_with_next_regen_amount_drives_the_weekly_regen_line() {
    let payload = serde_json::json!({
        "data": {
            "weeklyTokenLimit": {
                "percentUsed": 40,
                "resetAt": NOW + 86_400,
                "windowDays": 7,
                "maxCredits": "$1,000.00",
                "remainingCredits": "600",
                "nextRegenCredits": 50
            }
        },
        "planName": "Pro"
    });
    let parsed = parse_quotas(&payload).expect("payload parses");
    assert!(parsed.primary.is_none());
    let cost = parsed.cost.clone().expect("cost from maxCredits");
    assert_eq!(cost.limit, 1000.0);
    assert_eq!(cost.used, 400.0);
    assert_eq!(cost.next_regen_amount, Some(50.0));

    let result = parsed.into_result();
    assert!(result.usage.primary.is_informational);
    let weekly = result.usage.secondary.as_ref().expect("weekly lane");
    // 60% left + 5% per regen; 400 / 50 = 8 regens to full.
    assert_eq!(
        weekly.reset_description.as_deref(),
        Some("65% after next regen · Full in ~8 regens")
    );
    assert!(weekly.description_is_detail);
    assert!(weekly.reset_regenerates);
    // windowDays 7 survives the regen line, as upstream keeps it.
    assert_eq!(weekly.window_minutes, Some(10_080));
    assert_eq!(result.usage.login_method.as_deref(), Some("Pro"));
    // The Mac card hides Synthetic's cost block, so the credits never reach
    // the result.
    assert!(result.cost.is_none());
}

/// Upstream `ProviderPluginParityTests` "Synthetic fixture matches the
/// cut-over golden" payload.
const CUT_OVER_GOLDEN: &str = r#"{
  "plan": "Starter",
  "weeklyTokenLimit": {
    "nextRegenAt": "2026-04-17T05:19:30.000Z",
    "percentRemaining": 98.05884722222223,
    "maxCredits": "$36.00",
    "remainingCredits": "$35.30",
    "nextRegenCredits": "$0.72"
  },
  "rollingFiveHourLimit": {
    "nextTickAt": "2026-04-17T03:44:11.000Z",
    "tickPercent": 0.05,
    "remaining": 600,
    "max": 750,
    "limited": false
  },
  "search": {
    "hourly": {
      "limit": 250,
      "requests": 2,
      "renewsAt": "2026-04-17T04:30:01.494Z"
    }
  }
}"#;

fn at_millis(millis: i64) -> Option<DateTime<Utc>> {
    Utc.timestamp_millis_opt(millis).single()
}

#[test]
fn cut_over_golden_parses_like_upstream() {
    let payload: Value = serde_json::from_str(CUT_OVER_GOLDEN).expect("golden is JSON");
    let parsed = parse_quotas(&payload).expect("golden parses");

    let five_hour = parsed.primary.as_ref().expect("five-hour lane");
    assert_eq!(five_hour.used_percent, 20.0);
    assert_eq!(five_hour.window_minutes, None);
    assert_eq!(five_hour.resets_at, at(1_776_397_451));
    assert_eq!(five_hour.next_regen_percent, Some(5.0));

    let weekly = parsed.secondary.as_ref().expect("weekly lane");
    // Upstream asserts 1.9411527777777735 (100 - 98.05884722222223). serde_json
    // without `float_roundtrip` reads 98.05884722222223 one ULP high, so the
    // difference here is 1.4e-14 and never visible on the card.
    assert_eq!(weekly.used_percent, 1.9411527777777593);
    assert_eq!(weekly.window_minutes, None);
    assert_eq!(weekly.resets_at, at(1_776_403_170));

    let search = parsed.tertiary.as_ref().expect("search lane");
    assert_eq!(search.used_percent, 0.8);
    assert_eq!(search.window_minutes, None);
    assert_eq!(search.resets_at, at_millis(1_776_400_201_494));

    assert_eq!(parsed.plan.as_deref(), Some("Starter"));
    // Upstream's providerCost numbers; parsed, but kept off the result.
    assert_eq!(
        parsed.cost,
        Some(QuotaCost {
            used: 0.7000000000000028,
            limit: 36.0,
            next_regen_amount: Some(0.72),
        })
    );
}

#[test]
fn cut_over_golden_result_matches_the_mac_card() {
    let payload: Value = serde_json::from_str(CUT_OVER_GOLDEN).expect("golden is JSON");
    let result = parse_quotas(&payload).expect("golden parses").into_result();
    let usage = &result.usage;

    // 80% left + 5% per tick; 20 / 5 = 4 ticks to full.
    assert_eq!(usage.primary.used_percent, 20.0);
    assert_eq!(usage.primary.window_minutes, None);
    assert_eq!(usage.primary.resets_at, at(1_776_397_451));
    assert_eq!(
        usage.primary.reset_description.as_deref(),
        Some("85% after next regen · Full in ~4 regens")
    );
    assert!(usage.primary.description_is_detail);

    // $0.72 / $36 = 2% per regen; $0.70 / $0.72 is under 1.5 ticks.
    let weekly = usage.secondary.as_ref().expect("weekly lane");
    assert_eq!(weekly.used_percent, 1.9411527777777593);
    assert_eq!(weekly.window_minutes, None);
    assert_eq!(weekly.resets_at, at(1_776_403_170));
    assert_eq!(
        weekly.reset_description.as_deref(),
        Some("100% after next regen · Full in ~1 regen")
    );
    assert!(weekly.description_is_detail);

    assert!(usage.tertiary.is_none());
    assert_eq!(usage.extra_rate_windows.len(), 1);
    let search = &usage.extra_rate_windows[0];
    assert_eq!(search.id, "synthetic-search-hourly");
    assert_eq!(search.title, "Search hourly");
    assert_eq!(search.window.used_percent, 0.8);
    assert_eq!(search.window.resets_at, at_millis(1_776_400_201_494));
    assert!(!search.window.description_is_detail);

    assert_eq!(usage.login_method.as_deref(), Some("Starter"));
    assert!(result.cost.is_none());
}

/// Upstream `SyntheticPluginGoldenTests` "missing rolling lane keeps weekly
/// and search slots".
#[test]
fn missing_rolling_lane_keeps_weekly_and_search_slots() {
    let payload = serde_json::json!({
        "weeklyTokenLimit": {
            "nextRegenAt": "2026-04-17T05:19:30.000Z",
            "percentRemaining": 98.0,
            "maxCredits": "$36.00",
            "remainingCredits": "$35.30",
            "nextRegenCredits": "$0.72"
        },
        "search": {
            "hourly": {
                "limit": 250,
                "requests": 2,
                "renewsAt": "2026-04-17T04:30:01.494Z"
            }
        }
    });
    let parsed = parse_quotas(&payload).expect("payload parses");
    assert!(parsed.primary.is_none());
    assert_eq!(
        parsed.cost.as_ref().map(|cost| (cost.used, cost.limit)),
        Some((0.7000000000000028, 36.0))
    );

    let result = parsed.into_result();
    let usage = &result.usage;
    assert!(usage.primary.is_informational);
    let weekly = usage.secondary.as_ref().expect("weekly lane");
    assert_eq!(weekly.used_percent, 2.0);
    assert_eq!(
        weekly.reset_description.as_deref(),
        Some("100% after next regen · Full in ~1 regen")
    );
    assert_eq!(usage.extra_rate_windows.len(), 1);
    assert_eq!(usage.extra_rate_windows[0].window.used_percent, 0.8);
    assert!(result.cost.is_none());
}

#[test]
fn cost_used_falls_back_to_explicit_credits_then_percent() {
    let explicit = parse_quota(&serde_json::json!({
        "percentUsed": 10, "maxCredits": 20, "usedCredits": "$3"
    }))
    .expect("quota parses");
    assert_eq!(explicit.cost.expect("cost").used, 3.0);
    let from_percent = parse_quota(&serde_json::json!({ "percentUsed": 25, "maxCredits": 20 }))
        .expect("quota parses");
    assert_eq!(from_percent.cost.expect("cost").used, 5.0);
}

#[test]
fn fallback_collections_collect_quota_objects_in_sorted_key_order() {
    let payload = serde_json::json!({
        "tier": "Starter",
        "limits": {
            "zeta": { "used": 9, "limit": 10, "window": "1h" },
            "alpha": { "used": 1, "limit": 10, "window": "5h" },
            "mid": [{ "percentRemaining": 50, "window": "7d" }]
        }
    });
    let parsed = parse_quotas(&payload).expect("payload parses");
    assert_eq!(parsed.primary.as_ref().map(|q| q.used_percent), Some(10.0));
    assert_eq!(
        parsed.secondary.as_ref().map(|q| q.used_percent),
        Some(50.0)
    );
    assert_eq!(parsed.tertiary.as_ref().map(|q| q.used_percent), Some(90.0));
    assert_eq!(parsed.plan.as_deref(), Some("Starter"));

    let array_root = serde_json::json!([{ "used": 3, "limit": 4 }]);
    let parsed = parse_quotas(&array_root).expect("array root parses");
    assert_eq!(parsed.primary.as_ref().map(|q| q.used_percent), Some(75.0));
    assert!(parsed.secondary.is_none());
}

#[test]
fn reports_missing_quota_data_and_non_object_roots() {
    for payload in [
        serde_json::json!({ "plan": "Free" }),
        serde_json::json!({ "quotas": [] }),
        serde_json::json!({ "rollingFiveHourLimit": { "name": "five" } }),
    ] {
        let error = parse_quotas(&payload).expect_err("no quota data");
        assert_eq!(
            error.to_string(),
            "Parse error: Failed to parse Synthetic response: Missing quota data."
        );
    }
    for payload in [serde_json::json!("text"), serde_json::json!(4), Value::Null] {
        assert!(matches!(
            parse_quotas(&payload),
            Err(ProviderError::Parse(_))
        ));
    }
}

#[test]
fn maps_statuses_to_upstream_messages() {
    for status in [StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN] {
        assert_eq!(
            status_error(status).to_string(),
            "Invalid Synthetic API credentials."
        );
    }
    assert_eq!(
        status_error(StatusCode::TOO_MANY_REQUESTS).to_string(),
        "Synthetic API error: HTTP 429."
    );
    assert_eq!(
        status_error(StatusCode::BAD_GATEWAY).to_string(),
        "Synthetic API error: HTTP 502."
    );
}

/// Serve one canned HTTP response; the handle yields the lowercased request.
fn provider_serving(
    status_line: &'static str,
    body: String,
) -> (SyntheticProvider, std::thread::JoinHandle<String>) {
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
    let provider = SyntheticProvider::with_client(format!("http://{address}/v2/quotas"), client);
    (provider, server)
}

fn keyed_context() -> FetchContext {
    FetchContext {
        api_key: Some("test-synthetic-key".into()),
        ..FetchContext::default()
    }
}

#[tokio::test]
async fn sends_a_bearer_request_and_parses_the_pack_payload() {
    let (provider, server) = provider_serving("200 OK", pack_payload(NOW).to_string());

    let result = provider
        .fetch_usage(&keyed_context())
        .await
        .expect("quota fetch");
    let request = server.join().expect("test server thread");
    assert!(request.starts_with("get /v2/quotas "));
    assert!(request.contains("authorization: bearer test-synthetic-key"));
    assert_eq!(result.source_label, "api");
    assert_eq!(result.usage.primary.remaining_percent().round(), 64.0);
    assert_eq!(
        result.usage.login_method.as_deref(),
        Some("Synthetic Standard")
    );
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
    }
}

#[tokio::test]
async fn web_and_cli_sources_are_unsupported() {
    let provider = SyntheticProvider::new();
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
