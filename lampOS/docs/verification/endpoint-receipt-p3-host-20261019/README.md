# Endpoint receipt P3 follow-up: host verification

19 October 2026, macOS ARM64, Rust 1.96.0. Base `55271a2de4a003ce193244aa4f1061fd41adbea2` plus this
branch's test/documentation changes. Part of #1567; addresses the two P3s on #1704.

The normal-status-query fixture no longer requires a cooperative worker to exit
within its 200 ms grace on a loaded host. It checks that the kill receipt is
absent exactly when the lifetime kill count is zero in this single-worker case,
and still requires native reap and reader completion. The separate
status-query-error path is documented as requesting a kill without incrementing
the legacy lifetime counter. Old records lacking the new keys are distinguished
from new records with null receipts. Runtime source is unchanged.

## Checks

From `lampOS/`, using its own target and
`CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0`:

```sh
cargo +1.96.0 fmt --all --check
cargo +1.96.0 clippy --offline -j2 -p lamp-turn-end-wire --all-targets -- -D warnings
cargo +1.96.0 test --offline -j2 -p lamp-turn-end-wire -- --test-threads=1
cargo +1.96.0 clippy --offline -j2 -p lamp-turn-end-wire -p lamp-turn-end -p lamp-voice -p lamp-voice-eval -p lamp-memory -p lamp-cloud -p lamp-skills --all-targets -- -D warnings
cargo +1.96.0 test --offline -j2 -p lamp-turn-end-wire -p lamp-turn-end -p lamp-voice -p lamp-voice-eval -p lamp-memory -p lamp-cloud -p lamp-skills --no-fail-fast -- --test-threads=1
cargo +1.96.0 build --offline -j2 --workspace
```

All commands passed. The focused crate run passed 5 library and 12 process tests.
The full run passed 3,176 tests, failed none, and retained 25 existing ignored
tests across 96 summaries, including the custom process harness. The workspace
build used the same source and passed before the full dependency test run.

No lamp was accessed; these checks do not establish hardware latency or model
accuracy. ARM64 validation belongs to this follow-up PR's current-head CI run.
