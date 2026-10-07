mod common;
use common::{context, fixture, key, rpc, server};
use serde_json::json;
use ssteak::{app, cli::Options, helius::MAINNET_GENESIS, storage::Store};
use std::sync::atomic::{AtomicU64, Ordering};

fn store() -> Store {
    static ID: AtomicU64 = AtomicU64::new(0);
    Store::open(std::env::temp_dir().join(format!(
        "ssteak-app-{}-{}.sqlite3",
        std::process::id(),
        ID.fetch_add(1, Ordering::Relaxed)
    )))
    .unwrap()
}
fn options() -> Options {
    Options {
        address: key(9),
        json: true,
        offline: false,
        refresh: false,
        no_color: true,
    }
}
fn epoch(n: u64) -> serde_json::Value {
    rpc(json!({"epoch":n,"absoluteSlot":n*100}))
}
fn reward(n: u64) -> serde_json::Value {
    rpc(
        json!([{"epoch":n,"amount":123,"postBalance":1000000,"effectiveSlot":n*100,"commission":null}]),
    )
}
fn cold() -> Vec<serde_json::Value> {
    let mut calls = vec![
        rpc(json!(MAINNET_GENESIS)),
        epoch(42),
        rpc(json!({"context":{"slot":4200},"value":fixture("stake/initialized")})),
    ];
    calls.extend((12..=41).rev().map(reward));
    calls.push(epoch(42));
    calls
}

#[test]
fn cold_cache_restart_rollover_and_offline_use_only_required_calls() {
    let store = store();
    let mut options = options();
    let (client, thread) = server(cold());
    let report = app::inspect(&options, &store, Some(&client), &context(), false, |_| {});
    assert_eq!(app::exit_code(&report), 0, "{:?}", report.errors);
    assert_eq!(
        report
            .data
            .as_ref()
            .unwrap()
            .summary
            .latest_reward_lamports
            .as_deref(),
        Some("123")
    );
    let data = report.data.as_ref().unwrap();
    let window = (12..=41).rev().map(|n| n.to_string()).collect::<Vec<_>>();
    assert_eq!(data.requested_epochs, window);
    assert_eq!(
        data.rewards
            .iter()
            .map(|r| r.epoch.clone())
            .collect::<Vec<_>>(),
        window
    );
    let estimate = data.rewards[0].entries[0].account_return.as_ref().unwrap();
    assert_eq!(estimate.pre_reward_balance_lamports, "999877");
    // 3 setup calls, one reward request per epoch, and the closing epoch check.
    assert_eq!(thread.join().unwrap().len(), 34);
    let (client, thread) = server(vec![rpc(json!(MAINNET_GENESIS)), epoch(42), epoch(42)]);
    let cached = app::inspect(&options, &store, Some(&client), &context(), false, |_| {});
    assert_eq!(app::exit_code(&cached), 0, "{:?}", cached.errors);
    assert_eq!(thread.join().unwrap().len(), 3);
    // Rollover slides the window by one epoch: only epoch 42 is newly requested.
    let (client, thread) = server(vec![
        rpc(json!(MAINNET_GENESIS)),
        epoch(43),
        rpc(json!({"context":{"slot":4300},"value":fixture("stake/initialized")})),
        reward(42),
        epoch(43),
    ]);
    let expanded = app::inspect(&options, &store, Some(&client), &context(), false, |_| {});
    assert_eq!(app::exit_code(&expanded), 0, "{:?}", expanded.errors);
    assert_eq!(expanded.data.as_ref().unwrap().requested_epochs[0], "42");
    assert_eq!(expanded.data.as_ref().unwrap().rewards.len(), 30);
    assert_eq!(thread.join().unwrap().len(), 5);
    options.offline = true;
    let offline = app::inspect(
        &options,
        &Store::open(store.path()).unwrap(),
        None,
        &context(),
        false,
        |_| {},
    );
    assert_eq!(app::exit_code(&offline), 0, "{:?}", offline.errors);
    assert_eq!(
        offline.data.as_ref().unwrap().rewards,
        expanded.data.as_ref().unwrap().rewards
    );
    assert_eq!(offline.data.as_ref().unwrap().epoch_kind, "last_observed");
    assert!(offline.sources.iter().all(|s| s.cached));
}

