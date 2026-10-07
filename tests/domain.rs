use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use ssteak::domain::{
    Report, annualized_return, authority_relationship, checked_total, decode_stake, format_sol,
    parse_amount, summarize, validate_address,
};

const ADDRESS: &str = "11111111111111111111111111111111";

fn fixture(delegated: bool) -> Value {
    serde_json::from_str(if delegated {
        include_str!("fixtures/stake/delegated.json")
    } else {
        include_str!("fixtures/stake/initialized.json")
    })
    .unwrap()
}

#[test]
fn canonical_initialized_amounts_and_relationships_remain_exact() {
    let account = decode_stake(ADDRESS, &fixture(false), Some("both"), "fixture").unwrap();
    assert_eq!(account.state, "initialized");
    assert_eq!(
        account.balance_lamports.as_deref(),
        Some("18446744073709551615")
    );
    assert_eq!(
        account.undelegated_lamports.as_deref(),
        Some("18446744073707268735")
    );
    assert_eq!(account.delegated_lamports.as_deref(), Some("0"));
    assert_eq!(account.staker, Some(bs58::encode([1; 32]).into_string()));
    assert_eq!(
        account.withdrawer,
        Some(bs58::encode([2; 32]).into_string())
    );
    assert_eq!(
        authority_relationship(&account, account.staker.as_ref().unwrap()),
        Some("staker")
    );
    assert_eq!(authority_relationship(&account, ADDRESS), None);
    let lockup = account.lockup.as_ref().unwrap();
    assert_eq!(lockup.unix_timestamp, i64::MIN.to_string());
    assert_eq!(lockup.epoch, u64::MAX.to_string());
    let encoded = serde_json::to_value(account).unwrap();
    assert!(encoded["effective_lamports"].is_null());
    assert!(encoded["activating_lamports"].is_null());
    assert!(encoded["deactivating_lamports"].is_null());
}

#[test]
fn canonical_delegation_preserves_sentinels_without_inventing_active_stake() {
    let account = decode_stake(ADDRESS, &fixture(true), None, "fixture").unwrap();
    assert_eq!(account.state, "delegated");
    assert_eq!(
        account.delegated_lamports.as_deref(),
        Some("18446744073707268735")
    );
    assert_eq!(
        account.vote_address,
        Some(bs58::encode([4; 32]).into_string())
    );
    assert_eq!(
        account.activation_epoch.as_deref(),
        Some("18446744073709551615")
    );
    assert_eq!(
        account.deactivation_epoch.as_deref(),
        Some("18446744073709551615")
    );
    assert!(account.undelegated_lamports.is_none());
    assert!(account.effective_lamports.is_none());
    assert!(account.relationship.is_none());
}

#[test]
fn wrong_owner_truncated_future_layout_and_variant_stay_visible_unknown() {
    let original = fixture(true);
    let bytes = STANDARD
        .decode(original["data"][0].as_str().unwrap())
        .unwrap();
    let mut cases = Vec::new();
    let mut wrong_owner = original.clone();
    wrong_owner["owner"] = json!(ADDRESS);
    cases.push(wrong_owner);
    let mut unknown_variant = bytes.clone();
    unknown_variant[..4].copy_from_slice(&99u32.to_le_bytes());
    let mut future_flags = bytes.clone();
    future_flags[196] = 128;
    for raw in [
        bytes[..199].to_vec(),
        [bytes.clone(), vec![0]].concat(),
        unknown_variant,
        future_flags,
    ] {
        let mut rpc = original.clone();
        rpc["data"][0] = json!(STANDARD.encode(raw));
        cases.push(rpc);
    }
    for rpc in cases {
        let account = decode_stake(ADDRESS, &rpc, None, "fixture").unwrap();
        assert_eq!(account.state, "unsupported");
        assert!(account.delegated_lamports.is_none());
        assert!(account.staker.is_none());
        assert_eq!(
            account.balance_lamports.as_deref(),
            Some("18446744073709551615")
        );
    }
}

#[test]
fn malformed_rpc_and_rent_underflow_are_errors() {
    for lamports in [
        json!(-1),
        json!(1.5),
        json!("18446744073709551615"),
        json!(0),
        Value::Null,
    ] {
        let mut rpc = fixture(false);
        rpc["lamports"] = lamports;
        assert!(decode_stake(ADDRESS, &rpc, None, "fixture").is_err());
    }
    let mut rpc = fixture(false);
    rpc["data"][0] = json!("not base64");
    assert!(decode_stake(ADDRESS, &rpc, None, "fixture").is_err());
}

