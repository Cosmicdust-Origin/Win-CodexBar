//! Tests for Aerostack provider

use super::*;
use serde_json::json;

#[test]
fn test_parse_aerostack_response_complete() {
    let response = json!({
        "usage": {
            "ai_tokens": {
                "used": 1000,
                "limit": 5000
            }
        },
        "period": "2026-03",
        "tier": "Pro"
    });

    let result = parse_aerostack_response(&response);
    assert!(result.is_ok(), "Should parse valid response");
}

#[test]
fn test_parse_aerostack_response_minimal() {
    let response = json!({
        "usage": {
            "ai_tokens": {
                "used": 1000,
                "limit": 5000
            }
        }
    });

    let result = parse_aerostack_response(&response);
    assert!(result.is_ok(), "Should parse minimal response");
}

#[test]
fn test_parse_aerostack_response_used_only() {
    let response = json!({
        "usage": {
            "ai_tokens": {
                "used": 1000
            }
        }
    });

    let result = parse_aerostack_response(&response);
    assert!(result.is_ok(), "Should accept used without limit");
}

#[test]
fn test_parse_aerostack_response_limit_only() {
    let response = json!({
        "usage": {
            "ai_tokens": {
                "limit": 5000
            }
        }
    });

    let result = parse_aerostack_response(&response);
    assert!(result.is_ok(), "Should accept limit without used");
}

#[test]
fn test_parse_aerostack_response_zero_values() {
    let response = json!({
        "usage": {
            "ai_tokens": {
                "used": 0,
                "limit": 5000
            }
        }
    });

    let result = parse_aerostack_response(&response);
    assert!(result.is_ok(), "Should parse zero values");
}

#[test]
fn test_parse_aerostack_response_above_limit() {
    let response = json!({
        "usage": {
            "ai_tokens": {
                "used": 6000,
                "limit": 5000
            }
        }
    });

    let result = parse_aerostack_response(&response);
    assert!(result.is_ok(), "Should parse usage above limit");
}

#[test]
fn test_parse_aerostack_response_missing_tokens() {
    let response = json!({
        "usage": {
            "ai_tokens": {}
        }
    });

    let result = parse_aerostack_response(&response);
    assert!(result.is_err(), "Should fail with missing counters");
}

#[test]
fn test_parse_aerostack_response_missing_usage() {
    let response = json!({});

    let result = parse_aerostack_response(&response);
    assert!(result.is_err(), "Should fail with missing usage");
}

#[test]
fn test_parse_aerostack_response_invalid_period() {
    let response = json!({
        "usage": {
            "ai_tokens": {
                "used": 1000,
                "limit": 5000
            }
        },
        "period": "2026/03"
    });

    let result = parse_aerostack_response(&response);
    assert!(result.is_ok(), "Should parse despite invalid period");
}

#[test]
fn test_parse_aerostack_response_with_invalid_tier() {
    let response = json!({
        "usage": {
            "ai_tokens": {
                "used": 1000,
                "limit": 5000
            }
        },
        "tier": "x".repeat(121)
    });

    let result = parse_aerostack_response(&response);
    assert!(result.is_ok(), "Should parse with oversized tier (ignored)");
}

#[test]
fn test_parse_aerostack_response_negative_values() {
    let response = json!({
        "usage": {
            "ai_tokens": {
                "used": -1000,
                "limit": 5000
            }
        }
    });

    let result = parse_aerostack_response(&response);
    assert!(result.is_err(), "Should reject negative values");
}

#[test]
fn test_parse_aerostack_response_float_values() {
    let response = json!({
        "usage": {
            "ai_tokens": {
                "used": 1000.5,
                "limit": 5000
            }
        }
    });

    let result = parse_aerostack_response(&response);
    assert!(result.is_err(), "Should reject non-integer floats");
}

#[test]
fn test_format_number() {
    assert_eq!(super::format_number(1234.0), "1234");
    assert_eq!(super::format_number(0.0), "0");
    assert_eq!(super::format_number(1000000.0), "1000000");
}

#[test]
fn test_is_valid_period() {
    assert!(super::is_valid_period("2026-03"));
    assert!(super::is_valid_period("2026-12"));
    assert!(super::is_valid_period("2026-01"));
    assert!(!super::is_valid_period("2026-13"));
    assert!(!super::is_valid_period("2026-00"));
    assert!(!super::is_valid_period("26-03"));
    assert!(!super::is_valid_period("2026"));
    assert!(!super::is_valid_period("2026/03"));
    assert!(!super::is_valid_period("2026-3"));
}
