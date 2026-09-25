use crate::{domain::*, require, Error, Result};
use rust_decimal::Decimal;
use serde::Serialize;
use std::collections::BTreeMap;

pub fn amount(value: &str) -> Result<Decimal> {
    require(
        !value.is_empty() && value.len() <= 30 && value.trim() == value,
        "Amount must be an exact decimal string",
    )?;
    require(
        value
            .chars()
            .enumerate()
            .all(|(i, c)| c.is_ascii_digit() || c == '.' || (i == 0 && c == '-')),
        "Amount contains an unsupported character",
    )?;
    let parsed = Decimal::from_str_exact(value)
        .map_err(|_| Error::Validation("Invalid decimal amount".into()))?;
    require(parsed.scale() <= 8, "Amount exceeds eight decimal places")?;
    Ok(parsed)
}
/// Add decimal money without the decimal engine's implicit rescaling/rounding.
/// Inputs and outputs have at most eight decimal places. Aligning a 96-bit
/// mantissa by at most 10^8, then adding two aligned values, fits signed i128.
/// Only exact trailing zeros may be removed to fit Decimal's 96-bit mantissa.
pub fn exact_add(left: Decimal, right: Decimal) -> Result<Decimal> {
    require(
        left.scale() <= 8 && right.scale() <= 8,
        "Exact money exceeds eight decimal places",
    )?;
    let mut scale = left.scale().max(right.scale());
    let align = |value: Decimal| -> Result<i128> {
        value
            .mantissa()
            .checked_mul(10i128.pow(scale - value.scale()))
            .ok_or_else(|| Error::Validation("Exact money alignment overflow".into()))
    };
    let mut mantissa = align(left)?
        .checked_add(align(right)?)
        .ok_or_else(|| Error::Validation("Exact money addition overflow".into()))?;
    let maximum = Decimal::MAX.mantissa();
    while (mantissa > maximum || mantissa < -maximum) && scale > 0 && mantissa % 10 == 0 {
        mantissa /= 10;
        scale -= 1;
    }
    Decimal::try_from_i128_with_scale(mantissa, scale).map_err(|_| {
        Error::Validation("Exact money result cannot be represented without rounding".into())
    })
}
pub fn exact_sub(left: Decimal, right: Decimal) -> Result<Decimal> {
    exact_add(left, -right)
}
pub fn date(value: &str) -> Result<()> {
    require(
        value.len() == 10
            && value.bytes().enumerate().all(|(i, byte)| {
                if i == 4 || i == 7 {
                    byte == b'-'
                } else {
                    byte.is_ascii_digit()
                }
            }),
        "Date must use canonical YYYY-MM-DD",
    )?;
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| Error::Validation("Invalid calendar date".into()))?;
    Ok(())
}
pub fn validate_transaction(t: &Transaction) -> Result<()> {
    amount(&t.amount)?;
    date(&t.date)?;
    if let Some(v) = &t.balance {
        amount(v)?;
    }
    if let Some(v) = &t.posting_date {
        date(v)?;
    }
    require(
        t.currency.len() == 3 && t.currency.bytes().all(|c| c.is_ascii_uppercase()),
        "Currency must be three uppercase letters",
    )?;
    require(
        !t.account.trim().is_empty() && !t.description.trim().is_empty(),
        "Account and original description are required",
    )
}
/// A transfer excludes money from spending totals only when both reviewed rows
/// identify each other and exactly offset within one currency across accounts.
pub(crate) fn verified_transfer_peer<'a>(
    t: &Transaction,
    lookup: &BTreeMap<&str, &'a Transaction>,
) -> Result<Option<&'a Transaction>> {
    let Some(peer) = t
        .transfer_peer
        .as_deref()
        .and_then(|id| lookup.get(id))
        .copied()
    else {
        return Ok(None);
    };
    let value = amount(&t.amount)?;
    Ok((peer.id != t.id
        && peer.transfer_peer.as_deref() == Some(&t.id)
        && t.review == ReviewState::Accepted
        && peer.review == ReviewState::Accepted
        && t.account != peer.account
        && t.currency == peer.currency
        && !value.is_zero()
        && amount(&peer.amount)? == -value)
        .then_some(peer))
}
#[derive(Debug, Serialize)]
pub struct Total {
    pub currency: String,
    pub credits: String,
    pub debits: String,
    pub net: String,
    pub transaction_ids: Vec<String>,
    pub excluded_transfer_ids: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct Analysis {
    pub totals: Vec<Total>,
    pub pending: usize,
    pub duplicate_candidates: usize,
    pub balance_checks: Vec<BalanceCheck>,
}
#[derive(Debug, Serialize)]
pub struct BalanceCheck {
    pub transaction_id: String,
    pub previous_id: String,
    /// Every source row contributing to the movement since the previous balance.
    pub transaction_ids: Vec<String>,
    pub difference: String,
    pub reconciled: bool,
}
struct BalanceWindow<'a> {
    previous_id: &'a str,
    balance: Decimal,
    movement: Decimal,
    transaction_ids: Vec<String>,
}
pub fn analyse(transactions: &[Transaction]) -> Result<Analysis> {
    let mut lookup = BTreeMap::new();
    for t in transactions {
        validate_transaction(t)?;
        require(
            !t.id.is_empty() && lookup.insert(t.id.as_str(), t).is_none(),
            "Transaction identifiers must be nonempty and unique",
        )?;
    }
    for t in transactions {
        require(
            t.transfer_peer.is_none() || verified_transfer_peer(t, &lookup)?.is_some(),
            "Transfer marker requires a verified reciprocal pair in the complete transaction snapshot",
        )?;
    }
    let mut groups: BTreeMap<String, (Decimal, Decimal, Vec<String>, Vec<String>)> =
        BTreeMap::new();
    let mut previous: BTreeMap<(&str, &str, &str), BalanceWindow> = BTreeMap::new();
    let mut checks = vec![];
    for t in transactions {
        let a = amount(&t.amount)?;
        // Source order is authoritative. Accumulate rows without a balance;
        // comparing only the next balanced row would report a false discrepancy.
        let key = (
            t.account.as_str(),
            t.currency.as_str(),
            t.anchor.evidence_id(),
        );
        if let Some(window) = previous.get_mut(&key) {
            window.movement = exact_add(window.movement, a)?;
            window.transaction_ids.push(t.id.clone());
        }
        if let Some(balance) = &t.balance {
            let b = amount(balance)?;
            if let Some(window) = previous.get(&key) {
                let diff = exact_sub(exact_sub(b, window.balance)?, window.movement)?;
                checks.push(BalanceCheck {
                    transaction_id: t.id.clone(),
                    previous_id: window.previous_id.to_string(),
                    transaction_ids: window.transaction_ids.clone(),
                    difference: diff.to_string(),
                    reconciled: diff.is_zero(),
                });
            }
            previous.insert(
                key,
                BalanceWindow {
                    previous_id: &t.id,
                    balance: b,
                    movement: Decimal::ZERO,
                    transaction_ids: vec![],
                },
            );
        }
        if t.review != ReviewState::Accepted {
            continue;
        }
        let g = groups.entry(t.currency.clone()).or_default();
        if t.transfer_peer.is_some() {
            g.3.push(t.id.clone());
            continue;
        }
        if a.is_sign_negative() {
            g.1 = exact_add(g.1, -a)?;
        } else {
            g.0 = exact_add(g.0, a)?;
        }
        g.2.push(t.id.clone());
    }
    let mut totals = vec![];
    for (currency, (credits, debits, ids, excluded)) in groups {
        let net = exact_sub(credits, debits)?;
        totals.push(Total {
            currency,
            credits: credits.to_string(),
            debits: debits.to_string(),
            net: net.to_string(),
            transaction_ids: ids,
            excluded_transfer_ids: excluded,
        });
    }
    Ok(Analysis {
        totals,
        pending: transactions
            .iter()
            .filter(|t| t.review == ReviewState::Pending)
            .count(),
        duplicate_candidates: transactions
            .iter()
            .filter(|t| !t.duplicate_candidates.is_empty())
            .count(),
        balance_checks: checks,
    })
}
#[derive(Debug, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Proximity {
    Resolved {
        geodesic_m: f64,
        minimum_m: f64,
        maximum_m: f64,
        band: String,
    },
    Uncertain {
        reason: String,
    },
}
pub fn proximity(
    transaction_date: &str,
    address: &AddressAssociation,
    candidates: &[MerchantLocation],
    band_m: f64,
) -> Result<Proximity> {
    date(transaction_date)?;
    require(
        band_m.is_finite() && band_m > 0.0,
        "Distance band must be positive",
    )?;
    validate_coordinates(address.latitude, address.longitude, address.uncertainty_m)?;
    if transaction_date < address.valid_from.as_str()
        || address
            .valid_to
            .as_deref()
            .is_some_and(|d| transaction_date > d)
    {
        return Ok(Proximity::Uncertain {
            reason: "No applicable historical address".into(),
        });
    }
    let selected: Vec<_> = candidates
        .iter()
        .filter(|c| c.review == ReviewState::Accepted)
        .collect();
    if selected.len() != 1 {
        return Ok(Proximity::Uncertain {
            reason: "Exactly one reviewed branch is required; distance cannot select a branch"
                .into(),
        });
    }
    let c = selected[0];
    if !matches!(c.channel, Channel::InPerson) {
        return Ok(Proximity::Uncertain {
            reason: "Online or unknown transaction channel".into(),
        });
    }
    if c.valid_from.as_deref().is_none_or(|d| transaction_date < d)
        || c.valid_to.as_deref().is_some_and(|d| transaction_date > d)
    {
        return Ok(Proximity::Uncertain {
            reason: "Branch validity does not establish a location at transaction date".into(),
        });
    }
    let (Some(lat), Some(lon)) = (c.latitude, c.longitude) else {
        return Ok(Proximity::Uncertain {
            reason: "Branch coordinates unresolved".into(),
        });
    };
    validate_coordinates(lat, lon, c.uncertainty_m)?;
    let (a, b) = (address.latitude.to_radians(), lat.to_radians());
    let h = ((b - a) / 2.0).sin().powi(2)
        + a.cos() * b.cos() * ((lon - address.longitude).to_radians() / 2.0).sin().powi(2);
    let distance = 6_371_008.8 * 2.0 * h.clamp(0.0, 1.0).sqrt().asin();
    let uncertainty = address.uncertainty_m + c.uncertainty_m;
    let (min, max) = ((distance - uncertainty).max(0.0), distance + uncertainty);
    Ok(Proximity::Resolved {
        geodesic_m: distance,
        minimum_m: min,
        maximum_m: max,
        band: if max <= band_m {
            "within"
        } else if min > band_m {
            "outside"
        } else {
            "uncertain"
        }
        .into(),
    })
}
pub fn validate_coordinates(lat: f64, lon: f64, uncertainty: f64) -> Result<()> {
    require(
        lat.is_finite()
            && lon.is_finite()
            && uncertainty.is_finite()
            && (-90.0..=90.0).contains(&lat)
            && (-180.0..=180.0).contains(&lon)
            && uncertainty >= 0.0,
        "Invalid coordinates or uncertainty",
    )
}
