//! LithosAI console payloads and the card presentation.
//!
//! Ported from upstream CodexBar v0.73.0 `lithosai.ts`. Money is in nanos
//! (1 USD = 1,000,000,000 nanos) and must be a JavaScript safe integer.

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

use crate::core::{
    CostSnapshot, ProviderDisplayDetail, ProviderError, ProviderFetchResult, RateWindow,
    UsageSnapshot,
};
use crate::locale::LocaleKey;
use crate::providers::format;

/// `Number.MAX_SAFE_INTEGER`.
const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
const NANOS_PER_USD: f64 = 1e9;
const SECTION: &str = "Billing";

/// Upstream `invalid(field)`: a parse failure naming the field, never the body.
pub(super) fn invalid(field: &str) -> ProviderError {
    ProviderError::Parse(format!("Could not parse LithosAI {field}."))
}

pub(super) fn parse_object(body: &[u8]) -> Result<Map<String, Value>, ProviderError> {
    let value: Value = serde_json::from_slice(body).map_err(|_| invalid("JSON response"))?;
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(invalid("response object")),
    }
}

fn object(value: Option<&Value>) -> Result<&Map<String, Value>, ProviderError> {
    value
        .and_then(Value::as_object)
        .ok_or_else(|| invalid("response object"))
}

/// Upstream `amount`: a JSON number that is a safe integer (`4.0` counts).
fn amount(value: Option<&Value>) -> Result<i64, ProviderError> {
    let number = value.and_then(|value| match value {
        Value::Number(number) => Some(number),
        _ => None,
    });
    let integer = number.and_then(|number| {
        number.as_i64().or_else(|| {
            let float = number.as_f64()?;
            if float.fract() != 0.0 || float.abs() > MAX_SAFE_INTEGER as f64 {
                return None;
            }
            #[allow(
                clippy::cast_possible_truncation,
                reason = "the value is integral and within the safe-integer range"
            )]
            let integer = float as i64;
            Some(integer)
        })
    });
    integer
        .filter(|value| value.abs() <= MAX_SAFE_INTEGER)
        .ok_or_else(|| invalid("integer amount"))
}

