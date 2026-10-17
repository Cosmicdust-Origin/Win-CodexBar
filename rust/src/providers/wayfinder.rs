//! Wayfinder gateway provider.
//!
//! Wayfinder is a local-first gateway, so it has no account, quota, or
//! credential data. The provider exposes only gateway health, configured
//! models, and savings telemetry.

use async_trait::async_trait;
use reqwest::Url;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::net::IpAddr;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot, WayfinderRouteSummary, WayfinderUsageSnapshot,
    credentialed_http_client_builder,
};

pub const DEFAULT_GATEWAY_URL: &str = "http://127.0.0.1:8088";

const HEALTH_PATH: &str = "healthz";
const MODELS_PATH: &str = "router/models";
const SAVINGS_PATH: &str = "v1/savings?period=30d";
const METRICS_PATH: &str = "metrics";
const DECISION_LATENCY_METRIC: &str = "wayfinder_router_decision_latency_seconds";

#[derive(Debug, Clone, Deserialize)]
pub struct HealthResponse {
    pub status: String,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub offline: bool,
    #[serde(default)]
    pub missing_keys: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelsResponse {
    #[serde(default)]
    pub models: Vec<ModelResponse>,
    #[serde(default)]
    pub dry_run: bool,
}

impl ModelsResponse {
    pub fn model_count(&self) -> usize {
        self.models.len()
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelResponse {
    pub name: String,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub key_ok: bool,
}

/// `/v1/savings`. Upstream `WayfinderSavingsResponse` requires only the
/// totals and `by_route`; `period_days` and `unit` are optional extras here, so
/// a gateway that omits them still yields a card.
#[derive(Debug, Clone, Deserialize)]
pub struct SavingsResponse {
    #[serde(default = "default_period_days")]
    pub period_days: u32,
    #[serde(default = "default_unit")]
    pub unit: String,
    pub priced: bool,
    pub requests: u64,
    #[serde(default)]
    pub estimated_requests: u64,
    pub tokens: u64,
    pub realized: f64,
    pub baseline: f64,
    pub saved: f64,
    pub saved_pct: f64,
    #[serde(default)]
    pub by_route: BTreeMap<String, RouteResponse>,
}

/// Upstream `RouteBucket` requires only `requests`, `saved` and `tokens`.
#[derive(Debug, Clone, Deserialize)]
pub struct RouteResponse {
    pub requests: u64,
    #[serde(default)]
    pub realized: f64,
    #[serde(default)]
    pub baseline: f64,
    pub saved: f64,
    pub tokens: u64,
}

/// The savings query asks for `period=30d`.
fn default_period_days() -> u32 {
    30
}

fn default_unit() -> String {
    "usd".to_string()
}

/// Upstream `averageDecisionMilliseconds(fromPrometheusText:)`: the last
/// `<metric>_sum` and `<metric>_count` samples, as sum / count * 1000. A sample
/// line starts with the series name followed by a space or `{`, and its value
/// is the last space-separated token.
pub fn average_decision_ms(metrics_text: &str) -> Option<f64> {
    let sum_name = format!("{DECISION_LATENCY_METRIC}_sum");
    let count_name = format!("{DECISION_LATENCY_METRIC}_count");
    let mut sum = None;
    let mut count = None;
    for line in metrics_text.split('\n') {
        if let Some(value) = metric_value(line, &sum_name) {
            sum = Some(value);
        } else if let Some(value) = metric_value(line, &count_name) {
            count = Some(value);
        }
    }
    let (sum, count) = (sum?, count?);
    let average = sum / count * 1000.0;
    (count > 0.0 && average.is_finite()).then_some(average)
}

/// The sample value is the first token after the optional `{...}` label block.
/// Any trailing token is the optional timestamp and is ignored.
fn metric_value(line: &str, name: &str) -> Option<f64> {
    let line = line.trim_end_matches('\r');
    let mut rest = line.strip_prefix(name)?;
    if let Some(labels) = rest.strip_prefix('{') {
        rest = after_label_block(labels)?;
    }
    if !rest.starts_with(' ') {
        return None;
    }
    rest.split_whitespace().next()?.parse().ok()
}

/// Returns the text after the `}` closing a label block whose `{` is already
/// consumed. Quoted label values may hold `}`, spaces and `\"` escapes.
fn after_label_block(text: &str) -> Option<&str> {
    let mut in_quotes = false;
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if in_quotes {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_quotes = false;
            }
        } else if ch == '"' {
            in_quotes = true;
        } else if ch == '}' {
            return Some(&text[index + ch.len_utf8()..]);
        }
    }
    None
}

pub fn parse_gateway_url(raw: &str) -> Result<Url, ProviderError> {
    let trimmed = raw.trim();
    let url = Url::parse(trimmed)
        .map_err(|_| ProviderError::Other("Wayfinder gateway URL is invalid".to_string()))?;

    if url.scheme() != "http"
        || url.username() != ""
        || url.password().is_some()
        || url.fragment().is_some()
        || url.host_str().is_none()
    {
        return Err(ProviderError::Other(
            "Wayfinder gateway URL must be HTTP without credentials or a fragment".to_string(),
        ));
    }

    let scheme_end = trimmed.find("://").map(|index| index + 3).unwrap_or(0);
    let authority_end = trimmed[scheme_end..]
        .find(['/', '?', '#'])
        .map(|index| scheme_end + index)
        .unwrap_or(trimmed.len());
    let authority = &trimmed[scheme_end..authority_end];
    if authority.contains('%') || authority.chars().any(char::is_whitespace) {
        return Err(ProviderError::Other(
            "Wayfinder gateway URL contains an encoded or invalid host".to_string(),
        ));
    }

    if !is_loopback_host(&url) {
        return Err(ProviderError::Other(
            "Wayfinder gateway must use localhost or a loopback address".to_string(),
        ));
    }

    Ok(url)
}

fn is_loopback_host(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    let host = host.trim_matches(['[', ']']);
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

pub fn endpoint_url(base: &Url, path: &str) -> Result<Url, ProviderError> {
    let mut base = base.clone();
    let base_path = base.path().trim_end_matches('/');
    base.set_path(&format!("{base_path}/"));
    base.join(path)
        .map_err(|_| ProviderError::Other("Wayfinder gateway path is invalid".to_string()))
}

pub fn parse_health(raw: &str) -> Result<HealthResponse, ProviderError> {
    serde_json::from_str(raw)
        .map_err(|_| ProviderError::Parse("Invalid Wayfinder health response".to_string()))
}

pub fn parse_models(raw: &str) -> Result<ModelsResponse, ProviderError> {
    serde_json::from_str(raw)
        .map_err(|_| ProviderError::Parse("Invalid Wayfinder models response".to_string()))
}

pub fn parse_savings(raw: &str) -> Result<SavingsResponse, ProviderError> {
    serde_json::from_str(raw)
        .map_err(|_| ProviderError::Parse("Invalid Wayfinder savings response".to_string()))
}

/// Combine the three gateway responses into the snapshot the UI renders.
fn usage_snapshot(
    health: HealthResponse,
    models: ModelsResponse,
    savings: SavingsResponse,
    metrics_text: Option<&str>,
) -> WayfinderUsageSnapshot {
    let mut routes: Vec<WayfinderRouteSummary> = savings
        .by_route
        .into_iter()
        .map(|(name, route)| WayfinderRouteSummary {
            name,
            requests: route.requests,
            tokens: route.tokens,
            realized: route.realized,
            baseline: route.baseline,
            saved: route.saved,
        })
        .collect();
    // Upstream order: busiest route first, then by name.
    routes.sort_by(|a, b| {
        b.requests
            .cmp(&a.requests)
            .then_with(|| a.name.cmp(&b.name))
    });
    WayfinderUsageSnapshot {
        gateway_status: health.status,
        offline: health.offline,
        dry_run: models.dry_run,
        missing_keys: health.missing_keys,
        model_count: models.model_count(),
        models: models.models.into_iter().map(|model| model.name).collect(),
        requests: savings.requests,
        estimated_requests: savings.estimated_requests,
        tokens: savings.tokens,
        realized: savings.realized,
        baseline: savings.baseline,
        saved: savings.saved,
        saved_percent: savings.saved_pct,
        period_days: savings.period_days,
        unit: savings.unit,
        priced: savings.priced,
        routes,
        avg_decision_ms: metrics_text.and_then(average_decision_ms),
    }
}

pub struct WayfinderProvider {
    metadata: ProviderMetadata,
}

impl WayfinderProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Wayfinder,
                display_name: "Wayfinder",
                session_label: "Gateway",
                weekly_label: "Savings",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: None,
                status_page_url: None,
                tertiary_label_key: None,
            },
        }
    }

    async fn fetch_json<T: DeserializeOwned>(
        client: &reqwest::Client,
        url: &Url,
    ) -> Result<T, ProviderError> {
        let response = client.get(url.clone()).send().await?;
        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "Wayfinder gateway returned HTTP {}",
                response.status()
            )));
        }
        response
            .json::<T>()
            .await
            .map_err(|_| ProviderError::Parse("Invalid Wayfinder gateway response".to_string()))
    }

    async fn fetch_metrics(client: &reqwest::Client, url: &Url) -> Option<String> {
        let response = client.get(url.clone()).send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }
        response.text().await.ok()
    }
}

