//! Personal Info Redaction
//!
//! Redacts email addresses and other personal information for privacy,
//! useful when streaming or sharing screen.

use regex_lite::Regex;
use std::sync::OnceLock;

/// Placeholder for emails redacted out of logs, where a marker shows that
/// something was removed.
pub const EMAIL_PLACEHOLDER: &str = "Hidden";

/// Hide Personal Info removes emails from what the user sees, leaving no
/// placeholder (Mac `PersonalInfoRedactor.emailPlaceholder = ""`).
pub const UI_EMAIL_PLACEHOLDER: &str = "";

/// Get the compiled email regex
fn email_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        // Case-insensitive email pattern
        Regex::new(r"(?i)[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}").expect("Invalid email regex")
    })
}

/// Space before punctuation, left behind when an email is removed.
fn space_before_punctuation_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"\s+([:.,;])").expect("Invalid punctuation regex"))
}

fn repeated_space_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"\s{2,}").expect("Invalid space regex"))
}

fn bearer_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| Regex::new(r"(?i)(Bearer\s+)[^\s,;]+").expect("Invalid bearer regex"))
}

fn cookie_header_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?i)((?:cookie|set-cookie)\s*:\s*)[^\r\n]+")
            .expect("Invalid cookie header regex")
    })
}

fn query_secret_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r"(?i)([?&](?:token|code|device_code|verification_code|authorization_code|client_secret|api_key|access_token|refresh_token)=)[^&#\s]+",
        )
        .expect("Invalid query secret regex")
    })
}

fn json_secret_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r#"(?i)("?(?:api_key|apiKey|token|access_token|refresh_token|id_token|oauth_token|device_code|verification_code|authorization_code|client_secret)"?\s*[:=]\s*")[^"]+""#,
        )
        .expect("Invalid JSON secret regex")
    })
}

fn secret_field_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r"(?i)(\b(?:api_key|access_token|refresh_token|id_token|oauth_token|device_code|verification_code|authorization_code|client_secret|secret|password)\b\s*[:=]\s*)[^\s,;}\]]+",
        )
        .expect("Invalid secret field regex")
    })
}

fn api_key_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r"(?i)\b(?:sk|ghp|gho|github_pat|zai|nanogpt|openrouter|fk)-[A-Za-z0-9_\-]{8,}\b",
        )
        .expect("Invalid API key regex")
    })
}

/// Redacts local secrets before they enter logs or frontend-visible errors.
pub struct SecretRedactor;

impl SecretRedactor {
    pub fn redact(input: &str) -> String {
        let redacted = bearer_regex().replace_all(input, "${1}[REDACTED]");
        let redacted = cookie_header_regex().replace_all(&redacted, "${1}[REDACTED]");
        let redacted = query_secret_regex().replace_all(&redacted, "${1}[REDACTED]");
        let redacted = json_secret_regex().replace_all(&redacted, "${1}[REDACTED]\"");
        let redacted = secret_field_regex().replace_all(&redacted, "${1}[REDACTED]");
        api_key_regex()
            .replace_all(&redacted, "[REDACTED]")
            .to_string()
    }
}

/// Personal information redactor
pub struct PersonalInfoRedactor;

impl PersonalInfoRedactor {
    /// Redact a single email address if privacy mode is enabled
    ///
    /// # Arguments
    /// * `email` - The email address to potentially redact
    /// * `is_enabled` - Whether privacy/redaction mode is enabled
    ///
    /// # Returns
    /// The original email if disabled, or an empty string if enabled
    /// (Mac `PersonalInfoRedactor.redactEmail`)
    pub fn redact_email(email: Option<&str>, is_enabled: bool) -> String {
        match email {
            Some(e) if !e.trim().is_empty() => {
                if is_enabled {
                    UI_EMAIL_PLACEHOLDER.to_string()
                } else {
                    e.to_string()
                }
            }
            _ => String::new(),
        }
    }

    /// Redact all email addresses in a text string for a log line
    ///
    /// # Arguments
    /// * `text` - The text containing potential email addresses
    /// * `is_enabled` - Whether privacy/redaction mode is enabled
    ///
    /// # Returns
    /// The text with all emails replaced with "Hidden" if enabled
    pub fn redact_emails_in_text(text: Option<&str>, is_enabled: bool) -> Option<String> {
        let text = text?;
        if !is_enabled {
            return Some(text.to_string());
        }

        let regex = email_regex();
        Some(regex.replace_all(text, EMAIL_PLACEHOLDER).to_string())
    }

