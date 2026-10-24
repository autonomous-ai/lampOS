# Controls published writer/reader pair, 24 October 2026

Part of #2809. This is a test-only calibration/pins compatibility slice;
it does not complete the whole-lamp update row. Official native execution
and independent review are pending when this record is introduced.

| Release | Source | Controls asset | Bytes | SHA-256 |
|---|---|---|---|---|
| 6408 | `ee53765c8d014eb00e41c1a5a7054d2546c18bdb` | 668607702 | 2,463,440 | `fc3f73157e21c6b3f662ee469f5febeeaa56bbc84091e0b781157192a3961555` |
| 6498 | `846b3d203fc00ec3886d71f7673d8cc29cc4a634` | 670759835 | 2,463,440 | `fc3f73157e21c6b3f662ee469f5febeeaa56bbc84091e0b781157192a3961555` |

Both published Controls payloads are identical. Release identity and hashes
bind the selected bytes; they do not establish distinct implementations or
verify release signatures. No binary is committed. The workflow downloads
the named public release assets, checks size, SHA-256 and ELF AArch64 shape,
then makes them read-only. Ordinary dependency/download network is available.

The exact source under test is recorded from the CI checkout, including its
merge-preview revision and authored PR head. Before/after maps compare every
tracked regular file and exact symlink text. The artifact contains raw
stdout/stderr, arguments, pinned binary hashes, per-command exit/reap/group
receipts, full synthetic data files and terminal results. Released copies
are excluded from upload. Test, tee, receipt, cleanup and source-check exit
codes are retained independently.

Run the driver on the development machine:

```sh
cargo test --locked -p lamp-controls --test release_pair -- --test-threads=1
```

This runs one current-CLI smoke test and ignores the two actual-release
cases. It is not actual-pair qualification. The native ARM64 workflow runs:

```sh
cargo test --locked --release -p lamp-controls --test release_pair -- --ignored --nocapture --test-threads=1
```

It requires exactly **2 passed, 0 failed, 0 ignored, 1 filtered**, matching
two named terminal receipts with 18 successful child reaps each. Nothing
runs ARM64 executables or links ARM64 on the Mac.

The input is authored three-electrode calibration and strip replay, with
threshold expectations 17/10, 25/14 and 33/18. Both directions and return
reads compare complete stored values and output bytes. Refusal cases cover
overwrite, malformed/invalid files and unsafe unknown fields; benign
unknown-field acceptance and recovery are separate positive controls.
See the [fixture contract](../../../crates/controls/tests/release_pair/README.md).

No lamp, GPIO/I2C, acoustic measurement, real touch, motor, provider, service
restart or updater transaction participates. This establishes only the
selected published file writer/reader behavior if the native gate passes.
Physical calibration and action-to-feedback latency remain unverified.

## Local driver gates

Intel Mac: formatting, strict all-target Controls Clippy, full serial Controls
**509 passed, 0 failed, 3 ignored**, and workspace build pass with the four
Rust/manifest source hashes unchanged. The actual-release cases are two of
the ignored tests. All eight workflow shell blocks pass `bash -n`, YAML
parses, local links and whitespace pass; `actionlint` is unavailable.

The initial sandbox run stopped at Controls with 15 existing Unix-socket
bind failures (`Operation not permitted`); workspace was not run after that
failure. The socket-enabled rerun above is separate evidence. The first
fixture smoke exposed a 17/18 command-counter mismatch; a final original
reader reopening now makes all 18 execute. No actual published executable
was downloaded or run on the Mac. Native pair and ordinary Linux gates
remain pending.
