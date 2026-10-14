# Native ARM64 qualification of the current task head

[Run 38633124293](https://github.com/autonomous-ai/lampOS/actions/runs/38633124293)
passed on exact head `d794cf2aca8fd5a7891af6363f2155e9fd3091fe`: **4,365
passed / 0 failed / 34 ignored**, 282 completed harnesses. The job used
GitHub Ubuntu 22.04 ARM64 and Rust 1.96.0. Release workspace build, serial
tests, binary checksums and artifact upload all succeeded.

Commands: `cargo build --locked --release --workspace --bins --examples`,
then `cargo test --locked --release --workspace -- --test-threads=1`.
[Terminal result](result.json), [original raw output](run.txt),
[counts and every binary checksum](retention.json), and [copy receipt](copy-receipt.json)
retain the actual run and exact bytes. The artifact is named
`lampos-arm64-d794cf2aca8fd5a7891af6363f2155e9fd3091fe`.

| Binary for Task 31 | SHA-256 from the successful run |
| --- | --- |
| `lamp-actions` | `33a16f446b32697b608d6ab5307ebd4917b32265635800266240b3be541dc6cb` |
| `lamp-schedule` | `12598462e7005ecbef98838dde2a83cf9c6e557de204b3a1e1d0749c90290664` |
| `lamp-voice` | `a29fcf3986cc58cd52c9142a39a280d4ae2dce62d41d8c7c999b9760a448d19f` |

Four inherited output-filename warnings concern the Controls/Expression
`replay_cost` examples and their `.dwp` paths. They remain in the raw output;
this is not a warning-free workspace claim. The three production binary
names above are distinct from that collision.

## Not shown

This qualifies the named CI build and tests, not a lamp. No physical audio,
real provider, flash failure or privileged RR20-to-Other transition was
exercised here. First's separate off-state minute and seven enabled lamp cases
remain required by the task runbook.
