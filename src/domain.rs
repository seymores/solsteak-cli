//! Report values and native stake decoding, without I/O or activation estimates.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const STAKE_PROGRAM: &str = "Stake11111111111111111111111111111111111111";
pub const DECODER_VERSION: &str = "stake-v1";

// Decimal strings are the wire representation. Arithmetic passes through checked
// u64/u128 conversion below; never through JSON floating-point values.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Report {
    pub schema_version: u32,
    pub input: Option<Input>,
    pub network: Network,
    pub generated_at: Option<u64>,
    pub status: String,
    pub data: Option<Data>,
    pub coverage: Option<Coverage>,
    pub sources: Vec<Source>,
    pub warnings: Vec<Finding>,
    pub errors: Vec<ReportError>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub address: String,
    pub epochs: u16,
    pub offline: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Network {
    pub cluster: String,
    pub genesis_hash: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Data {
    pub selection: String,
    pub input_exists: Option<bool>,
    pub epoch: String,
    pub epoch_kind: String,
    pub requested_epochs: Vec<String>,
    pub stale: bool,
    pub snapshot_kind: String,
    pub summary: Summary,
    pub accounts: Vec<Account>,
    pub validators: Vec<Validator>,
    pub rewards: Vec<EpochReward>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub account_count: usize,
    pub validator_count: usize,
    pub balance_lamports: Option<String>,
    pub withdraw_authority_lamports: Option<String>,
    pub staker_only_lamports: Option<String>,
    pub delegated_lamports: Option<String>,
    pub latest_reward_lamports: Option<String>,
    pub amount_scope: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Account {
    pub address: String,
    pub relationship: Option<String>,
    pub state: String,
    pub balance_lamports: Option<String>,
    pub delegated_lamports: Option<String>,
    pub rent_reserve_lamports: Option<String>,
    pub undelegated_lamports: Option<String>,
    pub effective_lamports: Option<()>,
    pub activating_lamports: Option<()>,
    pub deactivating_lamports: Option<()>,
    pub staker: Option<String>,
    pub withdrawer: Option<String>,
    pub vote_address: Option<String>,
    pub activation_epoch: Option<String>,
    pub deactivation_epoch: Option<String>,
    pub lockup: Option<Lockup>,
    pub source_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lockup {
    pub epoch: String,
    pub unix_timestamp: String,
    pub custodian: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Validator {
    pub vote_address: String,
    pub name: Option<String>,
    pub commission: Option<u8>,
    pub state: String,
    pub delegated_lamports: Option<String>,
    pub concentration_denominator_lamports: Option<String>,
    pub source_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RewardRecord {
    pub epoch: String,
    pub amount_lamports: String,
    pub post_balance_lamports: String,
    pub effective_slot: String,
    pub commission: Option<u8>,
    pub source_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RewardEntry {
    pub address: String,
    pub state: String,
    pub latest_attempt: String,
    pub record: Option<RewardRecord>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpochReward {
    pub epoch: String,
    pub subtotal_lamports: Option<String>,
    pub entries: Vec<RewardEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    pub discovery: DiscoveryCoverage,
    pub validators: String,
    pub rewards: Vec<RewardCoverage>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscoveryCoverage {
    pub displayed: String,
    pub latest_attempt: String,
    pub staker_query: String,
    pub withdrawer_query: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RewardCoverage {
    pub epoch: String,
    pub recorded: usize,
    pub no_data: usize,
    pub failed: usize,
    pub not_queried: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: String,
    pub provider: String,
    pub observed_at: u64,
    pub slot: Option<String>,
    pub commitment: Option<String>,
    pub decoder_version: Option<String>,
    pub cached: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Finding {
    pub code: String,
    pub severity: String,
    pub address: Option<String>,
    pub evidence: Vec<String>,
    pub observed_at: Option<u64>,
    pub slot: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportError {
    pub code: String,
    pub message: String,
    pub address: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainError(pub &'static str);

impl fmt::Display for DomainError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for DomainError {}

pub fn validate_address(address: &str) -> bool {
    (32..=44).contains(&address.len()) && bs58::decode(address).onto(&mut [0u8; 32]) == Ok(32)
}

pub fn format_sol(lamports: u128) -> String {
    format!(
        "{}.{:09}",
        lamports / 1_000_000_000,
        lamports % 1_000_000_000
    )
}

pub fn parse_amount(value: &str) -> Result<u64, DomainError> {
    if value.is_empty()
        || !value.bytes().all(|byte| byte.is_ascii_digit())
        || (value.len() > 1 && value.starts_with('0'))
    {
        return Err(DomainError("Amount is not a canonical unsigned integer."));
    }
    value
        .parse()
        .map_err(|_| DomainError("Amount exceeds u64."))
}

pub fn checked_total(values: impl IntoIterator<Item = u128>) -> Result<u128, DomainError> {
    values.into_iter().try_fold(0u128, |sum, value| {
        sum.checked_add(value)
            .ok_or(DomainError("Total exceeds u128."))
    })
}

/// Only decoded authority equality establishes a relationship. It says nothing
/// about beneficial ownership or historical reward attribution.
pub fn authority_relationship(account: &Account, address: &str) -> Option<&'static str> {
    match (
        account.staker.as_deref() == Some(address),
        account.withdrawer.as_deref() == Some(address),
    ) {
        (true, true) => Some("both"),
        (true, false) => Some("staker"),
        (false, true) => Some("withdrawer"),
        (false, false) => None,
    }
}

/// Accepts the inner RPC account object (not its context wrapper). Only the
/// canonical 200-byte StakeStateV2 layout is supported; other layouts stay visible.
pub fn decode_stake(
    address: &str,
    account: &Value,
    relationship: Option<&str>,
    source_id: &str,
) -> Result<Account, DomainError> {
    if !validate_address(address) {
        return Err(DomainError("Invalid stake account address."));
    }
    if !matches!(relationship, None | Some("staker" | "withdrawer" | "both")) {
        return Err(DomainError("Invalid authority relationship."));
    }
    let balance = account["lamports"]
        .as_u64()
        .ok_or(DomainError("Invalid account balance."))?;
    let mut result = Account {
        address: address.into(),
        relationship: None,
        state: "unsupported".into(),
        balance_lamports: Some(balance.to_string()),
        delegated_lamports: None,
        rent_reserve_lamports: None,
        undelegated_lamports: None,
        effective_lamports: None,
        activating_lamports: None,
        deactivating_lamports: None,
        staker: None,
        withdrawer: None,
        vote_address: None,
        activation_epoch: None,
        deactivation_epoch: None,
        lockup: None,
        source_id: source_id.into(),
    };
    if account["owner"].as_str() != Some(STAKE_PROGRAM) || account["executable"] != false {
        return Ok(result);
    }
    let data = account["data"]
        .as_array()
        .ok_or(DomainError("Invalid account data encoding."))?;
    if data.len() != 2 || data[1] != "base64" {
        return Err(DomainError("Account data must use base64 encoding."));
    }
    let raw = STANDARD
        .decode(
            data[0]
                .as_str()
                .ok_or(DomainError("Invalid account data."))?,
        )
        .map_err(|_| DomainError("Invalid base64 account data."))?;
    if raw.len() != 200 {
        return Ok(result);
    }
    // Offsets verified against the upstream StakeStateV2 serialization; see docs/decoder-reference.md.
    let variant = u32::from_le_bytes(raw[..4].try_into().expect("length checked"));
    result.state = match variant {
        0 => "uninitialized",
        1 => "initialized",
        2 if raw[196] & !1 == 0 => "delegated",
        3 => "rewards_pool",
        _ => return Ok(result),
    }
    .into();
    if !matches!(variant, 1 | 2) {
        return Ok(result);
    }
    let read_u64 = |offset: usize| {
        u64::from_le_bytes(raw[offset..offset + 8].try_into().expect("layout checked"))
    };
    let read_key = |offset: usize| bs58::encode(&raw[offset..offset + 32]).into_string();
    let rent = read_u64(4);
    let principal = balance.checked_sub(rent).ok_or(DomainError(
        "Account balance is below recorded rent reserve.",
    ))?;
    result.relationship = relationship.map(str::to_owned);
    result.rent_reserve_lamports = Some(rent.to_string());
    result.staker = Some(read_key(12));
    result.withdrawer = Some(read_key(44));
    result.lockup = Some(Lockup {
        unix_timestamp: i64::from_le_bytes(raw[76..84].try_into().expect("layout checked"))
            .to_string(),
        epoch: read_u64(84).to_string(),
        custodian: read_key(92),
    });
    if variant == 1 {
        result.delegated_lamports = Some("0".into());
        result.undelegated_lamports = Some(principal.to_string());
    } else {
        result.vote_address = Some(read_key(124));
        result.delegated_lamports = Some(read_u64(156).to_string());
        result.activation_epoch = Some(read_u64(164).to_string());
        result.deactivation_epoch = Some(read_u64(172).to_string());
    }
    Ok(result)
}

fn sum_optional<'a>(
    values: impl Iterator<Item = Option<&'a str>>,
) -> Result<Option<String>, DomainError> {
    let mut total = 0u128;
    let mut known = true;
    for value in values {
        if let Some(value) = value {
            total = checked_total([total, u128::from(parse_amount(value)?)])?;
        } else {
            known = false;
        }
    }
    Ok(known.then(|| total.to_string()))
}

pub fn summarize(
    accounts: &[Account],
    selection: &str,
    latest_reward: Option<u128>,
    complete: bool,
) -> Result<Summary, DomainError> {
    if !matches!(selection, "authority" | "direct") {
        return Err(DomainError("Invalid selection mode."));
    }
    let mut unique = BTreeMap::new();
    for account in accounts {
        if let Some(previous) = unique.insert(&account.address, account)
            && previous != account
        {
            return Err(DomainError("Conflicting duplicate stake account."));
        }
    }
    let accounts: Vec<_> = unique.into_values().collect();
    let authority_known = selection == "authority"
        && accounts.iter().all(|a| {
            matches!(
                a.relationship.as_deref(),
                Some("both" | "staker" | "withdrawer")
            )
        });
    Ok(Summary {
        account_count: accounts.len(),
        validator_count: accounts
            .iter()
            .filter_map(|a| a.vote_address.as_ref())
            .collect::<BTreeSet<_>>()
            .len(),
        balance_lamports: sum_optional(accounts.iter().map(|a| a.balance_lamports.as_deref()))?,
        delegated_lamports: sum_optional(accounts.iter().map(|a| a.delegated_lamports.as_deref()))?,
        withdraw_authority_lamports: if authority_known {
            sum_optional(
                accounts
                    .iter()
                    .filter(|a| matches!(a.relationship.as_deref(), Some("both" | "withdrawer")))
                    .map(|a| a.balance_lamports.as_deref()),
            )?
        } else {
            None
        },
        staker_only_lamports: if authority_known {
            sum_optional(
                accounts
                    .iter()
                    .filter(|a| a.relationship.as_deref() == Some("staker"))
                    .map(|a| a.balance_lamports.as_deref()),
            )?
        } else {
            None
        },
        latest_reward_lamports: latest_reward.map(|value| value.to_string()),
        amount_scope: if complete { "total" } else { "subtotal" }.into(),
    })
}
