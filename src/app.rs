//! Cache-first inspection shared by the terminal and one-shot JSON.
use crate::{
    cli::{LaunchError, Options},
    domain::*,
    helius::{Helius, MAINNET_GENESIS, REWARD_BATCH_SIZE, RequestContext, RpcError},
    inspection, rewards,
    storage::{RewardAttempt, StorageError, Store},
    validators,
};
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

type Result<T> = std::result::Result<T, ReportError>;
impl From<RpcError> for ReportError {
    fn from(e: RpcError) -> Self {
        error(e.code, e.message)
    }
}
impl From<StorageError> for ReportError {
    fn from(e: StorageError) -> Self {
        error(
            if e == StorageError::UnsupportedSchema {
                "UNSUPPORTED_SCHEMA"
            } else {
                "STORAGE_FAILURE"
            },
            &e.to_string(),
        )
    }
}
impl From<DomainError> for ReportError {
    fn from(e: DomainError) -> Self {
        error("INVALID_RESPONSE", e.0)
    }
}
fn error(code: &str, message: &str) -> ReportError {
    ReportError {
        code: code.into(),
        message: message.into(),
        address: None,
    }
}
fn now() -> Option<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|t| t.as_secs())
}

pub fn failure(options: &Options, code: &'static str, message: &'static str) -> Report {
    serde_json::from_value(
        LaunchError {
            code,
            message,
            exit_code: 5,
        }
        .report(Some(options)),
    )
    .expect("fixed error schema")
}
pub fn exit_code(report: &Report) -> u8 {
    if report.errors.iter().any(|e| e.code == "INTERRUPTED") {
        130
    } else if report.errors.iter().any(|e| {
        matches!(
            e.code.as_str(),
            "STORAGE_FAILURE" | "UNSUPPORTED_SCHEMA" | "INTERNAL"
        )
    }) {
        5
    } else if report.data.is_none() {
        4
    } else if report.status == "partial" {
        3
    } else {
        0
    }
}