impl Default for WayfinderProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for WayfinderProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Wayfinder
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        if ctx.source_mode != SourceMode::Auto {
            return Err(ProviderError::UnsupportedSource(ctx.source_mode));
        }

        let base = parse_gateway_url(ctx.gateway_url.as_deref().unwrap_or(DEFAULT_GATEWAY_URL))?;
        let client = credentialed_http_client_builder()
            .timeout(std::time::Duration::from_secs(ctx.web_timeout.max(1)))
            .build()
            .map_err(ProviderError::Network)?;

        let health: HealthResponse =
            Self::fetch_json(&client, &endpoint_url(&base, HEALTH_PATH)?).await?;
        let models: ModelsResponse =
            Self::fetch_json(&client, &endpoint_url(&base, MODELS_PATH)?).await?;
        let savings: SavingsResponse =
            Self::fetch_json(&client, &endpoint_url(&base, SAVINGS_PATH)?).await?;
        // Metrics are best-effort enrichment; a failure must not fail the usage fetch.
        let metrics = Self::fetch_metrics(&client, &endpoint_url(&base, METRICS_PATH)?).await;

        let wayfinder_usage = usage_snapshot(health, models, savings, metrics.as_deref());

        Ok(
            ProviderFetchResult::new(UsageSnapshot::new(RateWindow::new(0.0)), "gateway")
                .with_wayfinder_usage(wayfinder_usage),
        )
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_loopback_http() {
        for raw in [
            "http://127.0.0.1:8088",
            "http://127.42.1.9:8088",
            "http://localhost:8088",
            "http://[::1]:8088",
        ] {
            assert!(parse_gateway_url(raw).is_ok(), "{raw}");
        }
    }

    #[test]
    fn rejects_unsafe_gateway_urls() {
        for raw in [
            "127.0.0.1:8088",
            "ftp://127.0.0.1:8088",
            "https://127.0.0.1:8088",
            "https://gateway.example.test/wayfinder",
            "http://192.168.1.20:8088",
            "http://user:password@127.0.0.1:8088",
            "http://127.0.0.1:8088/#fragment",
            "http://127.0.0.1%2f.attacker.test:8088",
            "https://proxy.test%2f.attacker.test/v1",
        ] {
            assert!(parse_gateway_url(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn endpoint_paths_preserve_gateway_prefix() {
        let base = parse_gateway_url("http://localhost:8088/wayfinder/").unwrap();
        assert_eq!(
            endpoint_url(&base, "healthz").unwrap().as_str(),
            "http://localhost:8088/wayfinder/healthz"
        );
    }

    #[test]
    fn parses_upstream_fixtures_without_identity_or_quota_fields() {
        let health = parse_health(include_str!("fixtures/wayfinder/health.json")).unwrap();
        let models = parse_models(include_str!("fixtures/wayfinder/models.json")).unwrap();
        let savings = parse_savings(include_str!("fixtures/wayfinder/savings.json")).unwrap();

        assert_eq!(health.status, "ok");
        assert!(!health.offline);
        assert!(health.missing_keys.is_empty());
        assert_eq!(models.model_count(), 2);
        assert!(!models.dry_run);
        assert_eq!(savings.requests, 14);
        assert_eq!(savings.tokens, 1028);
        assert_eq!(savings.by_route["local"].requests, 10);
        assert_eq!(savings.saved, 0.005694);
    }

    /// The frontend bridge reads this snapshot as camelCase (`savedPercent`,
    /// `missingKeys`, `dryRun`, ...). Snake_case keys leave those fields
    /// undefined and the tray panel throws while rendering the card.
    #[test]
    fn pack_snapshot_serializes_camel_case_keys_for_the_bridge() {
        let snapshot = usage_snapshot(
            parse_health(include_str!("fixtures/wayfinder/pack-health.json")).unwrap(),
            parse_models(include_str!("fixtures/wayfinder/pack-models.json")).unwrap(),
            parse_savings(include_str!("fixtures/wayfinder/pack-savings.json")).unwrap(),
            Some(PACK_METRICS),
        );

        let json = serde_json::to_value(&snapshot).unwrap();
        let mut actual: Vec<&str> = json
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        let mut expected = vec![
            "gatewayStatus",
            "offline",
            "dryRun",
            "missingKeys",
            "modelCount",
            "models",
            "requests",
            "estimatedRequests",
            "tokens",
            "realized",
            "baseline",
            "saved",
            "savedPercent",
            "periodDays",
            "unit",
            "priced",
            "routes",
            "avgDecisionMs",
        ];
        actual.sort_unstable();
        expected.sort_unstable();
        assert_eq!(actual, expected);

        assert_eq!(json["gatewayStatus"], "ok");
        assert_eq!(json["dryRun"], false);
        assert_eq!(json["missingKeys"], serde_json::json!([]));
        assert_eq!(json["modelCount"], 3);
        assert_eq!(json["savedPercent"], 57.1);
        assert_eq!(json["periodDays"], 30);
        assert_eq!(json["estimatedRequests"], 0);
        let avg = json["avgDecisionMs"].as_f64().unwrap();
        assert!((avg - 0.35).abs() < 1e-9, "{avg}");

        let route = json["routes"][0].as_object().unwrap();
        let mut route_keys: Vec<&str> = route.keys().map(String::as_str).collect();
        route_keys.sort_unstable();
        assert_eq!(
            route_keys,
            [
                "baseline", "name", "realized", "requests", "saved", "tokens"
            ]
        );
        assert_eq!(json["routes"][0]["name"], "cheap");
        assert_eq!(json["routes"][1]["name"], "premium");
    }

    /// `/metrics` body from the Mac-parity Wayfinder pack.
    const PACK_METRICS: &str = "# HELP wayfinder_router_decision_latency_seconds Router decision latency\n\
# TYPE wayfinder_router_decision_latency_seconds histogram\n\
wayfinder_router_decision_latency_seconds_sum 0.42\n\
wayfinder_router_decision_latency_seconds_count 1200\n";

    /// The pack's `/v1/savings` as the Mac rig serves it: no `period_days`,
    /// no `unit`, and route buckets with only requests/saved/tokens.
    #[test]
    fn mac_pack_savings_without_optional_fields_parses() {
        let savings =
            parse_savings(include_str!("fixtures/wayfinder/mac-pack-savings.json")).unwrap();
        assert_eq!(savings.period_days, 30);
        assert_eq!(savings.unit, "usd");
        assert_eq!(savings.requests, 1200);
        assert_eq!(savings.saved_pct, 57.1);
        assert_eq!(savings.by_route["cheap"].requests, 900);
        assert_eq!(savings.by_route["cheap"].realized, 0.0);
        assert_eq!(savings.by_route["premium"].saved, 2.5);

        // A missing upstream-required field still fails the parse.
        assert!(parse_savings(r#"{"priced":true,"requests":1,"tokens":1}"#).is_err());
        assert!(
            parse_savings(
                r#"{"priced":true,"requests":1,"tokens":1,"realized":0,"baseline":0,"saved":0,"saved_pct":0,"by_route":{"a":{"requests":1,"tokens":1}}}"#
            )
            .is_err()
        );
    }

    #[test]
    fn average_decision_latency_follows_upstream_prometheus_parse() {
        let average = average_decision_ms(PACK_METRICS).unwrap();
        assert!((average - 0.35).abs() < 1e-9, "{average}");

        // Labelled series, CRLF line ends, and the last sample wins.
        let labelled = "wayfinder_router_decision_latency_seconds_sum{route=\"cheap\"} 1\r\n\
wayfinder_router_decision_latency_seconds_count{route=\"cheap\"} 4\r\n\
wayfinder_router_decision_latency_seconds_count{route=\"all\"} 2\r\n";
        assert_eq!(average_decision_ms(labelled), Some(500.0));

        // A longer metric name sharing the prefix is not a match.
        assert_eq!(
            average_decision_ms(
                "wayfinder_router_decision_latency_seconds_sum_total 5\n\
wayfinder_router_decision_latency_seconds_count 1\n"
            ),
            None
        );
        // No samples or a zero count gives no latency.
        assert_eq!(
            average_decision_ms(
                "wayfinder_router_decision_latency_seconds_sum 0\n\
wayfinder_router_decision_latency_seconds_count 0\n"
            ),
            None
        );
        assert_eq!(average_decision_ms("up 1\n"), None);
    }

    #[test]
    fn metric_value_ignores_timestamps_and_quoted_label_blocks() {
        // Two-line sample with a trailing timestamp on each series: 0.0007 / 2 * 1000 = 0.35 ms.
        let stamped = "wayfinder_router_decision_latency_seconds_sum 0.0007 1700000000000\n\
wayfinder_router_decision_latency_seconds_count 2 1700000000000\n";
        let average = average_decision_ms(stamped).unwrap();
        assert!((average - 0.35).abs() < 1e-9, "{average}");

        // A label value with a space, a `}` and an escaped quote does not end the block early.
        assert_eq!(
            metric_value(r#"x{route="fast } lane \"a\""} 0.0007 1700000000000"#, "x"),
            Some(0.0007)
        );
        let labelled = "wayfinder_router_decision_latency_seconds_sum{route=\"fast } lane\"} 0.0007 1700000000000\n\
wayfinder_router_decision_latency_seconds_count{route=\"fast } lane\"} 2 1700000000000\n";
        let average = average_decision_ms(labelled).unwrap();
        assert!((average - 0.35).abs() < 1e-9, "{average}");

        // An unterminated label block yields no value.
        assert_eq!(metric_value("x{route=\"a 3", "x"), None);
    }

    #[test]
    fn routes_sort_by_requests_then_name() {
        let savings = parse_savings(
            r#"{"priced":true,"requests":30,"tokens":3,"realized":0,"baseline":0,"saved":0,"saved_pct":0,
                "by_route":{"alpha":{"requests":5,"saved":0,"tokens":1},
                            "zulu":{"requests":20,"saved":0,"tokens":1},
                            "beta":{"requests":5,"saved":0,"tokens":1}}}"#,
        )
        .unwrap();
        let snapshot = usage_snapshot(
            parse_health(include_str!("fixtures/wayfinder/pack-health.json")).unwrap(),
            parse_models(include_str!("fixtures/wayfinder/pack-models.json")).unwrap(),
            savings,
            None,
        );
        let names: Vec<&str> = snapshot.routes.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["zulu", "alpha", "beta"]);
        assert_eq!(snapshot.avg_decision_ms, None);
        // No latency: the key stays off the bridge payload.
        let json = serde_json::to_value(&snapshot).unwrap();
        assert!(json.get("avgDecisionMs").is_none());
    }

    #[test]
    fn malformed_payloads_are_rejected() {
        assert!(parse_health("{\"status\":").is_err());
        assert!(parse_models("{\"models\": [").is_err());
        assert!(parse_savings("{\"requests\": \"many\"}").is_err());
    }
}
