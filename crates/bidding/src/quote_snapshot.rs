//! Immutable, server-calculated QuoteSnapshotV1 canonicalization.

use chrono::{DateTime, SecondsFormat, Utc};
use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FinalizeQuoteSnapshotV1 {
    pub title: String,
    #[serde(default)]
    pub notes: Option<String>,
    pub tax_mode: TaxModeV1,
    pub lines: Vec<QuoteLineInputV1>,
    pub no_ceiling_review_reason: String,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TaxModeV1 {
    TaxInclusive,
    TaxExclusive,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PricingModeV1 {
    UnitPrice,
    LumpSum,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct QuoteLineInputV1 {
    pub description: String,
    pub pricing_mode: PricingModeV1,
    #[serde(default)]
    pub quantity: Option<String>,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub unit_price: Option<String>,
    #[serde(default)]
    pub entered_amount: Option<String>,
    pub tax_rate: String,
    pub user_confirmed: bool,
}

#[derive(Debug, Clone, Serialize)]
struct QuoteSnapshotLineV1 {
    id: Uuid,
    ordinal: usize,
    description: String,
    pricing_mode: PricingModeV1,
    quantity: Option<String>,
    unit: Option<String>,
    unit_price: Option<String>,
    entered_amount: Option<String>,
    tax_rate: String,
    basis_amount: String,
    net_amount: String,
    tax_amount: String,
    gross_amount: String,
    user_confirmed: bool,
}

#[derive(Debug, Clone, Serialize)]
struct NoCeilingReviewV1 {
    reviewed: bool,
    reason: String,
    actor_kind: &'static str,
    actor_id: Uuid,
    at: String,
}

#[derive(Debug, Clone, Serialize)]
struct QuoteSnapshotCanonicalV1 {
    schema_version: u8,
    quote_id: Uuid,
    project_id: Uuid,
    revision: i64,
    currency_code: &'static str,
    currency_scale: u8,
    tax_mode: TaxModeV1,
    title: String,
    notes: Option<String>,
    lines: Vec<QuoteSnapshotLineV1>,
    net_total: String,
    tax_total: String,
    gross_total: String,
    ceiling: Option<serde_json::Value>,
    no_ceiling_review: NoCeilingReviewV1,
    fact_revision: Option<i64>,
    pricing_revision: Option<i64>,
    pricing_set_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltQuoteSnapshotV1 {
    pub quote_id: Uuid,
    pub canonical_payload: Vec<u8>,
    pub content_sha256: String,
}

fn deterministic_uuid(material: &str) -> Uuid {
    let digest = Sha256::digest(material.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn parse_fixed(value: &str, scale: usize, max_integer_digits: usize) -> Result<Decimal, String> {
    let Some((integer, fraction)) = value.split_once('.') else {
        return Err("decimal must include a fixed scale".into());
    };
    if fraction.len() != scale
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        || integer.is_empty()
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || (integer.len() > 1 && integer.starts_with('0'))
        || integer.len() > max_integer_digits
    {
        return Err("decimal is not in canonical non-negative fixed-scale form".into());
    }
    value
        .parse::<Decimal>()
        .map_err(|_| "decimal is out of range".into())
}

fn fixed(value: Decimal, scale: u32) -> String {
    format!(
        "{:.*}",
        scale as usize,
        value.round_dp_with_strategy(scale, RoundingStrategy::MidpointAwayFromZero)
    )
}

pub fn build_quote_snapshot_v1(
    project_id: Uuid,
    quote_id: Uuid,
    revision: i64,
    actor_user_id: Uuid,
    finalized_at: DateTime<Utc>,
    input: &FinalizeQuoteSnapshotV1,
) -> Result<BuiltQuoteSnapshotV1, String> {
    if revision < 1 {
        return Err("quote revision must be positive".into());
    }
    let title = input.title.trim();
    if title.is_empty() || title.len() > 256 {
        return Err("quote title length is invalid".into());
    }
    let notes = input
        .notes
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    if notes.as_ref().is_some_and(|value| value.len() > 4096) {
        return Err("quote notes length is invalid".into());
    }
    let reason = input.no_ceiling_review_reason.trim();
    if reason.is_empty() || reason.len() > 1024 {
        return Err("no-ceiling review reason is required".into());
    }
    if input.lines.is_empty() || input.lines.len() > 10_000 {
        return Err("quote lines are empty or exceed the limit".into());
    }
    let mut lines = Vec::with_capacity(input.lines.len());
    let mut net_total = Decimal::ZERO;
    let mut tax_total = Decimal::ZERO;
    let mut gross_total = Decimal::ZERO;
    for (ordinal, line) in input.lines.iter().enumerate() {
        let description = line.description.trim();
        if description.is_empty() || description.len() > 4096 {
            return Err("quote line description is invalid".into());
        }
        if !line.user_confirmed {
            return Err("every quote line must be user confirmed".into());
        }
        let tax_rate = parse_fixed(&line.tax_rate, 6, 1)?;
        if tax_rate > Decimal::ONE {
            return Err("quote tax rate exceeds one".into());
        }
        let basis = match line.pricing_mode {
            PricingModeV1::UnitPrice => {
                if line.entered_amount.is_some() {
                    return Err("unit-price line cannot contain entered_amount".into());
                }
                let quantity = parse_fixed(
                    line.quantity
                        .as_deref()
                        .ok_or("unit-price quantity missing")?,
                    6,
                    9,
                )?;
                let unit = line
                    .unit
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or("unit-price unit missing")?;
                if unit.len() > 64 || quantity <= Decimal::ZERO {
                    return Err("unit-price quantity or unit is invalid".into());
                }
                let unit_price = parse_fixed(
                    line.unit_price.as_deref().ok_or("unit price missing")?,
                    6,
                    12,
                )?;
                (quantity * unit_price)
                    .round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero)
            }
            PricingModeV1::LumpSum => {
                if line.quantity.is_some() || line.unit.is_some() || line.unit_price.is_some() {
                    return Err("lump-sum line contains unit-price fields".into());
                }
                parse_fixed(
                    line.entered_amount
                        .as_deref()
                        .ok_or("lump-sum amount missing")?,
                    2,
                    18,
                )?
            }
        };
        let (net, tax, gross) = match input.tax_mode {
            TaxModeV1::TaxExclusive => {
                let net = basis;
                let tax = (net * tax_rate)
                    .round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero);
                (net, tax, net + tax)
            }
            TaxModeV1::TaxInclusive => {
                let gross = basis;
                let net = (gross / (Decimal::ONE + tax_rate))
                    .round_dp_with_strategy(2, RoundingStrategy::MidpointAwayFromZero);
                (net, gross - net, gross)
            }
        };
        for amount in [basis, net, tax, gross] {
            if amount < Decimal::ZERO || amount >= Decimal::new(10_i64.pow(18), 0) {
                return Err("quote amount overflow".into());
            }
        }
        net_total += net;
        tax_total += tax;
        gross_total += gross;
        lines.push(QuoteSnapshotLineV1 {
            id: deterministic_uuid(&format!("{quote_id}:{revision}:{ordinal}")),
            ordinal,
            description: description.to_owned(),
            pricing_mode: line.pricing_mode,
            quantity: line.quantity.clone(),
            unit: line.unit.as_deref().map(str::trim).map(str::to_owned),
            unit_price: line.unit_price.clone(),
            entered_amount: line.entered_amount.clone(),
            tax_rate: fixed(tax_rate, 6),
            basis_amount: fixed(basis, 2),
            net_amount: fixed(net, 2),
            tax_amount: fixed(tax, 2),
            gross_amount: fixed(gross, 2),
            user_confirmed: true,
        });
    }
    let canonical = QuoteSnapshotCanonicalV1 {
        schema_version: 1,
        quote_id,
        project_id,
        revision,
        currency_code: "CNY",
        currency_scale: 2,
        tax_mode: input.tax_mode,
        title: title.to_owned(),
        notes,
        lines,
        net_total: fixed(net_total, 2),
        tax_total: fixed(tax_total, 2),
        gross_total: fixed(gross_total, 2),
        ceiling: None,
        no_ceiling_review: NoCeilingReviewV1 {
            reviewed: true,
            reason: reason.to_owned(),
            actor_kind: "user",
            actor_id: actor_user_id,
            at: finalized_at.to_rfc3339_opts(SecondsFormat::Micros, true),
        },
        fact_revision: None,
        pricing_revision: None,
        pricing_set_sha256: None,
    };
    let canonical_payload = serde_json::to_vec(&canonical).map_err(|error| error.to_string())?;
    let content_sha256 = hex::encode(Sha256::digest(&canonical_payload));
    Ok(BuiltQuoteSnapshotV1 {
        quote_id,
        canonical_payload,
        content_sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn computes_tax_and_replays_exact_bytes() {
        let input = FinalizeQuoteSnapshotV1 {
            title: " 报价 ".into(),
            notes: None,
            tax_mode: TaxModeV1::TaxExclusive,
            lines: vec![QuoteLineInputV1 {
                description: "服务".into(),
                pricing_mode: PricingModeV1::UnitPrice,
                quantity: Some("2.000000".into()),
                unit: Some("项".into()),
                unit_price: Some("100.000000".into()),
                entered_amount: None,
                tax_rate: "0.060000".into(),
                user_confirmed: true,
            }],
            no_ceiling_review_reason: "招标文件未设置最高限价，已人工复核".into(),
        };
        let at = DateTime::parse_from_rfc3339("2026-01-02T03:04:05Z")
            .unwrap()
            .with_timezone(&Utc);
        let first = build_quote_snapshot_v1(
            Uuid::from_u128(1),
            Uuid::from_u128(3),
            1,
            Uuid::from_u128(2),
            at,
            &input,
        )
        .unwrap();
        let second = build_quote_snapshot_v1(
            Uuid::from_u128(1),
            Uuid::from_u128(3),
            1,
            Uuid::from_u128(2),
            at,
            &input,
        )
        .unwrap();
        assert_eq!(first, second);
        let value: serde_json::Value = serde_json::from_slice(&first.canonical_payload).unwrap();
        assert_eq!(value["gross_total"], "212.00");
        assert_eq!(value["tax_total"], "12.00");
    }
}
