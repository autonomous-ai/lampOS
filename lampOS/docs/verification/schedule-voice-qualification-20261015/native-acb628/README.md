# Native ARM64 run for the clarified disable runbook

[Run 38627179829](https://github.com/autonomous-ai/lampOS/actions/runs/38627179829)
passed for `acb628b494df0c38842ea467d1289eec8d35d2ca`: **4,338 passed / 0
failed / 33 ignored**, 279 completed harnesses, on GitHub Ubuntu 22.04 ARM64
with Rust 1.96.0. The release workspace build, serial tests, checksum step and
artifact upload succeeded. [Result](result.json), [original raw log](run.txt),
[counts and checksums](retention.json), and [copy receipt](copy-receipt.json)
retain the observations and exact bytes.

Commands: `cargo build --locked --release --workspace --bins --examples`,
then `cargo test --locked --release --workspace -- --test-threads=1`.
Four inherited output-filename warnings name the Controls/Expression
`replay_cost` examples and their `.dwp` paths. They remain recorded; this is
not a warning-free workspace claim.

The Actions, Schedule and Voice checksums equal the preceding `01192` run,
consistent with the verified documentation-only difference. This result
still belongs to `acb628`, based on `6573`; it does not qualify the newer
`1337` integration or `d794cf2` task head.

## Not shown

No lamp, real provider, physical audio or device scheduling transition was
exercised. The artifact is an exact CI build result, not a device result.
