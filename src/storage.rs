//! SQLite persistence. Calls block on a bounded worker and belong on a service thread.
use rusqlite::{Connection, TransactionBehavior};
use std::{
    fmt,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    time::Duration,
};

const SCHEMA_VERSION: u32 = 1;
const INITIAL_SCHEMA: &str = include_str!("../migrations/001_initial.sql");

type Result<T> = std::result::Result<T, StorageError>;
type Job = Box<dyn FnOnce(&mut Connection) + Send>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageError {
    Failure,
    UnsupportedSchema,
    InvalidData,
}
impl StorageError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Failure => "STORAGE_FAILURE",
            Self::UnsupportedSchema => "UNSUPPORTED_SCHEMA",
            Self::InvalidData => "INVALID_RESPONSE",
        }
    }
}
impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Failure => "Local storage failed. Check the database location, free space and permissions; preserve the file before recovery.",
            Self::UnsupportedSchema => "This database needs a newer ssteak version; it was not modified.",
            Self::InvalidData => "The observation cannot be saved because its data is invalid.",
        })
    }
}
impl std::error::Error for StorageError {}
impl From<rusqlite::Error> for StorageError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Failure
    }
}
impl From<std::io::Error> for StorageError {
    fn from(_: std::io::Error) -> Self {
        Self::Failure
    }
}
impl From<serde_json::Error> for StorageError {
    fn from(_: serde_json::Error) -> Self {
        Self::InvalidData
    }
}

#[derive(Clone, Debug)]
pub struct Store {
    sender: mpsc::SyncSender<Job>,
    path: Arc<PathBuf>,
}
impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = Arc::new(path.as_ref().to_path_buf());
        let worker_path = Arc::clone(&path);
        let (sender, jobs) = mpsc::sync_channel::<Job>(32);
        let (ready, opened) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("ssteak-storage".into())
            .spawn(move || match open_connection(&worker_path) {
                Ok(mut conn) => {
                    if ready.send(Ok(())).is_err() {
                        return;
                    }
                    for job in jobs {
                        job(&mut conn);
                    }
                }
                Err(error) => {
                    let _ = ready.send(Err(error));
                }
            })?;
        opened.recv().map_err(|_| StorageError::Failure)??;
        Ok(Self { sender, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn call<T: Send + 'static>(
        &self,
        operation: impl FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let (reply, result) = mpsc::sync_channel(1);
        self.sender
            .send(Box::new(move |conn| {
                let _ = reply.send(operation(conn));
            }))
            .map_err(|_| StorageError::Failure)?;
        result.recv().map_err(|_| StorageError::Failure)?
    }

    /// Useful when shutting down a service: confirms all prior jobs have completed.
    pub fn flush(&self) -> Result<()> {
        self.call(|_| Ok(()))
    }
}

pub fn default_path() -> Result<PathBuf> {
    directories::BaseDirs::new()
        .map(|dirs| dirs.data_dir().join("ssteak/ssteak.sqlite3"))
        .ok_or(StorageError::Failure)
}

