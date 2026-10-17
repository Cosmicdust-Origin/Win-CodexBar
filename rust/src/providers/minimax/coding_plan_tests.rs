use super::*;

#[test]
fn oversized_remains_yields_no_reset_line_and_keeps_usage() {
    // `remains` above ~8.3e15 ms cannot be represented as a timestamp:
    // `Duration::try_seconds` must return None, so `resets_at` is None and
    // the usage percentage survives (upstream #3758/#3764).
    assert_eq!(resets_at(None, Some(i64::MAX), now()), None);
}

#[test]
fn negative_remains_yields_no_reset_line() {
    assert_eq!(resets_at(None, Some(-5), now()), None);
    assert_eq!(resets_at(None, Some(0), now()), None);
}

#[test]
fn positive_remains_keeps_the_reset_line() {
    // 90_000 > 1_000_000? No: below the heuristic, treated as seconds.
    let reset = resets_at(None, Some(60), now()).expect("small remains");
    assert_eq!(reset, now() + Duration::seconds(60));
    // Above the heuristic: treated as milliseconds.
    let reset_ms = resets_at(None, Some(2_000_000), now()).expect("ms remains");
    assert_eq!(reset_ms, now() + Duration::seconds(2_000));
}

#[test]
fn end_time_in_the_future_wins_over_remains() {
    let end = now() + Duration::seconds(120);
    assert_eq!(resets_at(Some(end), Some(60), now()), Some(end));
}

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 8, 3, 12, 0, 0).unwrap()
}

#[test]
fn parses_remains_json_snake_case() {
    let json = serde_json::json!({
        "data": {
            "current_subscribe_title": "MiniMax Pro",
            "model_remains": [{
                "model_name": "General",
                "current_interval_total_count": 100,
                "current_interval_usage_count": 73,
                "start_time": 1785792000,
                "end_time": 1785878400,
                "current_interval_status": 0,
                "current_weekly_total_count": 1000,
                "current_weekly_usage_count": 850,
                "current_weekly_remaining_percent": 85,
                "weekly_start_time": 1785792000,
                "weekly_end_time": 1786396800,
                "current_weekly_status": 0
            }]
        }
    });
    let snapshot = parse_coding_plan_value(&json, now()).unwrap();
    let MiniMaxCodingPlanSnapshot::Remains { plan_name, rows } = snapshot else {
        panic!("expected Remains");
    };
    assert_eq!(plan_name.as_deref(), Some("MiniMax Pro"));
    // remaining percent 73 → used 27
    assert!((rows[0].percent - 27.0).abs() < 0.01);
    // window_minutes = (end-start)/60 = 86400/60 = 1440
    assert_eq!(rows[0].window_minutes, Some(1440));
    assert!(rows[0].resets_at.is_some());
    // General is text-gen → weekly row generated
    assert_eq!(rows.len(), 2);
    assert!(rows[1].is_weekly);
}

#[test]
fn weekly_row_without_start_and_end_is_seven_days() {
    let json = serde_json::json!({
        "data": {
            "model_remains": [{
                "model_name": "General",
                "current_interval_total_count": 100,
                "current_interval_usage_count": 60,
                "current_interval_status": 0,
                "current_weekly_total_count": 1000,
                "current_weekly_usage_count": 900,
                "current_weekly_status": 0,
                "weekly_remains_time": 172800000
            }]
        }
    });
    let snapshot = parse_coding_plan_value(&json, now()).unwrap();
    let MiniMaxCodingPlanSnapshot::Remains { rows, .. } = snapshot else {
        panic!("expected Remains");
    };
    assert_eq!(rows.len(), 2);
    // No start/end on the interval row: no length, as before.
    assert_eq!(rows[0].window_minutes, None);
    // The weekly row has no weekly start/end but is still 7 days.
    assert!(rows[1].is_weekly);
    assert_eq!(rows[1].window_minutes, Some(10_080));
    assert!((rows[1].percent - 10.0).abs() < 0.01);
}

#[test]
fn parses_counts_only_entry() {
    let json = serde_json::json!({
        "data": {
            "model_remains": [{
                "model_name": "General",
                "current_interval_total_count": 100,
                "current_interval_usage_count": 40,
                "start_time": 1722691200,
                "end_time": 1722777600,
                "current_interval_status": 0
            }]
        }
    });
    let snapshot = parse_coding_plan_value(&json, now()).unwrap();
    let MiniMaxCodingPlanSnapshot::Remains { rows, .. } = snapshot else {
        panic!("expected Remains");
    };
    // total=100, remaining=40 → used 60/100 = 60%
    assert!((rows[0].percent - 60.0).abs() < 0.01);
}

#[test]
fn parses_remaining_percent_with_boost_field_ignored() {
    let json = serde_json::json!({
        "data": {
            "model_remains": [{
                "model_name": "General",
                "current_interval_total_count": 0,
                "current_interval_usage_count": 0,
                "current_interval_remaining_percent": 50,
                "interval_boost_permill": 1500,
                "start_time": 1722691200,
                "end_time": 1722777600,
                "current_interval_status": 0
            }]
        }
    });
    let snapshot = parse_coding_plan_value(&json, now()).unwrap();
    let MiniMaxCodingPlanSnapshot::Remains { rows, .. } = snapshot else {
        panic!("expected Remains");
    };
    // remainingPercent 50 → used 50%; boost field is ignored dead plumbing
    assert!((rows[0].percent - 50.0).abs() < 0.01);
}

