# Task 31 follow-up ordering and standalone checks

Ready for lamp-voice: no

These are Third's development-host checks, on the Intel i7-10700K x86_64
Mac, macOS 26.6.2 / Darwin 25.6.0, Rust/Cargo 1.96.0. No lamp, provider,
physical audio, camera or motor was used. Test/build wall times do not
measure a lamp's interaction latency.

The promised camera look and unanswered-name SecondLook now take their
owed turns before a due reminder. Two new sequence regressions cover final
speaker retirement followed by a delayed provider terminal, and the existing
SecondLook window. Deferral does not consume or acknowledge the reminder.
The incoming task-news and CHECK tool changes were also reconciled.

On base `bc3fccb546a22baaef180fcd57b3291a44e0e4c1`, the
[676-path source map](focused/source-bc3.json) stayed unchanged during
[focused validation](focused/focused-bc3.json): format and strict all-target
Clippy for Actions, Schedule, Voice and Voice-eval passed; Voice `reminder`
passed 70 tests with two ignored timing probes, `second_look::` passed eight,
and `tools::` passed 18, with no failures. The
[source review](focused/merge-review-bc3.md) is narrower than a complete gate.

The [isolated baseline and implementation evidence](bc3-standalone/README.md)
retains all original outputs and command/source identities. The permitted
source copy passed the workspace/helper builds, 282 test executions with
zero failures and two ignored probes, and 72 default-off cases with 144
exact stdout comparisons. Its first restricted run remains failed: 16
Actions tests passed and 13 failed, nine reporting socket PermissionDenied
and four reporting readiness failures whose causes were not established.
Later suites and comparisons were not reached in that first run. The
permitted repeat changed execution permission, not source or assertions.

The current [1a source map](source-1a.json), based on
`1a38f2f3e6b8afd1e7896761a0da6149a2400f67`, differs from bc3 only in
upstream `crates/tasks/tests/harness.rs`. The
[rebase](rebase-1a.json) preserved all Task 31 bytes. Its complete affected-
package [gate failed](gate-1a/lamp-third-schedule-voice-1a-gate-results.json):
1,116 passed, five failed and eight ignored across 16 completed harnesses,
exit 101 after 683.341811104 seconds. Voice lib reported 923 passed / five
failed / eight ignored in 491.89 seconds. Formatting and strict all-target
Clippy passed; the workspace-build stage was not reached. All 676 recorded
source hashes stayed unchanged. The focused and standalone checks do not
replace this result or the required exact-head native ARM64 run.

All five [raw failures](gate-1a/lamp-third-schedule-voice-1a-gate-tests.txt)
are `motor/tests.rs:441`, `owned child timed out: None`:

- crash/stalled-control, caller 645, with crash versus hang unknown;
- explicit-current-segment, initial Ready wait at 905;
- failed-reap, initial Ready wait at 1001;
- frozen-config/executable, initial Ready wait at 782;
- idle-ready, initial Ready wait at 503.

These motor sources remain unchanged by Task 31. No timeout or assertion was
weakened, and no cause or passing complete gate is claimed.

## Correction and findings outside Task 31

The [ordering audit](motor-fullsuite-order-audit.md) locates two earlier
dcc motor failures at initial Ready waits. The crash/hang failure remains
mode-ambiguous. The two-second test helper checks its deadline before its
next poll, while production startup allows three seconds. These are source
facts, not a diagnosed cause or permission to widen a test deadline.

That audit also corrects the [retained diagnostic report](../schedule-voice-dcc-host-failure-20261015/clean7f-diagnostics/report.md)
at lines 99–100: four markerless fixture records did **not** all imply
no child was spawned. Three first started an original normal child, then
shut it down before checking a separate denied candidate. Their child-entry
phase was not captured. The fourth test explicitly never starts its child.
Original report bytes, counts and measured intervals remain unchanged.

The [incoming 8c review](incoming-8c-review.md) found a separate task-news
presence defect: the early-end path records a qualifying heard turn without
updating the new `said_at_us`, and confirmation skips the normal assignment.
With early-end enabled and no fresh face, news can stay silent despite a
recent conversation. That upstream source was not in the failed 1a gate;
no repair outside Task 31 was made. The review's suggested tests were not run.

First's motor-scope and Unknown-room questions remain open. Until the room
policy is settled, reminders require explicit Quiet; this can delay a
cold-start one-minute timer. The main task document gives seven lamp cases,
including due speech during an owed look, interruption, privacy, cancellation
and restart. None was performed on a lamp here.

## Touch handoff completed by First

At 18:17 UTC, [PR #728](https://github.com/autonomous-ai/lampOS/pull/728)
was merged. First's [retained report](review/lamp-third-pr-728-20261014T181737Z.json)
says it installed run 38599296426's `lamp-controls` at 01:16:03 local on
lamp-4ace with the existing controls/pins files and `--enable`, without
`--touch`: active, zero restarts, health `ok`, and button released/head not
touched. That is First's reported minute, not a measurement by Third.
The installed Controls artifact SHA-256 is
`8c7b8a1a6c3437853d616ae01244042057800a5a6bbc540006d78ce730b56ceb`.
First defers enabled touch, per-pad thresholds/baseline, E4/E8 checks and
the default flip until a person is present during office hours. The
[Queue snapshot](review/lamp-third-issue-705-20261014T181737Z.json) records
that disposition; #699 remains after the MVP and unclaimed.

[Retention](retention.json) maps the copied originals to bytes and hashes.
The nested bc3 bundle is a byte-identical mirror; its reference to historical
evidence in its original parent means the task branch's full verification
folder. The earlier published failure records are available separately in
[the dcc checkpoint](../schedule-voice-dcc-host-failure-20261015/README.md).