fn private_file(path: &Path) -> Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn private_permissions(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

fn migration_backup(conn: &Connection, path: &Path, version: u32) -> Result<()> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut name = path.as_os_str().to_os_string();
    name.push(format!(".pre-v{version}.sqlite3"));
    let target = PathBuf::from(name);
    let mut name = target.as_os_str().to_os_string();
    name.push(format!(
        ".tmp-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let temp = PathBuf::from(name);
    let result = (|| {
        private_file(&temp)?;
        conn.backup(rusqlite::MAIN_DB, &temp, None)?;
        let backup = Connection::open(&temp)?;
        let check: String = backup.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        if check != "ok" {
            return Err(StorageError::Failure);
        }
        drop(backup);
        std::fs::rename(&temp, &target)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn open_connection(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(parent)?;
    }
    if path.exists() && std::fs::metadata(path)?.permissions().readonly() {
        return Err(StorageError::Failure);
    }
    private_file(path)?;
    let mut conn = Connection::open(path)?;
    conn.busy_timeout(Duration::from_secs(5))?;
    let version: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(StorageError::UnsupportedSchema);
    }
    private_permissions(path)?;
    conn.execute_batch(
        "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;",
    )?;
    if version < SCHEMA_VERSION {
        migrate(&mut conn, path)?;
    }
    Ok(conn)
}

fn migrate(conn: &mut Connection, path: &Path) -> Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let version: u32 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version > SCHEMA_VERSION {
        return Err(StorageError::UnsupportedSchema);
    }
    if version == 0 {
        let tables: u32 = tx.query_row(
            "SELECT count(*) FROM sqlite_master WHERE type='table'",
            [],
            |r| r.get(0),
        )?;
        if tables > 0 {
            // The write reservation prevents competing migrations. A separate reader
            // backs up committed WAL state without backing up our write transaction.
            let source =
                Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            source.busy_timeout(Duration::from_secs(5))?;
            source.execute_batch("PRAGMA foreign_keys=ON;")?;
            migration_backup(&source, path, version)?;
        }
        tx.execute_batch(INITIAL_SCHEMA)?;
        tx.execute("INSERT INTO schema_history VALUES (1,unixepoch())", [])?;
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    fn store() -> Store {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Store::open(std::env::temp_dir().join(format!(
            "ssteak-fault-{}-{}.sqlite3",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
        .unwrap()
    }
    #[test]
    fn disk_full_failure_rolls_back_transaction_and_worker_remains_usable() {
        let store = store();
        store.call(|conn| { conn.execute_batch("CREATE TABLE fault(value BLOB); INSERT INTO fault VALUES ('committed'); PRAGMA max_page_count=40;")?; Ok(()) }).unwrap();
        assert!(
            store
                .call(|conn| {
                    let tx = conn.transaction()?;
                    tx.execute("INSERT INTO fault VALUES ('uncommitted')", [])?;
                    tx.execute("INSERT INTO fault VALUES (zeroblob(1000000))", [])?;
                    tx.commit()?;
                    Ok(())
                })
                .is_err()
        );
        assert_eq!(
            store
                .call(|conn| Ok(
                    conn.query_row("SELECT count(*) FROM fault", [], |r| r.get::<_, i32>(0))?
                ))
                .unwrap(),
            1
        );
    }
    #[test]
    fn independent_instances_use_bounded_lock_wait_and_keep_committed_data() {
        let store = store();
        let other = Store::open(store.path()).unwrap();
        let lock = Connection::open(store.path()).unwrap();
        lock.execute_batch("BEGIN IMMEDIATE;").unwrap();
        let start = std::time::Instant::now();
        assert!(
            other
                .call(|conn| {
                    conn.execute("INSERT INTO networks VALUES ('x','mainnet','1',NULL)", [])?;
                    Ok(())
                })
                .is_err()
        );
        assert!(start.elapsed() >= Duration::from_secs(4));
        assert!(start.elapsed() < Duration::from_secs(8));
        lock.execute_batch("ROLLBACK;").unwrap();
        other
            .call(|conn| {
                conn.execute("INSERT INTO networks VALUES ('x','mainnet','1',NULL)", [])?;
                Ok(())
            })
            .unwrap();
        assert_eq!(
            store
                .call(|conn| Ok(
                    conn.query_row("SELECT count(*) FROM networks", [], |r| r.get::<_, i32>(0))?
                ))
                .unwrap(),
            1
        );
    }
}

use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Default)]
pub struct SaveOutcome {
    pub published: bool,
    pub reward_conflicts: Vec<(String, u64)>,
}

impl Store {
    pub fn save_report(&self, report: &Value) -> Result<SaveOutcome> {
        validate_report(report)?;
        let report = report.clone();
        self.call(move |conn| save_snapshot(conn, &report))
    }

    pub fn load_report(&self, address: &str) -> Result<Option<Value>> {
        let address = address.to_owned();
        self.call(move |conn| {
            let saved:Option<(String,String,i64,i64,String)>=conn.query_row(
                "SELECT displayed.report_json,attempt.report_json,displayed.id,attempt.id,n.last_epoch FROM address_heads h JOIN networks n ON n.genesis_hash=h.network JOIN snapshots displayed ON displayed.id=COALESCE(h.latest_complete,h.latest_attempt) JOIN snapshots attempt ON attempt.id=h.latest_attempt WHERE h.address=?1 ORDER BY h.latest_attempt DESC LIMIT 1",
                [address],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
            let Some((saved,attempt,displayed_id,attempt_id,epoch))=saved else { return Ok(None); };
            let mut report:Value=serde_json::from_str(&saved)?;
            // Version-1 snapshots are never served as v2 data; rewards are reused by key.
            if report["schema_version"]!=2 || serde_json::from_str::<Value>(&attempt)?["schema_version"]!=2 { return Ok(None); }
            if report["data"]["epoch"]!=epoch { report["data"]["stale"]=json!(true); }
            report["data"]["epoch"]=json!(epoch);
            report["data"]["epoch_kind"]=json!("last_observed");
            if displayed_id!=attempt_id {
                let attempt:Value=serde_json::from_str(&attempt)?;
                let displayed=report["coverage"]["discovery"]["displayed"].clone();
                report["coverage"]["discovery"]=attempt["coverage"]["discovery"].clone();
                report["coverage"]["discovery"]["displayed"]=displayed;
                report["data"]["stale"]=json!(true);
                report["status"]=json!("partial");
                report["errors"]=attempt["errors"].clone();
                for warning in array(&attempt["warnings"])? {
                    if !array(&report["warnings"])?.contains(warning) { report["warnings"].as_array_mut().ok_or(StorageError::InvalidData)?.push(warning.clone()); }
                }
                // Keep baseline IDs intact; latest-attempt provenance gets its own IDs.
                for source in array(&attempt["sources"])? {
                    let mut source=source.clone();source["id"]=json!(format!("attempt-{}",string(&source["id"])?.trim_start_matches("attempt-")));
                    let sources=report["sources"].as_array_mut().ok_or(StorageError::InvalidData)?;
                    if let Some(existing)=sources.iter_mut().find(|s|s["id"]==source["id"]) { *existing=source; } else { sources.push(source); }
                }
            }
            Ok(Some(report))
        })
    }
}

fn string(value: &Value) -> Result<&str> {
    value.as_str().ok_or(StorageError::InvalidData)
}
fn array(value: &Value) -> Result<&Vec<Value>> {
    value.as_array().ok_or(StorageError::InvalidData)
}
fn chain(value: &Value) -> Result<u64> {
    crate::domain::parse_amount(string(value)?).map_err(|_| StorageError::InvalidData)
}
fn source<'a>(report: &'a Value, id: &str) -> Result<&'a Value> {
    array(&report["sources"])?
        .iter()
        .find(|s| s["id"] == id)
        .ok_or(StorageError::InvalidData)
}
fn clean(value: &Value) -> bool {
    match value {
        Value::String(s) => {
            !s.contains("://") && !s.contains("api-key=") && !s.chars().any(char::is_control)
        }
        Value::Object(o) => o.values().all(clean),
        Value::Array(a) => a.iter().all(clean),
        _ => true,
    }
}
fn validate_report(report: &Value) -> Result<()> {
    let parsed: crate::domain::Report = serde_json::from_value(report.clone())?;
    if parsed.schema_version != 2 || parsed.network.cluster != "mainnet" || !clean(report) {
        return Err(StorageError::InvalidData);
    }
    let input = parsed.input.ok_or(StorageError::InvalidData)?;
    if !crate::domain::validate_address(&input.address)
        || !crate::domain::validate_address(string(&report["network"]["genesis_hash"])?)
    {
        return Err(StorageError::InvalidData);
    }
    chain(&report["data"]["epoch"])?;
    for epoch in array(&report["data"]["requested_epochs"])? {
        chain(epoch)?;
    }
    let mut ids = BTreeSet::new();
    for s in array(&report["sources"])? {
        let id = string(&s["id"])?;
        if !ids.insert(id)
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(StorageError::InvalidData);
        }
        if !s["slot"].is_null() {
            chain(&s["slot"])?;
        }
    }
    let mut accounts = BTreeSet::new();
    for account in array(&report["data"]["accounts"])? {
        let address = string(&account["address"])?;
        if !crate::domain::validate_address(address) || !accounts.insert(address) {
            return Err(StorageError::InvalidData);
        }
        source(report, string(&account["source_id"])?)?;
        for field in [
            "balance_lamports",
            "delegated_lamports",
            "rent_reserve_lamports",
            "undelegated_lamports",
            "activation_epoch",
            "deactivation_epoch",
        ] {
            if !account[field].is_null() {
                chain(&account[field])?;
            }
        }
        if !account["lockup"].is_null() {
            chain(&account["lockup"]["epoch"])?;
        }
    }
    for validator in array(&report["data"]["validators"])? {
        if !validator["source_id"].is_null() {
            source(report, string(&validator["source_id"])?)?;
        }
    }
    Ok(())
}
fn ordering(report: &Value) -> Result<BTreeMap<String, u64>> {
    let mut slots = BTreeMap::new();
    for s in array(&report["sources"])? {
        let id = string(&s["id"])?;
        if matches!(
            id,
            "input" | "staker" | "withdrawer" | "epoch" | "discovery"
        ) && s["commitment"] == "finalized"
            && !s["slot"].is_null()
        {
            slots.insert(id.to_owned(), chain(&s["slot"])?);
        }
    }
    Ok(slots)
}
fn can_replace(previous: &Value, candidate: &Value) -> Result<bool> {
    let old = ordering(previous)?;
    let new = ordering(candidate)?;
    if old.is_empty() || old.keys().ne(new.keys()) || old.iter().any(|(id, slot)| new[id] < *slot) {
        return Ok(false);
    }
    let old_accounts = array(&previous["data"]["accounts"])?;
    let new_accounts = array(&candidate["data"]["accounts"])?;
    for (accounts, other) in [(old_accounts, new_accounts), (new_accounts, old_accounts)] {
        for account in accounts {
            if other.iter().any(|a| a == account) {
                continue;
            }
            let id = string(&account["source_id"])?;
            if old
                .get(id)
                .zip(new.get(id))
                .is_none_or(|(old, new)| new <= old)
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
fn complete(report: &Value) -> Result<bool> {
    let coverage = &report["coverage"]["discovery"];
    if coverage["displayed"] != "complete" || coverage["latest_attempt"] != "complete" {
        return Ok(false);
    }
    match string(&report["data"]["selection"])? {
        "direct" => Ok(array(&report["data"]["accounts"])?.len() == 1
            && report["data"]["accounts"][0]["address"] == report["input"]["address"]
            && report["data"]["accounts"][0]["relationship"].is_null()),
        "authority" => {
            Ok(coverage["staker_query"] == "succeeded"
                && coverage["withdrawer_query"] == "succeeded")
        }
        _ => Err(StorageError::InvalidData),
    }
}
fn save_snapshot(conn: &mut Connection, report: &Value) -> Result<SaveOutcome> {
    let network = string(&report["network"]["genesis_hash"])?;
    let address = string(&report["input"]["address"])?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let epoch = chain(&report["data"]["epoch"])?;
    let observed = report["generated_at"].as_i64();
    let prior_epoch: Option<String> = tx
        .query_row(
            "SELECT last_epoch FROM networks WHERE genesis_hash=?1",
            [network],
            |r| r.get(0),
        )
        .optional()?;
    if prior_epoch
        .as_ref()
        .is_none_or(|old| old.parse::<u64>().is_ok_and(|old| epoch >= old))
    {
        tx.execute("INSERT INTO networks VALUES (?1,'mainnet',?2,?3) ON CONFLICT(genesis_hash) DO UPDATE SET last_epoch=excluded.last_epoch,observed_at=excluded.observed_at",params![network,epoch.to_string(),observed])?;
    }
    let baseline:Option<(i64,String)>=tx.query_row("SELECT s.id,s.report_json FROM address_heads h JOIN snapshots s ON s.id=h.latest_complete WHERE h.network=?1 AND h.address=?2",params![network,address],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    let is_complete = complete(report)?;
    let publish = is_complete
        && match &baseline {
            None => true,
            Some((_, old)) => can_replace(&serde_json::from_str::<Value>(old)?, report)?,
        };
    tx.execute("INSERT INTO snapshots(network,address,selection,complete,report_json,ordering_json) VALUES (?1,?2,?3,?4,?5,?6)",params![network,address,string(&report["data"]["selection"])?,is_complete,report.to_string(),serde_json::to_string(&ordering(report)?)?])?;
    let id = tx.last_insert_rowid();
    for account in array(&report["data"]["accounts"])? {
        let account_address = string(&account["address"])?;
        tx.execute(
            "INSERT INTO account_observations VALUES (?1,?2,?3,?4)",
            params![
                id,
                account_address,
                account.to_string(),
                source(report, string(&account["source_id"])?)?.to_string()
            ],
        )?;
        tx.execute(
            "INSERT INTO members VALUES (?1,?2,?3)",
            params![id, account_address, account["relationship"].as_str()],
        )?;
    }
    for validator in array(&report["data"]["validators"])? {
        let Some(source_id) = validator["source_id"].as_str() else {
            continue;
        };
        let source = source(report, source_id)?;
        let vote = string(&validator["vote_address"])?;
        let old: Option<String> = tx
            .query_row(
                "SELECT source_json FROM validators WHERE network=?1 AND vote_address=?2",
                params![network, vote],
                |r| r.get(0),
            )
            .optional()?;
        let replace = match old {
            None => true,
            Some(old) => {
                let old: Value = serde_json::from_str(&old)?;
                old["id"] == source["id"]
                    && old["commitment"] == source["commitment"]
                    && !old["slot"].is_null()
                    && !source["slot"].is_null()
                    && chain(&source["slot"])? >= chain(&old["slot"])?
            }
        };
        if replace {
            tx.execute("INSERT INTO validators VALUES (?1,?2,?3,?4) ON CONFLICT(network,vote_address) DO UPDATE SET validator_json=excluded.validator_json,source_json=excluded.source_json",params![network,vote,validator.to_string(),source.to_string()])?;
        }
    }
    let complete_id = if publish {
        Some(id)
    } else {
        baseline.map(|(id, _)| id)
    };
    tx.execute("INSERT INTO address_heads VALUES (?1,?2,?3,?4) ON CONFLICT(network,address) DO UPDATE SET latest_attempt=excluded.latest_attempt,latest_complete=excluded.latest_complete",params![network,address,id,complete_id])?;
    // Only the latest attempt and complete baseline are needed; durable rewards are independent.
    tx.execute("DELETE FROM snapshots WHERE network=?1 AND address=?2 AND id!=?3 AND (?4 IS NULL OR id!=?4)",params![network,address,id,complete_id])?;
    let reward_conflicts = write_rewards(&tx, network, &report_reward_attempts(report)?)?;
    tx.commit()?;
    Ok(SaveOutcome {
        published: publish,
        reward_conflicts,
    })
}

#[derive(Clone, Debug)]
pub struct RewardAttempt {
    pub address: String,
    pub epoch: u64,
    pub state: String,
    pub record: Option<Value>,
    pub source: Option<Value>,
    pub observed_at: Option<i64>,
    pub error_code: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CachedReward {
    pub record: Option<Value>,
    pub source: Option<Value>,
    pub latest_attempt: String,
    pub error_code: Option<String>,
}

impl Store {
    /// Commits at most 1,000 entries atomically. Provider batches are smaller.
    pub fn commit_reward_batch(
        &self,
        network: &str,
        attempts: &[RewardAttempt],
    ) -> Result<Vec<(String, u64)>> {
        if attempts.len() > 1000 || !crate::domain::validate_address(network) {
            return Err(StorageError::InvalidData);
        }
        let network = network.to_owned();
        let attempts = attempts.to_vec();
        self.call(move |conn| {
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let conflicts = write_rewards(&tx, &network, &attempts)?;
            tx.commit()?;
            Ok(conflicts)
        })
    }

    pub fn reward(&self, network: &str, address: &str, epoch: u64) -> Result<Option<CachedReward>> {
        let network = network.to_owned();
        let address = address.to_owned();
        self.call(move |conn| {
            let row=conn.query_row(
                "SELECT r.record_json,r.source_json,c.state,c.error_code FROM reward_coverage c LEFT JOIN rewards r ON r.network=c.network AND r.address=c.address AND r.epoch=c.epoch WHERE c.network=?1 AND c.address=?2 AND c.epoch=?3",
                params![network,address,epoch.to_string()],|r|Ok((r.get::<_,Option<String>>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,String>(2)?,r.get::<_,Option<String>>(3)?))).optional()?;
            row.map(|(record,source,state,error)|Ok(CachedReward {
                record:record.map(|s|serde_json::from_str(&s)).transpose()?,source:source.map(|s|serde_json::from_str(&s)).transpose()?,latest_attempt:state,error_code:error,
            })).transpose()
        })
    }
}

fn write_rewards(
    conn: &Connection,
    network: &str,
    attempts: &[RewardAttempt],
) -> Result<Vec<(String, u64)>> {
    let mut conflicts = Vec::new();
    for attempt in attempts {
        if !crate::domain::validate_address(&attempt.address)
            || !matches!(
                attempt.state.as_str(),
                "recorded" | "no_data" | "failed" | "not_queried"
            )
            || (attempt.state == "recorded" && attempt.record.is_none())
            || attempt.observed_at.is_some_and(|t| t < 0)
            || attempt.error_code.as_ref().is_some_and(|s| {
                s.is_empty() || !s.bytes().all(|b| b.is_ascii_uppercase() || b == b'_')
            })
        {
            return Err(StorageError::InvalidData);
        }
        let mut conflict = false;
        if let Some(record) = &attempt.record {
            let parsed: crate::domain::RewardRecord = serde_json::from_value(record.clone())?;
            let source = attempt.source.as_ref().ok_or(StorageError::InvalidData)?;
            let parsed_source: crate::domain::Source = serde_json::from_value(source.clone())?;
            if !clean(record)
                || !clean(source)
                || parsed_source.id != parsed.source_id
                || chain(&record["epoch"])? != attempt.epoch
                || parsed.commission.is_some_and(|n| n > 100)
            {
                return Err(StorageError::InvalidData);
            }
            chain(&record["amount_lamports"])?;
            chain(&record["post_balance_lamports"])?;
            chain(&record["effective_slot"])?;
            if !source["slot"].is_null() {
                chain(&source["slot"])?;
            }
            let old: Option<String> = conn
                .query_row(
                    "SELECT record_json FROM rewards WHERE network=?1 AND address=?2 AND epoch=?3",
                    params![network, attempt.address, attempt.epoch.to_string()],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(old) = old {
                let old: Value = serde_json::from_str(&old)?;
                conflict = [
                    "epoch",
                    "amount_lamports",
                    "post_balance_lamports",
                    "effective_slot",
                    "commission",
                ]
                .iter()
                .any(|field| old[field] != record[field]);
                if conflict {
                    conflicts.push((attempt.address.clone(), attempt.epoch));
                }
            } else {
                conn.execute(
                    "INSERT INTO rewards VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                    params![
                        network,
                        attempt.address,
                        attempt.epoch.to_string(),
                        parsed.amount_lamports,
                        parsed.post_balance_lamports,
                        parsed.effective_slot,
                        parsed.commission,
                        record.to_string(),
                        source.to_string()
                    ],
                )?;
            }
        }
        let error = if conflict {
            Some("REWARD_CONFLICT")
        } else {
            attempt.error_code.as_deref()
        };
        conn.execute("INSERT INTO reward_coverage VALUES (?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(network,address,epoch) DO UPDATE SET observed_at=excluded.observed_at,state=excluded.state,error_code=CASE WHEN reward_coverage.error_code='REWARD_CONFLICT' THEN reward_coverage.error_code ELSE excluded.error_code END,retry_eligible=CASE WHEN reward_coverage.error_code='REWARD_CONFLICT' THEN 1 ELSE excluded.retry_eligible END WHERE (excluded.observed_at IS NOT NULL AND (reward_coverage.observed_at IS NULL OR excluded.observed_at>=reward_coverage.observed_at)) OR (excluded.observed_at IS NULL AND reward_coverage.observed_at IS NULL)",params![network,attempt.address,attempt.epoch.to_string(),attempt.observed_at,attempt.state,error,attempt.state!="recorded"||conflict])?;
        if conflict {
            conn.execute("UPDATE reward_coverage SET error_code='REWARD_CONFLICT',retry_eligible=1 WHERE network=?1 AND address=?2 AND epoch=?3",params![network,attempt.address,attempt.epoch.to_string()])?;
        }
    }
    Ok(conflicts)
}

fn report_reward_attempts(report: &Value) -> Result<Vec<RewardAttempt>> {
    let mut attempts = Vec::new();
    for epoch in array(&report["data"]["rewards"])? {
        let number = chain(&epoch["epoch"])?;
        for entry in array(&epoch["entries"])? {
            let record = if entry["record"].is_null() {
                None
            } else {
                Some(entry["record"].clone())
            };
            let source = record
                .as_ref()
                .map(|record| source(report, string(&record["source_id"])?).cloned())
                .transpose()?;
            attempts.push(RewardAttempt {
                address: string(&entry["address"])?.to_owned(),
                epoch: number,
                state: string(&entry["latest_attempt"])?.to_owned(),
                record,
                source,
                observed_at: report["generated_at"].as_i64(),
                error_code: None,
            });
        }
    }
    Ok(attempts)
}

#[cfg(test)]
#[test]
fn delayed_migration_rechecks_version_under_lock_before_touching_backup() {
    let path = std::env::temp_dir().join(format!(
        "ssteak-delayed-upgrade-{}.sqlite3",
        std::process::id()
    ));
    let mut conn = Connection::open(&path).unwrap();
    conn.execute_batch("CREATE TABLE legacy(value TEXT); INSERT INTO legacy VALUES ('original');")
        .unwrap();
    let _store = Store::open(&path).unwrap();
    let backup = PathBuf::from(format!("{}.pre-v0.sqlite3", path.display()));
    let original_backup = std::fs::read(&backup).unwrap();
    // A second opener may have observed version zero before the first upgraded.
    migrate(&mut conn, &path).unwrap();
    assert_eq!(std::fs::read(backup).unwrap(), original_backup);
}
