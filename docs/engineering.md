# Engineering rules

## Scope
- Build the Rust CLI described in spec.md.
- Use Beads for task ownership, dependencies, progress, and handoffs.
- Read the assigned task and its linked requirements before editing.
- Claim implementation work before starting.
- Record unrelated discoveries as follow-up tasks.

## Correctness
- Keep monetary amounts in integer lamports for arithmetic.
- Distinguish missing or unavailable data from zero.
- Validate user input before making network requests.
- Never print API keys or unredacted URLs containing API keys.
- Keep business logic separate from network access and terminal output.

## Verification
Run the checks relevant to the change, with these as the baseline:
- rustup run 1.99.0 cargo fmt --check
- rustup run 1.99.0 cargo clippy --all-targets --all-features --locked -- -D warnings
- rustup run 1.99.0 cargo test --locked

Use the pinned toolchain explicitly: Homebrew's standalone Cargo does not honor
rust-toolchain.toml. Rustup-managed Cargo on PATH also honors the pin.

Automated tests must run without a real Helius key.
Use fixtures or a local mock server for network-dependent behavior.
Live API checks are separate and explicitly requested.

## Handoff
Record in the bead:
- Branch and commit
- What changed
- Checks actually run and their results
- Remaining risks or blockers
- Exact next action

Close tasks after acceptance criteria and relevant checks pass. If the user
explicitly requires review or integration, keep them open until it is satisfied.
Completing local work does not require a remote merge.
Do not commit, merge, publish, or deploy unless instructed.
