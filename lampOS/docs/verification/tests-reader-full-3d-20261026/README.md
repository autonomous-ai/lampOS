# Failed full Linux reader-fixture gate on 3d

**FAILED: 7,454 passed / 3 failed / 53 ignored**, zero measured or filtered,
across 79 completed summaries. The actual twelve-package serial command
exited 101 after 1,088.342 s. This packet does not clear that failure.

Execution source was base `3d9722f38817a4dd1601fac051d36ac9f2292969` plus
only the three dirty Reader fixture/documentation files in
`source-binding.json`. All 10,564 tracked source entries and index bindings
remain equal before/after and equal the earlier focused-module source.
The [unapplied proposal](https://github.com/autonomous-ai/lampOS/tree/100ff0c98/lampOS/docs/verification/session-reader-boundary-source-20261026)
and [current file bindings](https://github.com/autonomous-ai/lampOS/tree/29677b0ea/lampOS/docs/verification/tests-reader-linux-boundary-20261026)
remain independently published. The candidate source was not committed at
this execution; do not describe this as clean-base or later-head execution.

## Failures

All three are in `lamp-voice-eval`'s integration target `it`:

- `relay_hardening::a_closed_runner_transport_stops_the_runtime_and_its_workers`
- `relay_hardening::an_early_relay_error_after_the_runtime_exited_still_stops_its_workers`
- `relay_hardening::workers_left_after_a_normal_exit_are_stopped_and_reported`

Their earlier relay assertions pass; each fails its final worker PID-existence
check. The failing executable SHA-256 is
`4750b34999755b985db25b1dbcaf0200781b35a719141c967b22b4b3935a344b`.
The full raw panic messages and failures are retained. The relay sources are
unchanged at base 3d. Reader's additions are `cfg(test)` within lamp-voice,
whereas this integration executable links its normal library and launches
shell fixtures. There is no direct Reader-test call path in these cases.

Cause remains **UNPROVEN**. The container has no init reaper and its startup
command execs `tail -f /dev/null`; orphaned zombie workers are a hypothesis,
not a captured state. An exact unchanged-3d module control is prepared
separately but no control result is included here. Do not substitute a later
standalone result for this failed full gate.

## Count and artifact audit

The auditor mapped the 67 sequential Cargo `Running` lines and 12 `Doc-tests`
lines in stderr to the 79 ordered stdout summaries. Every summary's literal
outcomes reconcile. There are **7,510 unique target-qualified terminal cases**:
7,508 have one-line terminals and two print a readiness JSON line between
their name and a separate `ok`. `audit.json` records those exact split spans;
`case-outcomes.tsv` binds every name/outcome to raw stdout byte offsets.
`target-summaries.json` maps each executed target to its actual Cargo artifact
and preserved executable hash. All twelve doctest summaries have zero tests.

The preserved collection contains **99 compiled executables, 1,389,546,968
bytes**: the 67 executed targets plus other compiled programs/examples used
or made by Cargo. Ninety-nine is not an executed-test-target count. All 99
host copies were independently rehashed against the retained manifest, with
matching guest before/after hashes. No executable bytes are committed.
`retained-binary-manifest.json` names paths inside the private evidence store;
those paths are not payload files in this publication.

The raw streams, command receipts, Cargo artifact records and projections
are copied without alteration. Raw whitespace is intentional. Projection
code ran only over existing receipts; it performed no tests or compilers.
The private controller ran one compiler and serial tests, kept source fixed,
retained outcomes before assertions, enforced a disk reserve and deadlines,
and stopped only its owned container. During the runtime phase, the release-critical
carrier also ran its one-job native Mac compiler gate on this host. The host
was not isolated; no attribution to that overlap is established. Scratch
orchestration stays outside published source.

## Scope and limits

This was an unprivileged Linux AMD64 container, default UID 1000, no added
capabilities, no network, read-only source/caches, and the same pinned image
as the focused module. Debug information was disabled; optimization zero,
debug assertions and overflow checks remained enabled. It is not a native
ARM64 or physical-lamp result and supplies no benchmark comparison.

The earlier focused reader module **14 passed / 0 failed / 1 unchanged
privileged test ignored** is a separate execution and remains linked above.
The new full run includes that module in its normal full context. Privileged
cross-user DAC stays unverified. A prior compile attempt stopped at the disk
reserve with exit 137 before any full-suite test ran; its receipt is included
separately. After cache space was reclaimed, compilation resumed, then the
one full runtime observation reported here failed as recorded.
