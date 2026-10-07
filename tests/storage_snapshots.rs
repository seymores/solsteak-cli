use serde_json::{Value, json};
use ssteak::storage::Store;
use std::sync::atomic::{AtomicU64, Ordering};
const ADDRESS: &str = "11111111111111111111111111111111";
fn store() -> Store {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    Store::open(std::env::temp_dir().join(format!(
        "ssteak-snapshot-{}-{}.sqlite3",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )))
    .unwrap()
}
fn report(slot: u64) -> Value {
    let mut r: Value =
        serde_json::from_str(include_str!("../docs/contracts/examples/complete.json")).unwrap();
    r["sources"][0]["slot"] = json!(slot.to_string());
    r
}
#[test]
fn restart_keeps_exact_observations_and_failed_refresh_preserves_complete_membership() {
    let store = store();
    let original = report(400);
    assert!(store.save_report(&original).unwrap().published);
    let mut partial = report(401);
    partial["coverage"]["discovery"]["latest_attempt"] = json!("partial");
    partial["coverage"]["discovery"]["displayed"] = json!("partial");
    partial["data"]["accounts"] = json!([]);
    partial["data"]["snapshot_kind"] = json!("latest_partial");
    partial["errors"] =
        json!([{"code":"PROVIDER_FAILURE","message":"Request failed.","address":null}]);
    assert!(!store.save_report(&partial).unwrap().published);
    let reopened = Store::open(store.path()).unwrap();
    let saved = reopened.load_report(ADDRESS).unwrap().unwrap();
    assert_eq!(saved["data"]["accounts"], original["data"]["accounts"]);
    assert_eq!(saved["coverage"]["discovery"]["displayed"], "complete");
    assert_eq!(saved["coverage"]["discovery"]["latest_attempt"], "partial");
    assert_eq!(saved["sources"][0], original["sources"][0]);
    assert_eq!(saved["errors"][0]["code"], "PROVIDER_FAILURE");
    assert_eq!(saved["data"]["stale"], true);
}
#[test]
fn late_lower_slot_and_incomparable_attempts_never_replace_baseline() {
    let store = store();
    assert!(store.save_report(&report(500)).unwrap().published);
    let other = Store::open(store.path()).unwrap();
    assert!(!other.save_report(&report(499)).unwrap().published);
    let mut incomparable = report(501);
    incomparable["sources"][0]["slot"] = Value::Null;
    assert!(!other.save_report(&incomparable).unwrap().published);
    assert_eq!(
        store.load_report(ADDRESS).unwrap().unwrap()["sources"][0]["slot"],
        "500"
    );
    assert!(other.save_report(&report(501)).unwrap().published);
}
#[test]
fn cold_partial_and_empty_complete_authority_snapshots_remain_distinct() {
    let store = store();
    let mut partial = report(1);
    partial["coverage"]["discovery"]["displayed"] = json!("partial");
    partial["coverage"]["discovery"]["latest_attempt"] = json!("partial");
    partial["data"]["snapshot_kind"] = json!("latest_partial");
    store.save_report(&partial).unwrap();
    assert_eq!(
        store.load_report(ADDRESS).unwrap().unwrap()["coverage"]["discovery"]["displayed"],
        "partial"
    );
    let empty: Value =
        serde_json::from_str(include_str!("../docs/contracts/examples/empty.json")).unwrap();
    assert!(store.save_report(&empty).unwrap().published);
    assert_eq!(
        store.load_report(ADDRESS).unwrap().unwrap()["data"]["accounts"],
        json!([])
    );
}
#[test]
fn malformed_values_unresolved_sources_and_secret_fields_are_rejected_atomically() {
    let store = store();
    for (field, bad) in [
        ("balance_lamports", json!("18446744073709551616")),
        ("source_id", json!("absent")),
    ] {
        let mut r = report(1);
        r["data"]["accounts"][0][field] = bad;
        assert!(store.save_report(&r).is_err());
    }
    let mut r = report(1);
    r["api_key"] = json!("do-not-persist");
    assert!(store.save_report(&r).is_err());
    assert!(store.load_report(ADDRESS).unwrap().is_none());
}
#[test]
fn concurrent_instances_publish_only_newest_complete_slot() {
    let store = store();
    store.save_report(&report(1)).unwrap();
    let first = Store::open(store.path()).unwrap();
    let second = Store::open(store.path()).unwrap();
    std::thread::scope(|scope| {
        scope.spawn(|| first.save_report(&report(200)).unwrap());
        scope.spawn(|| second.save_report(&report(100)).unwrap());
    });
    assert_eq!(
        store.load_report(ADDRESS).unwrap().unwrap()["sources"][0]["slot"],
        "200"
    );
}

