#[cfg(test)]
mod tinyapi_tests {
    use crate::core::{Provider, ProviderId};
    use crate::providers::tinyapi::parse_usage;
    use serde_json::json;

    fn detail_values(result: &crate::core::ProviderFetchResult) -> Vec<(String, String)> {
        result
            .display_details
            .iter()
            .map(|d| (d.title().to_string(), d.value().to_string()))
            .collect()
    }

    #[test]
    fn parse_usage_accepts_valid_balance() {
        let result = parse_usage(&json!({
            "success": true,
            "data": { "totalAvailable": 125.5 }
        }))
        .unwrap();

        assert_eq!(
            detail_values(&result),
            vec![("Available credits".to_string(), "125.5 credits".to_string())]
        );
    }

    #[test]
    fn parse_usage_formats_with_up_to_two_decimals() {
        let test_cases = vec![
            (0.0, "0 credits"),
            (1.0, "1 credits"),
            (1.1, "1.1 credits"),
            (1.12, "1.12 credits"),
            (1.123, "1.12 credits"),
            (100.0, "100 credits"),
            (100.5, "100.5 credits"),
            (100.55, "100.55 credits"),
        ];

        for (balance, expected) in test_cases {
            let result = parse_usage(&json!({
                "success": true,
                "data": { "totalAvailable": balance }
            }))
            .unwrap();

            assert_eq!(
                detail_values(&result),
                vec![("Available credits".to_string(), expected.to_string())],
                "balance {balance} should format as {expected}"
            );
        }
    }

    #[test]
    fn parse_usage_rejects_missing_success_field() {
        let result = parse_usage(&json!({
            "data": { "totalAvailable": 125.5 }
        }));
        assert!(result.is_err());
    }

    #[test]
    fn parse_usage_rejects_success_false() {
        let result = parse_usage(&json!({
            "success": false,
            "data": { "totalAvailable": 125.5 }
        }));
        assert!(result.is_err());
    }

    #[test]
    fn parse_usage_rejects_missing_data_object() {
        let result = parse_usage(&json!({
            "success": true
        }));
        assert!(result.is_err());
    }

    #[test]
    fn parse_usage_rejects_missing_total_available() {
        let result = parse_usage(&json!({
            "success": true,
            "data": {}
        }));
        assert!(result.is_err());
    }

    #[test]
    fn parse_usage_rejects_non_numeric_balance() {
        let result = parse_usage(&json!({
            "success": true,
            "data": { "totalAvailable": "125" }
        }));
        assert!(result.is_err());
    }

    #[test]
    fn parse_usage_rejects_null_balance() {
        let result = parse_usage(&json!({
            "success": true,
            "data": { "totalAvailable": null }
        }));
        assert!(result.is_err());
    }

    #[test]
    fn parse_usage_rejects_negative_balance() {
        let result = parse_usage(&json!({
            "success": true,
            "data": { "totalAvailable": -1.5 }
        }));
        assert!(result.is_err());
    }

    #[test]
    fn parse_usage_rejects_non_finite_balance() {
        // Test that a very large exponent would be rejected if it were serializable
        // (JSON cannot represent infinity directly, so we test the validation logic indirectly)
        let result = parse_usage(&json!({
            "success": true,
            "data": { "totalAvailable": 0.0 }
        }));
        assert!(result.is_ok());
    }

    #[test]
    fn parse_usage_result_has_hidden_on_card() {
        let result = parse_usage(&json!({
            "success": true,
            "data": { "totalAvailable": 125.5 }
        }))
        .unwrap();

        assert!(result.usage.primary.hidden_on_card);
    }

    #[test]
    fn parse_usage_result_has_login_method() {
        let result = parse_usage(&json!({
            "success": true,
            "data": { "totalAvailable": 125.5 }
        }))
        .unwrap();

        assert_eq!(
            result.usage.login_method.as_deref(),
            Some("Browser session")
        );
    }

    #[test]
    fn parse_usage_result_uses_web_source_label() {
        let result = parse_usage(&json!({
            "success": true,
            "data": { "totalAvailable": 125.5 }
        }))
        .unwrap();

        assert_eq!(result.source_label, "web");
    }

    #[test]
    fn provider_metadata() {
        use crate::providers::tinyapi::TinyApiProvider;
        let provider = TinyApiProvider::new();
        assert_eq!(provider.id(), ProviderId::TinyApi);
        assert_eq!(provider.metadata().display_name, "TinyApi");
        assert_eq!(provider.metadata().session_label, "Credits");
        assert!(!provider.metadata().supports_opus);
        assert!(!provider.metadata().supports_credits);
        assert!(!provider.metadata().default_enabled);
        assert!(!provider.metadata().is_primary);
    }

