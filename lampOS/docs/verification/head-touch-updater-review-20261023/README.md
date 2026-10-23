# Independent review of the head-pins updater prerequisite

PR #3269, exact `c062243b6da9b5dad14551d176ef79d81483758e`, based on
`2615c87b6`. Intel macOS, Rust 1.96.0; no device action or ARM64 linking.

## Existing tests

The clean head's fourteen transition cases passed in a fresh, checkout-specific
Cargo target: **14 passed, 0 failed**, exit zero, 170.60 seconds of test runtime.
Source and tests were clean before the independent controls were appended.
[Output](unchanged-head-tests.txt).

```sh
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/private/tmp/third-3269-target cargo test -q --locked --offline -p lamp-update --test round head_touch:: -- --test-threads=2
```

## Two reproduced P2 findings

The [added controls](regressions.rs.txt) append to
`lampOS/crates/update/tests/round/head_touch.rs` without changing its original
21,583 bytes. [Hashes and invocation](results.json) record that prefix and
confirm the complete test file stayed unchanged during the run.

1. A signed Voice unit with the head flag and absolute mute path, but an
   isolated `--controls-bus-root`, is accepted as paired with Expression and
   Controls on the normal bus. The refusal control receives `Switched(2)`.
   The runtime source routes Voice's head subscriptions and mute publisher
   to that isolated root; physical privacy still uses the normal root.
   Expression can therefore receive Hold without the corresponding Voice
   mute. The updater must check effective bus roots as part of its pair.
2. Already paired installed readers with build-local active pins and config
   are refused because an unused fixed product pins file still has legacy
   enabled. The ordinary binaries-only/settings-off control receives the
   new head-pins mismatch refusal instead of `Switched(2)`. This matches
   the build-folder layout of tracked lamp-4ace's Controls unit; the live
   lamp's files and flags were not read. Projection and verification must
   follow the active Controls pins path and preserve custom wiring.

```sh
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/private/tmp/third-3269-target cargo test -q --offline --locked -p lamp-update --test round third_review_ -- --test-threads=1 --nocapture
```

[Result](regressions.txt): **0 passed, 2 failed**, exit 101, at the intended
updater decisions; no compilation or environment failure. These tests use
mock systemd, channel and binaries. They establish the updater decisions,
not a played sleep/microphone observation. Production files are unchanged.

The source read found no further issue in projected-byte hashes, repeat
updates, rollback ordering or old-helper mismatch handling. Older-helper
coverage simulates its output/receipts, not an actual historical executable.
The exact-head HOLD is on #3269; #2948 retains its separate combined-carry
HOLD. Native [Linux Clippy 39952200929](https://github.com/autonomous-ai/lampOS/actions/runs/39952200929)
and [ARM64 39952201001](https://github.com/autonomous-ai/lampOS/actions/runs/39952201001)
both completed successfully on `c062243b6`. The ARM64 workflow's
sign/publish and failure-report jobs were skipped. Green existing CI does
not resolve the two added failing controls.


## Parent implementation control

The unchanged build-local-pins regression passes **1/0**, exit zero, with
the parent `2615c87b6` runtime. Only `apply.rs`, `round.rs` and `lib.rs` were
temporarily restored from that parent in the detached review lab; the new
head-touch module was then unused. The [log](parent-control.txt) gives the
exact single-test command. The [hash receipt](parent-control-hashes.json)
confirms all three production files were restored exactly to `c062243b6`
and the test file was unchanged. This proves the fixed-path refusal is
introduced by the prerequisite. It is not a full parent gate and makes
no claim that split buses were safe before this change.

Raw output files retain their original trailing blank lines. Markdown,
fixture text and JSON passed the focused whitespace check; two raw streams
in the preceding evidence commit report blank EOF lines under Git's generic
whitespace check and were deliberately kept byte-for-byte.
