mod common;

use common::{context, fixture, key, rpc, server};
use serde_json::json;
use ssteak::domain::{Account, Coverage, DiscoveryCoverage, RewardCoverage, Source, decode_stake};
use ssteak::validators::{fetch, findings, group};

fn account(byte: u8) -> Account {
    let mut account =
        decode_stake(&key(byte), &fixture("stake/delegated"), None, "accounts").unwrap();
    account.vote_address = Some(key(5));
    account.delegated_lamports = Some(u64::MAX.to_string());
    account
}

fn complete_coverage() -> Coverage {
    Coverage {
        discovery: DiscoveryCoverage {
            displayed: "complete".into(),
            latest_attempt: "complete".into(),
            staker_query: "succeeded".into(),
            withdrawer_query: "succeeded".into(),
        },
        validators: "complete".into(),
        rewards: vec![],
    }
}

#[test]
fn grouping_uses_recorded_delegation_and_current_commission_without_netting_it() {
    let response = json!({"current":[{"votePubkey":key(5),"commission":50,"name":"safe\u{001b}[31m\nname"}],"delinquent":[]});
    let accounts = vec![account(1), account(2)];
    let grouped = group(&accounts, Some(&response)).unwrap();
    assert_eq!(grouped.len(), 1);
    assert_eq!(
        grouped[0].delegated_lamports,
        Some((u128::from(u64::MAX) * 2).to_string())
    );
    assert_eq!(
        grouped[0].concentration_denominator_lamports,
        grouped[0].delegated_lamports
    );
    assert_eq!(grouped[0].commission, Some(50));
    assert_eq!(grouped[0].state, "current");
    assert!(
        !grouped[0]
            .name
            .as_ref()
            .unwrap()
            .chars()
            .any(char::is_control)
    );
    // The same account cannot increase concentration when repeated.
    assert_eq!(
        group(&[accounts[0].clone(), accounts[0].clone()], Some(&response)).unwrap()[0]
            .delegated_lamports,
        Some(u64::MAX.to_string())
    );
}

#[test]
fn missing_records_amounts_and_zero_denominator_remain_unknown() {
    let mut a = account(1);
    let absent = group(&[a.clone()], Some(&json!({"current":[],"delinquent":[]}))).unwrap();
    assert_eq!(absent[0].state, "unknown");
    assert_eq!(absent[0].commission, None);
    assert_eq!(absent[0].source_id, None);
    a.delegated_lamports = None;
    let unknown = group(&[a.clone()], None).unwrap();
    assert_eq!(unknown[0].delegated_lamports, None);
    assert_eq!(unknown[0].concentration_denominator_lamports, None);
    a.delegated_lamports = Some("0".into());
    assert_eq!(
        group(&[a], None).unwrap()[0].concentration_denominator_lamports,
        None
    );
}

#[test]
fn fetch_preserves_observation_provenance_without_inventing_a_context_slot() {
    let accounts: Vec<_> = fixture("helius/staker")["result"]["value"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            decode_stake(
                row["pubkey"].as_str().unwrap(),
                &row["account"],
                None,
                "accounts",
            )
            .unwrap()
        })
        .collect();
    let (client, thread) = server(vec![fixture("helius/validators")]);
    let (validators, source) = fetch(&client, &accounts, &context()).unwrap();
    assert!(!validators.is_empty());
    assert!(
        validators
            .iter()
            .all(|v| v.source_id.as_deref() == Some("validators"))
    );
    assert_eq!(source.slot, None);
    assert_eq!(source.commitment.as_deref(), Some("finalized"));
    assert_eq!(thread.join().unwrap()[0]["method"], "getVoteAccounts");
}

#[test]
fn malformed_validator_metadata_and_conflicting_classification_are_rejected() {
    for response in [
        json!({}),
        json!({"current":[{"votePubkey":key(5),"commission":101}],"delinquent":[]}),
        json!({"current":[{"votePubkey":key(5),"commission":0}],"delinquent":[{"votePubkey":key(5),"commission":0}]}),
    ] {
        let (client, thread) = server(vec![rpc(response)]);
        assert_eq!(
            fetch(&client, &[account(1)], &context()).unwrap_err().code,
            "INVALID_RESPONSE"
        );
        thread.join().unwrap();
    }
}

#[test]
fn evidence_based_findings_preserve_severity_and_observation_context() {
    let initialized =
        decode_stake(&key(1), &fixture("stake/initialized"), None, "accounts").unwrap();
    let mut delegated = account(2);
    delegated.deactivation_epoch = Some("12".into());
    let source = Source {
        id: "accounts".into(),
        provider: "helius".into(),
        observed_at: 100,
        slot: Some("500".into()),
        commitment: Some("finalized".into()),
        decoder_version: Some("stake-v1".into()),
        cached: false,
    };
    let mut validator_source = source.clone();
    validator_source.id = "validators".into();
    validator_source.slot = None;
    let validators = group(
        &[delegated.clone()],
        Some(&json!({"current":[],"delinquent":[{"votePubkey":key(5),"commission":10}]})),
    )
    .unwrap();
    let mut coverage = complete_coverage();
    coverage.discovery.latest_attempt = "partial".into();
    coverage.rewards.push(RewardCoverage {
        epoch: "12".into(),
        recorded: 1,
        no_data: 1,
        failed: 0,
        not_queried: 0,
    });
    let warnings = findings(
        &[initialized, delegated],
        &validators,
        &coverage,
        &[source, validator_source],
    );
    for code in [
        "UNDELEGATED_FUNDS",
        "DEACTIVATION_REQUESTED",
        "VALIDATOR_DELINQUENT",
        "INCOMPLETE_DISCOVERY",
        "INCOMPLETE_REWARDS",
    ] {
        assert!(
            warnings.iter().any(|w| w.code == code),
            "missing {code}: {warnings:?}"
        );
    }
    assert!(
        warnings
            .iter()
            .all(|warning| warning.code != "LOCKUP_PRESENT"),
        "lockup metadata belongs in account details, not attention findings: {warnings:?}"
    );
    let deactivation = warnings
        .iter()
        .find(|w| w.code == "DEACTIVATION_REQUESTED")
        .unwrap();
    assert_eq!(deactivation.severity, "info");
    assert_eq!(deactivation.observed_at, Some(100));
    assert_eq!(deactivation.slot.as_deref(), Some("500"));
    assert_eq!(
        warnings
            .iter()
            .find(|w| w.code == "VALIDATOR_DELINQUENT")
            .unwrap()
            .severity,
        "warning"
    );
    assert!(
        !warnings
            .iter()
            .any(|w| w.message.contains("withdrawable") || w.message.contains("healthy"))
    );
}

#[test]
fn unsupported_state_is_visible_and_sentinel_deactivation_is_not_a_finding() {
    let mut a = account(1);
    a.deactivation_epoch = Some(u64::MAX.to_string());
    a.lockup = None;
    assert!(findings(&[a.clone()], &[], &complete_coverage(), &[]).is_empty());
    a.state = "unsupported".into();
    a.deactivation_epoch = None;
    assert!(
        findings(&[a], &[], &complete_coverage(), &[])
            .iter()
            .any(|w| w.code == "UNSUPPORTED_STATE")
    );
}
