//! Current validator observations, exact delegation grouping and factual findings.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::domain::{
    Account, Coverage, DomainError, Finding, Source, Validator, checked_total, parse_amount,
    validate_address,
};
use crate::helius::{Helius, RequestContext, RpcError};
use crate::rewards::observation_source;

pub fn fetch(
    client: &Helius,
    accounts: &[Account],
    context: &RequestContext,
) -> Result<(Vec<Validator>, Source), RpcError> {
    let result = client.call(
        "getVoteAccounts",
        json!([{"commitment":"finalized","keepUnstakedDelinquents":true}]),
        context,
    )?;
    let validators = group(accounts, Some(&result)).map_err(|_| RpcError::invalid())?;
    let source = observation_source("validators".into(), None, Some("finalized"), None)?;
    Ok((validators, source))
}

/// The response is optional so unavailable validator records still produce
/// groups with exact stake amounts and Unknown current metadata.
pub fn group(
    accounts: &[Account],
    response: Option<&Value>,
) -> Result<Vec<Validator>, DomainError> {
    let mut observations = BTreeMap::new();
    if let Some(response) = response {
        for state in ["current", "delinquent"] {
            let rows = response[state]
                .as_array()
                .ok_or(DomainError("Invalid vote-account response."))?;
            for row in rows {
                let vote = row["votePubkey"]
                    .as_str()
                    .filter(|key| validate_address(key))
                    .ok_or(DomainError("Invalid vote address."))?;
                let commission = match row.get("commission") {
                    None | Some(Value::Null) => None,
                    Some(value) => Some(
                        value
                            .as_u64()
                            .filter(|n| *n <= 100)
                            .ok_or(DomainError("Invalid validator commission."))?
                            as u8,
                    ),
                };
                let name = match row.get("name") {
                    None | Some(Value::Null) => None,
                    Some(value) => Some(clean(
                        value
                            .as_str()
                            .ok_or(DomainError("Invalid validator name."))?,
                    )),
                };
                if observations
                    .insert(vote, (state, commission, name))
                    .is_some()
                {
                    return Err(DomainError("Duplicate vote-account observation."));
                }
            }
        }
    }
    let mut unique = BTreeMap::new();
    for account in accounts {
        if let Some(previous) = unique.insert(&account.address, account)
            && previous != account
        {
            return Err(DomainError("Conflicting duplicate stake account."));
        }
    }
    let mut denominator = Some(0u128);
    let mut totals: BTreeMap<&str, Option<u128>> = BTreeMap::new();
    for account in unique.into_values() {
        let delegated = account
            .delegated_lamports
            .as_deref()
            .map(parse_amount)
            .transpose()?
            .map(u128::from);
        denominator = add_known(denominator, delegated)?;
        if let Some(vote) = account.vote_address.as_deref() {
            if !validate_address(vote) {
                return Err(DomainError("Invalid delegated vote address."));
            }
            let total = totals.entry(vote).or_insert(Some(0));
            *total = add_known(*total, delegated)?;
        }
    }
    let denominator = denominator.filter(|n| *n > 0).map(|n| n.to_string());
    Ok(totals
        .into_iter()
        .map(|(vote, amount)| {
            let observation = observations.get(vote);
            Validator {
                vote_address: vote.into(),
                name: observation.and_then(|(_, _, name)| name.clone()),
                commission: observation.and_then(|(_, commission, _)| *commission),
                state: observation.map_or("unknown", |(state, _, _)| state).into(),
                delegated_lamports: amount.map(|n| n.to_string()),
                concentration_denominator_lamports: denominator.clone(),
                source_id: observation.map(|_| "validators".into()),
            }
        })
        .collect())
}

fn add_known(left: Option<u128>, right: Option<u128>) -> Result<Option<u128>, DomainError> {
    match (left, right) {
        (Some(left), Some(right)) => checked_total([left, right]).map(Some),
        _ => Ok(None),
    }
}

pub fn findings(
    accounts: &[Account],
    validators: &[Validator],
    coverage: &Coverage,
    sources: &[Source],
) -> Vec<Finding> {
    let mut result = Vec::new();
    for account in accounts {
        let source = sources.iter().find(|source| source.id == account.source_id);
        if account.state == "unsupported" {
            result.push(finding(
                "UNSUPPORTED_STATE",
                "warning",
                Some(&account.address),
                vec!["Account owner, layout or fields could not be validated.".into()],
                "This account remains visible, but unsupported fields are Unknown.",
                source,
            ));
        }
        if account.state == "initialized"
            && let Some(principal) = account
                .undelegated_lamports
                .as_deref()
                .and_then(|v| parse_amount(v).ok())
                .filter(|n| *n > 0)
        {
            result.push(finding(
                "UNDELEGATED_FUNDS",
                "info",
                Some(&account.address),
                vec![format!(
                    "Initialized account principal: {principal} lamports after the rent reserve."
                )],
                "This initialized stake account has undelegated principal.",
                source,
            ));
        }
        if let Some(epoch) = account
            .deactivation_epoch
            .as_deref()
            .and_then(|v| parse_amount(v).ok())
            .filter(|epoch| *epoch != u64::MAX)
        {
            result.push(finding(
                "DEACTIVATION_REQUESTED",
                "info",
                Some(&account.address),
                vec![format!("Recorded deactivation epoch: {epoch}.")],
                "Deactivation was requested; completion and effective stake remain Unknown.",
                source,
            ));
        }
    }
    for validator in validators {
        if validator.state == "delinquent" {
            let source = validator
                .source_id
                .as_ref()
                .and_then(|id| sources.iter().find(|source| &source.id == id));
            result.push(finding(
                "VALIDATOR_DELINQUENT",
                "warning",
                Some(&validator.vote_address),
                vec!["The provider listed this vote account as delinquent.".into()],
                "The provider currently reports this validator as delinquent.",
                source,
            ));
        }
    }
    if coverage.discovery.displayed != "complete"
        || matches!(
            coverage.discovery.latest_attempt.as_str(),
            "partial" | "failed"
        )
    {
        result.push(finding("INCOMPLETE_DISCOVERY","warning",None,vec!["One or more required discovery observations could not be validated.".into()],"Discovery is incomplete; displayed accounts do not establish the complete current set.",None));
    }
    for epoch in &coverage.rewards {
        if epoch.no_data > 0 || epoch.failed > 0 || epoch.not_queried > 0 {
            result.push(finding("INCOMPLETE_REWARDS","warning",None,vec![format!("Epoch {}: {} no-data, {} failed, {} not queried.",clean(&epoch.epoch),epoch.no_data,epoch.failed,epoch.not_queried)],"Some selected account rewards are unavailable; missing records do not mean zero earnings.",None));
        }
    }
    result
}

fn finding(
    code: &str,
    severity: &str,
    address: Option<&str>,
    evidence: Vec<String>,
    message: &str,
    source: Option<&Source>,
) -> Finding {
    Finding {
        code: code.into(),
        severity: severity.into(),
        address: address
            .filter(|key| validate_address(key))
            .map(str::to_owned),
        evidence,
        observed_at: source.map(|s| s.observed_at),
        slot: source.and_then(|s| s.slot.clone()),
        message: message.into(),
    }
}

fn clean(value: &str) -> String {
    value.chars().filter(|c| !c.is_control()).collect()
}