/// All storage and HTTP work belongs on a service worker, never the UI thread.
pub fn inspect(
    options: &Options,
    store: &Store,
    client: Option<&Helius>,
    context: &RequestContext,
    force_current: bool,
    mut progress: impl FnMut(Report),
) -> Report {
    let mut report = failure(options, "INTERNAL", "Inspection did not finish.");
    report.errors.clear();
    let mut cache_eligible = false;
    let result = (|| -> Result<()> {
        if let Some(saved) = store.load_report(&options.address)? {
            report = serde_json::from_value(saved).map_err(|_| {
                error(
                    "STORAGE_FAILURE",
                    "Saved observations are invalid; preserve the database before recovery.",
                )
            })?;
            report.input = Some(Input {
                address: options.address.clone(),
                epochs: options.epochs,
                offline: options.offline,
            });
            report.generated_at = now();
            for source in &mut report.sources {
                source.cached = true;
            }
            if let Some(data) = &mut report.data {
                cache_eligible = !data.stale;
                data.epoch_kind = "last_observed".into();
                data.stale = true;
            }
            hydrate(&mut report, store, options)?;
            finish(&mut report, options)?;
            progress(report.clone());
        }
        if options.offline {
            if report.data.is_none() {
                return Err(error(
                    "OFFLINE_MISS",
                    "No local data for this address. Inspect it online first.",
                ));
            }
            if report.coverage.as_ref().is_some_and(|c| {
                c.rewards
                    .iter()
                    .any(|r| r.recorded < report.data.as_ref().unwrap().accounts.len())
                    || c.validators != "complete"
                    || c.discovery.displayed != "complete"
            }) {
                add_error(
                    &mut report,
                    error(
                        "OFFLINE_MISS",
                        "Some requested observations are not saved locally; inspect online to fill the gaps.",
                    ),
                );
            }
            return Ok(());
        }
        let client =
            client.ok_or_else(|| error("INTERNAL", "Online transport was not initialized."))?;
        client.verify_mainnet(context)?;
        report.network.genesis_hash = Some(MAINNET_GENESIS.into());
        let (mut epoch, epoch_source) = read_epoch(client, context)?;
        let window = rewards::completed_epochs(epoch, options.epochs);
        // Freshness uses observation times, never the time a cached report was read.
        let reusable = cache_eligible
            && !force_current
            && !options.refresh
            && report.errors.is_empty()
            && report
                .data
                .as_ref()
                .is_some_and(|d| d.epoch == epoch.to_string())
            && report.coverage.as_ref().is_some_and(|c| {
                c.discovery.displayed == "complete" && c.discovery.latest_attempt == "complete"
            })
            && report
                .sources
                .iter()
                .filter(|s| {
                    matches!(
                        s.id.as_str(),
                        "input" | "staker" | "withdrawer" | "discovery"
                    )
                })
                .all(fresh);
        set_source(&mut report, epoch_source);
        if reusable {
            report.errors.clear();
            let data = report.data.as_mut().unwrap();
            data.stale = false;
            data.epoch_kind = "finalized".into();
        } else {
            current(&mut report, options, store, client, context, epoch)?;
        }
        if let Some(data) = &mut report.data {
            data.requested_epochs = window.iter().map(u64::to_string).collect();
        }
        if report.data.is_none() {
            return Ok(());
        }
        hydrate_window(&mut report, store, &window)?;
        finish(&mut report, options)?;
        progress(report.clone());
        fetch_rewards(&mut report, store, client, context, options, &mut progress)?;
        context.check()?;
        let (after, after_source) = read_epoch(client, context)?;
        if after != epoch {
            epoch = after;
            set_source(&mut report, after_source);
            // Retry current observations once; reward epoch identities remain frozen.
            current(&mut report, options, store, client, context, epoch)?;
            if let Some(data) = &mut report.data {
                data.requested_epochs = window.iter().map(u64::to_string).collect();
            }
            hydrate_window(&mut report, store, &window)?;
            fetch_rewards(&mut report, store, client, context, options, &mut progress)?;
            match read_epoch(client, context) {
                Ok((stable, _)) if stable == epoch => {}
                _ => add_error(
                    &mut report,
                    error(
                        "EPOCH_INCONSISTENCY",
                        "Epoch changed during reconciliation; retry current observations.",
                    ),
                ),
            }
        }
        finish(&mut report, options)?;
        save(&mut report, store)?;
        Ok(())
    })();
    if let Err(e) = result {
        if let Some(data) = &mut report.data {
            data.stale = true;
        }
        add_error(&mut report, e);
    }
    if let Err(e) = finish(&mut report, options) {
        add_error(&mut report, e);
        report.status = "error".into();
    }
    report
}

fn read_epoch(client: &Helius, context: &RequestContext) -> Result<(u64, Source)> {
    let value = client.call("getEpochInfo", json!([{"commitment":"finalized"}]), context)?;
    let epoch = value["epoch"].as_u64().ok_or_else(RpcError::invalid)?;
    let slot = value["absoluteSlot"]
        .as_u64()
        .ok_or_else(RpcError::invalid)?;
    Ok((
        epoch,
        rewards::observation_source("epoch".into(), Some(slot), Some("finalized"), None)?,
    ))
}
fn fresh(source: &Source) -> bool {
    now().is_some_and(|n| source.observed_at <= n && n - source.observed_at < 60)
}
fn set_source(report: &mut Report, source: Source) {
    report.sources.retain(|s| s.id != source.id);
    report.sources.push(source);
}
fn add_error(report: &mut Report, e: ReportError) {
    if !report.errors.contains(&e) {
        report.errors.push(e);
    }
}
fn save(report: &mut Report, store: &Store) -> Result<bool> {
    let result = store.save_report(
        &serde_json::to_value(&*report)
            .map_err(|_| error("INTERNAL", "Cannot encode observations."))?,
    )?;
    for (address, epoch) in result.reward_conflicts {
        conflict(report, &address, epoch);
    }
    Ok(result.published)
}
fn conflict(report: &mut Report, address: &str, epoch: u64) {
    add_error(
        report,
        ReportError {
            code: "REWARD_CONFLICT".into(),
            message: format!(
                "Epoch {epoch} revalidation disagrees with the saved reward; the original is preserved."
            ),
            address: Some(address.into()),
        },
    );
}

