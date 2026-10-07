# SolSteak

A local Rust terminal dashboard for inspecting native Solana stake through Helius.
V1 targets macOS Apple Silicon and mainnet, with SQLite persistence and one-shot
JSON output. It reads public chain data through your own Helius key; it never
signs, connects a wallet, or sends transactions.

```sh
export HELIUS_API_KEY='your-key'
ssteak -a <ADDRESS>
ssteak -a <ADDRESS> --epochs 10
ssteak -a <ADDRESS> --json > report.json
ssteak -a <ADDRESS> --offline
```

The TUI is keyboard-first: `j/k` move, `Enter` opens details, `r` refreshes,
`a` changes address, `?` shows help, and `q` quits. `--offline` performs no HTTP
and reads saved observations from the platform data directory (Application Support
on macOS). Only public observations are stored there; API keys are never persisted.
Unknown values and reward nulls are labeled rather than converted to zero.

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
ordering, blockers, and handoffs. No Git remote is configured.
