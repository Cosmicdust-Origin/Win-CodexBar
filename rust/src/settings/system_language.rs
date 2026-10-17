//! "Language: System": follow the Windows display language (Mac
//! `AppLanguage.system`, `PreferencesGeneralPane.swift:5-66`).

use super::{Language, Settings};

impl Language {
    /// The supported UI language for a BCP 47 tag such as `ja-JP` or
    /// `zh-Hant-TW`; `None` when the app has no translation for it.
    pub fn from_bcp47(tag: &str) -> Option<Self> {
        let tag = tag.trim().to_ascii_lowercase().replace('_', "-");
        let mut subtags = tag.split('-');
        let primary = subtags.next().unwrap_or_default();
        Some(match primary {
            "en" => Language::English,
            "zh" => {
                if subtags.any(|subtag| matches!(subtag, "hant" | "tw" | "hk" | "mo")) {
                    Language::ChineseTraditional
                } else {
                    Language::Chinese
                }
            }
            "ja" => Language::Japanese,
            "ko" => Language::Korean,
            "es" => Language::Spanish,
            "pt" => Language::PortugueseBrazil,
            "ru" => Language::Russian,
            "tr" => Language::Turkish,
            "uk" => Language::Ukrainian,
            _ => return None,
        })
    }

    /// The first supported language in a preference-ordered list of tags;
    /// English when none is supported (the Mac falls back the same way).
    pub fn first_supported<'a>(tags: impl IntoIterator<Item = &'a str>) -> Self {
        tags.into_iter()
            .find_map(Self::from_bcp47)
            .unwrap_or_default()
    }

    /// The Windows display language, mapped to a supported UI language.
    pub fn system() -> Self {
        let tags = system_ui_language_tags();
        Self::first_supported(tags.iter().map(String::as_str))
    }
}

impl Settings {
    /// When the language follows the system, set `ui_language` to
    /// `system`. Returns true when the language changed.
    pub fn resolve_system_language_with(&mut self, system: Language) -> bool {
        if !self.ui_language_follows_system || self.ui_language == system {
            return false;
        }
        self.ui_language = system;
        true
    }

    /// [`Self::resolve_system_language_with`] the current Windows display language.
    pub fn resolve_system_language(&mut self) -> bool {
        if !self.ui_language_follows_system {
            return false;
        }
        self.resolve_system_language_with(Language::system())
    }
}

/// The user's preferred UI languages, most preferred first: the display
/// language override, then the Windows language list.
#[cfg(windows)]
fn system_ui_language_tags() -> Vec<String> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let read = |path: &str, value: &str| -> Vec<String> {
        hkcu.open_subkey(path)
            .and_then(|key| key.get_value::<Vec<String>, _>(value))
            .unwrap_or_default()
    };
    let mut tags = read(r"Control Panel\Desktop", "PreferredUILanguages");
    tags.extend(read(
        r"Control Panel\International\User Profile",
        "Languages",
    ));
    tags.extend(read(
        r"Control Panel\Desktop\MuiCached",
        "MachinePreferredUILanguages",
    ));
    tags
}

#[cfg(not(windows))]
fn system_ui_language_tags() -> Vec<String> {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .map(|value| value.split('.').next().unwrap_or_default().to_string())
        .filter(|tag| !tag.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_windows_language_tags_to_supported_languages() {
        for (tag, expected) in [
            ("en-US", Some(Language::English)),
            ("en-GB", Some(Language::English)),
            ("ja-JP", Some(Language::Japanese)),
            ("ko-KR", Some(Language::Korean)),
            ("zh-CN", Some(Language::Chinese)),
            ("zh-Hans-CN", Some(Language::Chinese)),
            ("zh-SG", Some(Language::Chinese)),
            ("zh-TW", Some(Language::ChineseTraditional)),
            ("zh-Hant-HK", Some(Language::ChineseTraditional)),
            ("zh-HK", Some(Language::ChineseTraditional)),
            ("es-ES", Some(Language::Spanish)),
            ("es-419", Some(Language::Spanish)),
            ("pt-PT", Some(Language::PortugueseBrazil)),
            ("ru-RU", Some(Language::Russian)),
            ("tr-TR", Some(Language::Turkish)),
            ("uk-UA", Some(Language::Ukrainian)),
            ("de-DE", None),
            ("", None),
        ] {
            assert_eq!(Language::from_bcp47(tag), expected, "{tag}");
        }
    }

    #[test]
    fn first_supported_skips_unsupported_tags_and_falls_back_to_english() {
        assert_eq!(
            Language::first_supported(["de-DE", "ja-JP", "en-US"]),
            Language::Japanese
        );
        assert_eq!(
            Language::first_supported(["de-DE", "fr-FR"]),
            Language::English
        );
        assert_eq!(Language::first_supported([]), Language::English);
    }

    #[test]
    fn only_a_following_language_tracks_the_system() {
        let mut following = Settings {
            ui_language_follows_system: true,
            ui_language: Language::English,
            ..Settings::default()
        };
        assert!(following.resolve_system_language_with(Language::Korean));
        assert_eq!(following.ui_language, Language::Korean);
        assert!(!following.resolve_system_language_with(Language::Korean));

        let mut chosen = Settings {
            ui_language_follows_system: false,
            ui_language: Language::Spanish,
            ..Settings::default()
        };
        assert!(!chosen.resolve_system_language_with(Language::Korean));
        assert_eq!(chosen.ui_language, Language::Spanish);
    }
}
