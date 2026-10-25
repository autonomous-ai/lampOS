# V2 input follow-up: retained ARM64 failure, attempt 1

Audit prepared 26 October 2026, 02:18 +07. [PR #3503](https://github.com/autonomous-ai/lampOS/pull/3503),
head `d8cf817d6f6a6beb53fe10647683d6d9e94dd7a7`, failed
[ARM64 run 40224140937, attempt 1](https://github.com/autonomous-ai/lampOS/actions/runs/40224140937/attempts/1).
No test was rerun for this audit. Any retry remains a separate observation.

## Execution census

The single Starting-to-Summary window contains **10,003 PASS and one FAIL**:
10,004 unique binary/test identities and every ordinal 1 through 10,004 exactly
once, with denominator 11,308. Nextest skipped 106 tests and two binaries;
**1,304 selected tests were not run** after the failure. Those unrun cases are
kept separate from skips. Three already-running cases passed after cancellation.
The 24 report echoes after Summary (23 PASS and the same FAIL) are excluded.
Doc tests and the own-harness step were skipped after the failed nextest step.

The failed case is
`lamp-voice session_reader::tests::linux::failed_private_rollback_disables_only_metadata`,
ordinal 10,001. Its assertion at
[tests.rs:531](https://github.com/autonomous-ai/lampOS/blob/d8cf817d6f6a6beb53fe10647683d6d9e94dd7a7/lampOS/crates/voice/src/session_reader/tests.rs#L531)
is `report.initialization_failed && report.degraded && report.dropped_events == 1`.
The log prints no individual report-field values. It does print:
`lamp-voice serve: session reader unavailable; metadata remains private`.

## What the source establishes

The test injects grant failure at step 12 and skips rollback chmod at step 0.
The intended path opens the outer session gate at step 11; failure at 12 then
cannot restore its private mode, so initialization must fail. The test does
not record that it reached the intended injection boundary.

Fallible parent/path/ACL checks and filesystem operations run before step 12.
An earlier refusal while the session remains private can complete a verified
private rollback and return `Ok(false)`. With this test's `Ok(reader)` input,
that is the branch identified by the observed warning. It retains a private,
degraded writer, whose report hardcodes `initialization_failed=false`
(`service_trace.rs:275,301–315`). A finish-counter race cannot explain away
that field. An actual initialization error instead leaves no writer; flush
counts the one dropped event synchronously and finish sets both failure flags
(`session_state.rs:245–271`).

This identifies a missing fixture precondition. The exact earlier refusal is
**unknown**: `grant_with` discards the original apply error, and the test supplies
neither stage-hit evidence nor separate report diagnostics. A shared ancestor's
ctime changing during `no_acl` is one possible early refusal mechanism; no such
change was observed in this run. Production privacy checks should remain intact.
No caller or privacy regression is demonstrated by this failure.

The [prior source report on #3394](https://github.com/autonomous-ai/lampOS/pull/3394#issuecomment-6386376127) documents the
same missing injection-stage witness and preserves the same causal limitation.
It recommends stage evidence, separate assertions and a deterministic test-owned
setup. This packet does not claim a new reproduction of a specific cause.

## Exact source and separate green evidence

The failed job checked out `fdda2e3eecfa647460696759afa505c29e0c686c`, merging the
head into `a6ccb1aa3d8d27be64b9030dc814fc2f44e43fdb`. Public commit metadata confirms
complete tree `dc94a4d8faf269ffcbca2e2a7ec8431256965da2` for that checkout, matching the
head. The reader, reader tests, SessionState, ServiceTrace, ARM64 workflow and
nextest configuration are byte-identical across the head, its base and repaired
original head `4ace70555becb72bb6ddfa9624474e921c0f2a7c`; their Git blobs are in the census.

[Earlier ARM64 run 40222685198](https://github.com/autonomous-ai/lampOS/actions/runs/40222685198)
on that repaired original head passed 11,308 tests with zero LEAK rows and
106 skipped, including this exact case. Its complete tree is equal. That is
separate successful evidence; it does not establish the failed attempt's cause
or replace its result.

[census.json](census.json) retains identities, hashes, partial-run counts and
source conclusions. [selected-results.txt](selected-results.txt) preserves the
literal checkout, failure, assertion, cancellation, three draining PASS rows,
summary and unrun warning with GitHub timestamps. Repeated job/step prefixes
are removed; the panic row containing a process identifier and full raw logs
are omitted. [SHA256SUMS](SHA256SUMS) binds the three public files.
