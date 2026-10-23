# PR #3271 room-history failure: source comparison

This read-only comparison retains the failed [ARM run 39964412356](https://github.com/autonomous-ai/lampOS/actions/runs/39964412356). No test, rebuild, rerun, source change or stacked patch was performed for this audit.

The checked merge `a6a267bf26c1187170876292bd49c36a870ba0a3` has base `810e05109` and peer head `55f671b12`; its tree equals that authored head. The failure is the quiet-room **setup assertion** at `worker.rs:8632:17`, before constructing `Heeding` or sending any held line: reset `RoomTold`, call `at(now - 400_000_000)`, then assert `at(now - 90_000_000) == Quiet`. The actual diagnostic is `Unknown` versus `Quiet`. [Current raw failure](current-failure.json) preserves it without waiver.

[Source identities](source-identities.json) prove the full worker file is identical between base and peer (`4e3b891f…`), and the exact test function also equals the historical `f477d6c49` control baseline. `room_told.rs` and `ipc/src/lib.rs` are byte-identical across base, peer, boot `f121d6391`, and the two historical control refs. IPC samples `CLOCK_MONOTONIC`, then truncates nanoseconds to microseconds. Peer changes only its two declared fixture/doc files.

Boot PR #3190 at `f121d6391e53bf709ffbf07807e8657f7e86d3ce`, also parented by `810e05109`, adds a thread-local 600-second scenario epoch to this exact test, uses that clock in the worker under `cfg(test)` and for its face publication, and keeps the 400-second/90-second offsets and every original assertion. The exact corrected test function and complete scenario-clock file are byte-identical to the historical corrected `da595851f`. [Exact source slices](source-excerpts.json) preserve the original bytes.

A source-level conditional explains the vulnerable boundary: with **wrapping unsigned subtraction** and a sampled `now` in `[90 s, 400 s)`, the 400-second subtraction wraps while the 90-second subtraction does not. `RoomTold::at` stores the wrapped start; both relevant `saturating_sub` durations are zero, so it stays `Unknown`. This is arithmetic/source reasoning, not a measurement of this failed runner. The ARM log reports an optimized release invocation, but does not capture the sampled monotonic value or a compiler-artifact overflow-check record. No actual failed-run uptime is inferred.

The exact earlier Linux x86 namespace control is `case-001` in each historical run. [Commands, clock samples and binary identities](historical-controls.json), plus the [complete four raw streams](historical-control-streams.json), record:

| Frozen source | Host CLOCK_MONOTONIC immediately before exec | Exact case result |
| --- | --- | --- |
| Baseline `f477d6c49` | 60.090506284 s | Exit 101; 0 passed / 1 failed; checked subtraction overflow at worker.rs:8341:25 |
| Corrected `da595851f` | 60.825566460 s | Exit 0; 1 passed / 0 failed; same original assertions |

Those binaries used the default test profile with `overflow_checks: true`; their full hashes are retained. This establishes the fixture's need for explicit prior history and that the existing correction handled that controlled boundary. It is **not** a reproduction of release `Unknown` versus `Quiet`, nor a native ARM before/after comparison. The old corrected source and current boot patch are related by exact relevant source identity, not whole-binary identity. Historical controls and their original timestamps have not been relabelled as current runs.

Recommendation to First and the reviewers: reuse the already reviewed #3190 fixture-clock correction, subject to its existing HOLD disposition, rather than duplicate it in this peer fixture PR. No prerequisite, stacking, waiver or readiness decision is made here. The current ARM failure and its 1,341 selected unrun tests remain unwaived.

JSON `text` fields decode to exact UTF-8 bytes; their SHA256 and byte counts permit verification. Full referenced source objects and original control receipts remain external at the recorded paths. `SHA256SUMS` covers every included file, excluding itself.
