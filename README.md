# SolSteak

A local Rust terminal dashboard for inspecting native Solana stake through Helius.
It targets macOS Apple Silicon and mainnet, with SQLite persistence and one-shot
JSON output (report schema version 2). It reads public chain data through your own Helius key; it never
signs, connects a wallet, or sends transactions.

```sh
export HELIUS_API_KEY='your-key'
ssteak -a <ADDRESS>
ssteak -a <ADDRESS> --json > report.json
ssteak -a <ADDRESS> --offline
```

The TUI is keyboard-first: `j/k` move, `Left/Right` pick the chart epoch, `r` refreshes,
`a` changes address, `?` shows help, and `q` quits. `--offline` performs no HTTP
and reads saved observations from the platform data directory (Application Support
on macOS). Only public observations are stored there; API keys are never persisted.
Unknown values and reward nulls are labeled rather than converted to zero.

Rewards always cover the latest 30 completed epochs; there is no epoch option, and
`--epochs` is rejected as an invalid argument. The Account Detail panel is always
visible and starts with a chart for the selected account that sets the most recent 15
epochs (solid bars) against the 15 before them (shaded bars), paired by position:
`Up/Down` pick the account, `Left/Right` pick the pair, and `Tab` to the panel plus
`j/k` scrolls the fields below the chart. It shows exact lamports, gaps for epochs
without a recorded reward, and a "Change vs previous 15 epochs" line: the subtotal
difference and the change in the mean estimate, over pairs where both epochs have a
recorded reward only. That change is descriptive, not a forecast, and deposits,
withdrawals and commission all move it. Each recorded epoch shows an *annualized
account return estimate*: the reward over the pre-reward account balance, compounded
over 182.5 nominal two-day epochs. It is not validator or staking APY, and accounts
are never combined. The chart uses each account's current validator; historical
validator attribution is unverified. Totals since staking with a validator are not
provided. JSON output is schema version 2, including per-account `comparisons`; version 1 reports are not interchangeable.

## Development

Read [AGENTS.md](AGENTS.md), [engineering rules](docs/engineering.md), and the
[Beads workflow](docs/beads.md). Rust/Cargo, Beads (`bd`), and Dolt are development
prerequisites. `rust-toolchain.toml` pins Rust 1.99.0 with rustfmt and clippy.
No Node/npm setup is needed. Install the toolchain once:

```sh
rustup toolchain install 1.99.0 --profile minimal --component rustfmt,clippy
```

Use rustup explicitly below: Homebrew's standalone `cargo` bypasses the toolchain
file. With rustup-managed Cargo on your PATH, ordinary `cargo` also honors the pin.

```sh
bd prime
bd ready
bd show <id>
bd update <id> --claim
rustup run 1.99.0 cargo fmt --check
rustup run 1.99.0 cargo clippy --all-targets --all-features --locked -- -D warnings
rustup run 1.99.0 cargo test --locked
rustup run 1.99.0 cargo build --release --locked
```

Try the CLI without credentials or network calls:

```sh
rustup run 1.99.0 cargo run --locked -- --help
rustup run 1.99.0 cargo run --locked -- --version
rustup run 1.99.0 cargo run --locked -- --json
```

The last command intentionally demonstrates a structured missing-address error
(exit 2). Omit `--json` to receive plain errors on stderr. The wire format, local
storage recovery behavior, and launch rules are in [the application contract](docs/contracts/behavior.md).

Normal automated checks need no Helius credentials. Live checks require an explicit
request and a user-supplied key; never commit credentials. Beads holds task scope,
ordering, blockers, and handoffs. The Git remote is `origin` on GitHub.