fn current(
    report: &mut Report,
    options: &Options,
    store: &Store,
    client: &Helius,
    context: &RequestContext,
    epoch: u64,
) -> Result<()> {
    let previous = report.clone();
    let discovery = match inspection::discover(client, &options.address, context) {
        Ok(discovery) => discovery,
        Err(e) => {
            if let Some(coverage) = &mut report.coverage {
                coverage.discovery.latest_attempt = "failed".into();
                coverage.discovery.staker_query = "not_queried".into();
                coverage.discovery.withdrawer_query = "not_queried".into();
                if let Some(data) = &mut report.data {
                    data.stale = true;
                }
                add_error(report, e.into());
                save(report, store)?;
                return Ok(());
            }
            return Err(e.into());
        }
    };
    report.errors = discovery.errors;
    let epoch_source = report.sources.iter().find(|s| s.id == "epoch").cloned();
    report.warnings.clear();
    report.sources = discovery.sources;
    if let Some(source) = epoch_source {
        report.sources.push(source);
    }
    report.coverage = Some(Coverage {
        discovery: discovery.coverage,
        validators: "not_queried".into(),
        rewards: vec![],
    });
    let complete = report.coverage.as_ref().unwrap().discovery.displayed == "complete";
    report.data = Some(Data {
        selection: discovery.selection.clone(),
        input_exists: discovery.input_exists,
        epoch: epoch.to_string(),
        epoch_kind: "finalized".into(),
        requested_epochs: rewards::completed_epochs(epoch, options.epochs)
            .iter()
            .map(u64::to_string)
            .collect(),
        stale: false,
        snapshot_kind: if complete {
            "latest_complete"
        } else {
            "latest_partial"
        }
        .into(),
        summary: summarize(&discovery.accounts, &discovery.selection, None, complete)?,
        accounts: discovery.accounts,
        validators: vec![],
        rewards: vec![],
    });
    if report.coverage.as_ref().unwrap().discovery.displayed == "not_available"
        && previous.data.is_none()
    {
        report.data = None;
        return Ok(());
    }
    // Publish discovery before reward batches so even interrupted work is resumable.
    if !save(report, store)?
        && let Some(saved) = store.load_report(&options.address)?
    {
        let mut fallback: Report = serde_json::from_value(saved)
            .map_err(|_| error("STORAGE_FAILURE", "Saved snapshot is invalid."))?;
        fallback.input = report.input.clone();
        fallback.generated_at = now();
        for s in &mut fallback.sources {
            s.cached = true;
        }
        *report = fallback;
    }
    let data = report.data.as_mut().unwrap();
    data.epoch = epoch.to_string();
    data.epoch_kind = "finalized".into();
    let accounts = data.accounts.clone();
    if accounts.iter().all(|a| a.vote_address.is_none()) {
        data.validators = vec![];
        report.coverage.as_mut().unwrap().validators = "complete".into();
    } else {
        match validators::fetch(client, &accounts, context) {
            Ok((groups, source)) => {
                report.coverage.as_mut().unwrap().validators =
                    if groups.iter().all(|v| v.state != "unknown") {
                        "complete"
                    } else {
                        "partial"
                    }
                    .into();
                report.data.as_mut().unwrap().validators = groups;
                set_source(report, source);
            }
            Err(e) => {
                let mut groups = validators::group(&accounts, None)?;
                if let Some(old) = previous.data {
                    for group in &mut groups {
                        if let Some(cached) = old
                            .validators
                            .iter()
                            .find(|v| v.vote_address == group.vote_address)
                        {
                            group.name = cached.name.clone();
                            group.commission = cached.commission;
                            group.state = cached.state.clone();
                            group.source_id = cached.source_id.clone();
                        }
                    }
                    for source in previous
                        .sources
                        .into_iter()
                        .filter(|s| groups.iter().any(|v| v.source_id.as_ref() == Some(&s.id)))
                    {
                        set_source(report, source);
                    }
                }
                report.data.as_mut().unwrap().validators = groups;
                report.coverage.as_mut().unwrap().validators = "failed".into();
                add_error(report, e.into());
            }
        }
    }
    Ok(())
}