#[test]
fn totals_deduplicate_and_partition_authority_scopes_without_overlap() {
    let both = decode_stake(ADDRESS, &fixture(false), Some("both"), "fixture").unwrap();
    let staker = decode_stake(
        &bs58::encode([9; 32]).into_string(),
        &fixture(false),
        Some("staker"),
        "fixture",
    )
    .unwrap();
    let result = summarize(
        &[both.clone(), both.clone(), staker],
        "authority",
        Some(0),
        false,
    )
    .unwrap();
    assert_eq!(result.account_count, 2);
    assert_eq!(
        result.balance_lamports.as_deref(),
        Some("36893488147419103230")
    );
    assert_eq!(
        result.withdraw_authority_lamports.as_deref(),
        Some("18446744073709551615")
    );
    assert_eq!(
        result.staker_only_lamports.as_deref(),
        Some("18446744073709551615")
    );
    assert_eq!(result.amount_scope, "subtotal");
    let result = summarize(&[both], "direct", None, true).unwrap();
    assert!(result.withdraw_authority_lamports.is_none());
    assert!(result.staker_only_lamports.is_none());
}

#[test]
fn unknown_and_successful_empty_sets_have_different_totals() {
    let result = summarize(&[], "authority", Some(0), true).unwrap();
    assert_eq!(result.balance_lamports.as_deref(), Some("0"));
    let mut rpc = fixture(false);
    rpc["owner"] = json!(ADDRESS);
    let account = decode_stake(ADDRESS, &rpc, None, "fixture").unwrap();
    let result = summarize(&[account], "direct", None, true).unwrap();
    assert!(result.delegated_lamports.is_none());
    assert!(result.latest_reward_lamports.is_none());
}

#[test]
fn formatting_addresses_and_wire_reports_preserve_contract() {
    assert_eq!(format_sol(u64::MAX.into()), "18446744073.709551615");
    assert_eq!(format_sol(1), "0.000000001");
    assert!(validate_address(&bs58::encode([255; 32]).into_string()));
    assert!(validate_address(ADDRESS));
    assert!(!validate_address("0OIl"));
    assert!(!validate_address(&"1".repeat(33)));
    for raw in [
        include_str!("../docs/contracts/examples/complete.json"),
        include_str!("../docs/contracts/examples/partial.json"),
        include_str!("../docs/contracts/examples/empty.json"),
        include_str!("../docs/contracts/examples/offline.json"),
        include_str!("../docs/contracts/examples/error.json"),
        include_str!("../docs/contracts/examples/short-window.json"),
        include_str!("../docs/contracts/examples/short-previous.json"),
    ] {
        let report: Report = serde_json::from_str(raw).unwrap();
        assert_eq!(
            serde_json::to_value(report).unwrap(),
            serde_json::from_str::<Value>(raw).unwrap()
        );
    }
}

#[test]
fn arithmetic_checks_boundaries_and_rejects_noncanonical_amounts() {
    assert_eq!(checked_total([u128::MAX, 0]).unwrap(), u128::MAX);
    assert!(checked_total([u128::MAX, 1]).is_err());
    assert_eq!(parse_amount(&u64::MAX.to_string()).unwrap(), u64::MAX);
    for value in ["", "+1", "01", "-1", "1.0", "18446744073709551616"] {
        assert!(parse_amount(value).is_err());
    }
}

#[test]
fn annualized_return_follows_fr14_for_edge_inputs() {
    let pct = |amount, post| annualized_return(amount, post).unwrap().unwrap();
    let estimate = pct(1_000_000, 100_001_000_000);
    assert_eq!(estimate.pre_reward_balance_lamports, "100000000000");
    assert_eq!(estimate.annualized_percent, "0.1827");
    // A zero reward with a positive denominator is a valid 0%, even at u64::MAX.
    assert_eq!(pct(0, 1).annualized_percent, "0.0000");
    let big = pct(0, u64::MAX);
    assert_eq!(big.pre_reward_balance_lamports, u64::MAX.to_string());
    assert_eq!(big.annualized_percent, "0.0000");
    // Zero denominator is Unknown, not an error and not a percentage.
    assert_eq!(annualized_return(5, 5).unwrap(), None);
    // Reward above the post-balance underflows; unrepresentable results are invalid.
    assert!(annualized_return(6, 5).is_err());
    assert!(annualized_return(1, 2).is_err());
}

#[test]
fn contract_examples_match_the_calculated_estimates_and_comparisons() {
    use ssteak::rewards::{aggregate_epoch, compare};
    for raw in [
        include_str!("../docs/contracts/examples/complete.json"),
        include_str!("../docs/contracts/examples/partial.json"),
        include_str!("../docs/contracts/examples/offline.json"),
        include_str!("../docs/contracts/examples/empty.json"),
        include_str!("../docs/contracts/examples/short-window.json"),
        include_str!("../docs/contracts/examples/short-previous.json"),
    ] {
        let report: Report = serde_json::from_str(raw).unwrap();
        let data = report.data.unwrap();
        for epoch in &data.rewards {
            let entries = epoch.entries.clone();
            let (again, _, invalid) =
                aggregate_epoch(epoch.epoch.parse().unwrap(), entries).unwrap();
            assert!(invalid.is_empty());
            assert_eq!(&again, epoch);
        }
        let (comparisons, errors) = compare(&data.rewards, &data.accounts).unwrap();
        assert!(errors.is_empty());
        assert_eq!(comparisons, data.comparisons);
    }
}
