# Small fixture review evidence

This is the review correction for #2653 / #1806. The final change retains
three fixture speedups: Body, Controls and Name. Expression's natural
`--seconds 1` successful exit and Mood's original 600 ms finite run are
restored byte-for-byte to both the old and new parents.

Body retains a real 250 ms blocked joints write with the hand already 900 ms
old, crossing the unchanged 1 s freshness threshold only when write time is
included. Controls waits for its real publisher lease to expire. Name renews
heard every 100 ms until rising expires; its first five-renewal phase stays
unchanged. Both expiry observations have 5 s failure bounds. The PR changes
fixture code only.

The tested current candidate is `3e27c60182295af93cc085d241d2ec5b895d58cf` on
`1f92c96edfac69dd34ed8211b1518ca244a6dd69`, with the two review restorations
uncommitted during verification. Its [base-relative patch](candidate.patch)
is only Body 9+/7-, Controls 7+/3-, Name 5+/2-. It uses zero context; the
validation applies `--unidiff-zero`. The original PR rebased with
[range-diff `=`](range-diff.txt). The [source proof](source-proof.json) binds
the actual tested file contents rather than treating uncorrected HEAD alone
as the candidate.

## Current candidate gates

All five gates passed on native Mac, using the isolated Small target, jobs 4,
dev/test debug 0, incremental 0 and offline Cargo. Commands, exact statuses,
counts and timing boundaries are in [results.json](results.json); complete
combined output is in [final-gates.txt](raw/final-gates.txt).

| Gate | Exit | Wall seconds | Result |
| --- | ---: | ---: | --- |
| `cargo fmt --all --check` | 0 | 11.372513 | Passed |
| Strict all-target Clippy, five packages | 0 | 60.231846 | Passed |
| Full Body/Controls/Name/Expression/Mood tests | 0 | 123.112944 | 1,522 passed, 5 ignored, 0 failed, 0 filtered |
| `cargo build -q --workspace` | 0 | 152.622650 | Passed |
| `git diff --check` | 0 | 0.633053 | Passed |

Before Cargo, 1,328 tracked crate/build-input mtimes were refreshed in this
worktree. The aggregate content hash of all 6,146 tracked lampOS files was
identical before/after and after the gates. No main shared target was used.
These gate walls include any compilation; they are not fixture latency
measurements. Full exact-head Linux ARM64 workspace CI remains required
following publication. Mac runs the Mood UTC branch; the original Linux
UTC/system branches remain intact.

## Earlier measurements, retained without retries

These observations belong to old head `6a418d769efeac1751f6402ae4d9feb179a72133`
on parent `7a6c5d678047e016a8b79d5e4bb88391c7d2b24f`. “Before” means the original
PR's kill-at-started Expression fixture and 100 ms Mood fixture. “Restored”
means their original natural exit / 600 ms behavior on that old base. All
samples measure compiled exact-test process startup through exit, excluding
compilation; the first sample is retained. Raw output is in
[old-timings.txt](raw/old-timings.txt).

| Old-base fixture | Sample 1 | Sample 2 | Sample 3 |
| --- | --- | --- | --- |
| Expression before | 10.309955 s, pass | 9.562883 s, pass | 8.903723 s, pass |
| Expression restored | 11.834541 s, pass | 9.824626 s, pass | 11.265218 s, pass |
| Mood before | 6.899930 s, **exit 101** | 1.530577 s, pass | 0.250534 s, pass |
| Mood restored | 8.892313 s, **exit 101** | 1.445648 s, pass | 1.457271 s, **exit 101** |

All three Mood failures say `UTC: mood: Mood bus I/O did not stop; exit this
process before restarting`. The original-PR failure is at `program.rs:220:9`;
both restored-fixture failures are at the original successful-exit assertion,
`program.rs:219:9`. They remain failures in the record.

The real pre-PR parent was then selected by restoring the other three fixture
files, proving all 5,977 tracked lampOS contents matched that parent. Three
whole Mood suite observations passed: each had 1 library pass, 103 integration
passes and 2 ignored tests, zero binary tests and zero doc-tests. Their total
compiled-process walls were 5.709804, 3.698854 and 3.030836 s. Commands,
counts and the separate zero-doctest result remain in [results.json](results.json).
Their full success logs remain only at the external paths and hashes in
[provenance.json](provenance.json).

The matching exact-test comparison used the same command, cwd and recorded
environment as the failed final samples, including `--exact --nocapture`.
Exactly three original-parent observations all reproduced the identical
`program.rs:219:9` shutdown failure: **exit 101** at 6.898992, 6.641362 and
6.120626 s wall (6.70, 6.40 and 5.83 s libtest). See
[exact-parent output](raw/old-parent-exact.txt). Production and integration
binaries were byte-identical before/final, and their hashes were checked
before each sample. Final source was restored exactly afterward.

This establishes that the same failure occurs without this PR. Its I/O or
scheduling cause is unassigned. Concurrent Skills work is context only, not a
host-load explanation. Whole-suite passes and exact-test failures are both
retained; no sample was repeated until green.

## Expression failure controls on the old base

Two [original mutation patches](controls/serve-fails-after-started.patch)
([second patch](controls/serve-fails-after-two-steps.patch)) make serve fail
immediately after its started line or after two service steps. For each,
the original PR fixture wrongly passed; the restored natural-exit fixture
failed exactly one selected test at its original `status.success` assertion
(`alive_replay.rs:390:5`) with the intended error.

| Mutation | Original fixture | Restored fixture |
| --- | --- | --- |
| Failure after started | Exit 0, 1.355692 s | Intended exit 101, 1.562465 s |
| Failure after two steps | Exit 0, 1.127392 s | Intended exit 101, 0.894319 s |

All four control runs freshly compiled their production and fixture artifacts,
then restored exact source bytes. [Control test output](raw/old-controls.txt),
compiler commands/exits, artifact hashes and restoration checks are preserved.
Full Cargo compiler JSON remains only at the original external paths and
hashes in [provenance.json](provenance.json). These are old-base controls,
not tests rerun on the rebased candidate.

Upstream Name gained `--step-ms` grid support (69+/7- in `main.rs`). Expression
also gained rest-request fallback behavior; Mood gained optional tone input;
Msgs changed. The new candidate therefore received fresh final gates. Neither
these old measurements nor the old controls are presented as new-base results.
The three retained speedups were not remeasured after the rebase, and this
bundle makes no new timing claim for them or global #1806 completion claim.

## Evidence integrity

[Provenance](provenance.json) indexes each included raw stream by byte offset, original
length and SHA-256. Begin/end markers wrap the original bytes; closing markers
avoid blank file endings without changing the captured stream. Original run
directories and failed attempts remain intact. Bulk historical Cargo metadata
and selected measurement compiler logs are referenced there by command,
status, path and hash rather than copied. Whole-parent success logs, the
zero-doctest success log and full old-control compiler JSON are also omitted.
Their commands, exits, summary counts and artifact hashes remain in the bundle;
full-output verification needs the original local evidence directory. Those
external references are not portable with a repository checkout. Every actual
failure output, all 12 old timing outputs, all four control test outputs and
all current gate output are included. Python runners, source snapshots,
binaries and full per-file workspace hash maps are excluded.

[Validation](validation.json) records JSON/link checks, raw-stream equality,
patch checks, manual whitespace/EOF checks and unchanged tracked source.
No commit or push was performed by the packaging step.