#[test]
fn every_source_must_advance_and_equal_slots_cannot_change_account_state() {
    let store = store();
    let mut original = report(100);
    let mut epoch = original["sources"][0].clone();
    epoch["id"] = json!("epoch");
    epoch["slot"] = json!("120");
    original["sources"].as_array_mut().unwrap().push(epoch);
    store.save_report(&original).unwrap();
    let mut mixed = original.clone();
    mixed["sources"][0]["slot"] = json!("101");
    mixed["sources"][2]["slot"] = json!("119");
    assert!(!store.save_report(&mixed).unwrap().published);
    let mut changed = original.clone();
    changed["data"]["accounts"][0]["balance_lamports"] = json!("10");
    assert!(!store.save_report(&changed).unwrap().published);
    let mut enrichment = original.clone();
    enrichment["sources"][1]["slot"] = json!("1");
    assert!(store.save_report(&enrichment).unwrap().published);
}

#[test]
fn cached_view_uses_last_network_epoch_across_addresses_without_losing_source_age() {
    let store = store();
    let original = report(100);
    store.save_report(&original).unwrap();
    let mut other = report(200);
    let address = bs58::encode([2_u8; 32]).into_string();
    other["input"]["address"] = json!(address);
    other["data"]["accounts"][0]["address"] = json!(address);
    other["data"]["epoch"] = json!("999");
    store.save_report(&other).unwrap();
    let cached = store.load_report(ADDRESS).unwrap().unwrap();
    assert_eq!(cached["data"]["epoch"], "999");
    assert_eq!(cached["data"]["epoch_kind"], "last_observed");
    assert_eq!(cached["data"]["stale"], true);
    assert_eq!(cached["sources"][0], original["sources"][0]);
}
#[test]
fn repeated_saved_fallbacks_have_unique_bounded_attempt_provenance() {
    let store = store();
    store.save_report(&report(100)).unwrap();
    let mut partial = report(101);
    partial["coverage"]["discovery"]["latest_attempt"] = json!("partial");
    store.save_report(&partial).unwrap();
    for _ in 0..5 {
        let cached = store.load_report(ADDRESS).unwrap().unwrap();
        store.save_report(&cached).unwrap();
    }
    let cached = store.load_report(ADDRESS).unwrap().unwrap();
    let sources = cached["sources"].as_array().unwrap();
    let ids: std::collections::BTreeSet<_> =
        sources.iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(ids.len(), sources.len());
    assert!(sources.len() <= 4);
}

#[test]
fn advancing_epoch_source_does_not_authorize_changed_account_at_same_slot() {
    let store = store();
    let mut original = report(100);
    let mut epoch = original["sources"][0].clone();
    epoch["id"] = json!("epoch");
    epoch["slot"] = json!("120");
    original["sources"].as_array_mut().unwrap().push(epoch);
    store.save_report(&original).unwrap();
    let mut changed = original;
    changed["sources"][2]["slot"] = json!("121");
    changed["data"]["accounts"][0]["balance_lamports"] = json!("10");
    assert!(!store.save_report(&changed).unwrap().published);
}
