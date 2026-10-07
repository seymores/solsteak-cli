mod common;

use common::{context, fixture, key, rpc, server};
use serde_json::json;
use ssteak::domain::RewardEntry;
use ssteak::rewards::{aggregate_epoch, completed_epochs, fetch_batch};

#[test]
fn numeric_zero_null_and_large_rewards_keep_positional_identity_and_exact_totals() {
    let addresses = vec![key(1), key(2), key(3)];
    let record = |amount| json!({"epoch":42,"amount":amount,"postBalance":u64::MAX,"effectiveSlot":500,"commission":null});
    let (client, thread) = server(vec![rpc(json!([record(0), null, record(u64::MAX)]))]);
    let (entries, source) = fetch_batch(&client, &addresses, 42, &context()).unwrap();
    assert_eq!(
        entries.iter().map(|e| &e.address).collect::<Vec<_>>(),
        addresses.iter().collect::<Vec<_>>()
    );
    assert_eq!(entries[0].record.as_ref().unwrap().amount_lamports, "0");
    assert_eq!(entries[1].state, "no_data");
    assert!(entries[1].record.is_none());
    assert!(
        source
            .id
            .starts_with(&format!("reward-42-{}-", addresses[0]))
    );
    assert_eq!(source.slot, None);
    assert_eq!(source.commitment, None);
    let (epoch, coverage) = aggregate_epoch(42, entries).unwrap();
    assert_eq!(epoch.subtotal_lamports, Some(u64::MAX.to_string()));
    assert_eq!(
        (
            coverage.recorded,
            coverage.no_data,
            coverage.failed,
            coverage.not_queried
        ),
        (2, 1, 0, 0)
    );
    let calls = thread.join().unwrap();
    assert_eq!(calls[0]["method"], "getInflationReward");
    assert_eq!(
        calls[0]["params"],
        json!([addresses,{"epoch":42,"commitment":"finalized"}])
    );
}

#[test]
fn malformed_length_epoch_and_record_values_are_rejected() {
    let valid =
        json!({"epoch":42,"amount":0,"postBalance":1,"effectiveSlot":500,"commission":null});
    let mut invalid = vec![json!([]), json!([null, null]), json!([{}])];
    for (field, value) in [
        ("epoch", json!(43)),
        ("amount", json!(-1)),
        ("amount", json!(1.5)),
        ("postBalance", json!("1")),
        ("effectiveSlot", json!(null)),
        ("commission", json!(101)),
        ("commission", json!(-1)),
    ] {
        let mut record = valid.clone();
        record[field] = value;
        invalid.push(json!([record]));
    }
    for response in invalid {
        let (client, thread) = server(vec![rpc(response)]);
        assert_eq!(
            fetch_batch(&client, &[key(1)], 42, &context())
                .unwrap_err()
                .code,
            "INVALID_RESPONSE"
        );
        thread.join().unwrap();
    }
}

#[test]
fn verified_ten_address_fixture_preserves_order_and_returned_commission() {
    let addresses = fixture("helius/staker")["result"]["value"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["pubkey"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let (client, thread) = server(vec![fixture("helius/rewards-1")]);
    let (entries, _) = fetch_batch(&client, &addresses, 1050, &context()).unwrap();
    assert_eq!(entries.len(), 10);
    assert_eq!(
        entries[0].record.as_ref().unwrap().amount_lamports,
        "1273143"
    );
    assert_eq!(
        entries[9].record.as_ref().unwrap().amount_lamports,
        "406534"
    );
    assert_eq!(entries[0].record.as_ref().unwrap().commission, None);
    thread.join().unwrap();
}

#[test]
fn empty_set_is_zero_but_missing_records_are_unknown() {
    assert_eq!(
        aggregate_epoch(1, vec![]).unwrap().0.subtotal_lamports,
        Some("0".into())
    );
    let entries = ["no_data", "failed", "not_queried"]
        .iter()
        .enumerate()
        .map(|(i, state)| RewardEntry {
            address: key(i as u8),
            state: (*state).into(),
            latest_attempt: (*state).into(),
            record: None,
        })
        .collect();
    let (epoch, counts) = aggregate_epoch(1, entries).unwrap();
    assert_eq!(epoch.subtotal_lamports, None);
    assert_eq!(
        (counts.no_data, counts.failed, counts.not_queried),
        (1, 1, 1)
    );
    assert_eq!(completed_epochs(3, 5), vec![2, 1, 0]);
    assert_eq!(completed_epochs(0, 1), Vec::<u64>::new());
    assert_eq!(completed_epochs(1000, 2), vec![999, 998]);
}

#[test]
fn invalid_batches_and_cancelled_queries_do_not_connect() {
    use ssteak::helius::{Helius, RequestContext};
    use std::sync::{Arc, atomic::AtomicBool};
    use std::time::Duration;
    let client =
        Helius::with_endpoint("http://127.0.0.1:1", "key", Duration::from_millis(1)).unwrap();
    for addresses in [
        vec![],
        vec![key(1); 2],
        vec![key(1); 11],
        vec!["invalid".into()],
    ] {
        assert_eq!(
            fetch_batch(&client, &addresses, 1, &context())
                .unwrap_err()
                .code,
            "INVALID_ARGUMENTS"
        );
    }
    let cancelled = RequestContext::new(Duration::from_secs(1), Arc::new(AtomicBool::new(true)));
    assert_eq!(
        fetch_batch(&client, &[key(1)], 1, &cancelled)
            .unwrap_err()
            .code,
        "INTERRUPTED"
    );
}
