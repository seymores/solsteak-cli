# SolSteak TUI — technical requirements

Version: 0.7 · 7 October 2026 · Status: post-v1 30-epoch reward history and period-comparison requirements confirmed

This document records the released v1 foundation and the confirmed requirements for the next reward-history release. The user authorized development on 7 October 2026. Contract updates and provider-budget evidence are explicit prerequisite tasks; agents must not silently convert estimates into confirmed financial semantics.

## 1. Purpose and inherited decisions

Given a public Solana address, help someone understand where associated native SOL stake is delegated, what inflation rewards were recorded, and what deserves attention. The same commands must work when inspecting someone else's address. Never imply the operator owns the address.

Established project direction:

- Read-only initially; no signing, wallet connection, or custody.
- Understand stake accounts, delegation, rewards, and account condition with simple, answer-first presentation.
- Support both personal monitoring and exploration of other addresses.
- Personal tool used locally, implemented in Rust (confirmed 7 October).
- Helius is the required provider; the user supplies their own API key.
- Mainnet only in v1, using direct local RPC access; no service, daemon, or endpoint-selection feature.
- macOS Apple Silicon is the first supported platform. One-shot `--json` is required alongside the TUI.
- SQLite local persistence is required (confirmed 7 October); no DuckDB or database server in v1.
- Primary invocation: `ssteak -a <address>` launches an interactive, full-screen TUI. Intuitive terminal UX/UI is a first-release requirement, not optional polish.
- The earlier Elixir direction is superseded for this CLI.

Carry forward simplicity, progressive disclosure, and a single-dashboard experience into a terminal-native interface. Web mobile layout, traffic targets, and hosting requirements do not become TUI requirements. Earlier PostgreSQL, Phoenix, and Oban choices were assistant proposals, not confirmed constraints.

## 2. Confirmed first release

A local, interactive Rust TUI with a reusable domain core and polished keyboard-first dashboard. SQLite persistence is required; versioned one-shot JSON output is required in v1. It calls Helius directly using the user's API key. It needs neither a hosted SolSteak service nor a background daemon.

The initial release answers:

1. Which stake accounts are associated with this address, and through which authority?
2. How much SOL is held in those accounts, how much is delegated, and to which validators?
3. What inflation rewards were recorded for the latest 30 completed epochs, what annualized account-return estimate do they imply, and how do the latest 15 epochs compare with the 15 before them?
4. Which observations deserve attention, and which data remains unknown?

Exclude initially: transaction submission, liquid staking tokens, fiat prices, tax reporting, MEV reward accounting, validator rankings, continuous monitoring, notifications, network-wide indexing, and complete lifetime wallet history. These exclusions are confirmed for v1. Validator staking APY and total rewards since staking with a validator remain deferred; the next release adds only a clearly labeled annualized account-return estimate and a current-versus-previous 15-epoch comparison for each stake account.

## 3. TUI launch and UX contract

**Confirmed:** this is an interactive TUI application, not a command that prints formatted tables and exits. The dashboard stays open until the user quits. Application defaults below are frozen by [the behavior contract](docs/contracts/behavior.md). Provider budgets and storage DDL remain separately gated.

### 3.1 Launch and modes

```sh
ssteak -a <address>               # Interactive staking dashboard
ssteak -a <address> --offline     # Browse local cached observations
ssteak -a <address> --json        # Required v1 one-shot mode, no TUI
```

| Flag | Behavior |
| --- | --- |
| `-a, --address ADDRESS` | Required at launch; one wallet/authority or native stake-account address. |
| `--json` | Explicit noninteractive mode: one structured result and exit; no raw mode or alternate screen. |
| `--refresh` | Bypass reusable observations on initial load. |
| `--offline` | No network calls during the session; label cached data and age. Conflicts with `--refresh`. |
| `--no-color` | Monochrome interface with textual status and selection markers. |
| `-h, --help` / `-V, --version` | Text output without address, key, database, or terminal initialization. |

Remove the earlier `--details` and `--epochs` flags: detail expansion happens inside the TUI and the reward window is always the latest 30 completed epochs (the current 15 and the previous 15). Supplying either removed flag is an invalid-argument error. No separate account/reward/validator subcommands are required.

Validate arguments and online credentials before entering raw mode. Missing address/key produces concise guidance and exit 2. Use the user-supplied `HELIUS_API_KEY`; offline mode needs no key. If stdin or stdout is not a usable terminal, explain that `--json` is available and exit 2 without control codes; do not silently change modes.

Automatically inspect native stake-account input directly; otherwise discover both authority relationships even for unfunded addresses. Preserve existing attribution safeguards. No hidden lifetime history backfill. JSON stdout contains exactly one object for operational results/errors, with diagnostics on stderr. Help/version remain text. An exact `--json` token before the `--` delimiter selects a JSON error envelope even for malformed arguments; raw invalid values are not echoed. The behavior contract defines validation order and exit precedence.

### 3.2 Information hierarchy

**UX-01 — Answer-first, single-dashboard design.** Make balances, delegation, rewards, and issues understandable without learning commands or moving through multiple pages. Prefer expandable sections and contextual detail rather than a maze of tabs. No required splash screen or decorative charts; the reward chart is functional and exposes exact values through keyboard navigation.

| Region | Contents |
| --- | --- |
| Header | Product, inspected address, network, current epoch, online/offline status, and observation age. |
| Summary | Associated account balance, recorded delegated amount, account/validator counts, latest completed epoch reward total/subtotal, and coverage. |
| Attention | Visible findings and missing-data warnings, severity words, and affected account references. Material warnings cannot be hidden in details. |
| Stake accounts | Primary scrollable table: account, validator, balance (SOL), observed stake state, and latest-epoch reward/availability. Every discovered row is reachable. |
| Account Detail | Always visible for the selected account: the current-versus-previous 15-epoch reward chart and period summary first, then full addresses, authorities and relationship, stake fields, lockup, validator observations, reward history, and provenance. |
| Secondary sections | Collapsible validator grouping, with explicit coverage and no-data states. |
| Footer | Context-sensitive key hints, focused region, row position/count, and loading/retry status. |

