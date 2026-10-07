mod common;

use common::{context, fixture, key, rpc, server};
use serde_json::json;
use ssteak::domain::{Account, EpochReward};
use ssteak::domain::{RewardEntry, RewardRecord};
use ssteak::rewards::{aggregate_epoch, compare, completed_epochs, fetch_batch};

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
    let (epoch, coverage, invalid) = aggregate_epoch(42, entries).unwrap();
    assert!(invalid.is_empty());
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
            account_return: None,
        })
        .collect();
    let (epoch, counts, _) = aggregate_epoch(1, entries).unwrap();
    assert_eq!(epoch.subtotal_lamports, None);
    assert_eq!(
        (counts.no_data, counts.failed, counts.not_queried),
        (1, 1, 1)
    );
    assert_eq!(completed_epochs(3), vec![2, 1, 0]);
    assert_eq!(completed_epochs(0), Vec::<u64>::new());
    assert_eq!(
        completed_epochs(1000),
        (970..=999).rev().collect::<Vec<_>>()
    );
    assert_eq!(completed_epochs(30).len(), 30);
    assert_eq!(completed_epochs(20).len(), 20);
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

#[test]
fn estimates_are_per_account_and_invalid_inputs_keep_the_record() {
    let entry = |byte, amount: &str, post: &str| RewardEntry {
        address: key(byte),
        state: "recorded".into(),
        latest_attempt: "recorded".into(),
        record: Some(RewardRecord {
            epoch: "7".into(),
            amount_lamports: amount.into(),
            post_balance_lamports: post.into(),
            effective_slot: "1".into(),
            commission: None,
            source_id: "reward".into(),
        }),
        account_return: None,
    };
    let entries = vec![
        entry(1, "1000000", "100001000000"),
        entry(2, "2000000", "100002000000"),
        entry(3, "9", "5"),
        entry(4, "5", "5"),
    ];
    let (epoch, coverage, invalid) = aggregate_epoch(7, entries).unwrap();
    let pct = |i: usize| {
        epoch.entries[i]
            .account_return
            .as_ref()
            .map(|r| r.annualized_percent.as_str())
    };
    assert_eq!(
        (pct(0), pct(1), pct(2), pct(3)),
        (Some("0.1827"), Some("0.3657"), None, None)
    );
    // The underflowing account is an invalid response; the zero denominator is merely Unknown.
    assert_eq!(invalid.len(), 1);
    assert_eq!(invalid[0].code, "INVALID_RESPONSE");
    assert_eq!(invalid[0].address, Some(key(3)));
    // Recorded rewards still count and total exactly, with no combined estimate.
    assert_eq!(coverage.recorded, 4);
    assert_eq!(epoch.subtotal_lamports.as_deref(), Some("3000014"));
}

fn account(byte: u8) -> Account {
    let report: ssteak::domain::Report =
        serde_json::from_str(include_str!("../docs/contracts/examples/complete.json")).unwrap();
    Account {
        address: key(byte),
        ..report.data.unwrap().accounts[0].clone()
    }
}
/// `accounts[j][i]` is account `j`'s reward at window index `i` (0 newest):
/// `Some((amount, post_balance))` is recorded, `None` is no data.
fn window(accounts: &[Vec<Option<(u64, u64)>>]) -> Vec<EpochReward> {
    (0..accounts[0].len())
        .map(|i| {
            let entries = accounts
                .iter()
                .enumerate()
                .map(|(j, series)| match series[i] {
                    Some((amount, post)) => RewardEntry {
                        address: key(j as u8 + 1),
                        state: "recorded".into(),
                        latest_attempt: "recorded".into(),
                        record: Some(RewardRecord {
                            epoch: (1000 - i as u64).to_string(),
                            amount_lamports: amount.to_string(),
                            post_balance_lamports: post.to_string(),
                            effective_slot: "1".into(),
                            commission: None,
                            source_id: "reward".into(),
                        }),
                        account_return: None,
                    },
                    None => RewardEntry {
                        address: key(j as u8 + 1),
                        state: "no_data".into(),
                        latest_attempt: "no_data".into(),
                        record: None,
                        account_return: None,
                    },
                })
                .collect();
            aggregate_epoch(1000 - i as u64, entries).unwrap().0
        })
        .collect()
}
const BALANCE: u64 = 1_000_000;

