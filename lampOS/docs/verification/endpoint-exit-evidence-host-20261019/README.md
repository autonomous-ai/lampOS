# Endpoint exit evidence: host verification, 19 October 2026

Validation base: `6275fcc8cb2c229f8b18a6db3b4485cb46a11676` plus this branch's local patch. Later integration changes were not part of these local runs.

Part of #1567 and #1620. This is diagnostic instrumentation, with no switch,
no deadline changes, and no change to privacy or teardown success criteria.

## Host and commands

Run from `/private/tmp/lampos-seventh-1567/lampOS` on an Apple Silicon Mac,
Rust 1.96.0, using this worktree's own `target/`. No lamp was accessed.
All compilation commands use:

```sh
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export CARGO_INCREMENTAL=0
cargo +1.96.0 fmt --all --check
cargo +1.96.0 clippy -j2 -p lamp-turn-end-wire -p lamp-turn-end -p lamp-voice -p lamp-voice-eval -p lamp-memory -p lamp-cloud -p lamp-skills --all-targets -- -D warnings
cargo +1.96.0 test -j2 -p lamp-turn-end-wire -p lamp-turn-end -p lamp-voice -p lamp-voice-eval -p lamp-memory -p lamp-cloud -p lamp-skills --no-fail-fast -- --test-threads=1
cargo +1.96.0 build -j2 --workspace
```

## Results

- Formatting and clippy: passed, exit 0.
- Tests: passed, exit 0; 3,088 passed, 0 failed, 24 ignored across 95 result
  summaries, including the custom process harness.
- Workspace build: passed, exit 0. The first attempt was immediately
  interrupted (exit 130) because its disk precheck showed 16 GiB free, below
  the required 20 GiB floor. After authorized cleanup of only this worktree's
  build cache (and the parent cleaning its own cache), free space was
  20.85 GiB. The final build used a mandatory 20 GiB precheck and completed.
  Sources were unchanged; previously passed tests were not rerun after cache
  cleanup. The interrupted attempt was not a compiler failure.
- `git diff --check`: passed.

Existing ignored tests require pinned model/ONNX assets; explicit host timing
measurements (memory, socket, touch, speaker and polling); one/three-minute
provider runs; a four-virtual-hour release replay; or the unresolved #560
accepted-work case. They were not enabled or changed by this batch. ARM64 CI
and real-model qualification remain pending.

## What the regressions establish

`native_reap_is_visible_while_reader_and_teardown_are_pending` reaps a real
short-lived host process with a native wait, then holds the reader-completion
receipt behind a condition-variable barrier. Before releasing the barrier it
asserts `native_reaped: true`, a native exit duration, no reader receipt, no
legacy `last_exit`, and `Teardown::Pending`. After releasing it, teardown is
`Reaped`. There are no arbitrary sleeps in this regression. This is a
controlled supervisor-state test, not a reproduction of a stalled lamp pipe.

The process harness checks a normal close, forced kill during inference, and
replacement reset of the four current-worker receipts. The voice serializer
regression preserves the distinction in JSON. All these tests passed.

## Evidence limits

Durations share the current worker's spawn-attempt origin. A kill-request
receipt does not prove signal delivery; reader completion may be an error
return rather than EOF. The native receipt is recorded before reader drain,
while existing `last_exit` remains after drain. Neither receipt changes the
completion gate. No startup, acoustic, hardware or A733 latency was measured.
The original 18 October stop-side logs are still needed to diagnose those
failures; these host checks justify no larger timeout.

Raw host logs remain in `/private/tmp/lampos-seventh-watch-01a15185/` as
`1567-fmt.log`, `1567-clippy.log`, `1567-tests.log`, and `1567-build.log`.
