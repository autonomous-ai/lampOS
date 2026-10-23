# Voice fixtures near boot, 23 October 2026

The first Linux x86 run reproduced the original overflow and then caught a
new fault in the candidate. These are control results, not a passing gate.
The raw streams, commands, test census and source bindings are in
`initial-linux/`; `export-map.json` maps unchanged runner source to text
attachments. No binary, Cargo cache or source archive is included.

| Source and execution | Result |
|---|---|
| Base `f477d6c49`, held-lines case at 60.09 s | 0 passed / 1 failed, subtraction overflow at worker.rs:8341 |
| Initial candidate `b5f2e39d2`, individual cases at 60.x s | 9 passed / 1 failed; stopped, 76 not run |
| Same base binary, first hand-touch control at 60.70 s | 1 passed / 0 failed |

Both initial binaries list the same 3,583 tests. Each selected case ran in
its own Linux time namespace, with an outside real-time watchdog. The outer
clock and offsets stayed unchanged. Native Linux x86 ran in the existing
rootless local container; this is not an ARM64 or lamp result.

The candidate failure was
`coordinator::tests::hand_controls::a_head_touch_stops_the_lamp_while_it_says_its_answer`.
Its synthetic provider fault was sent at the raw clock, about 60.45 seconds,
before the receiver entered the fixture's 600-second epoch. The stale-message
error therefore reported a 539,550,547 microsecond age. Passing the unchanged
base case establishes that this failure belonged to the candidate.

The correction in `da595851f` starts the scope after every child's real
readiness barrier, before the first queued message and before the camera
observer samples its clock. Host-clock fixtures remain unscoped. The original
305 hand-control assertion macro bodies and all time bounds are unchanged.
`correction/` retains the exact patch, source hashes and original failure
stream hashes. Formatting and `git diff --check` pass for the corrected source.

## What was not shown

The corrected 86-case run, Mac before/after full-package gates, stricter
near-boot controls, Linux formatting/Clippy and native ARM64 qualification
have not finished. The original candidate's final source and storage checks
did not run after its stop; that limit is explicit in its terminal summary.
No retry erased the failure, and no product behavior or device was changed.
