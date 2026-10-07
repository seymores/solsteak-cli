# Frozen SQLite contract v1

This contract resolves spec.md DB-01–DB-10 and AC-27–AC-37. `storage.sql` is
schema version 1. `PRAGMA user_version` and `schema_history` record migrations.
Network identity is the verified mainnet genesis hash, never an endpoint or key.

## Records and exact values

Snapshots have database-allocated identities, authority/direct selection,
independent coverage and source observations, and membership referencing immutable
account observations. `address_heads` separately points to the latest attempt and
latest accepted complete snapshot. Direct snapshots contain exactly the inspected
account with null relationship; both authority queries must succeed for complete
authority discovery. Failed/partial attempts never replace the complete pointer.
A successful empty authority snapshot is complete. Membership, observations,
coverage, and pointers publish in one transaction. Reward identities do not depend
on snapshots and cannot be deleted by membership changes.

Report JSON is a serialization of the saved view, not an independent authority for
derived totals or findings. The service recomputes those when reconstructing a
view, and resolves durable rewards separately for the requested epoch range.
Account/validator JSON preserves all decoded fields and source references; source
JSON preserves the observation time, slot, provider, commitment and decoder.
All chain u64 values use canonical decimal strings, including sentinel epochs.
Amount totals may use canonical u128 strings. Validate bounds in Rust; no SQL
numeric casts, arithmetic, aggregation or monetary TEXT sorting. Null stays null.
No API keys, private material, endpoint URLs, or uncontrolled error messages are
accepted by the storage adapter. Error codes are uppercase identifiers only.

## Ordering and conflict rules

Process-local request counters and wall-clock time never order snapshots across
processes. Extract a map of finalized discovery/account-read source ID to u64 slot from
each report (`input`, `staker`, `withdrawer`, and optional `epoch`; legacy fixture
ID `discovery` is also accepted). Exclude reward and validator sources so changes
to lookback or enrichment cannot affect membership ordering. Sources without
finalized commitment are not discovery ordering evidence. Complete
snapshots replace the baseline only if source keys match and every source slot is
nondecreasing. The first complete snapshot establishes the baseline. Missing slots,
different source keys, or any lower slot keep the candidate as attempt metadata
without replacing the baseline. Equal account-source slots may refresh report/reward coverage only if the
corresponding account fields and membership are identical. Advancing an unrelated
epoch/input source does not authorize changing an account observed at the same
slot.
Do not invent one atomic slot from independent calls. Latest attempt means last
locally committed attempt, not a claim that its chain data is newer.

Validator replacement similarly requires the same source identity/commitment and a
nondecreasing known slot. Incomparable validator observations remain in their
snapshot report, without replacing shared current observations. Network epoch only
advances, never rolls back when a late process saves an older view.

A numeric reward is unique by network, account and requested epoch. The returned
epoch must match. Preserve the first committed numeric record and its original
source. A later null/error updates attempt coverage but never erases the record.
Revalidation compares epoch, amount, post-balance, effective slot and nullable
commission; differing values preserve the original and report `REWARD_CONFLICT`.
The conflict marker persists until a separately defined reconciliation policy;
resaving a view cannot clear it. Identical values are idempotent, including explicit
zero. Coverage and numeric
records commit atomically per bounded batch. Missing rows mean not queried;
no_data/failed rows remain retry eligible. Coverage ordering uses retrieval time
only for attempt metadata; known numeric records remain immutable regardless.

## Lifecycle and public adapter

Default path uses the platform data-directory API followed by
`ssteak/ssteak.sqlite3` (Application Support on macOS). There is no CLI or
environment path override in v1; `Store::open(path)` permits isolated test files.
Use a local filesystem outside live cloud synchronization. Expose the resolved
path in help. New directories are mode 0700 and database/backup files 0600 on Unix;
WAL/SHM inherit database permissions. Do not chmod unrelated existing parents.

Every connection enables foreign keys, WAL, synchronous FULL and 5-second busy
timeout. One worker owns the connection per Store with a bounded 32-command queue.
Clone handles share it. Synchronous adapter calls await that worker and must be
made on application service workers, never in drawing or event handling. SQLite
serializes independent processes. No network call takes place in a transaction.

`Store::open`, `path`, `load_report(address)`, `save_report(report)`,
`reward(network,address,epoch)`, and `commit_reward_batch(network,attempts)` are the
public interface. Report values use schema-v2 JSON at this persistence boundary (v1 snapshot
reports are not served as v2 data; reward rows are reused; see behavior.md
Version compatibility).
`load_report` selects the last complete baseline, otherwise last partial report;
it overlays latest discovery-attempt coverage without merging membership and
uses the network last-observed epoch (including observations of other addresses).
It labels that epoch last_observed and preserves original source times. Attempt
source IDs are deduplicated, so repeatedly saved fallback views stay bounded. The service applies offline/cache
labels, epoch-window changes, reward reuse and recalculated totals. `save_report`
returns whether its complete snapshot was published and any reward conflicts.

Before upgrading an existing older schema, acquire the IMMEDIATE write
reservation and recheck the version. Create a consistent backup from a separate
read-only connection using the SQLite backup API, verify it with `quick_check`, then apply numbered migrations in
one transaction. Keep the newest pre-upgrade backup per old schema version at
`<database>.pre-v<version>.sqlite3`; create a temporary backup before replacing that
backup. Never remove or reset the live database. Fresh empty files need no backup.
Refuse newer schemas before journal changes or writes. Failed migration rolls back
and returns recovery guidance, retaining the backup and original data. Corrupt,
read-only, unavailable and locked files return a storage error with no fallback.
A write succeeds only after commit; failures leave previously committed data
available and the service retains in-memory values with Not saved locally.