Default order: account balance descending, public key ascending as tie-breaker. Right-align numbers and use consistent SOL precision. Display unknowns as “Unknown” or “No data,” not zero. Separate total account balance from delegated/effective stake. Avoid red/green-only meaning, required emoji, and special icon fonts. Sanitize provider-supplied control characters.

Search filters visible rows only; summary totals remain for the full selected account set, labeled with “showing X of Y.” Sorting and scrolling do not make RPC calls. Full addresses remain accessible in details even when abbreviated in rows. No top-N-only account display.

### 3.3 Keyboard navigation

**UX-02 — Discoverable and keyboard-first.** Essential actions must work without a mouse. Mouse support is optional. Keep essential hints visible; `?` exposes complete help and field explanations.

| Key | Action |
| --- | --- |
| Up/Down or `j/k` | Move within the focused table/list or scroll focused content. |
| Tab / Shift-Tab | Cycle interactive regions; focus navigation, not tabbed pages. |
| Enter | Expand/collapse the validator section, or confirm input. |
| Esc | Close top overlay, cancel input, or clear search; never unexpectedly exit. |
| PageUp/PageDown, Home/End | Navigate long lists. |
| Left/Right | Select the epoch pair shown in the Account Detail chart (from the accounts table or Account Detail focus). |
| `/` | Search account/validator address or available name in the focused table. |
| `s` | Open labeled sort options for the focused table. |
| `r` | Refresh current address; disabled with explanation offline. |
| `a` | Open address-entry overlay to inspect another address. |
| `?` | Help overlay: keys, field meanings, and scope caveats. |
| `q` | Close the help overlay first; otherwise quit from dashboard. |
| Ctrl-C | Quit from any state with terminal cleanup, exit 130. |

While editing text, printable shortcut letters insert text instead of firing actions. Support pasted addresses. Enter validates and submits; Esc cancels. Invalid input stays focused with a local message and does not replace the current address. Each submitted address starts a new request generation: cancel prior work when possible and ignore late responses from earlier generations.

### 3.4 Loading, refresh, and failure states

**UX-03 — Nonblocking interface.** Render the shell before data arrives. Keep navigation, help, cancellation, and quit responsive throughout network work. Sections independently support loading, ready, empty, partial, failed, and stale states. Never show temporary zero balances as loading placeholders.

Display usable cache immediately with its age; refresh online on launch when stale or explicitly requested. Thereafter refresh is manual in v1: no hidden polling or daemon. Redrawing never triggers RPC calls. Each refresh freezes the latest 30 completed epochs; a newer current epoch must not relabel old rewards.

When refresh fails, retain prior data visibly marked stale and show the failure and retry action. A cold failure renders an actionable error panel with retry/address change/help/quit. Successful empty discovery says “No associated native stake accounts found.” Missing reward records are not the same as an empty portfolio.

Coalesce repeated refresh presses; no overlapping request storms. Preserve selected public key, focus, expanded sections, and scroll position where possible. If a selected account disappears, move predictably and show a notice. Progress/errors belong inside the TUI, not interleaved stdout/stderr logs.

### 3.5 Terminal quality and lifecycle

**UX-04 — Responsive sizing.** Target 120×35 cells for a comfortable layout; support 80×24 minimum. Use side-by-side details on wide terminals, stacked/expanded details on narrow terminals. Hide secondary columns before truncating primary amounts, keeping hidden fields available in details. Wrap full addresses when needed. Keep the header, material warning indicator, and key hints visible while content scrolls.

Below 80×24 show current dimensions, minimum-size guidance, and quit help. Restore the prior view on resize. Resizing must not reset selection or cause overlapping text or a panic.

**UX-05 — Safe lifecycle.** Raw mode and alternate screen are interactive-mode only. Restore input mode, cursor visibility, styles, alternate screen, and mouse mode if enabled on normal quit, Ctrl-C, handled termination signals, initialization failures after partial setup, and caught panics. Restoration cannot be guaranteed after uncatchable termination such as SIGKILL. Do not expose secrets through terminal errors.

**UX-06 — Measurable usability.** Proposed targets: visible shell within 200 ms; input-to-render p95 under 100 ms during fetches; no continuous idle redraw loop. Test 1,000 fixture rows with rendering limited to visible content. Verify light/dark backgrounds and monochrome mode manually. Network latency targets do not replace interface responsiveness targets.

## 4. Account discovery and attribution

**FR-01 — Input validation.** Accept base58 addresses decoding to 32 bytes. Do not require an on-curve address: an authority may be a program-derived address. A syntactically valid address with no funded system account may still be a stake authority.

**FR-02 — Discovery.** For wallet input, query the native Stake Program by both withdraw authority and stake authority. Dedupe by stake account address. Use the previously discussed offsets (staker 12, withdrawer 44) only after checking the supported serialized layout against canonical fixtures. Validate program owner, state variant, and decoded authority on every result. Do not silently discard unknown layouts.

**FR-03 — Direct account inspection.** For stake input, inspect that account alone. Report its authorities, delegation, rent reserve, lockup, and supported state fields. A missing account in auto mode still undergoes authority discovery. Report whether the input account exists separately from whether associated stake accounts were found.

**FR-04 — Authority scopes.** For authority input, every discovered account has a relationship label: withdrawer, staker, or both. For direct stake-account input, identify selection mode as direct and mark the authority relationship not applicable; the inspected account address is not asserted to be its own authority. “Associated account balance” is the deduplicated union. Withdraw-authority and staker-only subtotals are separate and must not be added twice. Authority relationships are not proof of beneficial ownership. Withdraw authority does not imply immediately withdrawable funds, particularly with lockups or active stake.

