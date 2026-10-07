mod common;

use base64::{Engine, engine::general_purpose::STANDARD};
use common::{context, fixture, key, rpc, server};
use serde_json::{Value, json};
use ssteak::inspection::discover;

fn account(staker: u8, withdrawer: u8) -> Value {
    let mut account = fixture("stake/initialized");
    let mut raw = STANDARD
        .decode(account["data"][0].as_str().unwrap())
        .unwrap();
    raw[12..44].fill(staker);
    raw[44..76].fill(withdrawer);
    account["data"][0] = json!(STANDARD.encode(raw));
    account
}
fn input(value: Value) -> Value {
    rpc(json!({"context":{"slot":10},"value":value}))
}
fn rows(slot: u64, accounts: Vec<(u8, Value)>) -> Value {
    rpc(
        json!({"context":{"slot":slot},"value":accounts.into_iter().map(|(k,a)|json!({"pubkey":key(k),"account":a})).collect::<Vec<_>>()}),
    )
}

#[test]
fn direct_stake_selects_only_input_and_skips_authority_queries() {
    let (client, thread) = server(vec![input(account(1, 2))]);
    let result = discover(&client, &key(9), &context()).unwrap();
    assert_eq!(result.selection, "direct");
    assert_eq!(result.input_exists, Some(true));
    assert_eq!(result.accounts.len(), 1);
    assert_eq!(result.accounts[0].address, key(9));
    assert_eq!(result.accounts[0].relationship, None);
    assert_eq!(result.coverage.displayed, "complete");
    assert_eq!(result.coverage.staker_query, "not_applicable");
    assert_eq!(thread.join().unwrap().len(), 1);
}

#[test]
fn absent_authority_queries_both_filters_and_deduplicates_from_decoded_relationships() {
    let both = account(1, 1);
    let (client, thread) = server(vec![
        input(Value::Null),
        rows(11, vec![(9, both.clone()), (8, account(1, 2))]),
        rows(12, vec![(9, both)]),
    ]);
    let result = discover(&client, &key(1), &context()).unwrap();
    assert_eq!(result.input_exists, Some(false));
    assert_eq!(result.selection, "authority");
    assert_eq!(result.accounts.len(), 2);
    assert_eq!(
        result
            .accounts
            .iter()
            .find(|a| a.address == key(9))
            .unwrap()
            .relationship
            .as_deref(),
        Some("both")
    );
    assert_eq!(
        result
            .accounts
            .iter()
            .find(|a| a.address == key(8))
            .unwrap()
            .relationship
            .as_deref(),
        Some("staker")
    );
    assert_eq!(result.coverage.displayed, "complete");
    let requests = thread.join().unwrap();
    for (i, offset) in [(1, 12), (2, 44)] {
        assert_eq!(requests[i]["method"], "getProgramAccounts");
        assert_eq!(
            requests[i]["params"][1]["filters"],
            json!([{"memcmp":{"offset":offset,"bytes":key(1)}}])
        );
        assert_eq!(requests[i]["params"][1]["withContext"], true);
    }
}

#[test]
fn empty_success_and_independent_failure_are_not_equivalent() {
    let failure = json!({"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"secret"}});
    for (second, expected) in [(rows(12, vec![]), "complete"), (failure, "partial")] {
        let (client, thread) = server(vec![input(Value::Null), rows(11, vec![]), second]);
        let result = discover(&client, &key(1), &context()).unwrap();
        assert_eq!(result.coverage.displayed, expected);
        assert!(result.accounts.is_empty());
        assert_eq!(result.errors.is_empty(), expected == "complete");
        assert!(!format!("{:?}", result.errors).contains("secret"));
        thread.join().unwrap();
    }
}

#[test]
fn mismatched_filters_are_rejected_and_unknown_layouts_remain_visible() {
    let mut unknown = account(1, 1);
    unknown["data"][0] = json!(STANDARD.encode([0u8; 201]));
    let (client, thread) = server(vec![
        input(Value::Null),
        rows(11, vec![(8, account(2, 2)), (9, unknown)]),
        rows(12, vec![]),
    ]);
    let result = discover(&client, &key(1), &context()).unwrap();
    assert_eq!(result.accounts.len(), 1);
    assert_eq!(result.accounts[0].address, key(9));
    assert_eq!(result.accounts[0].state, "unsupported");
    assert_eq!(result.accounts[0].relationship, None);
    assert_eq!(result.coverage.displayed, "partial");
    assert!(!result.errors.is_empty());
    thread.join().unwrap();
}

#[test]
fn conflicting_duplicates_select_newer_observation_without_inventing_both() {
    let (client, thread) = server(vec![
        input(Value::Null),
        rows(11, vec![(9, account(1, 2))]),
        rows(12, vec![(9, account(2, 1))]),
    ]);
    let result = discover(&client, &key(1), &context()).unwrap();
    assert_eq!(result.accounts.len(), 1);
    assert_eq!(
        result.accounts[0].relationship.as_deref(),
        Some("withdrawer")
    );
    assert_eq!(result.accounts[0].source_id, "withdrawer");
    assert_eq!(result.coverage.displayed, "partial");
    thread.join().unwrap();
}

#[test]
fn captured_mainnet_fixture_deduplicates_ten_accounts() {
    let (client, thread) = server(vec![
        fixture("helius/input"),
        fixture("helius/staker"),
        fixture("helius/withdrawer"),
    ]);
    let result = discover(
        &client,
        "9x29Aw3N92EPCX1weRsrKKwAkbLfzw2PGRrm4NbcifEJ",
        &context(),
    )
    .unwrap();
    assert_eq!(result.accounts.len(), 10);
    assert!(
        result
            .accounts
            .iter()
            .all(|a| a.relationship.as_deref() == Some("both"))
    );
    assert_eq!(result.coverage.displayed, "complete");
    assert_eq!(result.sources.len(), 3);
    thread.join().unwrap();
}

#[test]
fn malformed_context_and_direct_layout_cannot_claim_complete() {
    let (client, thread) = server(vec![rpc(json!({"value":null}))]);
    assert_eq!(
        discover(&client, &key(1), &context()).unwrap_err().code,
        "INVALID_RESPONSE"
    );
    thread.join().unwrap();
    let mut invalid = account(1, 1);
    invalid["lamports"] = json!(1);
    let (client, thread) = server(vec![input(invalid)]);
    let result = discover(&client, &key(1), &context()).unwrap();
    assert_eq!(result.coverage.displayed, "partial");
    assert_eq!(result.accounts[0].state, "unsupported");
    thread.join().unwrap();
}
