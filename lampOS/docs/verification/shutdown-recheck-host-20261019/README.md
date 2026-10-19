# Shutdown exit recheck: host verification, 19 October 2026

Part of #1620. Original verification base: `4fd3b33443dac390cd49d613139c35b02cc512b7` plus
this branch's local patch. These are macOS ARM64 host results, not lamp results.

## Regression and scope

Native reaping of one worker can take time. The next worker may exit during
that wait, after the coordinator's earlier polling pass. The deadline sweep
now checks its native status again before requesting forced termination.
An observed exit still runs normal resource cleanup; a nonzero exit remains a
failure, and a clean exit does not clear an earlier worker's failure. A status
query error still leads to termination and reaping. There is no extra wait and
no change to the shared 300 ms / diagnostic 750 ms grace.

The regression `capture_exiting_during_speaker_reap_is_not_forced` keeps capture
alive until speaker has been killed and reaped. A one-shot test-only callback
then closes capture's stdin and observes its native exit with `WNOWAIT`, leaving
the coordinator responsible for reaping it. The test checks that capture is not
terminated again, only speaker is recorded as forced, the speaker error survives,
all four workers are reaped before drop, and microphone permission is denied.

Mutation evidence: replacing only the deadline sweep with the base version
makes that test fail with `capture worker did not stop within shared 300 ms
... reaped with exit status: 0`. Restoring the patch passes. This is a controlled
ordering test, not a reproduction of the lamp's recorded 277 ms gap.

## Original gate

Rust 1.96.0, from `lampOS/`, using this worktree's own target directory:

```sh
export RUSTC_WRAPPER=''
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export CARGO_INCREMENTAL=0
cargo +1.96.0 fmt --all --check
cargo +1.96.0 clippy -j2 -p lamp-voice -p lamp-voice-eval -p lamp-memory -p lamp-cloud -p lamp-skills --all-targets -- -D warnings
cargo +1.96.0 test -j2 -p lamp-voice capture_exiting_during_speaker_reap_is_not_forced -- --test-threads=1 --nocapture
cargo +1.96.0 test -j2 -p lamp-voice -p lamp-voice-eval -p lamp-memory -p lamp-cloud -p lamp-skills --no-fail-fast -- --test-threads=1
cargo +1.96.0 build -j2 --workspace
```

- Formatting and clippy passed.
- Focused regression passed after the expected mutation failure.
- Full tests: 3,060 passed, 0 failed, 17 existing ignored tests in 87 summaries.
- Workspace build passed.

During fixture development, compilation caught a missing test-only initializer
in `process/ring_startup.rs`; it was added. An extra reply-cleared assertion
initially failed because the fixture passed a successful run with an active
reply. The final fixture passes an interrupted stop, matching the cancellation
path being tested. Sandbox-denied socket binds were environment failures and
were not counted as mutation proof. Final native tests ran with socket access.

## Rebased gate

Rebased onto `d9ba8d0f2a5070569fef3225a0924cdcdc005aba`, including
#1703's bounded cleanup-failure collector. The exit recheck records observed
exit failures through `CleanupStage::WorkerExit` and preserves earlier causes.
Independent static review found no blocking issue.

Formatting, the same five-package clippy command, and the full test command
above passed: 3,164 passed, 0 failed, 17 ignored. Workspace build passed.
The first gate stopped before the build because free disk was below 20 GiB.
After cleaning only this worktree's completed test cache, the workspace build
ran successfully; completed tests were not repeated.

## Not shown

A child can still exit between a fresh `try_exit(None)` and the kill request;
this does not eliminate that unavoidable race. The change addresses a known
exit that occurred while an earlier native wait was in progress. It neither
identifies the original lamp I/O/scheduling cause nor changes diagnostic writer
or endpoint timeouts. Real-lamp latency and Linux ARM64 CI remain separate checks.
