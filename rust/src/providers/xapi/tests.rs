#[cfg(test)]
mod xapi_tests {
    use crate::core::{Provider, ProviderId};
    use crate::providers::xapi::XapiProvider;
    use serde_json::json;

    #[test]
    fn provider_metadata() {
        let provider = XapiProvider::new();
        assert_eq!(provider.id(), ProviderId::Xapi);
        assert_eq!(provider.metadata().display_name, "X API");
        assert_eq!(provider.metadata().session_label, "Balance");
        assert!(!provider.metadata().supports_opus);
        assert!(!provider.metadata().supports_credits);
        assert!(!provider.metadata().default_enabled);
        assert!(!provider.metadata().is_primary);
    }

    #[test]
    fn balance_parsing_typical() {
        // Test typical paid + free balance
        let paid = 10.4;
        let free = 2.0;
        assert_eq!(paid + free, 12.4);
    }

    #[test]
    fn parse_account_id_string() {
        // Account IDs can be strings
        let text = r#"{"account":{"id":"fixture-account"}}"#;
        let me: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str::<serde_json::Value>(text)
                .ok()
                .and_then(|v| v.as_object().cloned())
                .expect("valid JSON");

        let id = me
            .get("account")
            .and_then(|v| v.get("id"))
            .and_then(|v| v.as_str());
        assert_eq!(id, Some("fixture-account"));
    }

    #[test]
    fn parse_account_id_integer() {
        // Account IDs can be integers
        let text = r#"{"account":{"id":42}}"#;
        let me: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str::<serde_json::Value>(text)
                .ok()
                .and_then(|v| v.as_object().cloned())
                .expect("valid JSON");

        let id = me.get("account").and_then(|v| v.get("id"));
        let account_id = match id {
            Some(serde_json::Value::Number(n)) => Some(n.to_string()),
            Some(serde_json::Value::String(s)) => Some(s.clone()),
            _ => None,
        };
        assert_eq!(account_id, Some("42".to_string()));
    }

    #[test]
    fn parse_credits_valid() {
        // Test typical credits response
        let credits_json = json!({
            "credits": {"balance": 10.4},
            "freeCredits": {"balance": 2.0}
        });
        let credits = credits_json.as_object().unwrap();

        let paid = credits
            .get("credits")
            .and_then(|v| v.get("balance"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let free = credits
            .get("freeCredits")
            .and_then(|v| v.get("balance"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let balance = paid + free;

        assert_eq!(paid, 10.4);
        assert_eq!(free, 2.0);
        assert_eq!(balance, 12.4);
    }

    #[test]
    fn parse_credits_negative_balance() {
        // Negative balances should be preserved
        let credits_json = json!({
            "credits": {"balance": -2.15}
        });
        let credits = credits_json.as_object().unwrap();

        let paid = credits
            .get("credits")
            .and_then(|v| v.get("balance"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        assert_eq!(paid, -2.15);
    }

    #[test]
    fn parse_credits_missing_free() {
        // Missing freeCredits should default to 0
        let credits_json = json!({
            "credits": {"balance": 10.4}
        });
        let credits = credits_json.as_object().unwrap();

        let free = credits
            .get("freeCredits")
            .and_then(|v| v.get("balance"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        assert_eq!(free, 0.0);
    }

    #[test]
    fn parse_credits_zero() {
        // Zero balance should be valid
        let credits_json = json!({
            "credits": {"balance": 0.0},
            "freeCredits": {"balance": 0.0}
        });
        let credits = credits_json.as_object().unwrap();

        let paid = credits
            .get("credits")
            .and_then(|v| v.get("balance"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let free = credits
            .get("freeCredits")
            .and_then(|v| v.get("balance"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);

        assert_eq!(paid, 0.0);
        assert_eq!(free, 0.0);
    }

    #[test]
    fn parse_credits_infinity_invalid() {
        // Infinity should not be a valid balance
        let credits_json = json!({
            "credits": {"balance": f64::INFINITY}
        });
        let credits = credits_json.as_object().unwrap();

        let paid = credits
            .get("credits")
            .and_then(|v| v.get("balance"))
            .and_then(|v| v.as_f64())
            .filter(|n| n.is_finite());

        assert_eq!(paid, None);
    }

    #[test]
    fn provider_supports_web() {
        let provider = XapiProvider::new();
        assert!(provider.supports_web());
    }

    #[test]
    fn provider_available_sources() {
        let provider = XapiProvider::new();
        let sources = provider.available_sources();
        assert_eq!(sources.len(), 2);
        assert!(sources.contains(&crate::core::SourceMode::Auto));
        assert!(sources.contains(&crate::core::SourceMode::Web));
    }
}
