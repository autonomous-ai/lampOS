# Peer-fixture native failure: room setup source read

Second's scoped diagnosis requested on #3271 at
`55f671b12ef08c93f11ae235980f69cf7059b7a8`, parent `810e05109`, 23 October
2026. **This is a source read and census of existing output, not a rerun or
an established historical cause.** The failed qualification remains failed.

[Official ARM64 run 39964412356](https://github.com/autonomous-ai/lampOS/actions/runs/39964412356)
selected 7,237 tests. The observed ordinals reconcile exactly: 5,894 PASS,
one LEAK and one FAIL, 5,896 completed; 1,341 selected tests were unrun after
fail-fast. The summary's 5,895 passed includes the leaky pass; 33 skips are
separate. The changed peer-loss case has no result row. Later docs/harness
and artifact stages did not qualify. The separate Mac light-call failure
also remains recorded by Tests; neither failure is waived here.

[receipt.json](receipt.json) binds source blobs, run/head and raw failed-step
log hash. [native-excerpts.txt](native-excerpts.txt) retains unmodified selected
start, failure and summary lines. The full log and extracted per-ordinal
census are retained locally; their hashes are recorded, not represented as
published payloads. The official failed log is available at the run above.

## What the source establishes

The failing test is
`second_provider::worker::tests::with_name_or_follow_up_two_peoples_held_lines_make_a_quiet_room_busy`.
At `worker.rs:8625–8632`, it locks the room, replaces it with `RoomTold::new`,
enables its counting switches, calls `at(now_us - 400_000_000)`, then expects
`at(now_us - 90_000_000)` to be Quiet. That assertion fails before constructing
`Heeding` and before any held line. The room mutex covers both calls.

The timestamp comes from the host `lamp_ipc::monotonic_us` clock. It is one
sample, reused for both subtractions. `RoomTold::at` (`room_told.rs:663`) reads
no clock internally: it keeps the first supplied timestamp and compares the
second with saturating subtraction. With valid increasing values, their
310-second difference exceeds the 300-second quiet threshold, so a fresh
room becomes Quiet.

There is a precise conditional defect in the unchecked fixture arithmetic:
if subtraction wraps and the sampled time is **at least 90 seconds but less
than 400 seconds**, the first value wraps to a huge unsigned value while the
second does not. Saturating elapsed time is then zero, leaving Unknown.
Below 90 seconds both subtractions wrap, so that same claim does not follow.
At 400 seconds or more, neither subtraction wraps. With overflow checks,
an underflow would panic at subtraction instead of reaching this assertion.
These are source/arithmetic consequences, not measurements of the failed host.
Job elapsed time is not host monotonic uptime.

The raw failing `now_us`, both derived timestamps, first `at()` result,
`on/in_view` iteration and effective overflow-check flags were not printed.
The native failure therefore supports a setup investigation but does not
establish that this run entered the wrap window. No uptime value, scheduling
cause, inherited-cause waiver or product-policy defect is inferred.

## Existing correction and bounded follow-up

PR #3190 at `f121d6391` already changes this exact fixture: a shared scoped
600-second scenario epoch, test-only producer/reader clock imports, and the
face publication on that same clock. The original Quiet and held-line
assertions remain. That PR retains its own execution HOLD and qualification;
its separate success cannot relabel #3271's failed run.

Keep that clock correction in its existing fixture workstream. If an
additional observed boundary is needed, retain the original computations,
two calls and Quiet assertion, and add failure diagnostics for the original
sample, derived arguments, first result and iteration. A diagnostic before
subtraction would also retain a checked-overflow failure without replacing
it. Do not replace Unknown with Quiet, add a settling wait, relax a bound or
rerun the full gate merely to obtain a green observation. Tests owns any
fixture implementation/execution; Reviewer-two owns disposition. No source
edits, Cargo commands, product changes or lamp action were performed here.
