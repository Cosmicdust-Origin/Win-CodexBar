//! IBM Bob profile and team-budget payloads, and the card presentation.
//!
//! Ported from upstream CodexBar v0.73.0 `IBMBobUsageFetcher.swift`
//! (`IBMBobProfileResponse`, `IBMBobTeamBudgetResponse`,
//! `IBMBobUsageSnapshot.toUsageSnapshot`).

use chrono::{DateTime, TimeZone, Utc};
use serde::Deserialize;

use crate::core::{
    MONTHLY_WINDOW_MINUTES, ProviderDisplayDetail, ProviderError, ProviderFetchResult, RateWindow,
    UsageSnapshot,
};

const DETAIL_SECTION: &str = "Bobcoin usage";

#[derive(Debug, Deserialize)]
pub(super) struct Profile {
    pub instances: Vec<Instance>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Instance {
    pub instance_id: String,
    #[serde(default)]
    instance_name: Option<String>,
    /// Older payloads name the instance `name`.
    #[serde(default, rename = "name")]
    legacy_name: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub plan_name: Option<String>,
    #[serde(default)]
    pub refresh_at: Option<RefreshAt>,
    #[serde(default)]
    pub region_domain: Option<String>,
    pub teams: Vec<Team>,
}

impl Instance {
    /// Upstream `instanceName ?? legacyName`: an empty `instance_name` does
    /// not fall back to `name`.
    pub fn name(&self) -> Option<&str> {
        self.instance_name
            .as_deref()
            .or(self.legacy_name.as_deref())
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct Team {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub budget_limit: Option<f64>,
}

/// `refresh_at` is Unix seconds or an ISO-8601 string.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(super) enum RefreshAt {
    Seconds(f64),
    Text(String),
}

#[derive(Debug, Deserialize)]
pub(super) struct TeamBudget {
    pub usage: f64,
    #[serde(default)]
    pub budget_limit: Option<f64>,
}

/// One visible team's monthly Bobcoin usage.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct TeamUsage {
    pub instance_name: String,
    pub team_name: String,
    pub plan_name: Option<String>,
    pub used: f64,
    pub limit: Option<f64>,
    pub resets_at: Option<DateTime<Utc>>,
}

fn parse_failure(detail: &str) -> ProviderError {
    ProviderError::Parse(format!("Could not parse IBM Bob usage: {detail}"))
}

/// Decode the profile without echoing any of the body in the error.
pub(super) fn parse_profile(body: &[u8]) -> Result<Profile, ProviderError> {
    serde_json::from_slice(body).map_err(|_| parse_failure("profile response is invalid"))
}

pub(super) fn parse_team_budget(body: &[u8]) -> Result<TeamBudget, ProviderError> {
    serde_json::from_slice(body).map_err(|_| parse_failure("team budget response is invalid"))
}

pub(super) fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "positive finite seconds; `as` saturates and chrono rejects out-of-range instants"
)]
pub(super) fn parse_refresh_at(value: Option<&RefreshAt>) -> Option<DateTime<Utc>> {
    match value? {
        RefreshAt::Seconds(seconds) if seconds.is_finite() && *seconds > 0.0 => {
            let whole = seconds.trunc();
            let nanos = ((seconds - whole) * 1e9).round() as u32;
            Utc.timestamp_opt(whole as i64, nanos.min(999_999_999))
                .single()
        }
        RefreshAt::Seconds(_) => None,
        RefreshAt::Text(text) => DateTime::parse_from_rfc3339(non_empty(Some(text))?)
            .ok()
            .map(|date| date.with_timezone(&Utc)),
    }
}

/// Combine one team's profile entry and budget response.
pub(super) fn team_usage(instance: &Instance, team: &Team, budget: &TeamBudget) -> TeamUsage {
    // Upstream `(budget.budgetLimit ?? team.budgetLimit)`, then drops a
    // negative limit (it does not fall back to the profile value).
    let limit = budget
        .budget_limit
        .or(team.budget_limit)
        .filter(|limit| *limit >= 0.0);
    TeamUsage {
        instance_name: non_empty(instance.name())
            .unwrap_or(&instance.instance_id)
            .to_owned(),
        team_name: non_empty(team.name.as_deref())
            .unwrap_or(&team.id)
            .to_owned(),
        plan_name: non_empty(instance.plan_name.as_deref()).map(str::to_owned),
        used: budget.usage.max(0.0),
        limit,
        resets_at: parse_refresh_at(instance.refresh_at.as_ref()),
    }
}

/// Upstream `bobcoins`: whole numbers without decimals, else two decimals.
pub(super) fn bobcoins(value: f64) -> String {
    if value.round() == value {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}

fn amount(used: f64, limit: Option<f64>) -> String {
    match limit {
        Some(limit) => format!("{} / {} Bobcoins", bobcoins(used), bobcoins(limit)),
        None => format!("{} Bobcoins used", bobcoins(used)),
    }
}

pub(super) fn build_result(teams: &[TeamUsage]) -> ProviderFetchResult {
    let used: f64 = teams.iter().map(|team| team.used).sum();
    // A total limit exists only when every team has one.
    let limits: Vec<f64> = teams.iter().filter_map(|team| team.limit).collect();
    let limit = (!limits.is_empty() && limits.len() == teams.len()).then(|| limits.iter().sum());
    let resets_at = teams.iter().filter_map(|team| team.resets_at).min();
    let summary = amount(used, limit);

    let primary = match limit {
        Some(limit) if limit > 0.0 => RateWindow::with_details(
            (used / limit * 100.0).clamp(0.0, 100.0),
            Some(MONTHLY_WINDOW_MINUTES),
            resets_at,
            Some(summary),
        ),
        // Upstream draws a 0% bar here; Windows shows the usage-only text
        // instead of an unearned "100% left".
        _ => RateWindow {
            window_minutes: Some(MONTHLY_WINDOW_MINUTES),
            resets_at,
            ..RateWindow::informational(summary)
        },
    };

    let mut plans: Vec<&str> = teams
        .iter()
        .filter_map(|team| team.plan_name.as_deref())
        .collect();
    plans.sort_unstable();
    plans.dedup();

    let mut usage = UsageSnapshot::new(primary).with_login_method("API key");
    if !plans.is_empty() {
        usage = usage.with_organization(plans.join(", "));
    }

    let mut result = ProviderFetchResult::new(usage, "api");
    for (index, team) in teams.iter().enumerate() {
        let title = if team.team_name == team.instance_name {
            team.team_name.clone()
        } else {
            format!("{} · {}", team.instance_name, team.team_name)
        };
        let row = ProviderDisplayDetail::new(
            format!("team-{index}"),
            title,
            amount(team.used, team.limit),
        )
        .and_then(|row| row.with_section_title(DETAIL_SECTION));
        let row = match &team.plan_name {
            Some(plan) => row.and_then(|row| row.with_secondary_value(plan.clone())),
            None => row,
        };
        result = result.with_display_detail(row);
    }
    result
}
