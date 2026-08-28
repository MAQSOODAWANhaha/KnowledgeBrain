//! Quote deep module: CNY Decimal draft math, QuoteSnapshotV1, eligibility.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal::RoundingStrategy::MidpointAwayFromZero;
use sha2::{Digest, Sha256};
use std::fmt::Write;
use std::str::FromStr;
use thiserror::Error;
use uuid::Uuid;

pub const SCHEMA_VERSION: i32 = 1;
pub const CURRENCY_CODE: &str = "CNY";
pub const CURRENCY_SCALE: u32 = 2;
pub const QTY_SCALE: u32 = 6;
pub const MAX_TITLE_BYTES: usize = 256;
pub const MAX_NOTES_BYTES: usize = 4096;
const MAX_AMOUNT_UNITS: i128 = 10i128.pow(20) - 1;
const MAX_QTY: &str = "1000000000.000000";
const MAX_UNIT_PRICE: &str = "1000000000000.000000";

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum QuoteError {
    #[error("{0}")]
    Invalid(String),
    #[error("QUOTE_AMOUNT_OVERFLOW")]
    Overflow,
    #[error("CEILING_BASIS_UNSPECIFIED")]
    CeilingBasisUnspecified,
    #[error("QUOTE_CEILING_EXCEEDED")]
    CeilingExceeded,
    #[error("QUOTE_LINE_INCOMPLETE")]
    IncompleteLine,
    #[error("QUOTE_LINE_UNCONFIRMED")]
    UnconfirmedLine,
    #[error("QUOTE_EMPTY")]
    Empty,
}

impl QuoteError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid(_) => "QUOTE_INVALID",
            Self::Overflow => "QUOTE_AMOUNT_OVERFLOW",
            Self::CeilingBasisUnspecified => "CEILING_BASIS_UNSPECIFIED",
            Self::CeilingExceeded => "QUOTE_CEILING_EXCEEDED",
            Self::IncompleteLine => "QUOTE_LINE_INCOMPLETE",
            Self::UnconfirmedLine => "QUOTE_LINE_UNCONFIRMED",
            Self::Empty => "QUOTE_EMPTY",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaxMode {
    TaxInclusive,
    TaxExclusive,
}

impl TaxMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TaxInclusive => "tax_inclusive",
            Self::TaxExclusive => "tax_exclusive",
        }
    }
}

impl FromStr for TaxMode {
    type Err = QuoteError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "tax_inclusive" => Ok(Self::TaxInclusive),
            "tax_exclusive" => Ok(Self::TaxExclusive),
            _ => Err(QuoteError::Invalid(
                "tax_mode must be tax_inclusive|tax_exclusive".into(),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PricingMode {
    UnitPrice,
    LumpSum,
}

impl PricingMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnitPrice => "unit_price",
            Self::LumpSum => "lump_sum",
        }
    }
}

impl FromStr for PricingMode {
    type Err = QuoteError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "unit_price" => Ok(Self::UnitPrice),
            "lump_sum" => Ok(Self::LumpSum),
            _ => Err(QuoteError::Invalid(
                "pricing_mode must be unit_price|lump_sum".into(),
            )),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CeilingBasis {
    TaxInclusive,
    TaxExclusive,
    Unspecified,
}

impl CeilingBasis {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::TaxInclusive => "tax_inclusive",
            Self::TaxExclusive => "tax_exclusive",
            Self::Unspecified => "unspecified",
        }
    }
}

impl FromStr for CeilingBasis {
    type Err = QuoteError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "tax_inclusive" => Ok(Self::TaxInclusive),
            "tax_exclusive" => Ok(Self::TaxExclusive),
            "unspecified" => Ok(Self::Unspecified),
            _ => Err(QuoteError::Invalid("invalid ceiling_basis".into())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Eligibility {
    Eligible,
    IneligibleCeilingChanged,
    IneligiblePricingChanged,
    IneligibleMultipleInputsChanged,
    Superseded,
}

impl Eligibility {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Eligible => "eligible",
            Self::IneligibleCeilingChanged => "ineligible_ceiling_changed",
            Self::IneligiblePricingChanged => "ineligible_pricing_changed",
            Self::IneligibleMultipleInputsChanged => "ineligible_multiple_inputs_changed",
            Self::Superseded => "superseded",
        }
    }

    pub fn transition(self, next: Self) -> Result<Self, QuoteError> {
        let allowed = matches!(
            (self, next),
            (
                Self::Eligible,
                Self::IneligibleCeilingChanged
                    | Self::IneligiblePricingChanged
                    | Self::IneligibleMultipleInputsChanged
                    | Self::Superseded
            ) | (
                Self::IneligibleCeilingChanged | Self::IneligiblePricingChanged,
                Self::IneligibleMultipleInputsChanged | Self::Superseded
            ) | (Self::IneligibleMultipleInputsChanged, Self::Superseded)
        );
        if allowed {
            Ok(next)
        } else {
            Err(QuoteError::Invalid(
                "QUOTE_SNAPSHOT_IMMUTABLE_OR_ELIGIBILITY_TRANSITION_INVALID".into(),
            ))
        }
    }

    pub fn after_input_change(self, ceiling_changed: bool, pricing_changed: bool) -> Option<Self> {
        match (ceiling_changed, pricing_changed) {
            (false, false) => None,
            (true, false) => match self {
                Self::Eligible => Some(Self::IneligibleCeilingChanged),
                Self::IneligiblePricingChanged => Some(Self::IneligibleMultipleInputsChanged),
                _ => None,
            },
            (false, true) => match self {
                Self::Eligible => Some(Self::IneligiblePricingChanged),
                Self::IneligibleCeilingChanged => Some(Self::IneligibleMultipleInputsChanged),
                _ => None,
            },
            (true, true) => match self {
                Self::Eligible
                | Self::IneligibleCeilingChanged
                | Self::IneligiblePricingChanged => Some(Self::IneligibleMultipleInputsChanged),
                _ => None,
            },
        }
    }
}

