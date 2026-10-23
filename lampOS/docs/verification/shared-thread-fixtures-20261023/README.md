# Guard the additional conversation fixtures

Five Voice fixtures write the process-wide conversation without acquiring
its existing `one_thread()` guard: Live sign-in, Live CountOnly, the ten
interrupted questions, photo yes/no answers, and room light-rule forgetting.
Each now holds that guard from before fixture work until cleanup finishes.
The sign-in fixture takes it before its existing agent-task lock. Assertions,
timeouts and production behavior are unchanged.

An ordered control runs the actual CountOnly fixture between the own-answers
fixture installing its conversation and judging its third line. The real
unguarded `mem::take(THREAD)` reproduces its original assertion failure:
**0 passed / 1 failed**. Adding the guard preserves every original case:
**1 passed / 0 failed**. Both fixtures retain their assertions and bounds.
[Complete controls and restored source](https://github.com/autonomous-ai/lampOS/blob/432b46426/lampOS/docs/verification/shared-thread-fixtures-20261023/README.md)
also retain the initial control-only channel teardown error and correction.
This proves an additional interference path, not the cause of #1569 row 79's
historical uncaptured panic. Tests' separate #3023 Music-stop guard remains
outside this correction.

On the Mac, Rust 1.96.0 `cargo fmt --all --check`, strict all-target Voice
Clippy, and `cargo test -q -p lamp-voice --offline --no-fail-fast --
--test-threads=2` passed: **3,815 passed, 0 failed, 14 ignored**, with no
filtering. All 6,519 tracked inputs stayed unchanged during the gate on
`ae4ab0022` plus these five guard additions. The changed test names occur
only in Voice under `crates/`; no other crate changed. Native PR ARM64 and
Linux Clippy qualification is recorded on the PR and Second's log. No device
operation or audio result is claimed.
