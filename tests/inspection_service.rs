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
        epochs: 1,
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
        json!([{"epoch":n,"amount":123,"postBalance":1000,"effectiveSlot":n*100,"commission":null}]),
    )
}
fn cold() -> Vec<serde_json::Value> {
    vec![
        rpc(json!(MAINNET_GENESIS)),
        epoch(42),
        rpc(json!({"context":{"slot":4200},"value":fixture("stake/initialized")})),
        reward(41),
        epoch(42),
    ]
}

#[test]
fn cold_cache_restart_offline_and_expanded_lookback_use_only_required_calls() {
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
    assert_eq!(thread.join().unwrap().len(), 5);
    let (client, thread) = server(vec![rpc(json!(MAINNET_GENESIS)), epoch(42), epoch(42)]);
    let cached = app::inspect(&options, &store, Some(&client), &context(), false, |_| {});
    assert_eq!(app::exit_code(&cached), 0, "{:?}", cached.errors);
    assert_eq!(thread.join().unwrap().len(), 3);
    options.epochs = 2;
    let (client, thread) = server(vec![
        rpc(json!(MAINNET_GENESIS)),
        epoch(42),
        reward(40),
        epoch(42),
    ]);
    let expanded = app::inspect(&options, &store, Some(&client), &context(), false, |_| {});
    assert_eq!(app::exit_code(&expanded), 0, "{:?}", expanded.errors);
    assert_eq!(thread.join().unwrap().len(), 4);
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
    let (client, thread) = server(vec![
        fixture("helius/genesis"),
        fixture("helius/epoch"),
        fixture("helius/input"),
        fixture("helius/staker"),
        fixture("helius/withdrawer"),
        fixture("helius/validators"),
        fixture("helius/rewards-1"),
        fixture("helius/epoch-after"),
    ]);
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
    assert_eq!(calls.len(), 8);
    assert_eq!(calls[6]["method"], "getInflationReward");
}
