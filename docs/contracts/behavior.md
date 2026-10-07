# Application contract (report schema v2)

This freezes the engineering defaults in spec.md v0.6 for application/CLI work.
The report wire format is `report.schema.json` (version 2); checked examples are
in `examples/`. The released version-1 schema and fixtures are kept unchanged as
`report.v1.schema.json` and `examples/v1/` (see Version compatibility).
Database DDL and measured provider limits remain separate prerequisites.

## CLI and output

The flags in spec.md §3.1 are the entire interface. Address is required except
for help/version. There is no epoch option: the reward window is always the latest
30 completed epochs (current 15 plus previous 15). `--epochs` (with or without a value, including `--epochs=N`)
is an unknown argument and fails with INVALID_ARGUMENTS under the same ordering,
JSON-envelope and no-echo rules as any other invalid argument, before any storage,
network or terminal setup; help/version must not mention it as an option. Duplicate
value flags and unknown arguments are errors. Boolean flags take no value. Help/version are
plain text and need no credentials, database, network or terminal setup.

An exact `--json` argument before the `--` delimiter requests JSON, including for
invalid arguments. `--json` used as a malformed address value still selects
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

JSON schema version is integer 2. Unsigned chain integers and lamport amounts are
canonical decimal strings; JSON counts/percentages are integers. Monetary totals
use checked u128; individual chain amounts use u64. Unknown data is null, never a
synthetic zero. `generated_at` and observation times are Unix seconds (integers);
null generated_at means the clock was unavailable. Addresses must decode to 32
bytes, including off-curve addresses; schema patterns are structural checks only.
`input` is `{address, offline}` (no epoch field) or null for argument-validation
failure so invalid raw input is never echoed.
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

Stable initial error codes: INVALID_ARGUMENTS, INVALID_ADDRESS,
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
Every entry also has `account_return` (below). Latest attempt state is separate from a preserved known record; null/error on a
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

## Annualized account return estimate

`account_return` is null unless the entry state is `recorded`, and is also null
when the estimate is Unknown. Otherwise it is
`{pre_reward_balance_lamports, annualized_percent}` for that stake account and
epoch only; never combine balances or estimates across accounts, epochs or
validators, and never add a summary-level return.

From the recorded `amount` and `post_balance` (u64), compute with checked integer
arithmetic `pre = post_balance - amount`. Then:

| Input | Result |
| --- | --- |
| `pre > 0` | `annualized_percent = ((1 + amount / pre)^182.5 - 1) × 100` |
| `amount = 0`, `pre > 0` | `"0.0000"` (a valid 0%) |
| `pre = 0` (zero denominator) | `account_return` null; no error |
| `amount > post_balance` (underflow) | `account_return` null; INVALID_RESPONSE error for that address; record kept as received |
| Result not finite or ≥ 1e12 percent | `account_return` null; INVALID_RESPONSE error for that address |