#[test]
fn placeholder_lane_skipped_for_video() {
    let json = serde_json::json!({
        "data": {
            "model_remains": [{
                "model_name": "video",
                "current_interval_total_count": 0,
                "current_interval_usage_count": 0,
                "current_interval_remaining_percent": 100,
                "current_interval_status": 3
            }]
        }
    });
    let snapshot = parse_coding_plan_value(&json, now()).unwrap();
    let MiniMaxCodingPlanSnapshot::Remains { rows, .. } = snapshot else {
        panic!("expected Remains");
    };
    // video is not text-gen → no weekly; interval is placeholder → skipped
    assert!(rows.is_empty());
}

#[test]
fn placeholder_general_weekly_kept_as_unlimited() {
    let json = serde_json::json!({
        "data": {
            "model_remains": [{
                "model_name": "General",
                "current_interval_total_count": 0,
                "current_interval_usage_count": 0,
                "current_interval_remaining_percent": 100,
                "current_interval_status": 3,
                "current_weekly_total_count": 0,
                "current_weekly_usage_count": 0,
                "current_weekly_remaining_percent": 100,
                "current_weekly_status": 3
            }]
        }
    });
    let snapshot = parse_coding_plan_value(&json, now()).unwrap();
    let MiniMaxCodingPlanSnapshot::Remains { rows, .. } = snapshot else {
        panic!("expected Remains");
    };
    // interval is placeholder → skipped, but weekly is unlimited → kept
    let weekly = rows.iter().find(|r| r.is_weekly).unwrap();
    assert!(weekly.is_unlimited);
    assert_eq!(weekly.reset_description.as_deref(), Some("Unlimited"));
    assert!((weekly.percent - 0.0).abs() < 0.01);
}

#[test]
fn base_resp_1004_is_auth_required() {
    let json = serde_json::json!({
        "data": {
            "base_resp": { "status_code": 1004 },
            "model_remains": []
        }
    });
    let err = parse_coding_plan_value(&json, now()).unwrap_err();
    assert!(matches!(err, ProviderError::AuthRequired));
}

#[test]
fn base_resp_status_msg_login_is_auth_required() {
    let json = serde_json::json!({
        "data": {
            "base_resp": { "status_code": 2000, "status_msg": "please log in" },
            "model_remains": []
        }
    });
    let err = parse_coding_plan_value(&json, now()).unwrap_err();
    assert!(matches!(err, ProviderError::AuthRequired));
}

#[test]
fn base_resp_other_status_is_other_error() {
    let json = serde_json::json!({
        "data": {
            "base_resp": { "status_code": 2000, "status_msg": "quota sync failed" },
            "model_remains": []
        }
    });
    let err = parse_coding_plan_value(&json, now()).unwrap_err();
    assert!(matches!(err, ProviderError::Other(_)));
}

#[test]
fn base_resp_without_status_code_parses_fine() {
    let json = serde_json::json!({
        "data": {
            "base_resp": {},
            "model_remains": [{
                "model_name": "General",
                "current_interval_total_count": 100,
                "current_interval_usage_count": 30,
                "current_interval_remaining_percent": 70,
                "start_time": 1785792000,
                "end_time": 1785878400,
                "current_interval_status": 0
            }]
        }
    });
    let snapshot = parse_coding_plan_value(&json, now()).unwrap();
    let MiniMaxCodingPlanSnapshot::Remains { rows, .. } = snapshot else {
        panic!("expected Remains");
    };
    // remainingPercent 70 → used 30%; no error from absent status_code
    assert!((rows[0].percent - 30.0).abs() < 0.01);
}

#[test]
fn empty_model_remains_is_parse_error() {
    let json = serde_json::json!({
        "data": {
            "model_remains": []
        }
    });
    let err = parse_coding_plan_value(&json, now()).unwrap_err();
    assert!(matches!(err, ProviderError::Parse(_)));
}

#[test]
fn parses_multi_service_json() {
    let json = serde_json::json!({
        "data": {
            "services": [{
                "service_type": "Text Generation Pro",
                "window_type": "Today",
                "time_range": "2026/08/03 00:00 - 2026/08/04 00:00",
                "usage": 250,
                "limit": 1000
            }]
        }
    });
    let snapshot = parse_coding_plan_value(&json, now()).unwrap();
    match snapshot {
        MiniMaxCodingPlanSnapshot::Services(rows) => {
            assert_eq!(rows.len(), 1);
            assert!((rows[0].percent - 25.0).abs() < 0.01);
            assert!(rows[0].service_type.contains("Pro"));
        }
        _ => panic!("expected Services"),
    }
}

#[test]
fn token_plan_predicate_matches_reporter_2062_message() {
    // Live reporter evidence (issue #254): the legacy remains endpoint 200s
    // with base_resp 2062 for Token Plan accounts.
    let json = serde_json::json!({
        "base_resp": {
            "status_code": 2062,
            "status_msg": "no active token plan subscription"
        }
    });
    let err = parse_coding_plan_value(&json, now()).unwrap_err();
    assert!(matches!(err, ProviderError::Other(_)));
    assert!(is_token_plan_without_coding_plan(&err));
}

#[test]
fn token_plan_predicate_rejects_unrelated_errors() {
    assert!(!is_token_plan_without_coding_plan(
        &ProviderError::AuthRequired
    ));
    assert!(!is_token_plan_without_coding_plan(&ProviderError::Other(
        "MiniMax coding plan status 2000".to_string()
    )));
    assert!(!is_token_plan_without_coding_plan(&ProviderError::Parse(
        "Missing coding plan data.".to_string()
    )));
    // Case-insensitive match on the message itself.
    assert!(is_token_plan_without_coding_plan(&ProviderError::Other(
        "No Active Token Plan Subscription".to_string()
    )));
}