**FR-05 — Coverage.** Current discovery cannot prove lifetime ownership, recover every closed account, or reconstruct historical authority transfers, splits, and merges. All historical reports state: rewards for the selected current account set, not lifetime earnings attributable to the wallet. Cached formerly associated accounts do not silently enter that set.

## 5. Balances, stake state, and findings

**FR-06 — Amounts.** Internally use integer lamports and decimal arithmetic; never binary floating point for money. Distinguish total account balance, recorded delegated amount, rent reserve, and effective active stake. Do not sum these overlapping values into a portfolio total. The behavior contract defines these fields: undelegated principal is checked balance minus rent only for validated initialized accounts; delegated/unsupported accounts do not infer withdrawable excess, and exact active amounts remain unknown.

**FR-07 — State correctness.** Decode initialized/undelegated and delegated accounts. Show activation and deactivation epochs where available. Exact effective/activating/deactivating amounts require a validated, current chain-compatible calculation and relevant historical inputs. Epoch comparison alone is not sufficient. Do not depend on the removed `getStakeActivation` RPC. Confirmed v1 behavior: render unvalidated effective/activating/deactivating amounts as “Unknown” and show observed delegation fields without claiming full activation. Exact activation calculations are not a v1 release blocker and must not be added without a separate validated scope decision.

**FR-08 — Findings.** Each finding contains a stable code, severity, affected address, observed evidence, observation time/slot, and plain-language explanation. Initial findings: undelegated funds, deactivation requested, provider-reported validator delinquency, unsupported state, incomplete discovery, and incomplete reward data. An intentional deactivation is informational, not automatically a fault. Lockup metadata remains available in account details without producing an attention finding. Avoid a generic “healthy” badge: say “No issues detected by these checks” only when required checks completed.

**FR-09 — Validators.** Group using vote-account public keys, not names. Report current commission, current/delinquent classification when available, and concentration using recorded delegated amounts, with that denominator labeled. A missing validator record is unknown. Current commission is not historical commission. Names are optional enrichment. Comparative yield, validator rankings, and total rewards since staking with a validator remain deferred until defensible historical delegation attribution exists.

## 6. Rewards and historical correctness

**FR-10 — Recorded inflation rewards.** Query `getInflationReward` by stake-account batches and explicit epoch. Preserve address-to-response ordering and store amount, returned epoch, effective slot, post-balance, and commission when present. Record provider and retrieval time. This feature reports inflation rewards only; it must not imply inclusion of MEV or other distributions.

**FR-11 — Missing results.** Distinguish recorded reward (including explicit numeric zero), no reward data returned, request failure, and not queried. RPC `null` means no reward data available; it is not automatically confirmed zero. Never manufacture a complete zero-valued epoch from nulls.

**FR-12 — Epoch boundaries.** Freeze exactly the latest 30 completed epochs at the start of each load or refresh, using finalized epoch context. With latest completed epoch L, the *current period* is epochs L down to L−14 and the *previous period* is L−15 down to L−29; pair `i` (0 is newest, 14 oldest) is (L−i, L−15−i). If fewer than 30 completed epochs exist, request the available range and report shortened coverage: positions with no existing epoch are absent (not gaps) and their pairs cannot be compared; with 15 or fewer completed epochs there is no previous period. The most recently completed reward epoch may still lack available records; label availability rather than promising immediate completeness. Refresh missing results on subsequent online runs. Completed discovery plus fully processed requests means query coverage, not proof of complete lifetime accounting.

**FR-13 — Totals.** Aggregate only returned numeric reward records. With missing data, label the result “recorded subtotal” and expose counts for every coverage state. Do not sum bank balance changes as rewards. Do not subtract validator commission again from the reward amount credited by the chain. Default human output must not say “earned this epoch” when it refers to a prior epoch.

**FR-14 — Annualized account-return estimate.** For each recorded stake-account reward, calculate the pre-reward account balance as `post_balance_lamports - amount_lamports` using checked integer arithmetic. When that balance is positive, calculate the epoch account return as `amount / pre_reward_balance` and annualize it as `(1 + epoch_return)^182.5 - 1`, using a fixed nominal two-day Solana epoch and 365-day year. Exact lamport inputs remain integer values; rounding applies only to the displayed percentage. A zero reward with a positive denominator produces 0%. A missing reward or post-balance, zero denominator, underflow, or invalid value produces `Unknown` or an invalid-response error as appropriate, never a synthetic percentage.

Call this value “Annualized account return estimate,” never “validator APY,” “staking APY,” or “real APY.” It uses total pre-reward account balance rather than historical effective stake and a nominal rather than measured epoch duration. Do not combine balances or return estimates across stake accounts. The latest completed epoch estimate is the headline; selecting an earlier graph point shows that epoch's estimate.

**FR-15 — Account reward chart.** The always-visible Account Detail panel starts with a reward chart for the selected stake account, with the account's current validator shown. The chart overlays the two periods by pair position: 15 columns, oldest pair left, each holding two adjacent bars, the previous-period epoch and the current-period epoch, drawn on one shared scale (the account's largest recorded reward in the window). The periods must be distinguishable without color (different fill glyphs plus a legend line). The chart follows the accounts-table selection (Up/Down) and Left/Right selects a pair. For the selected pair show both epoch numbers, exact rewards, coverage states, annualized account-return estimates, and the pair difference. Missing epochs render as gaps per period and keep partial coverage visible. Use only the account's current validator; label the historical validator attribution as unverified because delegation history is not reconstructed.

