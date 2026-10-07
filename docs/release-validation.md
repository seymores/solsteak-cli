# Release validation

Validated on macOS Apple Silicon (`arm64`) with Rust 1.99.0.

- `cargo build --release --locked` produced `target/release/ssteak`, a Mach-O
  64-bit arm64 executable.
- `otool -L` reports only macOS system libraries/frameworks: Security,
  CoreFoundation, libiconv, and libSystem. SQLite is bundled; no database server
  or Rust runtime is needed to run the binary.
- Strict clippy, formatting, and the complete fixture/mock test suite pass.
- The supplied public wallet was inspected live through Helius: 10 selected stake
  accounts, 8 validator groups, all 10 latest completed-epoch reward records, and
  an exact recorded subtotal of 58,447,695 lamports. This is a point-in-time
  observation, not a guarantee about future provider availability.
- Pseudoterminal and frame checks cover cleanup, signals, a full offline launch,
  80×24/120×35 layouts, resize, and 1,000 rows. The renderer avoids background
  color assumptions and deterministic frame tests cover no-color mode. Manual
  inspection on a user's light and dark terminal themes remains recommended.

Install locally by copying `target/release/ssteak` to a directory on `PATH`, set
`HELIUS_API_KEY`, then run `ssteak -a <public-address>`. `--offline` needs no key.
The first online inspection creates the private SQLite file in the platform data
directory; preserve it before manual recovery if storage reports an error.