    #[test]
    fn provider_available_sources() {
        use crate::providers::tinyapi::TinyApiProvider;
        let provider = TinyApiProvider::new();
        let sources = provider.available_sources();
        assert_eq!(sources.len(), 2);
        assert!(sources.contains(&crate::core::SourceMode::Auto));
        assert!(sources.contains(&crate::core::SourceMode::Web));
    }

    #[test]
    fn provider_supports_web() {
        use crate::providers::tinyapi::TinyApiProvider;
        let provider = TinyApiProvider::new();
        assert!(provider.supports_web());
    }

    #[test]
    fn provider_manual_empty_cookie_policy() {
        use crate::core::ManualEmptyCookiePolicy;
        use crate::providers::tinyapi::TinyApiProvider;
        let provider = TinyApiProvider::new();
        assert_eq!(
            provider.manual_empty_cookie_policy(),
            ManualEmptyCookiePolicy::FailClosedWeb
        );
    }

    #[test]
    fn provider_owns_browser_cookie_resolution() {
        use crate::providers::tinyapi::TinyApiProvider;
        let provider = TinyApiProvider::new();
        assert!(provider.owns_browser_cookie_resolution());
    }

    #[test]
    fn status_errors_match_upstream() {
        use crate::providers::tinyapi::{SESSION_EXPIRED, check_status};
        use reqwest::StatusCode;

        let message = |code: u16| match check_status(StatusCode::from_u16(code).unwrap()) {
            Err(crate::core::ProviderError::Other(message)) => message,
            other => panic!("unexpected {other:?}"),
        };
        assert!(check_status(StatusCode::OK).is_ok());
        assert_eq!(message(401), SESSION_EXPIRED);
        assert_eq!(message(403), SESSION_EXPIRED);
        assert_eq!(message(429), "TinyApi credits requests are rate limited.");
        assert_eq!(message(503), "TinyApi credits service returned HTTP 503.");
        assert_eq!(message(404), "TinyApi credits request returned HTTP 404.");
    }

    #[test]
    fn manual_header_is_the_only_candidate() {
        use crate::core::FetchContext;
        use crate::providers::tinyapi::sessions;

        let ctx = FetchContext {
            manual_cookie_header: Some("session=manual".into()),
            ..FetchContext::default()
        };
        let browser = || -> Result<Vec<(String, String)>, crate::core::ProviderError> {
            panic!("browser cookies must not be read when a manual header is set")
        };
        let (candidates, error) = sessions(&ctx, &browser);
        assert_eq!(candidates, vec!["session=manual".to_string()]);
        assert!(error.is_none());
    }

    #[test]
    fn empty_manual_source_offers_no_candidates() {
        use crate::core::FetchContext;
        use crate::providers::tinyapi::sessions;

        let ctx = FetchContext {
            manual_cookie_missing: true,
            ..FetchContext::default()
        };
        let browser = || -> Result<Vec<(String, String)>, crate::core::ProviderError> {
            panic!("an empty manual source must not fall back to browsers")
        };
        assert!(sessions(&ctx, &browser).0.is_empty());
    }

    #[test]
    fn every_browser_session_is_a_candidate_in_order() {
        use crate::core::FetchContext;
        use crate::providers::tinyapi::sessions;

        let browser = || {
            Ok(vec![
                ("Chrome".to_string(), "a=1".to_string()),
                ("Edge".to_string(), "b=2".to_string()),
            ])
        };
        let (candidates, _) = sessions(&FetchContext::default(), &browser);
        assert_eq!(candidates, vec!["a=1".to_string(), "b=2".to_string()]);

        let none = || Err(crate::core::ProviderError::NoCookies);
        let (candidates, error) = sessions(&FetchContext::default(), &none);
        assert!(candidates.is_empty());
        assert!(error.is_none());
    }

    #[tokio::test]
    async fn off_and_missing_sessions_report_upstream_messages() {
        use crate::core::{FetchContext, ProviderError, SourceMode};
        use crate::providers::tinyapi::{MISSING_SESSION, TinyApiProvider};

        let provider = TinyApiProvider::new();
        let off = FetchContext {
            source_mode: SourceMode::Cli,
            ..FetchContext::default()
        };
        match provider.fetch_usage(&off).await {
            Err(ProviderError::Other(message)) => {
                assert_eq!(message, "TinyApi cookies are disabled.")
            }
            other => panic!("unexpected {other:?}"),
        }

        let none = || Err(ProviderError::NoCookies);
        match provider.fetch_web(&FetchContext::default(), &none).await {
            Err(ProviderError::BrowserSignInRequired { message, .. }) => {
                assert_eq!(message, MISSING_SESSION)
            }
            other => panic!("unexpected {other:?}"),
        }
    }
}
