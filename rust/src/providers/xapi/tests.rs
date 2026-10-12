#[cfg(test)]
mod xapi_tests {
    use crate::core::{Provider, ProviderId};
    use crate::providers::xapi::{XapiProvider, parse_account_id, parse_credits};
    use serde_json::{Map, Value, json};

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().cloned().expect("JSON object")
    }

    fn detail_values(result: &crate::core::ProviderFetchResult) -> Vec<(String, String)> {
        result
            .display_details
            .iter()
            .map(|d| (d.title().to_string(), d.value().to_string()))
            .collect()
    }

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
    fn account_id_accepts_strings_and_safe_integers() {
        let id = |v: Value| parse_account_id(&object(json!({ "account": { "id": v } })));
        assert_eq!(id(json!("fixture-account")).unwrap(), "fixture-account");
        assert_eq!(id(json!(42)).unwrap(), "42");
        assert_eq!(id(json!(42.0)).unwrap(), "42");
        assert_eq!(
            id(json!(9_007_199_254_740_991_u64)).unwrap(),
            "9007199254740991"
        );
    }

    #[test]
    fn account_id_rejects_unsafe_or_malformed_values() {
        let id = |v: Value| parse_account_id(&object(json!({ "account": { "id": v } })));
        assert!(id(json!(9_007_199_254_740_992_u64)).is_err());
        assert!(id(json!(-1)).is_err());
        assert!(id(json!(1.5)).is_err());
        assert!(id(json!("")).is_err());
        assert!(id(json!("a/b")).is_err());
        assert!(id(json!("a".repeat(129))).is_err());
        assert!(parse_account_id(&object(json!({}))).is_err());
    }

    #[test]
    fn credits_sum_paid_and_free_into_a_card_hidden_balance() {
        let result = parse_credits(
            "1",
            &object(json!({ "credits": { "balance": 42.5 }, "freeCredits": { "balance": 7.25 } })),
        )
        .unwrap();

        assert_eq!(
            detail_values(&result),
            [
                ("Balance".to_string(), "$49.75".to_string()),
                ("Purchased credits".to_string(), "$42.50".to_string()),
                ("Free credits".to_string(), "$7.25".to_string()),
            ]
        );
        let primary = &result.usage.primary;
        assert!(primary.is_informational && primary.hidden_on_card);
        assert_eq!(
            primary.reset_description.as_deref(),
            Some("Balance: $49.75")
        );
        assert_eq!(result.cost.as_ref().and_then(|c| c.balance), Some(49.75));
    }

    #[test]
    fn credits_accept_integer_balances_and_missing_free_credits() {
        let result =
            parse_credits("1", &object(json!({ "credits": { "balance": 1234 } }))).unwrap();
        assert_eq!(
            detail_values(&result),
            [
                ("Balance".to_string(), "$1,234.00".to_string()),
                ("Purchased credits".to_string(), "$1,234.00".to_string()),
                ("Free credits".to_string(), "$0.00".to_string()),
            ]
        );
    }

    #[test]
    fn credits_keep_negative_balances_signed() {
        let result =
            parse_credits("1", &object(json!({ "credits": { "balance": -2.15 } }))).unwrap();
        assert_eq!(detail_values(&result)[0].1, "-$2.15");
    }

    #[test]
    fn credits_reject_missing_or_invalid_balances() {
        let parse = |v: Value| parse_credits("1", &object(v));
        assert!(parse(json!({})).is_err());
        assert!(parse(json!({ "credits": { "balance": "10" } })).is_err());
        assert!(parse(json!({ "credits": { "balance": 1 }, "freeCredits": {} })).is_err());
        assert!(parse(json!({ "credits": { "balance": 1 }, "freeCredits": null })).is_err());
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
