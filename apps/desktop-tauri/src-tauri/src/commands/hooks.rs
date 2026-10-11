//! Settings > Hooks: read and write `hooks.json` and the `hooks_enabled`
//! master switch (macOS `PreferencesHooksPane`).

use std::path::{Path, PathBuf};

use codexbar::core::{HookEventType, HookRule, HooksConfig, HooksConfigError};

use super::*;

/// One rule as the Hooks pane edits it. Event names stay strings on the wire
/// so an unknown name is rejected by `save_hooks_config` with a clear error.
/// `events` and `timeoutSecs` are not edited by the pane; they are carried
/// through so a hand-edited `hooks.json` survives a save.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookRulePayload {
    pub enabled: bool,
    #[serde(default)]
    pub event: Option<String>,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub provider: Option<String>,
    /// For `quota_low`: usage fraction 0..=1 (the pane edits it as 1..=100 %).
    #[serde(default)]
    pub threshold: Option<f64>,
    pub executable: String,
    #[serde(default)]
    pub arguments: Vec<String>,
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
}

fn default_timeout_secs() -> u64 {
    10
}

impl From<&HookRule> for HookRulePayload {
    fn from(rule: &HookRule) -> Self {
        Self {
            enabled: rule.enabled,
            event: rule.event.map(|e| e.as_str().to_string()),
            events: rule.events.iter().map(|e| e.as_str().to_string()).collect(),
            provider: rule.provider.clone(),
            threshold: rule.threshold,
            executable: rule.executable.to_string_lossy().into_owned(),
            arguments: rule.arguments.clone(),
            timeout_secs: rule.timeout_secs,
        }
    }
}

fn parse_event(rule: usize, name: &str) -> Result<HookEventType, HooksConfigError> {
    HookEventType::parse(name).ok_or_else(|| HooksConfigError::UnknownEvent {
        rule,
        event: name.to_string(),
    })
}

impl HookRulePayload {
    fn into_rule(self, index: usize) -> Result<HookRule, HooksConfigError> {
        let event = self
            .event
            .as_deref()
            .map(|name| parse_event(index, name))
            .transpose()?;
        let events = self
            .events
            .iter()
            .map(|name| parse_event(index, name))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(HookRule {
            enabled: self.enabled,
            event,
            events,
            provider: self.provider.filter(|p| !p.trim().is_empty()),
            threshold: self.threshold,
            executable: PathBuf::from(self.executable.trim()),
            arguments: self.arguments,
            timeout_secs: self.timeout_secs,
        })
    }
}

/// What the Hooks pane shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HooksConfigSnapshot {
    /// On only when both `settings.hooks_enabled` and `hooks.json` `enabled` are on.
    pub enabled: bool,
    pub settings_enabled: bool,
    pub file_enabled: bool,
    pub rules: Vec<HookRulePayload>,
    /// Where the rules live (`hooks.json` next to `settings.json`).
    pub path: String,
    /// Set when `hooks.json` exists but can't be read; the pane then refuses to save.
    pub error: Option<String>,
    pub max_rules: usize,
    pub max_arguments: usize,
}

fn snapshot(hooks_path: &Path, settings_enabled: bool) -> HooksConfigSnapshot {
    let (config, error) = match HooksConfig::load_existing(hooks_path) {
        Ok(config) => (config.unwrap_or_default(), None),
        Err(err) => (HooksConfig::default(), Some(err.to_string())),
    };
    HooksConfigSnapshot {
        enabled: settings_enabled && config.enabled,
        settings_enabled,
        file_enabled: config.enabled,
        rules: config.events.iter().map(HookRulePayload::from).collect(),
        path: hooks_path.display().to_string(),
        error,
        max_rules: HooksConfig::MAX_RULES,
        max_arguments: HookRule::MAX_ARGUMENTS,
    }
}

/// A save from the Hooks pane. `enabled` is set only by the Enable switch,
/// which writes both master flags; rule edits leave both flags as they are.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HooksConfigSave {
    #[serde(default)]
    pub enabled: Option<bool>,
    pub rules: Vec<HookRulePayload>,
}

/// Validate `request` and write `hooks.json`. Returns the master switch value
/// to store in `settings.hooks_enabled`, or `None` to leave it unchanged.
/// Nothing is written when validation fails or the existing file is unreadable.
fn write_hooks_file(
    hooks_path: &Path,
    request: HooksConfigSave,
) -> Result<Option<bool>, HooksConfigError> {
    let existing = HooksConfig::load_existing(hooks_path)?;
    let file_enabled = existing.is_some_and(|config| config.enabled);
    let events = request
        .rules
        .into_iter()
        .enumerate()
        .map(|(index, rule)| rule.into_rule(index))
        .collect::<Result<Vec<_>, _>>()?;
    let config = HooksConfig {
        enabled: request.enabled.unwrap_or(file_enabled),
        events,
    };
    config.validate()?;
    config
        .save_to(hooks_path)
        .map_err(HooksConfigError::Unwritable)?;
    Ok(request.enabled)
}