#[test]
fn offline_miss_and_failed_refresh_preserve_honest_state() {
    let store = store();
    let mut options = options();
    options.offline = true;
    let missing = app::inspect(&options, &store, None, &context(), false, |_| {});
    assert_eq!(app::exit_code(&missing), 4);
    assert_eq!(missing.errors[0].code, "OFFLINE_MISS");
    options.offline = false;
    let (client, thread) = server(cold());
    let first = app::inspect(&options, &store, Some(&client), &context(), false, |_| {});
    thread.join().unwrap();
    let (client, thread) = server(vec![
        json!({"jsonrpc":"2.0","id":1,"error":{"code":-32001,"message":"secret"}}),
    ]);
    let failed = app::inspect(&options, &store, Some(&client), &context(), true, |_| {});
    thread.join().unwrap();
    assert_eq!(app::exit_code(&failed), 3);
    assert!(failed.data.as_ref().unwrap().stale);
    assert_eq!(
        failed.data.as_ref().unwrap().accounts,
        first.data.as_ref().unwrap().accounts
    );
    assert!(!serde_json::to_string(&failed).unwrap().contains("secret"));
}

#[test]
fn supplied_wallet_fixture_produces_complete_deduplicated_report() {
    let store = store();
    let options = Options {
        address: "9x29Aw3N92EPCX1weRsrKKwAkbLfzw2PGRrm4NbcifEJ".into(),
        ..options()
    };
    let mut replies = vec![
        fixture("helius/genesis"),
        fixture("helius/epoch"),
        fixture("helius/input"),
        fixture("helius/staker"),
        fixture("helius/withdrawer"),
        fixture("helius/validators"),
    ];
    // The recorded fixture is epoch 1050; reuse its rows for each earlier epoch.
    replies.extend((0..30).map(|back| {
        let mut rewards = fixture("helius/rewards-1");
        for row in rewards["result"].as_array_mut().unwrap() {
            row["epoch"] = json!(1050 - back);
        }
        rewards
    }));
    replies.push(fixture("helius/epoch-after"));
    let (client, thread) = server(replies);
    let report = app::inspect(&options, &store, Some(&client), &context(), false, |_| {});
    assert_eq!(app::exit_code(&report), 0, "{:?}", report.errors);
    let data = report.data.unwrap();
    assert_eq!(data.accounts.len(), 10);
    assert!(
        data.accounts
            .iter()
            .all(|a| a.relationship.as_deref() == Some("both"))
    );
    assert_eq!(data.rewards[0].entries.len(), 10);
    assert_eq!(
        data.rewards[0].subtotal_lamports.as_deref(),
        Some("58447695")
    );
    let calls = thread.join().unwrap();
    assert_eq!(calls.len(), 37);
    assert_eq!(
        calls
            .iter()
            .filter(|c| c["method"] == "getInflationReward")
            .count(),
        30
    );
    assert_eq!(calls[6]["method"], "getInflationReward");
}

#[test]
fn shortened_chain_requests_only_available_epochs_and_labels_the_range() {
    let store = store();
    let (client, thread) = server(vec![
        rpc(json!(MAINNET_GENESIS)),
        epoch(3),
        rpc(json!({"context":{"slot":300},"value":fixture("stake/initialized")})),
        reward(2),
        reward(1),
        reward(0),
        epoch(3),
    ]);
    let report = app::inspect(&options(), &store, Some(&client), &context(), false, |_| {});
    assert_eq!(app::exit_code(&report), 0, "{:?}", report.errors);
    assert_eq!(
        report.data.as_ref().unwrap().requested_epochs,
        ["2", "1", "0"]
    );
    let finding = report
        .warnings
        .iter()
        .find(|w| w.code == "EPOCH_RANGE_SHORTENED")
        .unwrap();
    assert_eq!(finding.severity, "info");
    assert_eq!(thread.join().unwrap().len(), 7);
}
