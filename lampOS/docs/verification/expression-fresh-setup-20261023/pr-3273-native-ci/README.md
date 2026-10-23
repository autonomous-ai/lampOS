# Expression #3273 terminal CI

Draft/HOLD at `809d8493f`: [Linux Clippy 39969098189](https://github.com/autonomous-ai/lampOS/actions/runs/39969098189)
and required job 120466698642 succeeded. [ARM64 39969098194](https://github.com/autonomous-ai/lampOS/actions/runs/39969098194)
and required job 120466698823 failed. Watcher session 5834 exited 1 after both
workflows and direct required jobs were terminal. No run was dispatched again
or retried. The independent local Mac full-gate light-call failure remains unwaived.

ARM ran **5,896 of 7,237 selected cases**: **5,895 passed (one leaky), one failed,
33 separate skips, and 1,341 selected cases unrun**. All 5,896 unique case
identities and ordinals reconcile before the first Summary; later recaps are
excluded. Both changed Expression cases have literal PASS rows: session/input
loss 0.009 s and offline face 0.010 s. These single release observations are
not a speedup or device claim.

The failure is `second_provider::worker::tests::with_name_or_follow_up_two_peoples_held_lines_make_a_quiet_room_busy`
at worker.rs:8632: actual Unknown, expected Quiet. Its complete original
diagnostic is in [selected-raw-logs.json](selected-raw-logs.json). The leaky pass
is `same_voice::tests::each_line_the_check_says_is_said_to_its_journal`.
Doctests, own-harness tests and artifact export/upload were skipped after the
failed test step, not passed. No downloadable ELF/hash is asserted.

This was the parallel PR profile over 12 packages, partition hash:1/1, with
62 selected binary identities and 106 excluded binaries. The effective profile
default filter is all(); older workflow text about excluding realtime is stale.
The two custom-harness binaries were excluded from nextest, and their later
step was skipped. Skipped names are not individually logged; 33 is a summary
count, not a named skip census. [outcomes.json](outcomes.json) and
[case-outcomes.json](case-outcomes.json) preserve the actual census and scope.

Both CI logs checked `f6f4d09615dac3d818a0d75f84aad0072ef44711`, parents
`810e05109` and authored `809d8493f`; the checked tree equals the authored tree.
All 10,033 committed blobs/modes equal the source overlay used by the earlier
local Mac full gate, whose build identity remains explicitly pre-commit.
[source-binding.json](source-binding.json) binds Git objects, API projection,
local Mac source identity and CI configuration.

Peer #3271 checked a6a267bf2 with head 55f671b12 on the same base. The complete
worker.rs, room_told.rs, IPC clock and scenario_clock.rs are byte-identical
between base, peer and this PR. The exact 3,916-byte failed function has the
same SHA256. These facts do not capture the failed clock sample or prove cause.
Recommend keeping the clock correction in #3190's prerequisite workstream;
First/Reviewer own that disposition. No duplicate fix or stacked change was
made. The later train50 head 09f8e7412 is not qualified here; no rebase or retry
was performed. Reviewer-two's read 6367343654 found no P1/P2 in this two-file
source/control/local-receipt scope but retained all qualification holds.

[terminal-metadata.json](terminal-metadata.json) records actual required jobs,
all steps and workflow conclusions; conditional ancillary skips are separate.
Raw metadata is projected to explicit provenance fields without person
attribution. Selected log slices retain original bytes, offsets, lengths and
SHA256. [external-raw-files.json](external-raw-files.json) indexes complete
external logs; local paths are not remote availability promises. No local
ARM build, device access, workflow rerun or new test execution occurred while
collecting or sealing this receipt. [validation.json](validation.json) and
[manifest.json](manifest.json) bind the publication files.