**FR-16 — Period comparison.** For each stake account, compare the current period with the previous period using only *paired recorded positions*: pair `i` counts only when both its epochs have a numeric recorded reward (an explicit zero counts). Report the number of compared pairs (0–15) and the number of pairs left out. Over the compared pairs only, compute with checked `u128` integers the current and previous recorded subtotals and the signed difference `current − previous` in lamports, and the percent change of the subtotal (`difference / previous × 100`; Unknown when the previous subtotal is zero). Also report the mean annualized account-return estimate of each period over the compared pairs whose both estimates are available (FR-14), the number of such pairs, and the difference in percentage points. With no compared pairs, or no pairs with both estimates, the affected values are Unknown, never zero. Percentages are rounded only for display. Do not combine accounts, validators or periods beyond this per-account comparison, and do not rank accounts or validators.

Label the result “Change vs previous 15 epochs” and show direction in words (higher, lower, unchanged), not by color alone. The difference reflects reward amounts and the total pre-reward balance, so deposits, withdrawals, splits, merges, validator changes, commission and epoch-specific effects all move it; state that it is descriptive, not a forecast or a measure of validator quality, and that attribution to the current validator is unverified.

## 7. RPC plan and provider boundary

| Need | Initial source | Implementation notes |
| --- | --- | --- |
| Verify network identity | `getGenesisHash` | Verify mainnet identity on online sessions; persist for offline provenance. |
| Classify/read input | `getAccountInfo` | Do not require wallet account existence for authority discovery. |
| Discover stake accounts | Two filtered `getProgramAccounts` calls for `any` | Native Stake Program; finalized; request context; decode and dedupe. |
| Determine current epoch | `getEpochInfo` | Snapshot command context; invalidate epoch-sensitive caches across rollover. |
| Validator observations | `getVoteAccounts` | Share/cache global response where practical. |
| Reward records | `getInflationReward` | Explicit epoch; bounded address batches and concurrency. |
| Exact activation | Deferred | Display Unknown until separately validated; no activation-specific RPC work required in v1. |

Helius-specific pagination or enhanced endpoints may be adopted after a capability spike, behind the adapter. Do not invent plan limits or assume arbitrary batch sizes. Discovery failure, limits, or truncation must prevent a claim of complete discovery. Check both authority queries independently.

For A selected accounts and verified reward batch size B, the fixed 30-epoch window requires approximately `30 × ceil(A/B)` reward requests, excluding retries. Classification, discovery, epoch, validator, and state-calculation requests are additional. A JSON-RPC HTTP batch does not necessarily reduce provider billing units. No fixed “three calls per wallet” requirement.

Use finalized commitment where the method supports it. Preserve per-source context slots; independent calls are not an atomic snapshot. Do not describe `minContextSlot` as an exact historical snapshot selector. On detected epoch rollover during a report, retry the affected snapshot once or label inconsistency and return partial status.

## 8. Architecture and local persistence

Proposed layers:

1. Entry point and TUI: validate arguments, choose interactive/JSON mode, manage terminal lifecycle, and maintain focus, selection, expanded sections, overlays, and load states.
2. Application services: orchestrate inspection, discovery, rewards, and validator reports.
3. Domain core: decode stake state, classify authority relationships, calculate totals, produce findings. No terminal, database, or HTTP dependencies.
4. Provider adapter: transport, timeout/retry policy, normalized RPC errors, capability limits.
5. Persistence: SQLite repository, migrations, observations, rewards, and resumable query coverage.
6. Renderers: interactive TUI and one-shot JSON from the same domain result model. Keep the terminal event loop separate from asynchronous RPC/cache workers. Use typed input/resize/data events, request-generation identifiers, and bounded event delivery. UI state transitions must be testable without a real terminal.

Use one Rust Cargo package with a small binary entry point and library modules for the domain, Helius access, persistence, and rendering. Keep interfaces small; do not build a generic plugin framework or hosted service. The core should be independently testable; cross-language reuse by a future web application is not a v1 requirement.

Use typed models and errors. Represent individual on-chain lamport amounts as `u64`, perform checked wider aggregation (for example `u128`), and explicitly handle overflow and unknown values. Pin the Rust toolchain during the foundation prerequisite and retain the application lockfile as dependencies are selected. Git commits follow repository authority. Dependency selection remains an implementation decision, not a user requirement.

### 8.1 Persistence scope and responsibilities

**DB-01 — Embedded SQLite.** Persist data across process restarts without a server, daemon, or separate database installation. SQLite is the sole persistent datastore in v1. DuckDB and dual-write analytics storage are out of scope.

Use two layers: an in-memory dashboard model for rendering, selection, sorting, and navigation; SQLite for reusable observations, reward history, and resumable query coverage. Redraws and ordinary movement through loaded rows must not query SQLite or Helius. Database work runs outside the terminal event loop through a dedicated worker and bounded request queue.

SQLite is a local copy of observations, not the source of blockchain truth. Preserve provider, network, observation time, source slot when supplied, commitment, decoder version, and coverage. Never imply a database transaction makes separate RPC calls an atomic chain snapshot.

**DB-02 — Retention boundary.** Default design: retain latest usable state and durable reward records, not a time series of every refresh. Retain the last complete discovery snapshot and latest attempt separately; retain account observations needed to reconstruct that complete snapshot until its replacement is committed. Keep latest query status per account/epoch rather than an unbounded retry log. No lifetime wallet ownership reconstruction is implied.

These retention details are implementation defaults for this draft, not an additional confirmed user choice. No automatic deletion of recorded rewards in v1; future pruning must be explicit and preserve referential integrity.

### 8.2 Logical data model

Concrete DDL remains to be frozen in `spec.md`; these identities and invariants are required.

