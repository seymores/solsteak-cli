//! Classify an input and discover its current native stake account set.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::domain::{
    Account, DECODER_VERSION, DiscoveryCoverage, ReportError, STAKE_PROGRAM, Source,
    authority_relationship, decode_stake, validate_address,
};
use crate::helius::{Helius, RequestContext, RpcError};
use crate::rewards::observation_source;

#[derive(Debug)]
pub struct Discovery {
    pub selection: String,
    pub input_exists: Option<bool>,
    pub accounts: Vec<Account>,
    pub coverage: DiscoveryCoverage,
    pub sources: Vec<Source>,
    pub errors: Vec<ReportError>,
}

pub fn discover(
    client: &Helius,
    address: &str,
    context: &RequestContext,
) -> Result<Discovery, RpcError> {
    if !validate_address(address) {
        return Err(RpcError {
            code: "INVALID_ADDRESS",
            message: "Address must decode to exactly 32 bytes.",
        });
    }
    let input = client.call(
        "getAccountInfo",
        json!([address,{"encoding":"base64","commitment":"finalized"}]),
        context,
    )?;
    let (input_slot, value) = contextual_value(&input)?;
    if !value.is_null()
        && (!value.is_object() || !value["owner"].as_str().is_some_and(validate_address))
    {
        return Err(RpcError::invalid());
    }
    let mut result = Discovery {
        selection: "authority".into(),
        input_exists: Some(!value.is_null()),
        accounts: Vec::new(),
        coverage: DiscoveryCoverage {
            displayed: "complete".into(),
            latest_attempt: "complete".into(),
            staker_query: "not_queried".into(),
            withdrawer_query: "not_queried".into(),
        },
        sources: vec![observation_source(
            "input".into(),
            Some(input_slot),
            Some("finalized"),
            Some(DECODER_VERSION),
        )?],
        errors: Vec::new(),
    };
    if value["owner"] == STAKE_PROGRAM {
        result.selection = "direct".into();
        result.coverage.staker_query = "not_applicable".into();
        result.coverage.withdrawer_query = "not_applicable".into();
        let account = decode_stake(address, value, None, "input")
            .unwrap_or_else(|_| unsupported(address, value, "input"));
        if account.state == "unsupported" {
            result.errors.push(invalid(
                Some(address),
                "The input stake account layout or fields could not be validated.",
            ));
            result.coverage.displayed = "partial".into();
            result.coverage.latest_attempt = "partial".into();
        }
        result.accounts.push(account);
        return Ok(result);
    }

    let mut selected: BTreeMap<String, (u64, Account)> = BTreeMap::new();
    let mut queries = ["not_queried"; 2];
    for (index, (id, offset)) in [("staker", 12), ("withdrawer", 44)].into_iter().enumerate() {
        if let Err(error) = context.check() {
            result.errors.push(report_error(&error, None));
            continue;
        }
        let response = client.call("getProgramAccounts",json!([STAKE_PROGRAM,{"encoding":"base64","commitment":"finalized","withContext":true,"filters":[{"memcmp":{"offset":offset,"bytes":address}}]}]),context);
        let response = match response {
            Ok(response) => response,
            Err(error) => {
                queries[index] = "failed";
                result.errors.push(report_error(&error, None));
                continue;
            }
        };
        let parsed = contextual_value(&response).and_then(|(slot, rows)| {
            rows.as_array()
                .map(|rows| (slot, rows))
                .ok_or_else(RpcError::invalid)
        });
        let (slot, rows) = match parsed {
            Ok(parsed) => parsed,
            Err(error) => {
                queries[index] = "failed";
                result.errors.push(report_error(&error, None));
                continue;
            }
        };
        result.sources.push(observation_source(
            id.into(),
            Some(slot),
            Some("finalized"),
            Some(DECODER_VERSION),
        )?);
        queries[index] = "succeeded";
        for row in rows {
            if let Err(error) = context.check() {
                queries[index] = "failed";
                result.errors.push(report_error(&error, None));
                break;
            }
            let Some(key) = row["pubkey"].as_str().filter(|key| validate_address(key)) else {
                queries[index] = "failed";
                result.errors.push(invalid(
                    None,
                    "Discovery returned an invalid account identity.",
                ));
                continue;
            };
            let mut account = decode_stake(key, &row["account"], None, id)
                .unwrap_or_else(|_| unsupported(key, &row["account"], id));
            if account.state == "unsupported" {
                queries[index] = "failed";
                result.errors.push(invalid(
                    Some(key),
                    "A discovered account owner, layout or fields could not be validated.",
                ));
            } else {
                let matched = if index == 0 {
                    account.staker.as_deref()
                } else {
                    account.withdrawer.as_deref()
                };
                if matched != Some(address) {
                    queries[index] = "failed";
                    result.errors.push(invalid(
                        Some(key),
                        "A discovered account did not match the requested authority.",
                    ));
                    continue;
                }
                account.relationship = authority_relationship(&account, address).map(str::to_owned);
            }
            if let Some((old_slot, old)) = selected.get_mut(key) {
                // Compare actual decoded observations, never union relationships
                // inferred from which queries happened to return the key.
                let mut comparable = account.clone();
                comparable.source_id = old.source_id.clone();
                if *old != comparable {
                    queries[index] = "failed";
                    result.errors.push(invalid(
                        Some(key),
                        "Authority queries returned conflicting account observations.",
                    ));
                }
                if slot > *old_slot {
                    *old_slot = slot;
                    *old = account;
                }
            } else {
                selected.insert(key.to_owned(), (slot, account));
            }
        }
    }
    result.accounts = selected.into_values().map(|(_, account)| account).collect();
    result.accounts.sort_by(|a, b| {
        let a_balance = a
            .balance_lamports
            .as_deref()
            .and_then(|n| n.parse::<u64>().ok());
        let b_balance = b
            .balance_lamports
            .as_deref()
            .and_then(|n| n.parse::<u64>().ok());
        b_balance
            .cmp(&a_balance)
            .then_with(|| a.address.cmp(&b.address))
    });
    result.coverage.staker_query = queries[0].into();
    result.coverage.withdrawer_query = queries[1].into();
    if !result.errors.is_empty() {
        let any_observation = !result.accounts.is_empty() || queries.contains(&"succeeded");
        result.coverage.displayed = if any_observation {
            "partial"
        } else {
            "not_available"
        }
        .into();
        result.coverage.latest_attempt = if any_observation { "partial" } else { "failed" }.into();
    }
    Ok(result)
}

fn contextual_value(value: &Value) -> Result<(u64, &Value), RpcError> {
    let slot = value["context"]["slot"]
        .as_u64()
        .ok_or_else(RpcError::invalid)?;
    let result = value.get("value").ok_or_else(RpcError::invalid)?;
    Ok((slot, result))
}

fn report_error(error: &RpcError, address: Option<&str>) -> ReportError {
    ReportError {
        code: error.code.into(),
        message: error.message.into(),
        address: address.map(str::to_owned),
    }
}

fn invalid(address: Option<&str>, message: &str) -> ReportError {
    ReportError {
        code: "INVALID_RESPONSE".into(),
        message: message.into(),
        address: address.map(str::to_owned),
    }
}

fn unsupported(address: &str, value: &Value, source_id: &str) -> Account {
    Account {
        address: address.into(),
        relationship: None,
        state: "unsupported".into(),
        balance_lamports: value["lamports"].as_u64().map(|n| n.to_string()),
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
    }
}