#[test]
fn comparison_pairs_recorded_epochs_and_computes_exact_differences() {
    let mut series = vec![Some((2000, BALANCE)); 15];
    series.extend(vec![Some((1000, BALANCE)); 15]);
    let rewards = window(&[series.clone()]);
    let (result, errors) = compare(&rewards, &[account(1)]).unwrap();
    assert!(errors.is_empty());
    let c = &result[0];
    assert_eq!(
        (c.compared_pairs, c.left_out_pairs, c.estimate_pairs),
        (15, 0, 15)
    );
    assert_eq!(c.current_subtotal_lamports.as_deref(), Some("30000"));
    assert_eq!(c.previous_subtotal_lamports.as_deref(), Some("15000"));
    assert_eq!(c.difference_lamports.as_deref(), Some("15000"));
    assert_eq!(c.percent_change.as_deref(), Some("100.0000"));
    let (cur, prev, diff) = (
        c.current_mean_estimate_percent.as_deref().unwrap(),
        c.previous_mean_estimate_percent.as_deref().unwrap(),
        c.estimate_difference_pp.as_deref().unwrap(),
    );
    assert!(cur.parse::<f64>().unwrap() > prev.parse::<f64>().unwrap());
    assert!(!diff.starts_with('-'));
    // Lower current rewards give a negative difference with a signed percentage.
    series.splice(0..15, vec![Some((500, BALANCE)); 15]);
    let (result, _) = compare(&window(&[series]), &[account(1)]).unwrap();
    assert_eq!(result[0].difference_lamports.as_deref(), Some("-7500"));
    assert_eq!(result[0].percent_change.as_deref(), Some("-50.0000"));
    assert!(
        result[0]
            .estimate_difference_pp
            .as_deref()
            .unwrap()
            .starts_with('-')
    );
}

#[test]
fn gaps_zero_previous_and_short_chains_stay_unknown_not_zero() {
    let mut series = vec![Some((2000, BALANCE)); 15];
    series.extend(vec![Some((1000, BALANCE)); 15]);
    // A gap in either period leaves that pair out of every sum.
    series[2] = None;
    series[20] = None;
    let (result, _) = compare(&window(&[series.clone()]), &[account(1)]).unwrap();
    let c = &result[0];
    assert_eq!((c.compared_pairs, c.left_out_pairs), (13, 2));
    assert_eq!(c.current_subtotal_lamports.as_deref(), Some("26000"));
    assert_eq!(c.previous_subtotal_lamports.as_deref(), Some("13000"));
    // A previous period of explicit zeros: exact zero subtotal, percent change Unknown.
    let mut zeros = vec![Some((2000, BALANCE)); 15];
    zeros.extend(vec![Some((0, BALANCE)); 15]);
    let (result, errors) = compare(&window(&[zeros]), &[account(1)]).unwrap();
    assert!(errors.is_empty());
    assert_eq!(result[0].previous_subtotal_lamports.as_deref(), Some("0"));
    assert!(result[0].percent_change.is_none());
    assert_eq!(
        result[0].previous_mean_estimate_percent.as_deref(),
        Some("0.0000")
    );
    // Only 20 epochs exist: five pairs. With 15 or fewer there is no previous period.
    let (result, _) = compare(&window(&[series[..20].to_vec()]), &[account(1)]).unwrap();
    assert_eq!(result[0].compared_pairs + result[0].left_out_pairs, 5);
    let (result, _) = compare(&window(&[series[..15].to_vec()]), &[account(1)]).unwrap();
    let c = &result[0];
    assert_eq!(
        (c.compared_pairs, c.left_out_pairs, c.estimate_pairs),
        (0, 0, 0)
    );
    assert!(c.current_subtotal_lamports.is_none() && c.difference_lamports.is_none());
    assert!(c.current_mean_estimate_percent.is_none() && c.estimate_difference_pp.is_none());
}

#[test]
fn u64_range_sums_are_exact_and_accounts_never_combine() {
    let mut big = vec![Some((u64::MAX - 1, u64::MAX)); 15];
    big.extend(vec![Some((1, 2)); 15]);
    let mut small = vec![Some((10, BALANCE)); 15];
    small.extend(vec![Some((20, BALANCE)); 15]);
    let (result, errors) = compare(&window(&[big, small]), &[account(1), account(2)]).unwrap();
    let (a, b) = (&result[0], &result[1]);
    assert_eq!(a.address, key(1));
    let current = 15 * u128::from(u64::MAX - 1);
    assert_eq!(a.current_subtotal_lamports, Some(current.to_string()));
    assert_eq!(a.difference_lamports, Some((current - 15).to_string()));
    // The estimate pair is not representable (pre-reward balance 1), so no mean and no
    // percent: the change exceeds the displayable range and is reported invalid.
    assert_eq!(a.estimate_pairs, 0);
    assert!(a.percent_change.is_none() && a.current_mean_estimate_percent.is_none());
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].address, Some(key(1)));
    // The other account is unaffected and independent.
    assert_eq!(b.current_subtotal_lamports.as_deref(), Some("150"));
    assert_eq!(b.percent_change.as_deref(), Some("-50.0000"));
}