| Record | Identity and required purpose |
| --- | --- |
| Network metadata | Verified network identity (genesis hash), cluster label, last observed finalized epoch and time. |
| Discovery snapshot | Network + inspected address + generation; authority-query outcomes, source slots/times, completion status, last-complete pointer. |
| Discovery members | Snapshot + stake address; authority relationship (staker/withdrawer/both), or not applicable for direct selection, and reference to the account observation used in that snapshot. |
| Stake observation | Network + stake address + observation identity; exact amounts, decoded state, authorities, delegation, lockup, and provenance. |
| Validator observation | Network + vote address; latest usable observation, current metrics, provenance, and coverage. Shared across inspected addresses. |
| Inflation reward | Unique network + stake address + reward epoch; amount, post-balance, effective slot, commission when present, and provenance. |
| Reward query coverage | Unique network + stake address + requested epoch; latest attempt time, result state, redacted error code, retry eligibility. |
| Schema metadata | Migration version and migration history sufficient to safely open/upgrade the file. |

Missing coverage rows mean not queried. Persisted result states distinguish recorded reward, no data returned, and failed request; an in-flight request must never be mistaken for a completed one after a crash.

Foreign keys enforce snapshot/member relationships. Index address-based discovery, snapshot membership, network/account/epoch reward lookups, and validator lookup. Repeated imports are idempotent through unique keys and upserts, not duplicate insertion.

**DB-03 — Exact amounts.** Store lamport amounts as validated canonical unsigned decimal TEXT so the full u64 range survives SQLite's signed integer limit. Parse into `u64` and use checked `u128` aggregation in Rust. Do not use SQLite numeric casts, `SUM`, or lexicographic TEXT ordering for monetary calculations; aggregate and sort exact amounts in the domain layer. Other u64 on-chain fields, including sentinel epochs, must also round-trip losslessly. JSON amounts remain decimal strings. Zero, unknown, and unavailable are distinct values/states.

### 8.3 Cache policy and offline behavior

**DB-04 — Data-specific freshness.** Initial defaults below are tunable design choices. Expiration means “needs revalidation,” not “delete the record,” and does not schedule periodic polling.

| Data | Initial freshness/reuse policy |
| --- | --- |
| Authority discovery and stake observations | Reusable for 60 seconds when complete and compatible with the current request. |
| Validator observations | Reusable for 60 seconds, shared across addresses on the same network. |
| Finalized epoch context | Check on each online load/refresh; detected rollover invalidates epoch-sensitive snapshot freshness. |
| Recorded finalized rewards | Reuse without time-based expiry; retain across restarts. |
| Missing/error reward results | Retry on a later explicit load/refresh; honor provider backoff and current invocation budgets. Never permanently cache as zero. |
| Derived totals/findings/comparisons | Recompute from the selected data generation; do not persist as independent authoritative facts. |

Each load resolves the fixed 30-epoch window and queries only missing eligible account/epoch combinations; the first load after the window grows from 15 to 30 therefore fetches the previous period once, then reuses it. The normal `r` action refreshes current observations and retries missing rewards while reusing recorded rewards. Explicit `--refresh` also revalidates cached rewards within that window; it does not erase the cache first.

**DB-05 — Cache-first load.** Read the latest usable snapshot, build an in-memory model, and show observation age immediately. In online mode obtain epoch context and refresh stale/missing sections in the background. In offline mode make zero network requests, including genesis/epoch checks; use persisted network metadata. Label the epoch “last observed epoch,” not a verified current epoch. Resolve the offline reward range relative to that last observed epoch, clearly labeled. Missing data stays unknown; an uncached address shows “No local data for this address.”

### 8.4 Atomic refresh, resumability, and conflict handling

**DB-06 — Discovery publication.** Persist selection mode (authority or direct) with each snapshot. Direct inspection publishes a one-account snapshot after a validated account read; it does not wait for authority queries. For authority selection, treat the two authority queries as one discovery generation. Only publish a new complete membership set when both required queries completed without truncation and validation/coverage requirements passed. If one fails, preserve the last complete snapshot; expose the latest attempt separately as partial. Do not union old and new account sets into an unlabeled current total. With no complete baseline, show partial results as partial only.

Publish membership, referenced account observations, coverage, and the last-complete pointer in one short transaction. Never delete an account merely because it is absent from an incomplete response. Retire obsolete snapshot rows only after successful replacement; do not cascade deletion into durable rewards.

**DB-07 — Reward batches.** Commit validated reward records and their query coverage atomically per bounded batch. A rerun reuses committed records and retries unfinished/missing work; duplicate delivery cannot double count rewards. Network requests happen before transactions, never inside them. Null/error responses cannot overwrite a known recorded reward. If explicit revalidation returns a conflicting numeric reward, flag the inconsistency and preserve the prior record pending a defined reconciliation policy rather than silently changing it.

**DB-08 — Concurrency.** Enable WAL mode, foreign keys on every connection, and a bounded busy timeout (initial default 5 seconds). Use short write transactions and a single writer worker per app process; multiple local instances rely on SQLite locking and bounded retries. Do not share live connections unsafely between threads. Do not overwrite newer current observations with late lower-slot responses; preserve per-source slot ordering and reject stale generations. If observations are not comparable, retain separate attempt metadata rather than claiming an authoritative replacement.

Use `synchronous=FULL` as the initial durability setting; do not weaken durability without an explicit tradeoff. Permit normal automatic checkpoints; avoid long-lived read transactions and forced blocking checkpoints on every refresh. Active database/WAL files must live on a local filesystem, not a network share or live cloud-synced folder.

### 8.5 File lifecycle, privacy, and failures

**DB-09 — Location and migrations.** Proposed macOS default: the user's Application Support directory, under `ssteak/ssteak.sqlite3`, resolved through the platform directory API. Keep it outside the project repository and do not put durable rewards in an OS-purgeable cache directory. WAL/SHM companion files are expected. Use owner-only directory/file permissions where supported. Display the resolved database location in in-app help; never store the Helius API key, credential-bearing URLs, private keys, or seed phrases in database rows.

