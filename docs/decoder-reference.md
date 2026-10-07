# Native stake decoder

`stake-v1` reads the canonical 200-byte account representation from
[solana-stake-interface 5.0.0](https://docs.rs/solana-stake-interface/5.0.0/src/solana_stake_interface/state.rs.html).
The checked little-endian layout is:

| Offset | Field |
| ---: | --- |
| 0 | u32 variant: uninitialized 0, initialized 1, delegated 2, rewards pool 3 |
| 4 | u64 recorded rent reserve |
| 12, 44 | 32-byte staker and withdrawer |
| 76, 84, 92 | i64 lockup timestamp, u64 lockup epoch, 32-byte custodian |
| 124, 156 | 32-byte vote address, u64 recorded delegation |
| 164, 172 | u64 activation and deactivation epochs |
| 180, 188, 196 | 8 reserved bytes, u64 credits observed, u8 flags |

Only initialized/delegated variants carry authorities. The decoder checks owner,
non-executable status, exact account length, variant and known flags before using
those fields. Other layouts remain visible as unsupported. Padding is not assumed
zero. Epoch sentinels remain exact integers. Reserved bytes are never interpreted
as money or an activation calculation. Recorded rent is not a current network
rent calculation; principal is not a withdrawability estimate.

## Independent fixture verification

`tests/fixtures/stake/{initialized,delegated}.json` are synthetic boundary RPC
fixtures, not live account captures. Both payloads were independently verified
against the upstream Rust serializer on 7 October 2026. The fixture generator uses
distinct authority bytes, `u64::MAX`, `i64::MIN`, and credits beyond JavaScript's
safe-integer boundary. Normal tests need neither upstream crates nor network.

To repeat the independent verification without adding SDK dependencies to this
application, create a temporary Cargo project with this manifest, replacing the
absolute `path` with this checkout's generator path:

```toml
[package]
name = "ssteak-canonical-stake"
version = "0.0.0"
edition = "2024"

[[bin]]
name = "verify"
path = "/absolute/checkout/tests/fixtures/stake/generate.rs"

[dependencies]
base64 = "=0.23.1"
bincode = "=1.3.3"
serde_json = "1"
solana-pubkey = "=4.3.0"
solana-stake-interface = { version = "=5.0.0", features = ["serde"] }
```

From the repository directory, run
`rustup run 1.99.0 cargo run --manifest-path /path/to/temporary/Cargo.toml`.
The generator asserts both stored payloads match the canonical serialization;
it does not rewrite fixtures. Malformed sizes/owners/variants/flags and rent
underflow are exercised separately in `tests/domain.rs`.
