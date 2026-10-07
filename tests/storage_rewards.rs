use serde_json::{Value, json};
use ssteak::storage::{RewardAttempt, Store};
use std::sync::atomic::{AtomicU64, Ordering};
const ADDRESS: &str = "11111111111111111111111111111111";
fn store() -> Store {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let store = Store::open(std::env::temp_dir().join(format!(
        "ssteak-reward-{}-{}.sqlite3",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )))
    .unwrap();
    let mut r: Value =
        serde_json::from_str(include_str!("../docs/contracts/examples/complete.json")).unwrap();
    r["data"]["rewards"] = json!([]);
    r["coverage"]["rewards"] = json!([]);
    store.save_report(&r).unwrap();
    store
}
fn attempt(epoch: u64, amount: u64) -> RewardAttempt {
    RewardAttempt {
        address: ADDRESS.into(),
        epoch,
        state: "recorded".into(),
        record: Some(
            json!({"epoch":epoch.to_string(),"amount_lamports":amount.to_string(),"post_balance_lamports":u64::MAX.to_string(),"effective_slot":u64::MAX.to_string(),"commission":null,"source_id":"reward"}),
        ),
        source: Some(
            json!({"id":"reward","provider":"helius","observed_at":100,"slot":null,"commitment":null,"decoder_version":null,"cached":false}),
        ),
        observed_at: Some(100),
        error_code: None,
    }
}
#[test]
fn restart_reuses_unique_numeric_records_and_null_error_cannot_erase_zero() {
    let store = store();
    let reward = attempt(u64::MAX, 0);
    store
        .commit_reward_batch(ADDRESS, std::slice::from_ref(&reward))
        .unwrap();
    store
        .commit_reward_batch(ADDRESS, std::slice::from_ref(&reward))
        .unwrap();
    for state in ["no_data", "failed"] {
        let mut missing = reward.clone();
        missing.record = None;
        missing.source = None;
        missing.state = state.into();
        missing.observed_at = Some(101);
        store.commit_reward_batch(ADDRESS, &[missing]).unwrap();
    }
    let saved = Store::open(store.path())
        .unwrap()
        .reward(ADDRESS, ADDRESS, u64::MAX)
        .unwrap()
        .unwrap();
    assert_eq!(saved.record.unwrap()["amount_lamports"], "0");
    assert_eq!(saved.latest_attempt, "failed");
    assert_eq!(saved.source.unwrap()["observed_at"], 100);
}
#[test]
fn numeric_conflict_preserves_original_provenance_and_flags_inconsistency() {
    let store = store();
    store
        .commit_reward_batch(ADDRESS, &[attempt(1, u64::MAX)])
        .unwrap();
    assert_eq!(
        store
            .commit_reward_batch(ADDRESS, &[attempt(1, 1)])
            .unwrap(),
        vec![(ADDRESS.into(), 1)]
    );
    let saved = store.reward(ADDRESS, ADDRESS, 1).unwrap().unwrap();
    assert_eq!(
        saved.record.unwrap()["amount_lamports"],
        u64::MAX.to_string()
    );
    assert_eq!(saved.error_code.as_deref(), Some("REWARD_CONFLICT"));
}
#[test]
fn invalid_later_record_rolls_back_whole_batch_and_resume_is_idempotent() {
    let store = store();
    let good = attempt(1, 5);
    let mut invalid = attempt(2, 6);
    invalid.record.as_mut().unwrap()["epoch"] = json!("3");
    assert!(
        store
            .commit_reward_batch(ADDRESS, &[good.clone(), invalid])
            .is_err()
    );
    assert!(store.reward(ADDRESS, ADDRESS, 1).unwrap().is_none());
    store
        .commit_reward_batch(ADDRESS, std::slice::from_ref(&good))
        .unwrap();
    store.commit_reward_batch(ADDRESS, &[good]).unwrap();
    assert_eq!(
        store
            .reward(ADDRESS, ADDRESS, 1)
            .unwrap()
            .unwrap()
            .record
            .unwrap()["amount_lamports"],
        "5"
    );
}
#[test]
fn snapshot_import_saves_rewards_and_conflicts_do_not_overwrite_record() {
    let store = store();
    let mut report: Value =
        serde_json::from_str(include_str!("../docs/contracts/examples/complete.json")).unwrap();
    store.save_report(&report).unwrap();
    assert_eq!(
        store
            .reward(ADDRESS, ADDRESS, 899)
            .unwrap()
            .unwrap()
            .record
            .unwrap()["amount_lamports"],
        "0"
    );
    report["data"]["rewards"][0]["entries"][0]["record"]["amount_lamports"] = json!("9");
    assert_eq!(
        store.save_report(&report).unwrap().reward_conflicts,
        vec![(ADDRESS.into(), 899)]
    );
}

#[test]
fn snapshot_import_does_not_clear_an_unresolved_numeric_conflict() {
    let store = store();
    let report: Value =
        serde_json::from_str(include_str!("../docs/contracts/examples/complete.json")).unwrap();
    store.save_report(&report).unwrap();
    let mut conflicting = attempt(899, 5);
    conflicting.observed_at = Some(1791360001);
    store.commit_reward_batch(ADDRESS, &[conflicting]).unwrap();
    let mut saved = report;
    saved["generated_at"] = json!(1791360002);
    store.save_report(&saved).unwrap();
    assert_eq!(
        store
            .reward(ADDRESS, ADDRESS, 899)
            .unwrap()
            .unwrap()
            .error_code
            .as_deref(),
        Some("REWARD_CONFLICT")
    );
}

#[test]
fn reward_crash_child() {
    let Some(path) = std::env::var_os("SSTEAK_TEST_CRASH_DB") else {
        return;
    };
    if std::env::var("SSTEAK_TEST_CRASH_STAGE").unwrap() == "before" {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON; BEGIN IMMEDIATE;")
            .unwrap();
        conn.execute(
            "INSERT INTO rewards VALUES (?1,?1,'12','7','8','9',NULL,'{}','{}')",
            [ADDRESS],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO reward_coverage VALUES (?1,?1,'12',100,'recorded',NULL,0)",
            [ADDRESS],
        )
        .unwrap();
        std::process::exit(0);
    }
    let store = Store::open(path).unwrap();
    store
        .commit_reward_batch(ADDRESS, &[attempt(12, 7)])
        .unwrap();
    std::process::exit(0);
}

#[test]
fn process_exit_before_and_after_commit_recovers_all_or_none() {
    let store = store();
    for stage in ["before", "after"] {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "reward_crash_child"])
            .env("SSTEAK_TEST_CRASH_DB", store.path())
            .env("SSTEAK_TEST_CRASH_STAGE", stage)
            .status()
            .unwrap();
        assert!(status.success());
        let reopened = Store::open(store.path()).unwrap();
        let saved = reopened.reward(ADDRESS, ADDRESS, 12).unwrap();
        assert_eq!(saved.is_some(), stage == "after");
    }
}