Create the schema on first use with numbered, transactional migrations and parameterized SQL. Before upgrading an existing schema, create a consistent SQLite backup; do not naively copy only the live main file while WAL is active. Refuse to modify a database with a newer unsupported schema. On migration failure, roll back, preserve the original data, and show recovery guidance. Exact path overrides and backup retention remain spec-freeze decisions.

**DB-10 — Failure behavior.** No silent fallback to a new empty database and no automatic destructive reset on corruption. Initialization/open/migration failure is an actionable local-storage error. Mid-session write failure (disk full, read-only filesystem, or exhausted lock wait) retains the current in-memory display with “Not saved locally”; never claim persistence succeeded. Preserve the last committed snapshot and allow a later bounded retry. JSON mode reports the persistence failure with exit 5 even when fetched data is included. Cancellation or process failure must leave committed batches readable and uncommitted batches absent. Tests should distinguish transaction recovery from claims about untested hardware power-loss durability.

## 9. Output, errors, privacy, and budgets

Every JSON response includes `schema_version`, `input`, `network`, `generated_at`, `status`, `data`, `coverage`, `sources`, `warnings`, and `errors`. The report schema must advance to version 2 because removing the configurable epoch input and adding account-return estimates changes the wire contract. [The checked-in report schema](docs/contracts/report.schema.json), [synthetic examples](docs/contracts/examples/), and [application behavior](docs/contracts/behavior.md) must be updated together before implementation. Schema validation is structural; Rust must also enforce numeric bounds, unique identities, resolved source IDs and matching coverage counts.

For one-shot JSON mode, frozen exit codes: 0 complete report; 2 input/configuration error; 3 useful partial report; 4 provider/network failure with no usable report; 5 local persistence/internal failure; 130 interrupted. Attention findings alone do not mean command failure. An explicitly requested offline report can exit 0 if complete for its labeled cached observation; missing required cached data produces 3 or 4 with a specific offline error code.

TUI exit behavior: normal user quit returns 0 regardless of current data coverage; partial/error coverage stays visible in-app. Recoverable provider failures do not end the session. Startup errors use 2/4/5 as appropriate, fatal session errors use 5, and Ctrl-C uses 130.

Configuration: the user supplies their own API key through `HELIUS_API_KEY`. Never request a seed phrase or embed a shared key. No API key command-line flag or automatic `.env` loading in v1. A general config-file system is unnecessary initially: use command flags, the API key environment variable, and documented defaults. Use the proposed platform data location in DB-09; freeze path overrides before implementation.

Never log API keys, authorization headers, or unredacted credential-bearing URLs. No telemetry by default. Public addresses are sent to the configured RPC provider; explain this in help/privacy documentation. Local inspection history remains local unless the user exports it.

Proposed budgets: four in-flight requests; 10-second request timeout; at most two retries for transient failures; bounded jittered backoff respecting Retry-After within the command deadline. No retries for malformed requests or invalid credentials. The fixed 30-epoch report deadline is 120 seconds, to be rechecked against measured 30-epoch call times (the 15-epoch window measured about 5 seconds cold for 10 accounts). On deadline expiry, JSON mode returns usable partial results; TUI mode ends only the current fetch and remains open with partial/error status. All values remain proposals pending provider measurements.

Proposed engineering targets: cached/offline report p95 under 500 ms excluding installation; cold inspection p95 under 5 seconds for up to 20 stake accounts on a documented benchmark environment and provider plan. These are targets to validate, not provider guarantees. Larger inputs must be bounded, cancellable, and show progress inside the TUI (stderr only for JSON mode). Do not silently truncate accounts to meet a timing target.

## 10. Acceptance criteria

