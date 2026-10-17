//! Xiaomi MiMo provider implementation.
//!
//! Uses browser cookies to read balance and token-plan usage.

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, TimeZone, Utc};
use reqwest::Client;
use serde::Deserialize;

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};

const MIMO_API_BASE: &str = "https://platform.xiaomimimo.com/api/v1";
const BALANCE_DETAIL_ID: &str = "mimo-balance";

/// Reset-window pace (upstream `MiMoProviderDescriptor.pace`, v0.70.0):
/// `.calendarMonthResetWindow`.
pub(crate) const PACE_CAPABILITY: crate::core::ProviderPaceCapability =
    crate::core::ProviderPaceCapability::CALENDAR_MONTH_RESET_WINDOW;

pub struct MiMoProvider {
    metadata: ProviderMetadata,
    client: Client,
}

#[derive(Debug, Deserialize)]
struct BalanceResponse {
    code: i64,
    #[serde(default)]
    message: Option<String>,
    data: Option<BalanceData>,
}

#[derive(Debug, Deserialize)]
struct BalanceData {
    balance: String,
    currency: String,
    #[serde(default, alias = "cashBalance")]
    cash_balance: Option<String>,
    #[serde(default, alias = "giftBalance")]
    gift_balance: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenPlanDetailResponse {
    code: i64,
    data: Option<TokenPlanDetailData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenPlanDetailData {
    plan_code: Option<String>,
    current_period_end: Option<String>,
    #[serde(default)]
    expired: bool,
}

#[derive(Debug, Deserialize)]
struct TokenPlanUsageResponse {
    code: i64,
    data: Option<TokenPlanUsageData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TokenPlanUsageData {
    month_usage: Option<TokenPlanMonthUsage>,
}

#[derive(Debug, Deserialize)]
struct TokenPlanMonthUsage {
    #[serde(default)]
    items: Vec<TokenPlanUsageItem>,
}

#[derive(Debug, Deserialize)]
struct TokenPlanUsageItem {
    used: i64,
    limit: i64,
    percent: f64,
}

impl MiMoProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::MiMo,
                display_name: "Xiaomi MiMo",
                session_label: "Credits",
                weekly_label: "Window",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://platform.xiaomimimo.com/#/console/balance"),
                status_page_url: None,
                tertiary_label_key: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    async fn fetch_web(&self, cookie_header: &str) -> Result<ProviderFetchResult, ProviderError> {
        let cookie = normalize_cookie_header(cookie_header).ok_or(ProviderError::NoCookies)?;
        let balance: BalanceResponse = self.get_json("balance", &cookie).await?;
        if balance.code == 401 {
            return Err(ProviderError::AuthRequired);
        }
        if balance.code != 0 {
            return Err(ProviderError::Parse(format!(
                "MiMo balance error: {}",
                balance.message.unwrap_or_else(|| balance.code.to_string())
            )));
        }
        let data = balance
            .data
            .ok_or_else(|| ProviderError::Parse("MiMo balance payload missing".into()))?;
        let balance_row = balance_row_from_data(&data)?;

        let detail: Option<TokenPlanDetailResponse> =
            self.get_json("tokenPlan/detail", &cookie).await.ok();
        let usage: Option<TokenPlanUsageResponse> =
            self.get_json("tokenPlan/usage", &cookie).await.ok();
        let snapshot = snapshot_from_parts(detail, usage);
        Ok(ProviderFetchResult::new(snapshot, "web").with_display_detail(balance_row))
    }

    async fn get_json<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        cookie: &str,
    ) -> Result<T, ProviderError> {
        let response = self
            .client
            .get(format!("{MIMO_API_BASE}/{path}"))
            .header("Cookie", cookie)
            .header("Accept", "application/json, text/plain, */*")
            .header("Origin", "https://platform.xiaomimimo.com")
            .header(
                "Referer",
                "https://platform.xiaomimimo.com/#/console/balance",
            )
            .header("x-timeZone", "UTC+01:00")
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            || response.status() == reqwest::StatusCode::FORBIDDEN
        {
            return Err(ProviderError::AuthRequired);
        }
        if !response.status().is_success() {
            return Err(ProviderError::Other(format!(
                "MiMo API returned status {}",
                response.status()
            )));
        }
        response
            .json::<T>()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to parse MiMo response: {e}")))
    }
}

fn normalize_cookie_header(raw: &str) -> Option<String> {
    let known = [
        "api-platform_serviceToken",
        "userId",
        "api-platform_ph",
        "api-platform_slh",
    ];
    let required = ["api-platform_serviceToken", "userId"];
    let mut pairs = Vec::new();
    for chunk in raw.trim().split(';') {
        let Some((name, value)) = chunk.trim().split_once('=') else {
            continue;
        };
        let name = name.trim();
        let value = value.trim();
        if known.contains(&name) && !value.is_empty() {
            pairs.push((name.to_string(), value.to_string()));
        }
    }
    if required
        .iter()
        .all(|required| pairs.iter().any(|(name, _)| name == required))
    {
        pairs.sort_by(|a, b| a.0.cmp(&b.0));
        Some(
            pairs
                .into_iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<_>>()
                .join("; "),
        )
    } else {
        None
    }
}

/// Mirrors upstream `MiMoUsageSnapshot.toUsageSnapshot()`: the token plan is the
/// only bar, the plan code is the title-cased plan label, and the balance is a
/// "Credits" detail row (see [`balance_detail`]), not a second bar.
fn snapshot_from_parts(
    detail: Option<TokenPlanDetailResponse>,
    usage: Option<TokenPlanUsageResponse>,
) -> UsageSnapshot {
    let detail_data =
        detail.and_then(|response| (response.code == 0).then_some(response.data).flatten());
    let usage_item = usage
        .and_then(|response| (response.code == 0).then_some(response.data).flatten())
        .and_then(|data| data.month_usage)
        .and_then(|month| month.items.into_iter().next());

    let plan_name = detail_data
        .as_ref()
        .and_then(|data| data.plan_code.as_deref())
        .map(capitalize_words)
        .filter(|plan| !plan.trim().is_empty());
    let period_end = detail_data
        .as_ref()
        .and_then(|data| data.current_period_end.as_deref())
        .and_then(parse_mimo_date);

    // Upstream MiMoUsageSnapshot only builds the token window when `limit > 0`,
    // and `percent` is a 0-1 fraction that it scales to 0-100 and clamps.
    let primary = if let Some(item) = usage_item.filter(|item| item.limit > 0) {
        RateWindow::with_details(
            (item.percent * 100.0).clamp(0.0, 100.0),
            RateWindow::monthly_window_minutes(period_end),
            period_end,
            Some(format!(
                "{} / {} Credits",
                grouped_integer(item.used),
                grouped_integer(item.limit)
            )),
        )
        .with_description_as_detail()
    } else {
        RateWindow::with_details(
            0.0,
            RateWindow::monthly_window_minutes(period_end),
            period_end,
            Some("No token-plan usage".into()),
        )
    };

    let snapshot = UsageSnapshot::new(primary);
    match plan_name {
        Some(plan) => snapshot.with_login_method(plan),
        None => snapshot,
    }
}

fn parse_mimo_date(value: &str) -> Option<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|dt| Utc.from_utc_datetime(&dt))
}

