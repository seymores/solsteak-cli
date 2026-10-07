//! Explicit completed-epoch rewards for the selected current account set.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::json;

use crate::domain::{
    Account, Comparison, DomainError, EpochReward, ReportError, RewardCoverage, RewardEntry,
    RewardRecord, Source, annualized_percent, annualized_return, checked_total, parse_amount,
    validate_address,
};
use crate::helius::{Helius, REWARD_BATCH_SIZE, RequestContext, RpcError};

static OBSERVATION_ID: AtomicU64 = AtomicU64::new(0);

/// The reward window is fixed: the latest 30 completed epochs, newest first. The
/// first `PERIOD` are the current period and the rest the previous period.
pub const WINDOW: u64 = 30;
pub const PERIOD: usize = 15;
/// Deadline for one load or refresh of the fixed window.
pub const DEADLINE: Duration = Duration::from_secs(120);

pub fn completed_epochs(current: u64) -> Vec<u64> {
    (current.saturating_sub(WINDOW)..current).rev().collect()
}

pub fn fetch_batch(
    client: &Helius,
    addresses: &[String],
    epoch: u64,
    context: &RequestContext,
) -> Result<(Vec<RewardEntry>, Source), RpcError> {
    if addresses.is_empty()
        || addresses.len() > REWARD_BATCH_SIZE
        || addresses.iter().any(|a| !validate_address(a))
        || addresses.iter().collect::<BTreeSet<_>>().len() != addresses.len()
    {
        return Err(RpcError {
            code: "INVALID_ARGUMENTS",
            message: "Reward batches require 1-10 unique public addresses.",
        });
    }
    let result = client.call(
        "getInflationReward",
        json!([addresses, {"epoch":epoch,"commitment":"finalized"}]),
        context,
    )?;
    let rows = result
        .as_array()
        .filter(|rows| rows.len() == addresses.len())
        .ok_or_else(RpcError::invalid)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| RpcError {
            code: "INTERNAL",
            message: "Observation clock is unavailable.",
        })?
        .as_nanos();
    let serial = OBSERVATION_ID.fetch_add(1, Ordering::Relaxed);
    let source = observation_source(
        format!(
            "reward-{epoch}-{}-{stamp}-{}-{serial}",
            addresses[0],
            std::process::id()
        ),
        None,
        None,
        None,
    )?;
    let mut entries = Vec::with_capacity(rows.len());
    // This RPC has no address field: its documented positional order is the
    // identity contract. Never sort returned records independently of requests.
    for (address, row) in addresses.iter().zip(rows) {
        context.check()?;
        let record = if row.is_null() {
            None
        } else {
            if row["epoch"].as_u64() != Some(epoch) {
                return Err(RpcError::invalid());
            }
            let amount = row["amount"].as_u64().ok_or_else(RpcError::invalid)?;
            let balance = row["postBalance"].as_u64().ok_or_else(RpcError::invalid)?;
            let slot = row["effectiveSlot"]
                .as_u64()
                .ok_or_else(RpcError::invalid)?;
            let commission = match row.get("commission") {
                None | Some(serde_json::Value::Null) => None,
                Some(value) => Some(
                    value
                        .as_u64()
                        .filter(|n| *n <= 100)
                        .ok_or_else(RpcError::invalid)? as u8,
                ),
            };
            Some(RewardRecord {
                epoch: epoch.to_string(),
                amount_lamports: amount.to_string(),
                post_balance_lamports: balance.to_string(),
                effective_slot: slot.to_string(),
                commission,
                source_id: source.id.clone(),
            })
        };
        let state = if record.is_some() {
            "recorded"
        } else {
            "no_data"
        };
        entries.push(RewardEntry {
            address: address.clone(),
            state: state.into(),
            latest_attempt: state.into(),
            record,
            account_return: None,
        });
    }
    Ok((entries, source))
}

pub fn aggregate_epoch(
    epoch: u64,
    mut entries: Vec<RewardEntry>,
) -> Result<(EpochReward, RewardCoverage, Vec<ReportError>), DomainError> {
    let mut coverage = RewardCoverage {
        epoch: epoch.to_string(),
        recorded: 0,
        no_data: 0,
        failed: 0,
        not_queried: 0,
    };
    let mut addresses = BTreeSet::new();
    let mut total = 0u128;
    let mut invalid = vec![];
    for entry in &mut entries {
        if !validate_address(&entry.address) || !addresses.insert(&entry.address) {
            return Err(DomainError("Invalid or duplicate reward account."));
        }
        if !matches!(
            entry.latest_attempt.as_str(),
            "recorded" | "no_data" | "failed" | "not_queried"
        ) {
            return Err(DomainError("Invalid reward attempt state."));
        }
        match (entry.state.as_str(), &entry.record) {
            ("recorded", Some(record)) => {
                if parse_amount(&record.epoch)? != epoch
                    || record.commission.is_some_and(|n| n > 100)
                {
                    return Err(DomainError(
                        "Reward metadata does not match the requested epoch.",
                    ));
                }
                let post = parse_amount(&record.post_balance_lamports)?;
                parse_amount(&record.effective_slot)?;
                let amount = parse_amount(&record.amount_lamports)?;
                total = checked_total([total, u128::from(amount)])?;
                coverage.recorded += 1;
                // An unusable estimate stays Unknown; the recorded reward is kept.
                entry.account_return = annualized_return(amount, post).unwrap_or_else(|e| {
                    invalid.push(ReportError {
                        code: "INVALID_RESPONSE".into(),
                        message: format!("Epoch {epoch} return estimate unavailable: {}", e.0),
                        address: Some(entry.address.clone()),
                    });
                    None
                });
            }
            ("no_data", None) => {
                coverage.no_data += 1;
                entry.account_return = None;
            }
            ("failed", None) => {
                coverage.failed += 1;
                entry.account_return = None;
            }
            ("not_queried", None) => {
                coverage.not_queried += 1;
                entry.account_return = None;
            }
            _ => return Err(DomainError("Reward state and record disagree.")),
        }
    }
    let subtotal_lamports =
        (entries.is_empty() || coverage.recorded > 0).then(|| total.to_string());
    Ok((
        EpochReward {
            epoch: epoch.to_string(),
            subtotal_lamports,
            entries,
        },
        coverage,
        invalid,
    ))
}