| ID | Scenario | Required result |
| --- | --- | --- |
| AC-01 | Same account matches both authorities | One account; relationship `both`; no double counting. |
| AC-02 | Wallet is staker but not withdrawer | Visible as staker-only; never described as withdrawable wallet wealth. |
| AC-03 | Valid authority address has no funded account | Authority discovery still runs. |
| AC-04 | One discovery filter fails | Partial coverage, explicit error, no complete total claim. |
| AC-05 | No associated accounts; both discovery queries succeed | Successful empty report with scope and time. |
| AC-06 | Mixed reward object, null, and failed request | Recorded subtotal and separate coverage states; null never becomes zero. |
| AC-07 | Historical reward predates current authority relationship | Described as account reward; not attributed to historical wallet ownership. |
| AC-08 | Unsupported stake layout or unavailable activation inputs | Visible unknown state; no invented effective balance. |
| AC-09 | Rewards query interrupted and rerun | Committed batches reused; no duplicate totals. |
| AC-10 | Provider returns 429, timeout, or invalid credentials | Bounded retries for transient cases; no credential retry loop; secrets redacted. |
| AC-11 | JSON output redirected to file | Exactly one parseable object; no progress text; exact lamport strings. |
| AC-12 | Offline report | Zero network requests; explicit observation age and missing-data coverage. |
| AC-13 | Epoch rolls over during requests | Bounded reconciliation or marked inconsistency; no mixed-epoch complete report. |
| AC-14 | Large u64 fields and high-balance fixtures | Exact parsing, persistence, aggregation, and output without overflow/rounding. |
| AC-15 | `ssteak -a ADDRESS` with configured Helius key | Persistent interactive dashboard, immediate shell, progressive loading; no hidden history backfill. |
| AC-16 | Expand account details versus `--json` | Consistent values and coverage; JSON never enters raw mode or alternate screen. |
| AC-17 | Missing address/key, or invalid/conflicting flags | Actionable error before terminal initialization or RPC; help/version work without credentials. |
| AC-18 | Keyboard-only first use | Footer/help makes account expansion, refresh, address change, navigation, and quit discoverable. |
| AC-19 | RPC stalled/rate-limited | Input remains responsive and section-level loading/error states stay visible. |
| AC-20 | Resize wide → 80×24 → below minimum → wide | No crash or lost selection; small-size guidance and restored layout. |
| AC-21 | Quit, Ctrl-C, partial initialization failure, caught panic | Terminal mode and cursor restored. |
| AC-22 | Switch to B while A fetches remain in flight | Late A responses cannot populate B's view. |
| AC-23 | Refresh fails after a successful load | Prior data remains visible with stale labels and retry; never replaced with zero. |
| AC-24 | Search, sort, scroll, resize, redraw | No extra RPC requests; row counts and portfolio scope stay clear. |
| AC-25 | No usable terminal, with/without JSON | Plain actionable error or valid one-shot JSON; no accidental raw mode. |
| AC-26 | Text input contains shortcut letters or invalid pasted address | Normal text entry and local validation; old address retained until valid submission. |
| AC-27 | Restart after successful refresh | SQLite reconstructs the cached address view and rewards with original provenance and age. |
| AC-28 | Complete discovery followed by one failed authority query | Prior complete membership/amounts remain intact; latest attempt is separately partial. |
| AC-29 | Terminate before/after reward batch commit | Uncommitted batch absent, committed batch reusable; resume produces no duplicates. |
| AC-30 | Reward null/error after previously recorded value | Known reward preserved; missing/error status never replaces it with zero. |
| AC-31 | Two local instances write; late lower-slot response arrives | No corruption, bounded lock handling, and no stale overwrite of newer current state. |
| AC-32 | Disk full, read-only file, lock timeout, corrupt file | Visible storage failure; no destructive reset or false saved status; committed data preserved where readable. |
| AC-33 | Migration succeeds, fails, or encounters newer schema | Versioned upgrade/rollback works; consistent pre-upgrade backup; unsupported schema unchanged. |
| AC-34 | All-u64-range amounts and sentinel epochs round-trip | Exact SQLite read/write and Rust totals/sorting, without lossy SQL arithmetic. |
| AC-35 | Fresh cache, stale cache, or epoch rollover with the fixed 30-epoch window | Correct reuse/invalidation; query only required missing data except explicit revalidation. |
| AC-36 | Offline restart without API key | No HTTP calls; persisted network and last-observed epoch used with explicit age/scope. |
| AC-37 | TUI redraw/navigation during database work | No per-frame database queries; interface remains responsive. |
| AC-38 | Launch with `--epochs` | Invalid-argument error; no terminal, storage, or network initialization. |
| AC-39 | Latest 30 completed epochs include recorded, null, failed, and cached results | Exactly 30 ordered positions in two periods of 15, with gaps and explicit partial coverage. |
| AC-40 | Positive, zero, missing, underflowing, or zero-denominator reward inputs | Account-return estimate follows FR-14; invalid or unavailable inputs never become a fabricated percentage. |
| AC-41 | Multiple stake accounts share the current validator | Each account has its own graph and estimate; no combined return percentage. |
| AC-42 | Account changed validators during the 30-epoch window | Group under the current validator with historical attribution explicitly unverified. |
| AC-43 | Keyboard navigation through table rows and epoch pairs | The chart and period summary in Account Detail stay visible and synchronized with the selected row and pair at 80×24 and 120×35, including monochrome mode, with the two periods distinguishable without color. |
| AC-44 | Pairs where one or both epochs lack a recorded reward | Only pairs with both numeric records are compared; the compared and left-out counts are shown; gaps stay visible per period. |
| AC-45 | Positive, zero, missing, negative-difference, zero-previous and u64-range subtotals | Subtotals and the signed difference are exact integers; percent change is Unknown for a zero previous subtotal; estimate means and percentage-point difference follow FR-16; unavailable inputs never become a fabricated number. |
| AC-46 | Fewer than 30 completed epochs (for example 20, and 15 or fewer) | Available positions are requested and labeled shortened; pairs without both epochs are not compared; no previous period means the comparison is Unknown. |
| AC-47 | Epoch rollover or warm cache after a 30-epoch load | Only newly eligible epochs are requested; pairs shift by one epoch without relabeling old rewards or mixing periods. |

Test domain behavior with fixed RPC fixtures covering these cases. Add deterministic UI state-transition tests, frame snapshots at 80×24 and 120×35, and pseudoterminal tests for keyboard input, resizing, and cleanup. Manually verify light/dark terminal themes and monochrome mode. Domain tests alone do not satisfy the TUI requirement. Keep live provider smoke tests opt-in; normal automated tests must not require paid credentials. Existing provider evidence covers reward retrieval, but the mandatory 30-epoch call budget still requires release validation. Validate exact stake calculations against authoritative reference results if included.

## 11. Delivery sequence and spec gate

1. **Contract update:** remove the configurable epoch input, define schema version 2, freeze the account-return estimate fields, and update behavior/examples before dependent implementation.
2. **Reward window and calculation:** make the latest 30 completed epochs the sole online/offline window, preserve resumable cache behavior, and calculate per-account estimates and the FR-16 period comparison from validated records.
3. **Account reward chart:** add the overlaid current-versus-previous chart and period summary to Account Detail, exact selected pair values, coverage gaps, pair navigation, and attribution labels.
4. **Integration and release:** update JSON/CLI/docs, run contract and terminal-frame tests, validate provider call budgets, and perform hands-on monochrome and supported-size checks.

Rust packaging: build a native `ssteak` executable for the user's machine. macOS Apple Silicon is the confirmed first supported target; other operating systems and public binary distribution are not required initially. Building from source requires the pinned Rust toolchain; running the resulting executable should not require Rust, a database server, or an application runtime. Validate actual native library dependencies before release. A single executable does not mean a universal cross-platform binary or the absence of a local data file.