fn hydrate(report: &mut Report, store: &Store, options: &Options) -> Result<()> {
    let epoch = report
        .data
        .as_ref()
        .ok_or_else(|| error("STORAGE_FAILURE", "Saved snapshot has no account data."))?
        .epoch
        .parse::<u64>()
        .map_err(|_| error("STORAGE_FAILURE", "Saved epoch is invalid."))?;
    let window = rewards::completed_epochs(epoch, options.epochs);
    hydrate_window(report, store, &window)
}
fn hydrate_window(report: &mut Report, store: &Store, window: &[u64]) -> Result<()> {
    let Some(data) = &mut report.data else {
        return Ok(());
    };
    data.requested_epochs = window.iter().map(u64::to_string).collect();
    let addresses = data
        .accounts
        .iter()
        .map(|a| a.address.clone())
        .collect::<Vec<_>>();
    data.rewards.clear();
    report.coverage.as_mut().unwrap().rewards.clear();
    for &epoch in window {
        let mut entries = vec![];
        for address in &addresses {
            entries.push(cached_entry(report, store, address, epoch)?);
        }
        let (reward, coverage) = rewards::aggregate_epoch(epoch, entries)?;
        report.data.as_mut().unwrap().rewards.push(reward);
        report.coverage.as_mut().unwrap().rewards.push(coverage);
    }
    Ok(())
}
fn cached_entry(
    report: &mut Report,
    store: &Store,
    address: &str,
    epoch: u64,
) -> Result<RewardEntry> {
    let mut entry = RewardEntry {
        address: address.into(),
        state: "not_queried".into(),
        latest_attempt: "not_queried".into(),
        record: None,
    };
    if let Some(saved) = store.reward(MAINNET_GENESIS, address, epoch)? {
        entry.latest_attempt = saved.latest_attempt.clone();
        entry.state = saved.latest_attempt;
        if let Some(record) = saved.record {
            entry.record = Some(
                serde_json::from_value(record)
                    .map_err(|_| error("STORAGE_FAILURE", "Saved reward is invalid."))?,
            );
            entry.state = "recorded".into();
            let mut source: Source = serde_json::from_value(
                saved
                    .source
                    .ok_or_else(|| error("STORAGE_FAILURE", "Saved reward source is missing."))?,
            )
            .map_err(|_| error("STORAGE_FAILURE", "Saved reward source is invalid."))?;
            source.cached = true;
            if !report.sources.iter().any(|s| s.id == source.id) {
                report.sources.push(source);
            }
        }
        if saved.error_code.as_deref() == Some("REWARD_CONFLICT") {
            conflict(report, address, epoch);
        }
    }
    Ok(entry)
}

fn fetch_rewards(
    report: &mut Report,
    store: &Store,
    client: &Helius,
    context: &RequestContext,
    options: &Options,
    progress: &mut impl FnMut(Report),
) -> Result<()> {
    let count = report.data.as_ref().unwrap().rewards.len();
    for index in 0..count {
        let reward = &report.data.as_ref().unwrap().rewards[index];
        let epoch = reward
            .epoch
            .parse::<u64>()
            .map_err(|_| error("INTERNAL", "Invalid reward window."))?;
        let missing = reward
            .entries
            .iter()
            .filter(|e| options.refresh || e.record.is_none())
            .map(|e| e.address.clone())
            .collect::<Vec<_>>();
        for addresses in missing.chunks(REWARD_BATCH_SIZE) {
            context.check()?;
            let (entries, source, failure) =
                match rewards::fetch_batch(client, addresses, epoch, context) {
                    Ok((entries, source)) => (entries, Some(source), None),
                    Err(e) => {
                        let entries = addresses
                            .iter()
                            .map(|a| RewardEntry {
                                address: a.clone(),
                                state: "failed".into(),
                                latest_attempt: "failed".into(),
                                record: None,
                            })
                            .collect();
                        (entries, None, Some(e))
                    }
                };
            if let Some(source) = source.clone() {
                set_source(report, source);
            }
            let attempts = entries
                .iter()
                .map(|entry| RewardAttempt {
                    address: entry.address.clone(),
                    epoch,
                    state: entry.latest_attempt.clone(),
                    record: entry
                        .record
                        .as_ref()
                        .map(|r| serde_json::to_value(r).expect("reward serialization")),
                    source: source
                        .as_ref()
                        .map(|s| serde_json::to_value(s).expect("source serialization")),
                    observed_at: now().and_then(|t| i64::try_from(t).ok()),
                    error_code: failure.as_ref().map(|e| e.code.into()),
                })
                .collect::<Vec<_>>();
            let saved = store.commit_reward_batch(MAINNET_GENESIS, &attempts);
            for entry in entries {
                let resolved = if saved.is_ok() {
                    cached_entry(report, store, &entry.address, epoch)?
                } else {
                    let prior = report.data.as_ref().unwrap().rewards[index]
                        .entries
                        .iter()
                        .find(|e| e.address == entry.address)
                        .unwrap();
                    if prior.record.is_some() {
                        let mut old = prior.clone();
                        old.latest_attempt = entry.latest_attempt;
                        old
                    } else {
                        entry
                    }
                };
                let old = report.data.as_mut().unwrap().rewards[index]
                    .entries
                    .iter_mut()
                    .find(|e| e.address == resolved.address)
                    .unwrap();
                *old = resolved;
            }
            match saved {
                Ok(conflicts) => {
                    for (address, epoch) in conflicts {
                        conflict(report, &address, epoch);
                    }
                }
                Err(e) => add_error(report, e.into()),
            }
            if let Some(e) = failure {
                add_error(report, e.into());
            }
            finish(report, options)?;
            progress(report.clone());
            context.check()?;
        }
    }
    Ok(())
}