/// Signed four-digit decimal, never `-0.0000`.
fn signed_percent(value: f64) -> String {
    match format!("{value:.4}").as_str() {
        "-0.0000" => "0.0000".into(),
        text => text.into(),
    }
}

fn recorded_record<'a>(epoch: &'a EpochReward, address: &str) -> Option<&'a RewardRecord> {
    epoch
        .entries
        .iter()
        .find(|e| e.address == address && e.state == "recorded")
        .and_then(|e| e.record.as_ref())
}

/// FR-16 for every account, in order. Only pairs whose two epochs both have a
/// recorded reward are compared. Returns invalid-response errors for results that
/// cannot be shown; those values stay Unknown.
pub fn compare(
    rewards: &[EpochReward],
    accounts: &[Account],
) -> Result<(Vec<Comparison>, Vec<ReportError>), DomainError> {
    let pairs = rewards.len().saturating_sub(PERIOD).min(PERIOD);
    let mut errors = vec![];
    let mut out = vec![];
    for account in accounts {
        let (mut current, mut previous, mut compared) = (0u128, 0u128, 0usize);
        let (mut cur_sum, mut prev_sum, mut estimated) = (0f64, 0f64, 0usize);
        for i in 0..pairs {
            let (Some(c), Some(p)) = (
                recorded_record(&rewards[i], &account.address),
                recorded_record(&rewards[PERIOD + i], &account.address),
            ) else {
                continue;
            };
            let (ca, cb) = (
                parse_amount(&c.amount_lamports)?,
                parse_amount(&c.post_balance_lamports)?,
            );
            let (pa, pb) = (
                parse_amount(&p.amount_lamports)?,
                parse_amount(&p.post_balance_lamports)?,
            );
            current = checked_total([current, u128::from(ca)])?;
            previous = checked_total([previous, u128::from(pa)])?;
            compared += 1;
            if let (Ok(Some((_, cur))), Ok(Some((_, prev)))) =
                (annualized_percent(ca, cb), annualized_percent(pa, pb))
            {
                cur_sum += cur;
                prev_sum += prev;
                estimated += 1;
            }
        }
        let mut comparison = Comparison {
            address: account.address.clone(),
            compared_pairs: compared,
            left_out_pairs: pairs - compared,
            current_subtotal_lamports: None,
            previous_subtotal_lamports: None,
            difference_lamports: None,
            percent_change: None,
            estimate_pairs: estimated,
            current_mean_estimate_percent: None,
            previous_mean_estimate_percent: None,
            estimate_difference_pp: None,
        };
        if compared > 0 {
            // Both sums are at most 15 u64 values, so they fit i128 exactly.
            let difference = current as i128 - previous as i128;
            comparison.current_subtotal_lamports = Some(current.to_string());
            comparison.previous_subtotal_lamports = Some(previous.to_string());
            comparison.difference_lamports = Some(difference.to_string());
            // Exact integer percent to four digits, rounded half away from zero; a zero
            // previous subtotal has no division and stays Unknown.
            let scaled = difference.unsigned_abs() * 1_000_000;
            if let Some(mut digits) = scaled.checked_div(previous) {
                if 2 * (scaled % previous) >= previous {
                    digits += 1;
                }
                if digits / 10_000 >= 1_000_000_000_000 {
                    errors.push(ReportError {
                        code: "INVALID_RESPONSE".into(),
                        message: "Period change is not representable.".into(),
                        address: Some(account.address.clone()),
                    });
                } else {
                    let sign = if difference < 0 && digits != 0 {
                        "-"
                    } else {
                        ""
                    };
                    comparison.percent_change =
                        Some(format!("{sign}{}.{:04}", digits / 10_000, digits % 10_000));
                }
            }
        }
        if estimated > 0 {
            let (cur, prev) = (cur_sum / estimated as f64, prev_sum / estimated as f64);
            comparison.current_mean_estimate_percent = Some(format!("{cur:.4}"));
            comparison.previous_mean_estimate_percent = Some(format!("{prev:.4}"));
            comparison.estimate_difference_pp = Some(signed_percent(cur - prev));
        }
        out.push(comparison);
    }
    Ok((out, errors))
}

pub(crate) fn observation_source(
    id: String,
    slot: Option<u64>,
    commitment: Option<&str>,
    decoder: Option<&str>,
) -> Result<Source, RpcError> {
    let observed_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| RpcError {
            code: "INTERNAL",
            message: "Observation clock is unavailable.",
        })?
        .as_secs();
    Ok(Source {
        id,
        provider: "helius".into(),
        observed_at,
        slot: slot.map(|n| n.to_string()),
        commitment: commitment.map(str::to_owned),
        decoder_version: decoder.map(str::to_owned),
        cached: false,
    })
}
