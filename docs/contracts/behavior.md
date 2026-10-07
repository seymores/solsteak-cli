# V1 application contract

This freezes the engineering defaults in spec.md for application/CLI work.
The report wire format is `report.schema.json`; checked examples are in `examples/`.
Database DDL and measured provider limits remain separate prerequisites.

## CLI and output

The flags in spec.md §3.1 are the entire v1 interface. Address is required except
for help/version. Epoch count is an integer 1–100, default 1. Duplicate value flags
and unknown arguments are errors. Boolean flags take no value. Help/version are
plain text and need no credentials, database, network or terminal setup.

An exact `--json` argument before the `--` delimiter requests JSON, including for
invalid arguments. `--json` used as a malformed address/epoch value still selects
JSON error output. Help/version successfully recognized by the parser take
precedence over JSON. Invalid arguments never echo arbitrary supplied values;
show a stable error code and safe guidance instead. Do not accept API keys as flags.
Missing, empty, non-Unicode or whitespace-containing online keys are configuration
errors. Offline ignores the key entirely. Keys are never part of the report model.

Validate arguments/address, then credentials, then TTY requirements, before any
storage, HTTP or raw-mode setup. Interactive mode requires both stdin/stdout to be
terminals and a nonempty TERM other than `dumb`. JSON has no TTY requirement.
No implicit mode switching. Plain errors go to stderr; JSON operational results
and errors are one object plus newline on stdout. Help/version go to stdout.
Failure writing stdout returns 5 without trying to write a second report.

JSON schema version is integer 1. Unsigned chain integers and lamport amounts are
canonical decimal strings; JSON counts/percentages are integers. Monetary totals
use checked u128; individual chain amounts use u64. Unknown data is null, never a
synthetic zero. `generated_at` and observation times are Unix seconds (integers);
null generated_at means the clock was unavailable. Addresses must decode to 32
bytes, including off-curve addresses; schema patterns are structural checks only.
`input` is null for argument-validation failure so invalid raw input is never echoed.
Network genesis hash is null until verified (configured cluster is always mainnet).

Exit selection for JSON: interruption 130; input/configuration 2; persistence or
internal failure 5; no usable observation 4; usable incomplete/stale/inconsistent
online observation 3; otherwise 0. Status is error for 2/4/5, interrupted for 130,
partial for 3 and complete for 0. A persistence error can retain data with status
error. Offline completeness is judged against the explicitly labeled saved
observation; age alone does not cause exit 3. Missing required offline data is 3
when useful data remains, otherwise 4. All returned numeric rewards, including
zero, plus complete required observations permit complete status. Null/error/not
queried rewards cause partial status; they do not prove zero earnings. Empty
successful discovery is complete with zero account/reward counts. Findings alone
do not change the exit code. TUI normal quit is 0, fatal error 5, Ctrl-C 130.

Stable initial error codes: INVALID_ARGUMENTS, INVALID_ADDRESS, INVALID_EPOCHS,
MISSING_API_KEY, INVALID_API_KEY, TERMINAL_REQUIRED, PROVIDER_AUTH, PROVIDER_RATE_LIMIT,
PROVIDER_TIMEOUT, PROVIDER_FAILURE, INVALID_RESPONSE, WRONG_NETWORK, DEADLINE,
OFFLINE_MISS, STORAGE_FAILURE, UNSUPPORTED_SCHEMA, EPOCH_INCONSISTENCY,
REWARD_CONFLICT, INTERNAL, INTERRUPTED. A development-only NOT_IMPLEMENTED error
uses exit 5 while an entry point has no operational backend; never fake success.

## Report semantics

`data` is null until a usable observation exists. Input selection mode is auto
until classified, then authority or direct. Direct selection contains only the
input stake account; its relationship is null. Authority selection labels each
account staker, withdrawer or both and deduplicates before aggregation.

`summary.balance_lamports` is the sum of account balances for the selected set.
`withdraw_authority_lamports` includes withdrawer and both; `staker_only_lamports`
includes only staker. Both subtotals are null for direct selection. Unknown account
amounts make the corresponding summary null; never silently sum an incomplete
field. Incomplete discovery labels even known selected-set sums as subtotals.
`delegated_lamports` is the recorded delegation amount, not effective stake. For a
validated initialized account it is zero; for an unsupported state it is null.
Rent reserve overlaps balance. Never add balance, rent, delegation or effective
stake together. All effective/activating/deactivating amounts remain null in v1.

For a validated initialized account, `undelegated_lamports` is balance minus rent
reserve, checked for underflow. For delegated or unsupported states it is null;
no excess-balance heuristic claims withdrawability. Invalid balance/rent values
produce INVALID_RESPONSE and Unknown. Show SOL to nine fractional digits from
integer arithmetic; full precision is available wherever amounts appear.

