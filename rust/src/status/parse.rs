//! Pure parsers for the two status feed shapes upstream reads
//! (`parseStatuspageStatus`, `parseGoogleWorkspaceStatus`).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer};

use super::{ProviderStatus, StatusIndicator};

#[derive(Debug, thiserror::Error)]
#[error("invalid status feed: {0}")]
pub struct ParseError(#[from] serde_json::Error);

/// Upstream decodes every feed date strictly: an unparseable date fails the
/// whole feed rather than being dropped.
fn strict_date<'de, D: Deserializer<'de>>(de: D) -> Result<Option<DateTime<Utc>>, D::Error> {
    Option::<String>::deserialize(de)?
        .map(|raw| {
            DateTime::parse_from_rfc3339(&raw)
                .map(|date| date.with_timezone(&Utc))
                .map_err(|_| serde::de::Error::custom("invalid ISO8601 date"))
        })
        .transpose()
}

pub fn parse_statuspage_status(data: &[u8]) -> Result<ProviderStatus, ParseError> {
    #[derive(Deserialize)]
    struct Response {
        page: Option<Page>,
        status: Status,
    }
    #[derive(Deserialize)]
    struct Page {
        #[serde(default, deserialize_with = "strict_date")]
        updated_at: Option<DateTime<Utc>>,
    }
    #[derive(Deserialize)]
    struct Status {
        indicator: String,
        description: Option<String>,
    }

    let response: Response = serde_json::from_slice(data)?;
    Ok(ProviderStatus {
        indicator: StatusIndicator::from_raw(&response.status.indicator),
        description: response.status.description,
        updated_at: response.page.and_then(|page| page.updated_at),
    })
}

#[derive(Deserialize)]
struct WorkspaceIncident {
    #[serde(default, deserialize_with = "strict_date")]
    begin: Option<DateTime<Utc>>,
    #[serde(default, deserialize_with = "strict_date")]
    end: Option<DateTime<Utc>>,
    #[serde(default, deserialize_with = "strict_date")]
    modified: Option<DateTime<Utc>>,
    external_desc: Option<String>,
    status_impact: Option<String>,
    severity: Option<String>,
    affected_products: Option<Vec<WorkspaceProduct>>,
    currently_affected_products: Option<Vec<WorkspaceProduct>>,
    most_recent_update: Option<WorkspaceUpdate>,
    updates: Option<Vec<WorkspaceUpdate>>,
}

#[derive(Deserialize)]
struct WorkspaceProduct {
    id: String,
}

#[derive(Deserialize)]
struct WorkspaceUpdate {
    #[serde(default, deserialize_with = "strict_date")]
    when: Option<DateTime<Utc>>,
    status: Option<String>,
    text: Option<String>,
}

impl WorkspaceIncident {
    fn is_active(&self) -> bool {
        self.end.is_none()
    }

    fn is_relevant(&self, product_id: &str) -> bool {
        self.currently_affected_products
            .as_ref()
            .or(self.affected_products.as_ref())
            .is_some_and(|products| products.iter().any(|product| product.id == product_id))
    }

    fn latest_update(&self) -> Option<&WorkspaceUpdate> {
        self.most_recent_update
            .as_ref()
            .or_else(|| self.updates.as_ref().and_then(|updates| updates.last()))
    }
}

pub fn parse_google_workspace_status(
    data: &[u8],
    product_id: &str,
) -> Result<ProviderStatus, ParseError> {
    let incidents: Vec<WorkspaceIncident> = serde_json::from_slice(data)?;
    let active: Vec<&WorkspaceIncident> = incidents
        .iter()
        .filter(|incident| incident.is_relevant(product_id) && incident.is_active())
        .collect();
    let Some(first) = active.first() else {
        return Ok(ProviderStatus {
            indicator: StatusIndicator::Operational,
            description: None,
            updated_at: None,
        });
    };

    let mut best = (StatusIndicator::Operational, *first, first.latest_update());
    for incident in &active {
        let update = incident.latest_update();
        let indicator = workspace_indicator(
            update
                .and_then(|update| update.status.as_deref())
                .or(incident.status_impact.as_deref()),
            incident.severity.as_deref(),
        );
        if indicator.rank() > best.0.rank() {
            best = (indicator, *incident, update);
        }
    }

    let (indicator, incident, update) = best;
    let description = workspace_summary(
        update
            .and_then(|update| update.text.as_deref())
            .or(incident.external_desc.as_deref()),
    );
    let updated_at = update
        .and_then(|update| update.when)
        .or(incident.modified)
        .or(incident.begin);
    Ok(ProviderStatus {
        indicator,
        description,
        updated_at,
    })
}

fn workspace_indicator(status: Option<&str>, severity: Option<&str>) -> StatusIndicator {
    match status.map(str::to_uppercase).as_deref() {
        Some("AVAILABLE") => return StatusIndicator::Operational,
        Some("SERVICE_INFORMATION") => return StatusIndicator::Minor,
        Some("SERVICE_DISRUPTION") => return StatusIndicator::Major,
        Some("SERVICE_OUTAGE") => return StatusIndicator::Critical,
        Some("SERVICE_MAINTENANCE" | "SCHEDULED_MAINTENANCE") => {
            return StatusIndicator::Maintenance;
        }
        _ => {}
    }
    match severity.map(str::to_lowercase).as_deref() {
        Some("medium") => StatusIndicator::Major,
        Some("high") => StatusIndicator::Critical,
        _ => StatusIndicator::Minor,
    }
}

/// First meaningful line of a Workspace update, with Markdown bold, links
/// and list markers stripped.
fn workspace_summary(text: Option<&str>) -> Option<String> {
    static LINK: std::sync::OnceLock<regex_lite::Regex> = std::sync::OnceLock::new();
    let link = LINK.get_or_init(|| {
        regex_lite::Regex::new(r"\[([^\]]+)\]\([^)]+\)").expect("static markdown link regex")
    });
    let normalized = text?.replace("\r\n", "\n").replace('\r', "\n");
    for raw_line in normalized.split('\n') {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_lowercase();
        if lower.starts_with("**summary")
            || lower.starts_with("**description")
            || lower == "summary"
        {
            continue;
        }
        let unbolded = trimmed.replace("**", "");
        let unlinked = link.replace_all(&unbolded, "$1");
        let cleaned = unlinked.strip_prefix("- ").unwrap_or(&unlinked).trim();
        if !cleaned.is_empty() {
            return Some(cleaned.to_string());
        }
    }
    None
}