fn finish(report: &mut Report, options: &Options) -> Result<()> {
    if let (Some(data), Some(coverage)) = (&mut report.data, &mut report.coverage) {
        coverage.rewards.clear();
        for epoch in &mut data.rewards {
            let (value, count) = rewards::aggregate_epoch(
                parse_amount(&epoch.epoch)?,
                std::mem::take(&mut epoch.entries),
            )?;
            *epoch = value;
            coverage.rewards.push(count);
        }
        let latest = data
            .rewards
            .first()
            .and_then(|r| r.subtotal_lamports.as_deref())
            .map(|n| n.parse::<u128>())
            .transpose()
            .map_err(|_| error("INTERNAL", "Invalid reward subtotal."))?;
        data.summary = summarize(
            &data.accounts,
            &data.selection,
            latest,
            coverage.discovery.displayed == "complete",
        )?;
        report.warnings =
            validators::findings(&data.accounts, &data.validators, coverage, &report.sources);
        if data.requested_epochs.len() < usize::from(options.epochs) {
            report.warnings.push(Finding {
                code: "EPOCH_RANGE_SHORTENED".into(),
                severity: "info".into(),
                address: None,
                evidence: vec![format!(
                    "{} completed epochs are available.",
                    data.requested_epochs.len()
                )],
                observed_at: now(),
                slot: None,
                message: "Fewer completed epochs exist than requested.".into(),
            });
        }
        for e in &report.errors {
            let code = match e.code.as_str() {
                "STORAGE_FAILURE" | "UNSUPPORTED_SCHEMA" => "NOT_SAVED_LOCALLY",
                "EPOCH_INCONSISTENCY" => "EPOCH_INCONSISTENCY",
                "REWARD_CONFLICT" => "REWARD_CONFLICT",
                _ => continue,
            };
            report.warnings.push(Finding {
                code: code.into(),
                severity: "warning".into(),
                address: e.address.clone(),
                evidence: vec![e.message.clone()],
                observed_at: now(),
                slot: None,
                message: if code == "NOT_SAVED_LOCALLY" {
                    "Not saved locally; check the database and retry.".into()
                } else {
                    e.message.clone()
                },
            });
        }
        let partial = (!options.offline && data.stale)
            || !report.errors.is_empty()
            || coverage.discovery.displayed != "complete"
            || coverage.validators != "complete"
            || coverage
                .rewards
                .iter()
                .any(|r| r.no_data + r.failed + r.not_queried > 0);
        report.status = if partial { "partial" } else { "complete" }.into();
    } else {
        report.status = "error".into();
    }
    match exit_code(report) {
        130 => report.status = "interrupted".into(),
        4 | 5 => report.status = "error".into(),
        _ => {}
    }
    Ok(())
}
