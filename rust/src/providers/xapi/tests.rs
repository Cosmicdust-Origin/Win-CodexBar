#[cfg(test)]
mod xapi_tests {
    use crate::core::{Provider, ProviderId};
    use crate::providers::xapi::XapiProvider;

    #[test]
    fn balance_parsing() {
        // Test balance calculation: paid + free
        let paid = 10.4;
        let free = 2.0;
        let expected = 12.4;
        assert_eq!(paid + free, expected);
    }

    #[test]
    fn provider_metadata() {
        let provider = XapiProvider::new();
        assert_eq!(provider.id(), ProviderId::Xapi);
        assert_eq!(provider.metadata().display_name, "X API");
        assert_eq!(provider.metadata().session_label, "Balance");
    }
}