fn parse_decimal(value: Option<&str>) -> Option<f64> {
    value?.trim().parse().ok()
}

/// Upstream `MiMoUsageSnapshot.balanceDetail`, shown as the "Balance" row of a
/// "Credits" section: "CN¥12.34 (Paid: CN¥8.00 / Granted: CN¥4.34)".
fn balance_text(
    balance: f64,
    currency: &str,
    cash_balance: Option<&str>,
    gift_balance: Option<&str>,
) -> String {
    let currency = currency.trim();
    let total = currency_string(balance, currency);
    let Some(cash) = parse_decimal(cash_balance) else {
        return total;
    };
    let Some(gift) = parse_decimal(gift_balance) else {
        return total;
    };
    format!(
        "{total} (Paid: {} / Granted: {})",
        currency_string(cash, currency),
        currency_string(gift, currency)
    )
}

fn balance_detail(
    balance: f64,
    currency: &str,
    cash_balance: Option<&str>,
    gift_balance: Option<&str>,
) -> Option<ProviderDisplayDetail> {
    ProviderDisplayDetail::new(
        BALANCE_DETAIL_ID,
        "Balance",
        balance_text(balance, currency, cash_balance, gift_balance),
    )
    .and_then(|row| row.with_section_title("Credits"))
}

/// The "Credits" balance row for a decoded `balance` payload, as `fetch_web`
/// builds it. A non-numeric balance is a parse error, reported before any
/// plan or usage request is made.
fn balance_row_from_data(
    data: &BalanceData,
) -> Result<Option<ProviderDisplayDetail>, ProviderError> {
    let balance_value = data
        .balance
        .parse::<f64>()
        .map_err(|_| ProviderError::Parse("MiMo balance value invalid".into()))?;
    Ok(balance_detail(
        balance_value,
        &data.currency,
        data.cash_balance.as_deref(),
        data.gift_balance.as_deref(),
    ))
}

