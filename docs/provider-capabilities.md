# Helius capability evidence — 7 October 2026

Read-only probe of the user-supplied mainnet address
`9x29Aw3N92EPCX1weRsrKKwAkbLfzw2PGRrm4NbcifEJ` used the configured environment key.
No credential or credential-bearing URL is persisted. `tests/fixtures/helius/`
contains public RPC results and `measurement.json` contains request timings.

Nine requests succeeded: genesis, finalized epoch, input account, two filtered
Stake Program queries, rewards at completed epochs 1050 and 951, vote accounts,
and a second epoch check. Both authority filters returned the same 10 accounts;
the union is 10. All 10 rewards were numeric at both tested epochs. Epoch context
remained 1051. Method times were 53–264 ms in this run. The standard vote response
contained 681 records; the checked-in fixture retains only the selected validators.
The fixture subset must not be used to assert network-wide validator completeness.

The key's commercial plan name is not exposed by these RPCs and is unknown. The
methods needed for this address are available. This evidence is not a guarantee
that every account has 100 epochs of history, nor a maximum provider capacity test.
No live epoch rollover occurred; bounded reconciliation is verified using mocks.

## Selected implementation bounds

- Standard `getProgramAccounts`, base64, finalized and `withContext:true`, with
  authority memcmp offsets 12 and 44. Validate every decoded account. The standard
  method is documented to return all matching accounts; malformed/oversized/error
  responses are failures, never silently truncated successful discovery. Do not
  use incremental `changedSinceSlot` for full membership snapshots.
- Reward batches contain at most 10 addresses: the size verified here. This is an
  application choice, not a claimed provider maximum. Explicit epoch, positional
  response matching and null/error coverage are mandatory.
- At most four requests in flight per process; 10-second per-request timeout,
  two retries for transient network/429/5xx errors, cancellable bounded backoff.
  Honor Retry-After only within the remaining operation deadline. No auth retry.
- 30-second default load budget, 120 seconds when more than one epoch is requested.
  All operations are cancellable between requests and bounded during requests;
  deadline produces partial/error data, never account truncation to fake success.
- 64 MiB response ceiling protects local resources. Exceeding it returns a visible
  incomplete-response error. No hidden top-N account cap.
- Verify genesis `5eykt4UsFv8P8NJdTREpY1vzqKqZKvdpKuc147dw2N9d` before treating a
  session as mainnet. Offline uses persisted identity without a network check.

## Primary references

- [Helius getProgramAccounts](https://www.helius.dev/docs/api-reference/rpc/http/getprogramaccounts)
  defines filtered full account discovery and response encoding.
- [Helius getInflationReward](https://www.helius.dev/docs/api-reference/rpc/http/getinflationreward)
  defines explicit epochs and nullable results in request order.
- [Helius plans](https://www.helius.dev/docs/billing/plans) describes account-specific
  entitlements; the app does not infer a user's plan from a successful request.
- Canonical stake decoding references and offsets are in `docs/decoder-reference.md`.

Live checks are opt-in; all normal tests use checked-in fixtures or localhost mocks.