Confirmed decisions and remaining recommendations:

| Decision | Status | Choice |
| --- | --- | --- |
| TUI purpose | Confirmed | Personal tool used locally with intuitive terminal UX/UI. |
| Language | Confirmed | Rust. |
| Interaction | Confirmed | `ssteak -a <address>` launches an interactive full-screen TUI. |
| Provider and credentials | Confirmed | Helius; user-supplied API key. |
| Deployment | Confirmed | Direct local Helius RPC access; no service or daemon. |
| Network | Confirmed | Mainnet only in v1; no cluster or endpoint selection. |
| JSON mode | Confirmed | Versioned one-shot `--json` output ships in v1. |
| Storage | Confirmed | SQLite local persistence for cached state, rewards, and query coverage; no DuckDB or database server. |
| First-release depth | Confirmed | Current account state and fixed 30-epoch inflation reward history with a current-versus-previous 15-epoch comparison; defer lifetime reconstruction and yield ranking. |
| Dashboard layout | Frozen engineering default | Summary, attention, scrollable accounts, contextual detail, and collapsible secondary sections. |
| UX requirement | Confirmed | Good terminal-native UI and intuitive interaction are part of v1. |
| Refresh | Confirmed | Manual refresh after initial cache/stale-data loading; no polling. |
| Interaction details | Frozen engineering default | Discoverable keyboard controls, address switching, responsive 80×24 minimum layout; transitions defined in docs/contracts/behavior.md. |
| Reward window | Confirmed | Always the latest 30 completed epochs: current 15 plus previous 15; `--epochs` is removed. |
| Period comparison | Confirmed | Per stake account: paired recorded epochs only; subtotal difference and percent change, and mean annualized account-return estimate change; descriptive, never a ranking or forecast. |
| Reward visualization | Confirmed (revised) | Overlaid current-versus-previous 15-epoch chart for the selected stake account in the always-visible Account Detail panel, with exact keyboard-selected pair details. |
| Return estimate | Confirmed | Per-account annualized return estimate using pre-reward account balance and a fixed nominal two-day epoch; never labeled validator/staking APY. |
| Validator lifetime total | Deferred | Requires historical delegation reconstruction; do not claim total rewards since staking with a validator. |
| Activation precision | Confirmed | Display Unknown until validated; exact calculations deferred from the v1 critical path. |
| First platform | Confirmed | macOS Apple Silicon; locally built executable. |

Then freeze in prerequisite beads: Rust toolchain/dependency versions, exact command flags/defaults, TUI layout/keymap/state transitions, schemas, field definitions, provider limits, decoder references, fixtures, database schema, error codes, deadlines, and acceptance tests. Each implementation task should point to requirement and acceptance IDs. Coding agents must not resolve financial semantics by inventing convenient defaults.

### 11.1 Remaining technical gates

The application contract in `docs/contracts/behavior.md`, report schema, and
examples describe schema version 2 with the 30-epoch window and the FR-16
comparison. Version 2 never shipped, so it was amended in place; version-1 fixtures
stay for compatibility testing and mixed schema semantics are rejected. Rust 1.99.0 remains pinned in `rust-toolchain.toml`.

The durable reward rows already contain the exact inputs needed for FR-14 and
FR-16; the derived estimate and comparison do not require a storage migration unless the approved schema
design proves otherwise. Provider-call budgets must be rechecked for the mandatory
30-epoch window before release. Each dependent bead stays blocked on the contract
task, and the TUI graph stays blocked on the domain/window task.

## 12. Evidence and references

Prior context: SolSteak dashboard discussion (27 September); stake discovery and reward history discussions (30 September); Elixir and Helius discussions (6 October). Prior assistant recommendations are not treated as user-approved requirements. On 7 October the user explicitly selected local personal usage, their own Helius API key, Rust, and `ssteak -a <address>`; these supersede conflicting earlier proposals. The subsequent user clarification requires an interactive TUI with intuitive terminal UX/UI, superseding the one-shot report default and full-screen UI exclusion in v0.2. The user then selected SQLite local persistence after comparison with DuckDB; v0.4 makes SQLite required. The user subsequently accepted macOS Apple Silicon, mainnet-only direct access, manual refresh, one completed reward epoch by default (range 1–100), required JSON in v1, and Unknown for exact activation amounts until validated. Version 0.5 records those decisions. Version 0.6 supersedes the configurable reward lookback with a fixed reward window and adds the per-account annualized return estimate and reward chart defined in FR-14 and FR-15. Version 0.7 (7 October 2026) extends the window to the latest 30 completed epochs and adds the FR-16 comparison of the most recent 15 epochs against the preceding 15; the user selected an overlaid chart by pair position, subtotal and mean-estimate change over paired recorded epochs only, and an in-place amendment of report schema version 2, which had not shipped.

Current official documentation checked on 7 October 2026:

- [getProgramAccounts](https://solana.com/docs/rpc/http/getprogramaccounts): filtered program-account discovery and response context.
- [getInflationReward](https://solana.com/docs/rpc/http/getinflationreward): per-address epoch rewards and nullable results.
- [getVoteAccounts](https://solana.com/docs/rpc/http/getvoteaccounts): validator vote-account observations.
- [Removed getStakeActivation](https://solana.com/docs/rpc/deprecated/getstakeactivation): do not build against the removed RPC.

SQLite reference documentation reviewed during the persistence discussion:

- [Appropriate uses for SQLite](https://www.sqlite.org/whentouse.html).
- [Write-ahead logging](https://www.sqlite.org/wal.html).

Provider plan entitlements, reward retention, and packaging dependencies still need verification in their prerequisite/release tasks. Exact stake-state calculations are deferred and may display Unknown in v1.