fn hooks_path() -> Result<PathBuf, String> {
    HooksConfig::path().ok_or_else(|| "config directory unavailable".to_string())
}

#[tauri::command]
pub fn get_hooks_config() -> Result<HooksConfigSnapshot, String> {
    Ok(snapshot(&hooks_path()?, Settings::load().hooks_enabled))
}

#[tauri::command]
pub fn save_hooks_config(
    app: tauri::AppHandle,
    request: HooksConfigSave,
) -> Result<HooksConfigSnapshot, String> {
    let path = hooks_path()?;
    let write_guard = super::settings::UPDATE_SETTINGS_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut settings = Settings::load();
    let master = write_hooks_file(&path, request).map_err(|e| e.to_string())?;
    let settings_changed = master.is_some_and(|on| on != settings.hooks_enabled);
    if let Some(on) = master {
        settings.hooks_enabled = on;
    }
    if settings_changed {
        settings.save().map_err(|e| e.to_string())?;
    }
    drop(write_guard);
    if settings_changed {
        events::emit_settings_changed(&app);
    }
    Ok(snapshot(&path, settings.hooks_enabled))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!(
                "codexbar-hooks-{name}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn hooks(&self) -> PathBuf {
            self.0.join("hooks.json")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            // Test scratch only: a leftover temp dir is harmless.
            let _cleanup = std::fs::remove_dir_all(&self.0);
        }
    }

    fn rule(event: &str) -> HookRulePayload {
        HookRulePayload {
            enabled: true,
            event: Some(event.to_string()),
            events: Vec::new(),
            provider: None,
            threshold: None,
            executable: r"C:\Tools\my-command.exe".to_string(),
            arguments: Vec::new(),
            timeout_secs: 10,
        }
    }

    fn save(path: &Path, enabled: Option<bool>, rules: Vec<HookRulePayload>) -> Option<bool> {
        write_hooks_file(path, HooksConfigSave { enabled, rules }).unwrap()
    }

    #[test]
    fn save_and_load_round_trip_a_quota_low_rule_with_two_arguments() {
        let dir = TempDir::new("roundtrip");
        let mut low = rule("quota_low");
        low.provider = Some("codex".to_string());
        low.threshold = Some(0.85);
        low.arguments = vec!["--title".to_string(), "Codex low".to_string()];
        assert_eq!(
            save(&dir.hooks(), Some(true), vec![low.clone()]),
            Some(true)
        );

        let view = snapshot(&dir.hooks(), true);
        assert!(view.enabled);
        assert!(view.file_enabled);
        assert_eq!(view.error, None);
        assert_eq!(view.rules, vec![low]);
        assert_eq!(view.max_rules, 32);
        assert_eq!(view.max_arguments, 32);

        let raw: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.hooks()).unwrap()).unwrap();
        assert_eq!(raw["enabled"], true);
        assert_eq!(raw["events"][0]["event"], "quota_low");
        assert_eq!(raw["events"][0]["provider"], "codex");
        assert_eq!(raw["events"][0]["threshold"], 0.85);
        assert_eq!(raw["events"][0]["executable"], r"C:\Tools\my-command.exe");
        assert_eq!(raw["events"][0]["arguments"][1], "Codex low");
    }

    #[test]
    fn enable_switch_writes_both_flags_and_rule_edits_write_neither() {
        let dir = TempDir::new("enable");
        // The switch: hooks.json enabled and the settings flag to store.
        assert_eq!(save(&dir.hooks(), Some(true), vec![]), Some(true));
        assert!(HooksConfig::load_from(&dir.hooks()).enabled);

        // A rule edit keeps the file flag and asks for no settings change.
        assert_eq!(save(&dir.hooks(), None, vec![rule("quota_reset")]), None);
        assert!(HooksConfig::load_from(&dir.hooks()).enabled);

        assert_eq!(save(&dir.hooks(), Some(false), vec![]), Some(false));
        assert!(!HooksConfig::load_from(&dir.hooks()).enabled);

        // Partly on: file off, settings on, so the pane shows off.
        let view = snapshot(&dir.hooks(), true);
        assert!(!view.enabled);
        assert!(view.settings_enabled);
        assert!(!view.file_enabled);
    }

    #[test]
    fn rejects_a_33rd_rule_and_leaves_the_file_alone() {
        let dir = TempDir::new("rules");
        save(&dir.hooks(), Some(true), vec![rule("quota_low")]);
        let before = std::fs::read(dir.hooks()).unwrap();
        let rules: Vec<_> = (0..33).map(|_| rule("quota_reached")).collect();
        let err = write_hooks_file(
            &dir.hooks(),
            HooksConfigSave {
                enabled: None,
                rules,
            },
        );
        assert_eq!(
            err,
            Err(HooksConfigError::TooManyRules { count: 33, max: 32 })
        );
        assert_eq!(std::fs::read(dir.hooks()).unwrap(), before);
        let rules: Vec<_> = (0..32).map(|_| rule("quota_reached")).collect();
        assert_eq!(save(&dir.hooks(), None, rules), None);
        assert_eq!(HooksConfig::load_from(&dir.hooks()).events.len(), 32);
    }

    #[test]
    fn rejects_a_33rd_argument() {
        let dir = TempDir::new("args");
        let mut too_many = rule("usage_updated");
        too_many.arguments = (0..33).map(|i| format!("a{i}")).collect();
        let err = write_hooks_file(
            &dir.hooks(),
            HooksConfigSave {
                enabled: None,
                rules: vec![rule("quota_low"), too_many],
            },
        );
        assert_eq!(
            err,
            Err(HooksConfigError::TooManyArguments {
                rule: 1,
                count: 33,
                max: 32
            })
        );
        assert!(!dir.hooks().exists());
    }

    #[test]
    fn rejects_an_unknown_event() {
        let dir = TempDir::new("event");
        let err = write_hooks_file(
            &dir.hooks(),
            HooksConfigSave {
                enabled: Some(true),
                rules: vec![rule("quota_low"), rule("quota_exhausted")],
            },
        );
        assert_eq!(
            err,
            Err(HooksConfigError::UnknownEvent {
                rule: 1,
                event: "quota_exhausted".to_string()
            })
        );
        let mut listed = rule("quota_low");
        listed.events = vec!["nope".to_string()];
        assert!(matches!(
            write_hooks_file(
                &dir.hooks(),
                HooksConfigSave {
                    enabled: None,
                    rules: vec![listed]
                }
            ),
            Err(HooksConfigError::UnknownEvent { rule: 0, .. })
        ));
        assert!(!dir.hooks().exists());
    }

    #[test]
    fn unreadable_hooks_json_is_reported_and_never_overwritten() {
        let dir = TempDir::new("corrupt");
        std::fs::write(dir.hooks(), "{ \"enabled\": true, \"events\": [ {").unwrap();
        let view = snapshot(&dir.hooks(), true);
        assert!(view.error.is_some());
        assert!(view.rules.is_empty());
        assert!(!view.enabled);

        let err = write_hooks_file(
            &dir.hooks(),
            HooksConfigSave {
                enabled: Some(true),
                rules: vec![],
            },
        );
        assert!(matches!(err, Err(HooksConfigError::Unreadable(_))));
        assert_eq!(
            std::fs::read_to_string(dir.hooks()).unwrap(),
            "{ \"enabled\": true, \"events\": [ {"
        );
    }

    #[test]
    fn fields_the_pane_does_not_edit_survive_a_save() {
        let dir = TempDir::new("passthrough");
        std::fs::write(
            dir.hooks(),
            r#"{"enabled":true,"events":[{"events":["quota_low","refresh_failed"],"executable":"C:\\Tools\\a.exe","timeoutSecs":45}]}"#,
        )
        .unwrap();
        let view = snapshot(&dir.hooks(), false);
        assert_eq!(view.rules[0].event, None);
        assert_eq!(view.rules[0].events, vec!["quota_low", "refresh_failed"]);
        assert_eq!(view.rules[0].timeout_secs, 45);

        save(&dir.hooks(), None, view.rules.clone());
        let config = HooksConfig::load_from(&dir.hooks());
        assert!(config.enabled);
        assert_eq!(config.events[0].event, None);
        assert_eq!(
            config.events[0].events,
            vec![HookEventType::QuotaLow, HookEventType::RefreshFailed]
        );
        assert_eq!(config.events[0].timeout_secs, 45);
    }

    #[test]
    fn a_blank_provider_means_any_provider() {
        let dir = TempDir::new("provider");
        let mut any = rule("quota_reached");
        any.provider = Some(String::new());
        save(&dir.hooks(), None, vec![any]);
        assert_eq!(
            HooksConfig::load_from(&dir.hooks()).events[0].provider,
            None
        );
    }
}
