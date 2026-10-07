# Changelog

Format follows [Keep a Changelog](https://keepachangelog.com/). Dates are 2026.

## [Unreleased] - 7 October

Reward history and period comparison (spec v0.7). Report schema 2 had not shipped,
so it was amended in place; schema 1 reports are not interchangeable with it.

### Added
- Fixed 30-epoch reward window: the latest 30 completed epochs, split into the
  current 15 and the previous 15, resolved on every load, refresh and offline run.
- Per-account annualized account return estimate for each recorded epoch
  (reward over pre-reward balance, compounded over 182.5 nominal two-day epochs).
  It is not validator or staking APY.
- Per-account "Change vs previous 15 epochs" comparison (`data.comparisons` in the
  JSON report): subtotal difference in lamports, percent change, and the change in
  mean estimate, over pairs where both epochs have a recorded reward only. Unknown
  values are `null`, never zero.
- Always-visible Account Detail panel with a paired chart for the selected account:
  previous period as shaded bars beside current period as solid bars, gap markers
  for missing rewards, and exact values for the selected pair. `Left`/`Right` pick
  the pair; `Tab` to the panel and `j`/`k` scroll the account fields below it.
- `report.v1.schema.json` and `docs/contracts/examples/v1/` kept for compatibility
  tests; new `short-window` and `short-previous` schema examples.
- Opt-in live checks recorded in `docs/release-validation.md`: 30 reward requests
  and about 7.8 s cold for a 10-account wallet.

### Changed
- Report schema version is 2: `input` is `{address, offline}`, `requested_epochs`
  and reward coverage hold up to 30 epochs, and each reward entry carries
  `account_return`.
- Reward requests scale as `30 x ceil(accounts / 10)`; recorded rewards are reused,
  so a warm reload makes no reward requests and a rollover requests one epoch.
- Report deadline is a fixed 120 seconds.
- Saved version-1 snapshots are not served as current data; one online refresh
  writes a version-2 snapshot and stored rewards are reused. No database migration.
- Dashboard layout: header is two rows and the accounts table keeps at least four
  rows so the chart fits at 80x24.
- Help, README, spec and contract documents describe the new behavior and labels.

### Removed
- `--epochs`. Passing it is an `INVALID_ARGUMENTS` error before any storage,
  network or terminal setup. `INVALID_EPOCHS` no longer exists.
- The Enter toggle for account details and the separate rewards section.

### Repository
- `.gitignore` ignores `.superpowers/` (local brainstorming artifacts) and
  `.claude/worktrees/` (linked agent worktrees).
- README, `AGENTS.md` and docs no longer say the repository is local-only with
  no remote; commit and push still need user authority.

### Notes
- Historical validator attribution is unverified: charts use each account's current
  validator because delegation history is not reconstructed.
- The comparison is descriptive. Deposits, withdrawals, splits, merges and
  commission all move it; it is not a forecast or a validator ranking.

## [0.1.0] - v1 foundation

- Read-only Rust TUI and one-shot `--json` for native Solana stake on mainnet via
  Helius, with SQLite persistence, offline mode, discovery by staker and withdrawer
  authority, validator grouping and findings.
