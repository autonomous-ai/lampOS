# Shutdown failure aggregation: host verification, 19 October 2026

Part of #1620 and PR #1703. Validation base:
`6275fcc8cb2c229f8b18a6db3b4485cb46a11676` plus this branch's review fix.
These are macOS ARM64 host results, not lamp measurements.

## Behavior checked

Cleanup records failures by stage and worker. Distinct entries are returned in
first-seen order, so a forced speaker, a forced capture and an endpoint failure
can all be reported. Repeated observations of one stage/worker keep the first
detail and a saturating count. Production has at most 128 keys (136 with the
test-only fixture stage), with each stored error message bounded to 1,024 UTF-8
bytes. A short single error preserves the original native error/kind; a joined
summary uses the first error's kind. Existing trace emission remains unchanged.

The termination fixture covers one or both audio workers finishing diagnostics
900 ms after Stop, beyond the existing shared 750 ms diagnostic grace. It checks
both forced-stop causes and both certification errors when both workers are
late, all four children reaped before drop, microphone permission denied, reply
cleared, and both certification trace rows retained. Missing speaker diagnostics
alone still fail an otherwise clean shutdown. Peer sockets remain alive until
shutdown returns, and capture exits only after sending its finish receipt.

Collector tests cover native error preservation, speaker/capture/endpoint and
certification causes in first-seen order, 10,000 repeated observations, UTF-8
truncation, saturating counts, and every stage/owner key. The worst-case test
summary remains below 160 KiB. Collecting failures never skips a cleanup call.

## Gate

Rust 1.96.0, from `lampOS/`, with the checkout's own `target/` directory:

```sh
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export CARGO_INCREMENTAL=0
cargo +1.96.0 fmt --all --check
cargo +1.96.0 clippy --offline -j2 -p lamp-voice -p lamp-voice-eval -p lamp-memory -p lamp-cloud -p lamp-skills --all-targets -- -D warnings
cargo +1.96.0 test --offline -j2 -p lamp-voice --lib coordinator::tests::termination:: -- --test-threads=1
cargo +1.96.0 test --offline -j2 -p lamp-voice --lib coordinator::cleanup:: -- --test-threads=1
cargo +1.96.0 test --offline -j2 -p lamp-voice -p lamp-voice-eval -p lamp-memory -p lamp-cloud -p lamp-skills --no-fail-fast -- --test-threads=1
cargo +1.96.0 build --offline -j2 --workspace
```

- Formatting and all-target clippy passed.
- Focused termination: 16 passed; collector: 4 passed; neither had a failure.
- Full tests: 3,042 passed, 0 failed, 16 existing ignored tests in 87 summaries.
- Workspace build: passed.

The original single-worker fixture exposed a socket-lifetime race during its
initial development; its peer lifetime was corrected before these runs. The
review fix adds the two-late-workers case from Tests' peer review, which the
initial single-worker case did not cover.

## Not shown

No stop deadline, forced-termination order, microphone revocation or restart-guard
success criterion changed. These synthetic host tests do not measure acoustic
latency, physical microphone closure or the cause of the lamp's delayed workers.
The first builder's metadata reports three endpoint-Pending failures in 124
requested stops (none muted the next run), and two invalid recording finishes
in six fixture takes. This patch preserves their available cleanup causes; it
neither increases a timeout nor identifies the I/O or scheduling cause.
Linux ARM64 validation is reported on the pull request for its current head.