impl FromStr for Eligibility {
    type Err = QuoteError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "eligible" => Ok(Self::Eligible),
            "ineligible_ceiling_changed" => Ok(Self::IneligibleCeilingChanged),
            "ineligible_pricing_changed" => Ok(Self::IneligiblePricingChanged),
            "ineligible_multiple_inputs_changed" => Ok(Self::IneligibleMultipleInputsChanged),
            "superseded" => Ok(Self::Superseded),
            _ => Err(QuoteError::Invalid("invalid eligibility".into())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteLineInput {
    pub id: Uuid,
    pub ordinal: i32,
    pub description: String,
    pub pricing_mode: PricingMode,
    pub quantity: Option<String>,
    pub unit: Option<String>,
    pub unit_price: Option<String>,
    pub entered_amount: Option<String>,
    pub tax_rate: String,
    pub user_confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteLineComputed {
    pub id: Uuid,
    pub ordinal: i32,
    pub description: String,
    pub pricing_mode: PricingMode,
    pub complete: bool,
    pub quantity: Option<String>,
    pub unit: Option<String>,
    pub unit_price: Option<String>,
    pub entered_amount: Option<String>,
    pub tax_rate: String,
    pub basis_amount: Option<String>,
    pub net_amount: Option<String>,
    pub tax_amount: Option<String>,
    pub gross_amount: Option<String>,
    pub user_confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteTotals {
    pub net_total: String,
    pub tax_total: String,
    pub gross_total: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CeilingIdentity {
    pub amount: String,
    pub currency_code: String,
    pub basis: CeilingBasis,
    pub ceiling_revision: i64,
    pub ceiling_identity_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoCeilingReview {
    pub reviewed: bool,
    pub reason: String,
    pub actor_kind: String,
    pub actor_id: Uuid,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuoteSnapshotV1 {
    pub quote_id: Uuid,
    pub project_id: Uuid,
    pub revision: i64,
    pub currency_code: String,
    pub currency_scale: u32,
    pub tax_mode: TaxMode,
    pub title: String,
    pub notes: Option<String>,
    pub lines: Vec<QuoteLineComputed>,
    pub net_total: String,
    pub tax_total: String,
    pub gross_total: String,
    pub ceiling: Option<CeilingIdentity>,
    pub no_ceiling_review: Option<NoCeilingReview>,
    pub fact_revision: i64,
    pub pricing_revision: i64,
    pub pricing_set_sha256: String,
}

impl QuoteSnapshotV1 {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, QuoteError> {
        build_quote_snapshot_v1(self)
    }

    pub fn content_sha256(&self) -> Result<String, QuoteError> {
        Ok(sha256_hex(&self.canonical_bytes()?))
    }
}

pub fn parse_decimal_string(raw: &str, max_scale: u32) -> Result<Decimal, QuoteError> {
    if raw.is_empty()
        || raw
            .as_bytes()
            .first()
            .is_some_and(|b| *b == b'+' || *b == b'-')
        || raw.contains('e')
        || raw.contains('E')
        || raw.starts_with('.')
        || raw.ends_with('.')
        || raw.bytes().filter(|b| *b == b'.').count() > 1
        || !raw.bytes().all(|b| b.is_ascii_digit() || b == b'.')
    {
        return Err(QuoteError::Invalid(
            "decimal must be a non-negative CNY string without exponent, sign, or float".into(),
        ));
    }
    if let Some((_, frac)) = raw.split_once('.')
        && frac.len() > max_scale as usize
    {
        return Err(QuoteError::Invalid(format!(
            "decimal scale exceeds {max_scale}"
        )));
    }
    let value = Decimal::from_str_exact(raw)
        .map_err(|_| QuoteError::Invalid("decimal is not exact".into()))?;
    if value.is_sign_negative() {
        return Err(QuoteError::Invalid("negative decimal is forbidden".into()));
    }
    Ok(value)
}

pub fn format_fixed(value: Decimal, scale: u32) -> Result<String, QuoteError> {
    if value.is_sign_negative() {
        return Err(QuoteError::Invalid("negative decimal is forbidden".into()));
    }
    let rounded = value.round_dp_with_strategy(scale, MidpointAwayFromZero);
    if rounded.is_sign_negative() {
        return Err(QuoteError::Invalid("negative decimal is forbidden".into()));
    }
    let mantissa = rounded.mantissa();
    let current_scale = rounded.scale();
    let units = if current_scale < scale {
        mantissa.checked_mul(pow10(scale - current_scale)?)
    } else if current_scale > scale {
        Some(mantissa / pow10(current_scale - scale)?)
    } else {
        Some(mantissa)
    }
    .ok_or(QuoteError::Overflow)?;
    if units < 0 {
        return Err(QuoteError::Invalid("negative decimal is forbidden".into()));
    }
    let factor = pow10(scale)?;
    let int = units / factor;
    let frac = units % factor;
    Ok(format!("{int}.{frac:0width$}", width = scale as usize))
}

fn pow10(exp: u32) -> Result<i128, QuoteError> {
    10i128.checked_pow(exp).ok_or(QuoteError::Overflow)
}

fn amount_in_range(value: Decimal) -> Result<(), QuoteError> {
    if value.is_sign_negative() {
        return Err(QuoteError::Invalid("negative amount is forbidden".into()));
    }
    let scaled = value.round_dp_with_strategy(CURRENCY_SCALE, MidpointAwayFromZero);
    let units = scaled
        .mantissa()
        .checked_mul(pow10(CURRENCY_SCALE.saturating_sub(scaled.scale()))?)
        .ok_or(QuoteError::Overflow)?;
    let normalized = if scaled.scale() > CURRENCY_SCALE {
        units / pow10(scaled.scale() - CURRENCY_SCALE)?
    } else {
        units
    };
    if normalized > MAX_AMOUNT_UNITS {
        return Err(QuoteError::Overflow);
    }
    Ok(())
}

fn qty_in_range(value: Decimal) -> Result<(), QuoteError> {
    let max = Decimal::from_str_exact(MAX_QTY).expect("max qty literal");
    if value <= Decimal::ZERO || value > max {
        return Err(QuoteError::Invalid(
            "quantity must be > 0 and <= 10^9".into(),
        ));
    }
    Ok(())
}

fn unit_price_in_range(value: Decimal) -> Result<(), QuoteError> {
    let max = Decimal::from_str_exact(MAX_UNIT_PRICE).expect("max unit price literal");
    if value < Decimal::ZERO || value > max {
        return Err(QuoteError::Invalid(
            "unit_price must be >= 0 and <= 10^12".into(),
        ));
    }
    Ok(())
}

pub fn compute_line(
    input: &QuoteLineInput,
    tax_mode: TaxMode,
) -> Result<QuoteLineComputed, QuoteError> {
    if input.ordinal < 0 {
        return Err(QuoteError::Invalid("ordinal must be >= 0".into()));
    }
    let tax_rate = parse_decimal_string(&input.tax_rate, QTY_SCALE)?;
    if tax_rate < Decimal::ZERO || tax_rate > Decimal::ONE {
        return Err(QuoteError::Invalid("tax_rate must be in [0,1]".into()));
    }
    let tax_rate_s = format_fixed(tax_rate, QTY_SCALE)?;
    match input.pricing_mode {
        PricingMode::UnitPrice => compute_unit_price_line(input, tax_mode, tax_rate, tax_rate_s),
        PricingMode::LumpSum => compute_lump_sum_line(input, tax_mode, tax_rate, tax_rate_s),
    }
}

fn compute_unit_price_line(
    input: &QuoteLineInput,
    tax_mode: TaxMode,
    tax_rate: Decimal,
    tax_rate_s: String,
) -> Result<QuoteLineComputed, QuoteError> {
    if input.entered_amount.is_some() {
        return Err(QuoteError::Invalid(
            "unit_price line must not carry entered_amount".into(),
        ));
    }
    let quantity = input
        .quantity
        .as_deref()
        .map(|raw| parse_decimal_string(raw, QTY_SCALE))
        .transpose()?;
    let unit = input
        .unit
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let unit_price = input
        .unit_price
        .as_deref()
        .map(|raw| parse_decimal_string(raw, QTY_SCALE))
        .transpose()?;
    if let Some(quantity) = quantity {
        qty_in_range(quantity)?;
    }
    if let Some(unit_price) = unit_price {
        unit_price_in_range(unit_price)?;
    }
    let complete = quantity.is_some() && unit.is_some() && unit_price.is_some();
    if !complete {
        return Ok(incomplete_line(
            input,
            quantity
                .map(|value| format_fixed(value, QTY_SCALE))
                .transpose()?,
            unit,
            unit_price
                .map(|value| format_fixed(value, QTY_SCALE))
                .transpose()?,
            None,
            tax_rate_s,
        ));
    }
    let quantity = quantity.expect("complete");
    let unit_price = unit_price.expect("complete");
    let product = quantity
        .checked_mul(unit_price)
        .ok_or(QuoteError::Overflow)?;
    ensure_numeric(product, 30, 6)?;
    let basis = product.round_dp_with_strategy(CURRENCY_SCALE, MidpointAwayFromZero);
    amount_in_range(basis)?;
    let amounts = tax_amounts(basis, tax_rate, tax_mode)?;
    Ok(QuoteLineComputed {
        id: input.id,
        ordinal: input.ordinal,
        description: input.description.clone(),
        pricing_mode: PricingMode::UnitPrice,
        complete: true,
        quantity: Some(format_fixed(quantity, QTY_SCALE)?),
        unit,
        unit_price: Some(format_fixed(unit_price, QTY_SCALE)?),
        entered_amount: None,
        tax_rate: tax_rate_s,
        basis_amount: Some(format_fixed(basis, CURRENCY_SCALE)?),
        net_amount: Some(amounts.net),
        tax_amount: Some(amounts.tax),
        gross_amount: Some(amounts.gross),
        user_confirmed: input.user_confirmed,
    })
}

fn compute_lump_sum_line(
    input: &QuoteLineInput,
    tax_mode: TaxMode,
    tax_rate: Decimal,
    tax_rate_s: String,
) -> Result<QuoteLineComputed, QuoteError> {
    if input.quantity.is_some() || input.unit.is_some() || input.unit_price.is_some() {
        return Err(QuoteError::Invalid(
            "lump_sum line must not carry quantity, unit, or unit_price".into(),
        ));
    }
    let Some(raw) = input.entered_amount.as_deref() else {
        return Ok(incomplete_line(input, None, None, None, None, tax_rate_s));
    };
    let entered = parse_decimal_string(raw, CURRENCY_SCALE)?;
    amount_in_range(entered)?;
    let amounts = tax_amounts(entered, tax_rate, tax_mode)?;
    Ok(QuoteLineComputed {
        id: input.id,
        ordinal: input.ordinal,
        description: input.description.clone(),
        pricing_mode: PricingMode::LumpSum,
        complete: true,
        quantity: None,
        unit: None,
        unit_price: None,
        entered_amount: Some(format_fixed(entered, CURRENCY_SCALE)?),
        tax_rate: tax_rate_s,
        basis_amount: Some(format_fixed(entered, CURRENCY_SCALE)?),
        net_amount: Some(amounts.net),
        tax_amount: Some(amounts.tax),
        gross_amount: Some(amounts.gross),
        user_confirmed: input.user_confirmed,
    })
}

fn incomplete_line(
    input: &QuoteLineInput,
    quantity: Option<String>,
    unit: Option<String>,
    unit_price: Option<String>,
    entered_amount: Option<String>,
    tax_rate: String,
) -> QuoteLineComputed {
    QuoteLineComputed {
        id: input.id,
        ordinal: input.ordinal,
        description: input.description.clone(),
        pricing_mode: input.pricing_mode,
        complete: false,
        quantity,
        unit,
        unit_price,
        entered_amount,
        tax_rate,
        basis_amount: None,
        net_amount: None,
        tax_amount: None,
        gross_amount: None,
        user_confirmed: input.user_confirmed,
    }
}

struct TaxAmounts {
    net: String,
    tax: String,
    gross: String,
}

fn tax_amounts(
    basis: Decimal,
    tax_rate: Decimal,
    tax_mode: TaxMode,
) -> Result<TaxAmounts, QuoteError> {
    amount_in_range(basis)?;
    let (net, tax, gross) = match tax_mode {
        TaxMode::TaxExclusive => {
            let net = basis;
            let tax = net
                .checked_mul(tax_rate)
                .ok_or(QuoteError::Overflow)?
                .round_dp_with_strategy(CURRENCY_SCALE, MidpointAwayFromZero);
            let gross = net.checked_add(tax).ok_or(QuoteError::Overflow)?;
            (net, tax, gross)
        }
        TaxMode::TaxInclusive => {
            let gross = basis;
            let divisor = Decimal::ONE
                .checked_add(tax_rate)
                .ok_or(QuoteError::Overflow)?;
            let net = if divisor.is_zero() {
                return Err(QuoteError::Invalid("tax inclusive divisor is zero".into()));
            } else {
                gross
                    .checked_div(divisor)
                    .ok_or(QuoteError::Overflow)?
                    .round_dp_with_strategy(CURRENCY_SCALE, MidpointAwayFromZero)
            };
            let tax = gross.checked_sub(net).ok_or(QuoteError::Overflow)?;
            (net, tax, gross)
        }
    };
    amount_in_range(net)?;
    amount_in_range(tax)?;
    amount_in_range(gross)?;
    Ok(TaxAmounts {
        net: format_fixed(net, CURRENCY_SCALE)?,
        tax: format_fixed(tax, CURRENCY_SCALE)?,
        gross: format_fixed(gross, CURRENCY_SCALE)?,
    })
}

fn ensure_numeric(value: Decimal, precision: u32, scale: u32) -> Result<(), QuoteError> {
    if value.is_sign_negative() {
        return Err(QuoteError::Invalid("negative decimal is forbidden".into()));
    }
    let integer_digits = precision.saturating_sub(scale);
    let max_int = pow10(integer_digits)?;
    let abs = value.abs();
    let int_part = abs.trunc();
    if int_part >= Decimal::from_i128_with_scale(max_int, 0) {
        return Err(QuoteError::Overflow);
    }
    Ok(())
}

pub fn preview_totals(lines: &[QuoteLineComputed]) -> Result<QuoteTotals, QuoteError> {
    let mut net = Decimal::ZERO;
    let mut tax = Decimal::ZERO;
    let mut gross = Decimal::ZERO;
    for line in lines {
        if !line.complete {
            continue;
        }
        net = net
            .checked_add(parse_decimal_string(
                line.net_amount
                    .as_deref()
                    .ok_or(QuoteError::IncompleteLine)?,
                CURRENCY_SCALE,
            )?)
            .ok_or(QuoteError::Overflow)?;
        tax = tax
            .checked_add(parse_decimal_string(
                line.tax_amount
                    .as_deref()
                    .ok_or(QuoteError::IncompleteLine)?,
                CURRENCY_SCALE,
            )?)
            .ok_or(QuoteError::Overflow)?;
        gross = gross
            .checked_add(parse_decimal_string(
                line.gross_amount
                    .as_deref()
                    .ok_or(QuoteError::IncompleteLine)?,
                CURRENCY_SCALE,
            )?)
            .ok_or(QuoteError::Overflow)?;
    }
    amount_in_range(net)?;
    amount_in_range(tax)?;
    amount_in_range(gross)?;
    Ok(QuoteTotals {
        net_total: format_fixed(net, CURRENCY_SCALE)?,
        tax_total: format_fixed(tax, CURRENCY_SCALE)?,
        gross_total: format_fixed(gross, CURRENCY_SCALE)?,
    })
}

pub fn normalize_title(title: &str) -> Result<String, QuoteError> {
    let trimmed = title.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_TITLE_BYTES {
        return Err(QuoteError::Invalid(
            "title must trim to 1..256 bytes".into(),
        ));
    }
    Ok(trimmed.to_string())
}

pub fn normalize_notes(notes: Option<&str>) -> Result<Option<String>, QuoteError> {
    let Some(raw) = notes else {
        return Ok(None);
    };
    if raw.trim().is_empty() {
        return Ok(None);
    }
    if raw.len() > MAX_NOTES_BYTES {
        return Err(QuoteError::Invalid("notes exceed 4096 bytes".into()));
    }
    Ok(Some(raw.to_string()))
}

pub fn assert_finalizable(lines: &[QuoteLineComputed]) -> Result<(), QuoteError> {
    if lines.is_empty() {
        return Err(QuoteError::Empty);
    }
    for line in lines {
        if line.description.trim().is_empty() {
            return Err(QuoteError::Invalid("line description is empty".into()));
        }
        if !line.complete {
            return Err(QuoteError::IncompleteLine);
        }
        if !line.user_confirmed {
            return Err(QuoteError::UnconfirmedLine);
        }
    }
    Ok(())
}

pub fn check_ceiling(
    totals: &QuoteTotals,
    ceiling: Option<&CeilingIdentity>,
    no_ceiling: Option<&NoCeilingReview>,
) -> Result<(), QuoteError> {
    match ceiling {
        Some(ceiling) => {
            if no_ceiling.is_some() {
                return Err(QuoteError::Invalid(
                    "ceiling and no_ceiling_review are mutually exclusive".into(),
                ));
            }
            if ceiling.currency_code != CURRENCY_CODE {
                return Err(QuoteError::Invalid("ceiling currency must be CNY".into()));
            }
            if ceiling.basis == CeilingBasis::Unspecified {
                return Err(QuoteError::CeilingBasisUnspecified);
            }
            let cap = parse_decimal_string(&ceiling.amount, CURRENCY_SCALE)?;
            let compared = match ceiling.basis {
                CeilingBasis::TaxInclusive => {
                    parse_decimal_string(&totals.gross_total, CURRENCY_SCALE)?
                }
                CeilingBasis::TaxExclusive => {
                    parse_decimal_string(&totals.net_total, CURRENCY_SCALE)?
                }
                CeilingBasis::Unspecified => return Err(QuoteError::CeilingBasisUnspecified),
            };
            if compared > cap {
                return Err(QuoteError::CeilingExceeded);
            }
            Ok(())
        }
        None => {
            let review = no_ceiling.ok_or_else(|| {
                QuoteError::Invalid("no-ceiling review is required when ceiling is absent".into())
            })?;
            if !review.reviewed
                || review.reason.trim().is_empty()
                || review.reason.len() > 512
                || review.actor_kind != "user"
            {
                return Err(QuoteError::Invalid(
                    "no-ceiling review must be a durable user confirmation with a bounded reason"
                        .into(),
                ));
            }
            Ok(())
        }
    }
}

/// Unique storage seam. Rust and SQL emit identical UTF-8 bytes.
pub fn build_quote_snapshot_v1(snapshot: &QuoteSnapshotV1) -> Result<Vec<u8>, QuoteError> {
    if snapshot.currency_code != CURRENCY_CODE || snapshot.currency_scale != CURRENCY_SCALE {
        return Err(QuoteError::Invalid("currency must be CNY scale 2".into()));
    }
    if snapshot.quote_id.is_nil()
        || snapshot.project_id.is_nil()
        || snapshot.revision <= 0
        || snapshot.fact_revision < 0
        || snapshot.pricing_revision < 0
        || !is_lower_sha256(&snapshot.pricing_set_sha256)
    {
        return Err(QuoteError::Invalid(
            "snapshot identities and revisions must be canonical".into(),
        ));
    }
    let title = normalize_title(&snapshot.title)?;
    let notes = normalize_notes(snapshot.notes.as_deref())?;
    assert_finalizable(&snapshot.lines)?;
    let mut lines = snapshot.lines.clone();
    lines.sort_by_key(|line| line.ordinal);
    let mut line_ids = std::collections::HashSet::new();
    for window in lines.windows(2) {
        if window[0].ordinal == window[1].ordinal {
            return Err(QuoteError::Invalid("line ordinals must be unique".into()));
        }
    }
    for line in &lines {
        if line.id.is_nil() || !line_ids.insert(line.id) {
            return Err(QuoteError::Invalid(
                "line ids must be unique non-nil UUIDs".into(),
            ));
        }
        let recomputed = compute_line(
            &QuoteLineInput {
                id: line.id,
                ordinal: line.ordinal,
                description: line.description.clone(),
                pricing_mode: line.pricing_mode,
                quantity: line.quantity.clone(),
                unit: line.unit.clone(),
                unit_price: line.unit_price.clone(),
                entered_amount: line.entered_amount.clone(),
                tax_rate: line.tax_rate.clone(),
                user_confirmed: line.user_confirmed,
            },
            snapshot.tax_mode,
        )?;
        if recomputed != *line {
            return Err(QuoteError::Invalid(
                "computed quote line differs from authoritative inputs".into(),
            ));
        }
    }
    let totals = preview_totals(&lines)?;
    if snapshot.net_total != totals.net_total
        || snapshot.tax_total != totals.tax_total
        || snapshot.gross_total != totals.gross_total
    {
        return Err(QuoteError::Invalid(
            "snapshot totals differ from recomputed lines".into(),
        ));
    }
    check_ceiling(
        &totals,
        snapshot.ceiling.as_ref(),
        snapshot.no_ceiling_review.as_ref(),
    )?;
    if snapshot.ceiling.as_ref().is_some_and(|ceiling| {
        ceiling.ceiling_revision < 0 || !is_lower_sha256(&ceiling.ceiling_identity_sha256)
    }) || snapshot
        .no_ceiling_review
        .as_ref()
        .is_some_and(|review| review.actor_id.is_nil())
    {
        return Err(QuoteError::Invalid(
            "ceiling identity or review actor is invalid".into(),
        ));
    }
    let mut out = String::new();
    out.push('{');
    write_key(&mut out, "schema_version");
    write!(out, "{SCHEMA_VERSION}").expect("write number");
    write_comma_key(&mut out, "quote_id");
    write_json_string(&mut out, &snapshot.quote_id.to_string());
    write_comma_key(&mut out, "project_id");
    write_json_string(&mut out, &snapshot.project_id.to_string());
    write_comma_key(&mut out, "revision");
    write!(out, "{}", snapshot.revision).expect("write number");
    write_comma_key(&mut out, "currency_code");
    write_json_string(&mut out, CURRENCY_CODE);
    write_comma_key(&mut out, "currency_scale");
    write!(out, "{CURRENCY_SCALE}").expect("write number");
    write_comma_key(&mut out, "tax_mode");
    write_json_string(&mut out, snapshot.tax_mode.as_str());
    write_comma_key(&mut out, "title");
    write_json_string(&mut out, &title);
    write_comma_key(&mut out, "notes");
    match notes.as_deref() {
        Some(value) => write_json_string(&mut out, value),
        None => out.push_str("null"),
    }
    write_comma_key(&mut out, "lines");
    out.push('[');
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write_line_json(&mut out, line)?;
    }
    out.push(']');
    write_comma_key(&mut out, "net_total");
    write_json_string(&mut out, &snapshot.net_total);
    write_comma_key(&mut out, "tax_total");
    write_json_string(&mut out, &snapshot.tax_total);
    write_comma_key(&mut out, "gross_total");
    write_json_string(&mut out, &snapshot.gross_total);
    write_comma_key(&mut out, "ceiling");
    match &snapshot.ceiling {
        Some(ceiling) => write_ceiling_json(&mut out, ceiling),
        None => out.push_str("null"),
    }
    write_comma_key(&mut out, "no_ceiling_review");
    match &snapshot.no_ceiling_review {
        Some(review) => write_review_json(&mut out, review),
        None => out.push_str("null"),
    }
    write_comma_key(&mut out, "fact_revision");
    write!(out, "{}", snapshot.fact_revision).expect("write number");
    write_comma_key(&mut out, "pricing_revision");
    write!(out, "{}", snapshot.pricing_revision).expect("write number");
    write_comma_key(&mut out, "pricing_set_sha256");
    write_json_string(&mut out, &snapshot.pricing_set_sha256);
    out.push('}');
    Ok(out.into_bytes())
}

fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn write_line_json(out: &mut String, line: &QuoteLineComputed) -> Result<(), QuoteError> {
    if !line.complete {
        return Err(QuoteError::IncompleteLine);
    }
    out.push('{');
    write_key(out, "id");
    write_json_string(out, &line.id.to_string());
    write_comma_key(out, "ordinal");
    write!(out, "{}", line.ordinal).expect("write number");
    write_comma_key(out, "description");
    write_json_string(out, &line.description);
    write_comma_key(out, "pricing_mode");
    write_json_string(out, line.pricing_mode.as_str());
    write_comma_key(out, "quantity");
    write_opt_string(out, line.quantity.as_deref());
    write_comma_key(out, "unit");
    write_opt_string(out, line.unit.as_deref());
    write_comma_key(out, "unit_price");
    write_opt_string(out, line.unit_price.as_deref());
    write_comma_key(out, "entered_amount");
    write_opt_string(out, line.entered_amount.as_deref());
    write_comma_key(out, "tax_rate");
    write_json_string(out, &line.tax_rate);
    write_comma_key(out, "basis_amount");
    write_json_string(
        out,
        line.basis_amount
            .as_deref()
            .ok_or(QuoteError::IncompleteLine)?,
    );
    write_comma_key(out, "net_amount");
    write_json_string(
        out,
        line.net_amount
            .as_deref()
            .ok_or(QuoteError::IncompleteLine)?,
    );
    write_comma_key(out, "tax_amount");
    write_json_string(
        out,
        line.tax_amount
            .as_deref()
            .ok_or(QuoteError::IncompleteLine)?,
    );
    write_comma_key(out, "gross_amount");
    write_json_string(
        out,
        line.gross_amount
            .as_deref()
            .ok_or(QuoteError::IncompleteLine)?,
    );
    write_comma_key(out, "user_confirmed");
    out.push_str(if line.user_confirmed { "true" } else { "false" });
    out.push('}');
    Ok(())
}

fn write_ceiling_json(out: &mut String, ceiling: &CeilingIdentity) {
    out.push('{');
    write_key(out, "amount");
    write_json_string(out, &ceiling.amount);
    write_comma_key(out, "currency_code");
    write_json_string(out, &ceiling.currency_code);
    write_comma_key(out, "basis");
    write_json_string(out, ceiling.basis.as_str());
    write_comma_key(out, "ceiling_revision");
    write!(out, "{}", ceiling.ceiling_revision).expect("write number");
    write_comma_key(out, "ceiling_identity_sha256");
    write_json_string(out, &ceiling.ceiling_identity_sha256);
    out.push('}');
}

fn write_review_json(out: &mut String, review: &NoCeilingReview) {
    out.push('{');
    write_key(out, "reviewed");
    out.push_str(if review.reviewed { "true" } else { "false" });
    write_comma_key(out, "reason");
    write_json_string(out, &review.reason);
    write_comma_key(out, "actor_kind");
    write_json_string(out, &review.actor_kind);
    write_comma_key(out, "actor_id");
    write_json_string(out, &review.actor_id.to_string());
    write_comma_key(out, "at");
    write_json_string(out, &format_utc_micros(review.at));
    out.push('}');
}

pub fn format_utc_micros(value: DateTime<Utc>) -> String {
    format!(
        "{}.{:06}Z",
        value.format("%Y-%m-%dT%H:%M:%S"),
        value.timestamp_subsec_micros()
    )
}

fn write_key(out: &mut String, key: &str) {
    write_json_string(out, key);
    out.push(':');
}

fn write_comma_key(out: &mut String, key: &str) {
    out.push(',');
    write_key(out, key);
}

fn write_opt_string(out: &mut String, value: Option<&str>) {
    match value {
        Some(value) => write_json_string(out, value),
        None => out.push_str("null"),
    }
}

pub fn write_json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn dec(raw: &str) -> Decimal {
        Decimal::from_str_exact(raw).unwrap()
    }

    fn line_unit(id: u128, ordinal: i32, qty: &str, price: &str, rate: &str) -> QuoteLineInput {
        QuoteLineInput {
            id: Uuid::from_u128(id),
            ordinal,
            description: "防火墙".into(),
            pricing_mode: PricingMode::UnitPrice,
            quantity: Some(qty.into()),
            unit: Some("套".into()),
            unit_price: Some(price.into()),
            entered_amount: None,
            tax_rate: rate.into(),
            user_confirmed: true,
        }
    }

    fn line_lump(id: u128, ordinal: i32, amount: &str, rate: &str) -> QuoteLineInput {
        QuoteLineInput {
            id: Uuid::from_u128(id),
            ordinal,
            description: "实施服务".into(),
            pricing_mode: PricingMode::LumpSum,
            quantity: None,
            unit: None,
            unit_price: None,
            entered_amount: Some(amount.into()),
            tax_rate: rate.into(),
            user_confirmed: true,
        }
    }

    #[test]
    fn rejects_signed_exponent_and_negative_zero() {
        for raw in ["+1.00", "-1.00", "-0", "-0.00", "1e2", "1E-1", ".1", "1."] {
            assert!(parse_decimal_string(raw, 2).is_err(), "{raw}");
        }
        assert_eq!(format_fixed(dec("0"), 2).unwrap(), "0.00");
        assert_eq!(format_fixed(dec("1.2"), 6).unwrap(), "1.200000");
    }

    #[test]
    fn unit_price_complete_tuple_and_half_away_rounding() {
        let line = compute_line(
            &line_unit(1, 0, "2.000000", "1.005000", "0.130000"),
            TaxMode::TaxExclusive,
        )
        .unwrap();
        assert!(line.complete);
        assert_eq!(line.basis_amount.as_deref(), Some("2.01"));
        assert_eq!(line.net_amount.as_deref(), Some("2.01"));
        assert_eq!(line.tax_amount.as_deref(), Some("0.26"));
        assert_eq!(line.gross_amount.as_deref(), Some("2.27"));
    }

    #[test]
    fn unit_price_partial_tuple_remains_an_editable_incomplete_line() {
        let mut input = line_unit(11, 0, "2.000000", "600.000000", "0.130000");
        input.unit = None;
        input.unit_price = None;
        input.user_confirmed = false;

        let line = compute_line(&input, TaxMode::TaxExclusive).unwrap();

        assert!(!line.complete);
        assert_eq!(line.quantity.as_deref(), Some("2.000000"));
        assert_eq!(line.unit, None);
        assert_eq!(line.unit_price, None);
        assert_eq!(line.basis_amount, None);
        assert!(!line.user_confirmed);
    }

    #[test]
    fn lump_sum_forbids_quantity_fields() {
        let mut input = line_lump(2, 0, "100.00", "0.130000");
        input.quantity = Some("1.000000".into());
        assert!(compute_line(&input, TaxMode::TaxExclusive).is_err());
    }

    #[test]
    fn inclusive_tax_uses_half_away_from_zero() {
        let line = compute_line(
            &line_lump(3, 0, "100.00", "0.130000"),
            TaxMode::TaxInclusive,
        )
        .unwrap();
        assert_eq!(line.basis_amount.as_deref(), Some("100.00"));
        assert_eq!(line.net_amount.as_deref(), Some("88.50"));
        assert_eq!(line.tax_amount.as_deref(), Some("11.50"));
        assert_eq!(line.gross_amount.as_deref(), Some("100.00"));
    }

    #[test]
    fn totals_sum_already_rounded_line_amounts() {
        let a = compute_line(&line_lump(1, 0, "0.15", "0.130000"), TaxMode::TaxExclusive).unwrap();
        let b = compute_line(&line_lump(2, 1, "0.15", "0.130000"), TaxMode::TaxExclusive).unwrap();
        assert_eq!(a.tax_amount.as_deref(), Some("0.02"));
        let totals = preview_totals(&[a, b]).unwrap();
        assert_eq!(totals.net_total, "0.30");
        assert_eq!(totals.tax_total, "0.04");
        assert_eq!(totals.gross_total, "0.34");
    }

    #[test]
    fn overflow_on_unit_price_product() {
        let err = compute_line(
            &line_unit(
                9,
                0,
                "1000000000.000000",
                "1000000000000.000000",
                "0.000000",
            ),
            TaxMode::TaxExclusive,
        )
        .unwrap_err();
        assert_eq!(err, QuoteError::Overflow);
    }

    #[test]
    fn numeric_20_2_maximum_matches_postgres() {
        let maximum = compute_line(
            &line_lump(10, 0, "999999999999999999.99", "0.000000"),
            TaxMode::TaxExclusive,
        )
        .expect("numeric(20,2) maximum must be accepted by Rust and PostgreSQL");
        assert_eq!(
            maximum.gross_amount.as_deref(),
            Some("999999999999999999.99")
        );
        let overflow = compute_line(
            &line_lump(11, 0, "1000000000000000000.00", "0.000000"),
            TaxMode::TaxExclusive,
        )
        .unwrap_err();
        assert_eq!(overflow, QuoteError::Overflow);
    }

    #[test]
    fn ceiling_matrix() {
        let totals = QuoteTotals {
            net_total: "100.00".into(),
            tax_total: "13.00".into(),
            gross_total: "113.00".into(),
        };
        let mut ceiling = CeilingIdentity {
            amount: "113.00".into(),
            currency_code: "CNY".into(),
            basis: CeilingBasis::TaxInclusive,
            ceiling_revision: 3,
            ceiling_identity_sha256: "a".repeat(64),
        };
        assert!(check_ceiling(&totals, Some(&ceiling), None).is_ok());
        ceiling.amount = "112.99".into();
        assert_eq!(
            check_ceiling(&totals, Some(&ceiling), None).unwrap_err(),
            QuoteError::CeilingExceeded
        );
        ceiling.amount = "100.00".into();
        ceiling.basis = CeilingBasis::TaxExclusive;
        assert!(check_ceiling(&totals, Some(&ceiling), None).is_ok());
        ceiling.basis = CeilingBasis::Unspecified;
        assert_eq!(
            check_ceiling(&totals, Some(&ceiling), None).unwrap_err(),
            QuoteError::CeilingBasisUnspecified
        );
    }

    #[test]
    fn eligibility_is_one_way() {
        assert!(
            Eligibility::Eligible
                .transition(Eligibility::IneligibleCeilingChanged)
                .is_ok()
        );
        assert!(
            Eligibility::IneligibleCeilingChanged
                .transition(Eligibility::IneligiblePricingChanged)
                .is_err()
        );
        assert!(
            Eligibility::IneligibleCeilingChanged
                .transition(Eligibility::IneligibleMultipleInputsChanged)
                .is_ok()
        );
        assert!(
            Eligibility::IneligibleMultipleInputsChanged
                .transition(Eligibility::Eligible)
                .is_err()
        );
        assert_eq!(
            Eligibility::Eligible.after_input_change(true, true),
            Some(Eligibility::IneligibleMultipleInputsChanged)
        );
    }

    #[test]
    fn canonical_bytes_cover_chinese_escape_notes_and_ceiling() {
        let quote_id = Uuid::from_u128(0x11111111_1111_1111_1111_111111111111);
        let project_id = Uuid::from_u128(0x22222222_2222_2222_2222_222222222222);
        let line = compute_line(
            &QuoteLineInput {
                id: Uuid::from_u128(0x33333333_3333_3333_3333_333333333333),
                ordinal: 0,
                description: "含\"引号\"与\\反斜杠".into(),
                pricing_mode: PricingMode::LumpSum,
                quantity: None,
                unit: None,
                unit_price: None,
                entered_amount: Some("100.00".into()),
                tax_rate: "0.130000".into(),
                user_confirmed: true,
            },
            TaxMode::TaxExclusive,
        )
        .unwrap();
        let snapshot = QuoteSnapshotV1 {
            quote_id,
            project_id,
            revision: 1,
            currency_code: "CNY".into(),
            currency_scale: 2,
            tax_mode: TaxMode::TaxExclusive,
            title: "投标报价一览表".into(),
            notes: Some("备注\"A\"\\B".into()),
            lines: vec![line],
            net_total: "100.00".into(),
            tax_total: "13.00".into(),
            gross_total: "113.00".into(),
            ceiling: Some(CeilingIdentity {
                amount: "1000000.00".into(),
                currency_code: "CNY".into(),
                basis: CeilingBasis::TaxInclusive,
                ceiling_revision: 3,
                ceiling_identity_sha256:
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
            }),
            no_ceiling_review: None,
            fact_revision: 4,
            pricing_revision: 2,
            pricing_set_sha256: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
                .into(),
        };
        let bytes = build_quote_snapshot_v1(&snapshot).unwrap();
        let expected = concat!(
            r#"{"schema_version":1,"#,
            r#""quote_id":"11111111-1111-1111-1111-111111111111","#,
            r#""project_id":"22222222-2222-2222-2222-222222222222","#,
            r#""revision":1,"currency_code":"CNY","currency_scale":2,"#,
            r#""tax_mode":"tax_exclusive","title":"投标报价一览表","#,
            r#""notes":"备注\"A\"\\B","lines":[{"id":"33333333-3333-3333-3333-333333333333","#,
            r#""ordinal":0,"description":"含\"引号\"与\\反斜杠","pricing_mode":"lump_sum","#,
            r#""quantity":null,"unit":null,"unit_price":null,"entered_amount":"100.00","#,
            r#""tax_rate":"0.130000","basis_amount":"100.00","net_amount":"100.00","#,
            r#""tax_amount":"13.00","gross_amount":"113.00","user_confirmed":true}],"#,
            r#""net_total":"100.00","tax_total":"13.00","gross_total":"113.00","#,
            r#""ceiling":{"amount":"1000000.00","currency_code":"CNY","basis":"tax_inclusive","#,
            r#""ceiling_revision":3,"ceiling_identity_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"#,
            r#""no_ceiling_review":null,"fact_revision":4,"pricing_revision":2,"#,
            r#""pricing_set_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}"#
        );
        assert_eq!(String::from_utf8(bytes.clone()).unwrap(), expected);
        assert_eq!(sha256_hex(&bytes), sha256_hex(expected.as_bytes()));
    }

    #[test]
    fn canonical_bytes_null_notes_and_no_ceiling_review() {
        let at = Utc.with_ymd_and_hms(2026, 8, 23, 1, 2, 3).unwrap()
            + chrono::Duration::microseconds(456);
        let line =
            compute_line(&line_lump(1, 0, "10.00", "0.000000"), TaxMode::TaxInclusive).unwrap();
        let mut snapshot = QuoteSnapshotV1 {
            quote_id: Uuid::from_u128(1),
            project_id: Uuid::from_u128(2),
            revision: 7,
            currency_code: "CNY".into(),
            currency_scale: 2,
            tax_mode: TaxMode::TaxInclusive,
            title: "报价".into(),
            notes: None,
            lines: vec![line],
            net_total: "10.00".into(),
            tax_total: "0.00".into(),
            gross_total: "10.00".into(),
            ceiling: None,
            no_ceiling_review: Some(NoCeilingReview {
                reviewed: true,
                reason: "招标文件未设置最高限价，已人工复核".into(),
                actor_kind: "user".into(),
                actor_id: Uuid::from_u128(9),
                at,
            }),
            fact_revision: 0,
            pricing_revision: 0,
            pricing_set_sha256: "cc".repeat(32),
        };
        let text = String::from_utf8(build_quote_snapshot_v1(&snapshot).unwrap()).unwrap();
        assert!(text.contains(r#""notes":null"#));
        assert!(text.contains(r#""no_ceiling_review":{"reviewed":true"#));
        assert!(text.contains("2026-08-23T01:02:03.000456Z"));
        assert!(!text.contains("ceiling_revision"));

        snapshot.lines[0].gross_amount = Some("999.99".into());
        assert!(build_quote_snapshot_v1(&snapshot).is_err());
        snapshot.lines[0].gross_amount = Some("10.00".into());
        snapshot.lines[0].user_confirmed = false;
        assert_eq!(
            build_quote_snapshot_v1(&snapshot).unwrap_err(),
            QuoteError::UnconfirmedLine
        );
    }

    #[test]
    fn finalize_rejects_empty_incomplete_and_unconfirmed() {
        assert_eq!(assert_finalizable(&[]).unwrap_err(), QuoteError::Empty);
        let mut line =
            compute_line(&line_lump(1, 0, "10.00", "0.000000"), TaxMode::TaxExclusive).unwrap();
        line.user_confirmed = false;
        assert_eq!(
            assert_finalizable(&[line.clone()]).unwrap_err(),
            QuoteError::UnconfirmedLine
        );
        line.user_confirmed = true;
        line.complete = false;
        assert_eq!(
            assert_finalizable(&[line]).unwrap_err(),
            QuoteError::IncompleteLine
        );
    }
}