`pre_reward_balance_lamports` is the exact decimal u64 string. Convert amount and
pre-reward balance to IEEE 754 double, divide, and apply `powf`; the exponent 182.5
is a fixed nominal two-day epoch in a 365-day year. Only the percentage is
rounded: formatted to exactly four fractional digits (correctly rounded from the
double's exact value) and emitted as a decimal string
(`^(0|[1-9][0-9]{0,11})\.[0-9]{4}$`), so no binary float is exposed on the wire.
Lamport amounts never pass through floats; only this displayed estimate does.

The headline estimate is the latest completed epoch's entry; an earlier graph
point shows that epoch's own estimate. Human text calls it “Annualized account
return estimate” and states that it uses total pre-reward account balance, not
effective stake, and a nominal epoch length. Never call it validator, staking or
real APY. It is derived on report construction from persisted reward fields;
no storage change or persisted derived value is required.

## Period comparison

`data.comparisons` has one object per account, in `data.accounts` order (an empty
array when there are no accounts). The window is `data.requested_epochs`, newest
first: the first 15 entries are the current period and the next entries (up to 15)
the previous period. Pair `i` is `(requested_epochs[i], requested_epochs[15+i])`
and exists only when index `15+i` exists, so a chain with 15 or fewer completed
epochs has no pairs. A pair is *compared* only when both entries have state
`recorded` with a record (an explicit zero counts); every other existing pair is
*left out*. `compared_pairs + left_out_pairs` equals the number of existing pairs.

| Field | Rule |
| --- | --- |
| `current_subtotal_lamports`, `previous_subtotal_lamports` | Checked u128 sums of `amount` over compared pairs only; null when `compared_pairs` is 0. |
| `difference_lamports` | Signed decimal string `current − previous`; null when `compared_pairs` is 0. |
| `percent_change` | `difference / previous × 100` to four fractional digits by exact integer arithmetic (multiply by 10^6 before dividing), rounded half away from zero, as a signed decimal string; null when the previous subtotal is zero or nothing was compared. Never `-0.0000`. |
| `estimate_pairs` | Compared pairs where both entries have a non-null `account_return`. |
| `current_mean_estimate_percent`, `previous_mean_estimate_percent` | Arithmetic mean over the `estimate_pairs` of the unrounded FR-14 percentages (same double computation), formatted to four fractional digits; null when `estimate_pairs` is 0. |
| `estimate_difference_pp` | Difference of the two unrounded means in percentage points, formatted signed to four fractional digits (never `-0.0000`); null when either mean is null. |

Unknown is null, never zero. A comparison is per account; never add accounts,
validators or epochs beyond the paired sums above, never add a summary-level
comparison, and never rank. Rounding only affects strings. Text calls this
“Change vs previous 15 epochs”, states the direction in words (higher, lower,
unchanged) and the compared and left-out pair counts, and says it is descriptive:
it moves with deposits, withdrawals, splits, merges, commission and epoch effects
as well as the chain's reward, is not a forecast or validator-quality measure, and
uses the current validator only (historical attribution unverified).

## Epoch and coverage

Read finalized epoch at the start, freeze exactly the latest 30 completed epochs in
descending order (`data.requested_epochs`, at most 30, unique) and check finalized
epoch again after fetching. Every online load and refresh resolves this window;
cached recorded rewards are reused, missing/error coverage is retried, and an
explicit `--refresh` revalidates the window. If fewer than 30 completed
epochs exist, request only those and emit EPOCH_RANGE_SHORTENED (info) with
evidence naming the available count. On rollover, retry current observations
once within the same budget, retaining the original reward window. Publish as
consistent only when the reconciliation epoch remains stable. A second rollover
or insufficient deadline is EPOCH_INCONSISTENCY/partial; never relabel rewards.
Offline resolves the same 30-position window from the saved last-observed epoch,
labeled `last_observed`, and never verifies genesis or epoch online. Epochs outside
cached coverage are `not_queried` gaps, never zero. The window is always one
reward position per requested epoch: `data.rewards` and `coverage.rewards` carry
exactly the epochs in `requested_epochs`, in the same order.

Discovery coverage distinguishes latest attempt from displayed snapshot, including
which staker/withdrawer queries succeeded, failed or were not queried. Direct mode
marks authority queries not applicable. A previous complete snapshot remains
separate from failed refresh membership. Data is never a union of generations.
Reward and validator coverage are separate. Graph gaps are exactly the entries whose
state is not `recorded`. `data.stale` and source cache flags
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
| EPOCH_RANGE_SHORTENED | info | Fewer than 30 completed epochs exist; report the available range. |
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
place details beside accounts; at 80–119 stack them (detail is always shown). Below 80x24 show guidance and
quit help while preserving the previous state. Render only visible rows.

Input/overlays receive keys before dashboard bindings. During address/search text
entry printable keys insert text, Enter applies, Esc cancels, Ctrl-C quits. Outside
text entry q closes the help overlay first; only q on dashboard quits and Esc clears
the focused search.
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

Paired chart (spec FR-15): the Account Detail panel is always visible, beside the
accounts table at width 120 or greater and below it otherwise, and shows the
selected row. Its body is one scrolling body (chart lines first, then the account
fields); Tab to Account Detail and j/k/PageUp/PageDown/Home/End scroll it, clamped to
the wrapped content. Tab order is accounts, detail, attention, validators. Up/Down
move the table row (and reset the detail scroll); Left/Right select the pair when the
table or detail has focus: Left is an older pair, Right a newer one, clamped with no
wrap. The pair is kept by its current-period epoch number so refresh and rollover
preserve it; if it left the window the newest pair is selected, and a new address
clears it. Changing row keeps the pair.

The chart shows the selected account only: up to 15 columns, oldest pair left, each
with two adjacent bars, previous period then current period, on one scale (the
account's largest recorded reward in the window). Current bars use eighth-block
glyphs (heights adapt to the panel, at least one row); previous bars use the shade
glyph `░` over whole rows (at least one row for a nonzero reward), so the periods
differ without color. Non-recorded epochs are labeled gap markers on the bar's
bottom row (- no data, ! failed, ? not queried, 0 recorded zero), never zero-height
bars; an epoch outside the window is blank. A caret marks the selected pair. Column
width is 3 cells on narrow panels and 5 on panels wide enough (inner width 75 or
more); monochrome mode adds no color dependence. Positions without a previous epoch
show only the current bar.

Text lines, in priority order within the same scrolling body: the “Change vs previous
15 epochs” summary (compared and left-out counts, subtotal difference and percent
change, mean estimate change; Unknown values shown as Unknown), the bars and caret,
the selected pair (both epoch numbers, exact rewards, coverage, each estimate,
the pair's lamport difference only when compared, latest attempt when it differs),
the legend (`░` previous 15, `█` current 15, gap markers), the latest completed
epoch's annualized account return estimate, and the attribution label. At 80x24 the
first three groups must be visible; the rest are reached by scrolling. The title
carries the account and its current validator, or none. Text input and overlays take
keys first. Selection changes, resize and redraw never trigger I/O.

Sections transition from loading to ready/empty/partial/failed. Refresh of usable
data retains it while loading, and on failure marks it stale with retry guidance.
Database failure marks Not saved locally independently of fetch completion. Bounded
worker queues feed typed events; drawing reads memory only and occurs on state or
terminal-size changes. Restore terminal on all catchable termination paths in UX-05.

## Version compatibility

Schema version 2 replaces version 1; the two never mix. A v2 reader rejects any
document whose `schema_version` is not 2 and a v1 reader rejects v2 documents, with
no field-by-field upgrade. v1 differs by `input.epochs`, a variable-length window,
INVALID_EPOCHS and the absence of `account_return`. v1 files stay in `examples/v1/`
and `report.v1.schema.json` only to test that rejection and any retained legacy
decoding; do not extend them.

SQLite DDL and `user_version` stay unchanged. Durable reward rows hold every
FR-14 input and are reused as is. Saved snapshot report JSON carries its schema
version: persisted version-1 snapshot reports are not served as v2 data (offline
shows “No local data for this address” until one online refresh writes a v2
snapshot), while reward rows survive and are reused for the 30-position window.
Storage must write and validate schema-v2 report JSON.