A reward entry is unique per selected stake account/requested epoch. A recorded
entry contains the returned epoch (equal to requested), amount, post-balance,
effective slot, nullable commission, and source ID. Other states carry no record.
Latest attempt state is separate from a preserved known record; null/error on a
later attempt cannot erase it. Each epoch has counts for recorded/no_data/failed/
not_queried, summing to selected account count. A recorded subtotal is null if no
numeric reward is known for a nonempty set; zero only for explicit zeros or a
successful empty set. No wallet ownership attribution, MEV, APY or historical
commission inference. Existing numeric conflicts retain prior records and emit
REWARD_CONFLICT; persistence policy is finalized by the storage contract.

Validator grouping uses vote addresses, with nullable current commission and
current/delinquent/unknown state. Concentration is represented by exact numerator
and denominator lamports, displayed as a ratio; zero/unknown denominator is Unknown.
Names are optional. `sources` record independent RPC/cache observations; source IDs
referenced by accounts, validators and reward records must resolve. Slots remain
per source, never an invented atomic snapshot slot. Unknown layouts remain visible.

## Epoch and coverage

Read finalized epoch at the start, freeze the last N completed epochs in descending
order and check finalized epoch again after fetching. If fewer than N completed
epochs exist, request only those and emit EPOCH_RANGE_SHORTENED (info). On rollover, retry current observations
once within the same budget, retaining the original reward window. Publish as
consistent only when the reconciliation epoch remains stable. A second rollover
or insufficient deadline is EPOCH_INCONSISTENCY/partial; never relabel rewards.
Offline uses the saved epoch and never verifies genesis or epoch online.

Discovery coverage distinguishes latest attempt from displayed snapshot, including
which staker/withdrawer queries succeeded, failed or were not queried. Direct mode
marks authority queries not applicable. A previous complete snapshot remains
separate from failed refresh membership. Data is never a union of generations.
Reward and validator coverage are separate. `data.stale` and source cache flags
explain old values; online refresh errors remain visible alongside old values.

## Findings

All findings carry code, severity, affected address (nullable for report-wide
findings), evidence, observed time/slot and explanation. Initial codes:

| Code | Severity | Evidence |
| --- | --- | --- |
| UNDELEGATED_FUNDS | info | Valid initialized account with positive checked balance minus rent. |
| DEACTIVATION_REQUESTED | info | Decoded deactivation epoch is not the sentinel; no completion inference. |
| VALIDATOR_DELINQUENT | warning | Provider currently classifies the vote account delinquent. |
| UNSUPPORTED_STATE | warning | Owner/layout/state cannot be safely decoded. |
| INCOMPLETE_DISCOVERY | warning | Required query failed, truncated, unvalidated or not queried. |
| INCOMPLETE_REWARDS | warning | At least one selected account/epoch lacks a numeric record. |
| EPOCH_RANGE_SHORTENED | info | Fewer completed epochs exist than requested; report the available range. |
| EPOCH_INCONSISTENCY | warning | Bounded reconciliation did not produce stable context. |
| REWARD_CONFLICT | warning | Revalidation disagrees with a previously persisted numeric record. |
| NOT_SAVED_LOCALLY | warning | Storage write failed; in-memory data may remain useful. |

Finding evidence contains only sanitized strings; it must not include credentials
or raw URLs. Remove control characters from all provider text before display.
Show “No issues detected by these checks” only when all required checks completed.

## UI transitions

Use the layout and keymap in spec.md §§3.2–3.5. Focus cycles through visible
interactive regions in screen order; collapsed regions have no child focus target.
Default is account table with first row selected. Sort is balance descending then
address ascending; unknown amounts sort last. Search only filters visible rows,
not report totals; show X of Y. Details wrap full keys. At width 120 or greater,
place details beside accounts; at 80–119 stack them. Below 80x24 show guidance and
quit help while preserving the previous state. Render only visible rows.

Input/overlays receive keys before dashboard bindings. During address/search text
entry printable keys insert text, Enter applies, Esc cancels, Ctrl-C quits. Outside
text entry q/Esc closes the top overlay/detail first; only q on dashboard quits.
Page navigation uses visible page height; Home/End selects first/last filtered row.
Sort overlay lists labeled available columns; unknown values sort last in either
direction. Help includes field meanings, attribution limits, keymap and DB path.

Submitting a valid new address starts a fresh generation, cancels prior work,
clears address-specific selection/expansion/search, preserves focus where valid,
and shows cache or loading for the new address. Invalid submission retains the old
address and input focus. Refresh coalesces repeated requests without queueing;
preserve selection by public key, otherwise select the nearest surviving row by
previous index and show a notice. Late generation events are ignored. Offline r
shows an explanation without scheduling work. Sorting/navigation never triggers I/O.

Sections transition from loading to ready/empty/partial/failed. Refresh of usable
data retains it while loading, and on failure marks it stale with retry guidance.
Database failure marks Not saved locally independently of fetch completion. Bounded
worker queues feed typed events; drawing reads memory only and occurs on state or
terminal-size changes. Restore terminal on all catchable termination paths in UX-05.