    /// Remove every email from user-visible text when Hide Personal Info is
    /// on, then tidy the gap it leaves (Mac `PersonalInfoRedactor.redactEmails`,
    /// `PersonalInfoRedactor.swift:41-55`): "Signed in as a@b.com." becomes
    /// "Signed in as.".
    pub fn hide_emails_in_text(text: Option<&str>, is_enabled: bool) -> Option<String> {
        let text = text?;
        if !is_enabled || !email_regex().is_match(text) {
            return Some(text.to_string());
        }
        let removed = email_regex().replace_all(text, UI_EMAIL_PLACEHOLDER);
        let removed = space_before_punctuation_regex().replace_all(&removed, "${1}");
        let removed = repeated_space_regex().replace_all(&removed, " ");
        Some(removed.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_email_disabled() {
        let email = "test@example.com";
        assert_eq!(
            PersonalInfoRedactor::redact_email(Some(email), false),
            email
        );
    }

    #[test]
    fn test_redact_email_enabled() {
        let email = "test@example.com";
        assert_eq!(PersonalInfoRedactor::redact_email(Some(email), true), "");
    }

    #[test]
    fn test_redact_email_none() {
        assert_eq!(PersonalInfoRedactor::redact_email(None, true), "");
        assert_eq!(PersonalInfoRedactor::redact_email(Some(""), true), "");
        assert_eq!(PersonalInfoRedactor::redact_email(Some("  "), true), "");
    }

    #[test]
    fn test_redact_emails_in_text() {
        let text = "Contact me at user@example.com or admin@test.org for help";
        let result = PersonalInfoRedactor::redact_emails_in_text(Some(text), true);
        assert_eq!(
            result,
            Some("Contact me at Hidden or Hidden for help".to_string())
        );
    }

    #[test]
    fn test_redact_emails_disabled() {
        let text = "Contact me at user@example.com";
        let result = PersonalInfoRedactor::redact_emails_in_text(Some(text), false);
        assert_eq!(result, Some(text.to_string()));
    }

    #[test]
    fn hide_emails_in_text_removes_emails_without_a_placeholder() {
        for (input, expected) in [
            ("proof@example.com", ""),
            ("Signed in as proof@example.com.", "Signed in as."),
            ("proof@example.com's Organization", "'s Organization"),
            (
                "Contact me at user@example.com or admin@test.org for help",
                "Contact me at or for help",
            ),
            ("Account: proof@example.com, Pro", "Account:, Pro"),
            ("No email here", "No email here"),
        ] {
            let hidden =
                PersonalInfoRedactor::hide_emails_in_text(Some(input), true).expect("text");
            assert_eq!(hidden, expected, "{input}");
            assert!(!hidden.contains('@'), "{input}");
            assert!(!hidden.contains("example.com"), "{input}");
        }
        assert_eq!(
            PersonalInfoRedactor::hide_emails_in_text(Some("proof@example.com"), false),
            Some("proof@example.com".to_string())
        );
        assert_eq!(PersonalInfoRedactor::hide_emails_in_text(None, true), None);
    }

    #[test]
    fn redacts_cookie_header_values() {
        let input = "cookie: session=abc123; cf_clearance=secret";
        let redacted = SecretRedactor::redact(input);
        assert!(!redacted.contains("abc123"));
        assert!(!redacted.contains("secret"));
        assert_eq!(redacted, "cookie: [REDACTED]");
    }

    #[test]
    fn redacts_bearer_tokens() {
        let input = "Authorization: Bearer sk-test-secret-token";
        assert_eq!(
            SecretRedactor::redact(input),
            "Authorization: Bearer [REDACTED]"
        );
    }

    #[test]
    fn redacts_url_query_tokens() {
        let input = "https://example.com/callback?token=abc&code=def";
        let redacted = SecretRedactor::redact(input);
        assert!(!redacted.contains("abc"));
        assert!(!redacted.contains("def"));
        assert!(redacted.contains("token=[REDACTED]"));
        assert!(redacted.contains("code=[REDACTED]"));
    }

    #[test]
    fn redacts_json_secret_fields() {
        let input = r#"{"api_key":"secret-value","client_secret":"other-secret"}"#;
        let redacted = SecretRedactor::redact(input);
        assert!(!redacted.contains("secret-value"));
        assert!(!redacted.contains("other-secret"));
        assert!(redacted.contains(r#""api_key":"[REDACTED]""#));
        assert!(redacted.contains(r#""client_secret":"[REDACTED]""#));
    }

    #[test]
    fn redacts_factory_api_keys() {
        let input = "Factory key fk-test-key-abcdef";
        let redacted = SecretRedactor::redact(input);
        assert!(!redacted.contains("fk-test-key"));
        assert_eq!(redacted, "Factory key [REDACTED]");
    }

    #[test]
    fn redacts_oauth_code_and_token_fields() {
        let input = "device_code=DEV-SECRET verification_code: VERIFY-SECRET id_token=ID-SECRET";
        let redacted = SecretRedactor::redact(input);
        assert!(!redacted.contains("DEV-SECRET"));
        assert!(!redacted.contains("VERIFY-SECRET"));
        assert!(!redacted.contains("ID-SECRET"));
        assert_eq!(
            redacted,
            "device_code=[REDACTED] verification_code: [REDACTED] id_token=[REDACTED]"
        );
    }
}
