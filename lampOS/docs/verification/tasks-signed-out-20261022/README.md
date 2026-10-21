# Tasks signed-out review correction, 2026-10-22

PR #2637 restores the original 600 ms negative observation in
`targets_from_machines_are_written_and_used_while_signed_in`. Counting
sign-in calls did not establish that the machines timer was due or that
its result had been used. The fixture now checks the unchanged read count
on each diagnostic during that interval, sends a targets request, and waits
for a later signed-out idle owner report including that request. The owner
publishes the report after scheduling jobs. The final read-count and target
assertions remain. Production code is unchanged.

The other fixture improvements remain: five setups configure an unstarted
rig before its only start, and setup tests use a 20 ms look interval. The
special restart cases keep their existing first start.

## Measured correction and controls

Base: published PR `2e84acbe7755267a60b7abfb50ae3ee7adc915a8`.
Three compiled exact-test processes before and after, including launch,
execution, cleanup and output capture; compilation excluded:

| Counter-based PR before (s) | Corrected observation after (s) |
|---|---|
| 1.686588 / 1.002755 / 1.023472 | 2.152739 / 1.751555 / 1.559590 |

All six passed. This correction restores coverage and takes longer. Its
repeated-use target was under 2 s; both repeats met it. The first corrected
run missed that target and is retained. These are fixture process times,
not device interaction latency.

Both replayable controls compiled and failed the original `asked while
signed out` read-count assertion: [remove the sign-in guard](missing-sign-in-guard.patch),
and [also delay the wrong machines worker by 800 ms](missing-sign-in-guard-delayed-worker.patch).
Each selected exactly one test, exited 101 with one failure, and restored
exact final source bytes. The second control ensures a busy wrong worker
cannot hide behind the completed-pass condition.

## Gates and the retained failure

Exact argv, cwd, exits, timeouts and output digests are in [commands.json](commands.json).
From `lampOS/`, formatting and strict all-target Clippy for Tasks plus all
12 reverse dependents passed. Tasks: **209 passed, 2 existing ignored**;
serial Voice: **3,238 passed, 14 existing ignored**. The complete final
11-package dependent run passed **2,806 tests, 12 existing ignored**.
Workspace build and diff checks passed. There were zero failures in these
final gates; the combined passing suite count is **6,253**.

The first dependent attempt stopped after 1,673 passes at
`daily::tests::forward_claim_and_date_correction_preserve_exact_days_across_restart`,
Skills `daily.rs:854`, with `WouldBlock`, "daily journal directory is already
owned". Its full output is retained in [observations.txt](observations.txt).
With the entire crates tree restored to the actual pre-PR base
`7a6c5d678047e016a8b79d5e4bb88391c7d2b24f`, three whole parallel Skills
library runs passed all 119 tests (1.441766 / 1.466027 / 1.391230 s).
The failure was not reproduced on that base; no inherited cause is claimed.
The Tasks integration test is not linked into the Skills library. Final
source was restored before rerunning the complete failed dependent stage
once. Already-passing gates were not repeated.

The earlier #2637 row-76 shutdown observations remain historical evidence
in its prior PR discussion; that signature did not occur in these gates.

## Evidence boundaries

[results.json](results.json) records timings, counts, environment, relevant
source hashes and exact restoration. [observations.txt](observations.txt)
retains full raw failure/control/timing/comparison streams between labelled
boundaries. Bulk successful build/test output and metadata remain in their
external directories, with SHA-256 and byte counts in the command record;
this commit does not claim to include them. The source-mtime freshness
record's digest is included; source bytes stayed unchanged. No runner,
source snapshots or binary artifacts are committed.

Native Linux ARM64 and Linux all-target Clippy on the updated PR head are
still required. No ARM64 build ran on the Mac and no device was contacted.