/// Upstream `text`: a trimmed non-empty string.
fn text(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Account {
    pub organization_id: String,
    pub organization_name: Option<String>,
    pub email: Option<String>,
}

pub(super) fn parse_me(me: &Map<String, Value>) -> Result<Account, ProviderError> {
    let organization = object(me.get("activeOrganization"))?;
    let organization_id =
        text(organization.get("id")).ok_or_else(|| invalid("active organization"))?;
    let valid_id = (1..=128).contains(&organization_id.len())
        && organization_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    if !valid_id {
        return Err(invalid("organization ID"));
    }
    let email = text(object(me.get("user"))?.get("email"));
    Ok(Account {
        organization_id,
        organization_name: text(organization.get("name")),
        email,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Billing {
    pub balance_nanos: i64,
    pub has_card: bool,
    pub on_hold: bool,
}

pub(super) fn parse_billing(billing: &Map<String, Value>) -> Result<Billing, ProviderError> {
    let balance_nanos = amount(billing.get("balanceNanos"))?;
    let flag = |field: &str| {
        billing
            .get(field)
            .and_then(Value::as_bool)
            .ok_or_else(|| invalid(field))
    };
    Ok(Billing {
        balance_nanos,
        has_card: flag("hasCard")?,
        on_hold: flag("onHold")?,
    })
}

/// The inclusive UTC month-to-date range, as upstream builds it from
/// `toISOString()`.
pub(super) fn spend_range(now: DateTime<Utc>) -> (String, String) {
    let end = now.format("%Y-%m-%d").to_string();
    let start = format!("{}-01", &end[..7]);
    (start, end)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Spend {
    pub today_nanos: i64,
    pub month_nanos: i64,
}

fn is_iso_day(day: &str) -> bool {
    let bytes = day.as_bytes();
    bytes.len() == 10
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            _ => byte.is_ascii_digit(),
        })
}

/// Sparse rows are summed across models and keys; an empty report is zero.
pub(super) fn parse_spend(
    report: &Map<String, Value>,
    start: &str,
    end: &str,
) -> Result<Spend, ProviderError> {
    let days = match (report.get("start"), report.get("end"), report.get("days")) {
        (Some(Value::String(s)), Some(Value::String(e)), Some(Value::Array(days)))
            if s == start && e == end =>
        {
            days
        }
        _ => return Err(invalid("spend range")),
    };
    let mut spend = Spend {
        today_nanos: 0,
        month_nanos: 0,
    };
    for raw in days {
        let day = object(Some(raw))?;
        let date = match day.get("day") {
            Some(Value::String(date))
                if is_iso_day(date) && date.as_str() >= start && date.as_str() <= end =>
            {
                date
            }
            _ => return Err(invalid("spend day")),
        };
        let nanos = amount(day.get("nanos"))?;
        if nanos < 0 {
            return Err(invalid("spend amount"));
        }
        spend.month_nanos = spend
            .month_nanos
            .checked_add(nanos)
            .filter(|sum| *sum <= MAX_SAFE_INTEGER)
            .ok_or_else(|| invalid("integer amount"))?;
        if date == end {
            spend.today_nanos += nanos;
        }
    }
    Ok(spend)
}

/// Nanos are safe integers, so the conversion to `f64` is exact.
fn usd(nanos: i64) -> f64 {
    nanos as f64 / NANOS_PER_USD
}

/// Upstream `money`: positive sub-cent amounts are labelled explicitly.
pub(super) fn money(value: f64) -> String {
    if value > 0.0 && value < 0.01 {
        "Less than $0.01".to_owned()
    } else {
        format::usd(value)
    }
}

fn row(id: &str, title: &str, value: impl Into<String>) -> Option<ProviderDisplayDetail> {
    ProviderDisplayDetail::new(id, title, value).and_then(|row| row.with_section_title(SECTION))
}

/// The prepaid balance as a cost block (omitted for a debt), plus upstream's
/// "Billing" rows.
/// Upstream has no quota bar; the Windows primary slot carries the balance
/// as an informational value.
pub(super) fn result(
    account: &Account,
    billing: &Billing,
    spend: Option<Spend>,
) -> ProviderFetchResult {
    let balance = usd(billing.balance_nanos);
    let balance_text = money(balance);
    // Upstream's snapshot has no window (lithosai.ts:71-79). The primary
    // carries the balance for the tray and float bar only; the card shows it
    // in the Billing row and the Credits block, as the Mac does.
    let mut usage = UsageSnapshot::new(
        RateWindow::informational(format!("Balance: {balance_text}")).hidden_on_card(),
    )
    .with_login_method("Browser session");
    if let Some(email) = &account.email {
        usage = usage.with_email(email.clone());
    }
    if let Some(organization) = &account.organization_name {
        usage = usage.with_organization(organization.clone());
    }

    let mut rows = vec![
        row("balance", "Balance", balance_text),
        row(
            "payment-card",
            "Payment card",
            if billing.has_card {
                "Added"
            } else {
                "Not added"
            },
        ),
        row(
            "account-status",
            "Account status",
            if billing.on_hold { "On hold" } else { "Active" },
        ),
    ];
    match spend {
        Some(spend) => {
            rows.push(row(
                "spend-today",
                "Today (UTC)",
                money(usd(spend.today_nanos)),
            ));
            rows.push(row(
                "spend-month",
                "This month (UTC)",
                money(usd(spend.month_nanos)),
            ));
        }
        None => rows.push(row("spend", "Spend", "Unavailable")),
    }

    let result = ProviderFetchResult::new(usage, "web");
    // The shared cost block floors balances at zero, which would show a debt
    // as "$0.00". Upstream keeps negative balances visible, so a debt is left
    // to the primary line and the Balance row.
    let result = if billing.balance_nanos >= 0 {
        // Upstream `.prepaidCredits` (LithosAIProviderDescriptor.swift:16-19):
        // a "Credits" block reading "Balance: $X" (MenuCardView+Costs.swift:511-518),
        // drawn after the Billing rows, which keep their own "Balance" row
        // (lithosai.ts:63). The Mac card shows the balance twice.
        result.with_cost(
            CostSnapshot::new(0.0, "USD", "Prepaid credits")
                .with_balance(balance)
                .with_card_title(LocaleKey::CreditsTitle),
        )
    } else {
        result
    };
    result.with_display_details(rows.into_iter().flatten())
}