/// en_US currency text as upstream `UsageFormatter.currencyString` renders it
/// (`.currency(code:).locale(en_US)`): symbol prefix, grouped, two decimals.
/// Kept local to MiMo so the shared formatter keeps its current output.
fn currency_string(value: f64, code: &str) -> String {
    let prefix = match code.trim().to_ascii_uppercase().as_str() {
        "CNY" => "CN¥".to_string(),
        "USD" => "$".to_string(),
        "EUR" => "€".to_string(),
        "GBP" => "£".to_string(),
        "" => String::new(),
        other => format!("{other}\u{a0}"),
    };
    let fixed = format!("{:.2}", value.abs());
    let (whole, fraction) = fixed.split_once('.').unwrap_or((fixed.as_str(), "00"));
    let sign = if value < 0.0 && fixed != "0.00" {
        "-"
    } else {
        ""
    };
    format!("{sign}{prefix}{}.{fraction}", group_digits(whole))
}

/// en_US decimal grouping ("1,000,000"), as upstream `fullCountString`.
fn grouped_integer(value: i64) -> String {
    let digits = group_digits(&value.unsigned_abs().to_string());
    if value < 0 {
        format!("-{digits}")
    } else {
        digits
    }
}

/// Inserts a comma every three digits from the right of an ASCII digit run.
fn group_digits(digits: &str) -> String {
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// Foundation `String.capitalized`: words split on whitespace, first letter
/// upper-cased, the rest lower-cased.
fn capitalize_words(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut at_word_start = true;
    for ch in value.chars() {
        if ch.is_whitespace() {
            at_word_start = true;
            out.push(ch);
        } else if at_word_start {
            at_word_start = false;
            out.extend(ch.to_uppercase());
        } else {
            out.extend(ch.to_lowercase());
        }
    }
    out
}

impl Default for MiMoProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for MiMoProvider {
    fn id(&self) -> ProviderId {
        ProviderId::MiMo
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Web => {
                let cookie = match ctx.manual_cookie_header.as_deref() {
                    Some(cookie) => cookie.to_string(),
                    None => crate::providers::browser_cookie_header(&["platform.xiaomimimo.com"])?,
                };
                self.fetch_web(&cookie).await
            }
            SourceMode::OAuth | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Web]
    }

    fn supports_web(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mimo_cookie_requires_service_token_and_user_id() {
        assert!(normalize_cookie_header("api-platform_serviceToken=abc; userId=42").is_some());
        assert!(normalize_cookie_header("api-platform_serviceToken=abc").is_none());
    }

    #[test]
    fn mimo_balance_text_includes_paid_and_granted_components() {
        assert_eq!(
            balance_text(12.5, "CNY", Some("8.25"), Some("4.25")),
            "CN¥12.50 (Paid: CN¥8.25 / Granted: CN¥4.25)"
        );
        assert_eq!(balance_text(12.5, "CNY", Some("8.25"), None), "CN¥12.50");
    }

    #[test]
    fn mimo_currency_string_matches_en_us_currency_style() {
        assert_eq!(currency_string(1234.5, "CNY"), "CN¥1,234.50");
        assert_eq!(currency_string(-3.0, "usd"), "-$3.00");
        assert_eq!(currency_string(0.0, " CNY "), "CN¥0.00");
        assert_eq!(currency_string(7.25, "HKD"), "HKD\u{a0}7.25");
        assert_eq!(currency_string(-0.001, "USD"), "$0.00");
        assert_eq!(currency_string(999.999, "USD"), "$1,000.00");
    }

    #[test]
    fn mimo_grouped_integer_and_capitalize_follow_foundation() {
        assert_eq!(grouped_integer(0), "0");
        assert_eq!(grouped_integer(999), "999");
        assert_eq!(grouped_integer(1_000_000), "1,000,000");
        assert_eq!(grouped_integer(-12_345), "-12,345");
        assert_eq!(capitalize_words("pro"), "Pro");
        assert_eq!(capitalize_words("MAX plan"), "Max Plan");
    }

    #[test]
    fn mimo_labels_match_upstream_descriptor() {
        let provider = MiMoProvider::new();
        assert_eq!(provider.metadata().session_label, "Credits");
        assert_eq!(provider.metadata().weekly_label, "Window");
    }

    /// The whole Mac-parity MiMo pack (routes.json): one Credits bar with the
    /// grouped detail line, plan "Pro", and the balance as a Credits section row.
    #[test]
    fn mimo_parity_pack_matches_upstream_card_fields() {
        let detail: Option<TokenPlanDetailResponse> = serde_json::from_str(
            r#"{"code":0,"data":{"planCode":"pro","currentPeriodEnd":"2026-11-01 00:00:00","expired":false}}"#,
        )
        .ok();
        assert!(detail.is_some());
        let snapshot = snapshot_from_parts(detail, usage_response(PACK_USAGE_JSON));

        assert_eq!(snapshot.primary.used_percent, 37.0);
        assert_eq!(
            snapshot.primary.reset_description.as_deref(),
            Some("370,000 / 1,000,000 Credits")
        );
        assert!(snapshot.primary.description_is_detail);
        assert_eq!(
            snapshot.primary.resets_at,
            Some(Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap())
        );
        assert!(snapshot.secondary.is_none());
        assert_eq!(snapshot.login_method.as_deref(), Some("Pro"));

        let row = balance_detail(12.34, "CNY", Some("8.00"), Some("4.34")).expect("balance row");
        assert_eq!(row.id(), BALANCE_DETAIL_ID);
        assert_eq!(row.section_title(), Some("Credits"));
        assert_eq!(row.title(), "Balance");
        assert_eq!(row.value(), "CN¥12.34 (Paid: CN¥8.00 / Granted: CN¥4.34)");
    }

    /// A `balance` response decoded into `BalanceResponse` (synthetic values from
    /// the Mac-parity MiMo pack) yields the same "Credits" row `fetch_web` builds.
    #[test]
    fn mimo_balance_response_decodes_into_credits_row() {
        let balance: BalanceResponse = serde_json::from_str(
            r#"{"code":0,"data":{"balance":"12.34","currency":"CNY","cashBalance":"8.00","giftBalance":"4.34"}}"#,
        )
        .expect("balance response decodes");
        assert_eq!(balance.code, 0);
        let data = balance.data.expect("balance payload");

        let row = balance_row_from_data(&data)
            .expect("balance row builds")
            .expect("balance row present");
        assert_eq!(row.id(), BALANCE_DETAIL_ID);
        assert_eq!(row.title(), "Balance");
        assert_eq!(row.section_title(), Some("Credits"));
        assert_eq!(row.value(), "CN¥12.34 (Paid: CN¥8.00 / Granted: CN¥4.34)");
    }

    #[test]
    fn mimo_non_numeric_balance_is_a_parse_error() {
        let data = BalanceData {
            balance: "twelve".to_string(),
            currency: "CNY".to_string(),
            cash_balance: None,
            gift_balance: None,
        };
        assert!(matches!(
            balance_row_from_data(&data),
            Err(ProviderError::Parse(_))
        ));
    }

    #[test]
    fn mimo_without_plan_code_has_no_login_method() {
        let snapshot = snapshot_with_usage(usage_response(PACK_USAGE_JSON));
        assert!(snapshot.login_method.is_none());
    }

    /// `tokenPlan/usage` payload from the Mac-parity MiMo pack (synthetic values).
    const PACK_USAGE_JSON: &str = r#"{
        "code": 0,
        "data": {
            "monthUsage": {
                "percent": 0.37,
                "items": [
                    { "name": "credits", "used": 370000, "limit": 1000000, "percent": 0.37 }
                ]
            }
        }
    }"#;

    fn usage_response(json: &str) -> Option<TokenPlanUsageResponse> {
        serde_json::from_str(json).ok()
    }

    fn snapshot_with_usage(usage: Option<TokenPlanUsageResponse>) -> UsageSnapshot {
        snapshot_from_parts(None, usage)
    }

    fn usage_item_json(used: i64, limit: i64, percent: f64) -> String {
        format!(
            r#"{{"code":0,"data":{{"monthUsage":{{"percent":{percent},"items":[{{"name":"credits","used":{used},"limit":{limit},"percent":{percent}}}]}}}}}}"#
        )
    }

    #[test]
    fn mimo_token_percent_fraction_is_scaled_to_percentage() {
        let snapshot = snapshot_with_usage(usage_response(PACK_USAGE_JSON));
        assert_eq!(snapshot.primary.used_percent, 37.0);
        assert_eq!(
            snapshot.primary.reset_description.as_deref(),
            Some("370,000 / 1,000,000 Credits")
        );
    }

    #[test]
    fn mimo_token_percent_is_clamped_to_0_100() {
        let over = snapshot_with_usage(usage_response(&usage_item_json(10, 10, 1.5)));
        assert_eq!(over.primary.used_percent, 100.0);
        let under = snapshot_with_usage(usage_response(&usage_item_json(0, 10, -0.2)));
        assert_eq!(under.primary.used_percent, 0.0);
    }

    #[test]
    fn mimo_token_window_requires_positive_limit() {
        let snapshot = snapshot_with_usage(usage_response(&usage_item_json(5, 0, 0.5)));
        assert_eq!(snapshot.primary.used_percent, 0.0);
        assert_eq!(
            snapshot.primary.reset_description.as_deref(),
            Some("No token-plan usage")
        );
    }

    #[test]
    fn mimo_token_usage_without_percent_is_ignored() {
        let usage =
            usage_response(r#"{"code":0,"data":{"monthUsage":{"items":[{"used":1,"limit":10}]}}}"#);
        assert!(usage.is_none());
        let snapshot = snapshot_with_usage(usage);
        assert_eq!(snapshot.primary.used_percent, 0.0);
        assert_eq!(
            snapshot.primary.reset_description.as_deref(),
            Some("No token-plan usage")
        );
    }

    #[test]
    fn mimo_token_plan_uses_calendar_month_minutes() {
        // Period end 2026-03-01 → February cycle is 28 days.
        let period_end = Utc.with_ymd_and_hms(2026, 3, 1, 0, 0, 0).unwrap();
        let primary = RateWindow::with_details(
            40.0,
            RateWindow::monthly_window_minutes(Some(period_end)),
            Some(period_end),
            Some("400/1000 tokens".into()),
        );
        assert_eq!(primary.window_minutes, Some(28 * 24 * 60));
        assert_eq!(primary.resets_at, Some(period_end));
    }
}
