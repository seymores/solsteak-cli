//! Explicit completed-epoch rewards for the selected current account set.

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;

use crate::domain::{
    DomainError, EpochReward, RewardCoverage, RewardEntry, RewardRecord, Source, checked_total,
    parse_amount, validate_address,
};
use crate::helius::{Helius, REWARD_BATCH_SIZE, RequestContext, RpcError};

static OBSERVATION_ID: AtomicU64 = AtomicU64::new(0);

pub fn completed_epochs(current: u64, count: u16) -> Vec<u64> {
    (current.saturating_sub(u64::from(count.min(100)))..current)
        .rev()
        .collect()
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
        });
    }
    Ok((entries, source))
}

pub fn aggregate_epoch(
    epoch: u64,
    entries: Vec<RewardEntry>,
) -> Result<(EpochReward, RewardCoverage), DomainError> {
    let mut coverage = RewardCoverage {
        epoch: epoch.to_string(),
        recorded: 0,
        no_data: 0,
        failed: 0,
        not_queried: 0,
    };
    let mut addresses = BTreeSet::new();
    let mut total = 0u128;
    for entry in &entries {
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
                parse_amount(&record.post_balance_lamports)?;
                parse_amount(&record.effective_slot)?;
                total = checked_total([total, u128::from(parse_amount(&record.amount_lamports)?)])?;
                coverage.recorded += 1;
            }
            ("no_data", None) => coverage.no_data += 1,
            ("failed", None) => coverage.failed += 1,
            ("not_queried", None) => coverage.not_queried += 1,
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
    ))
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
