# Turn-end-wire: spawn return without a stopwatch

Part of #1339. This packet qualifies base `756be347e13d2e52e7268414295dda8fc554c267` plus the two-file
uncommitted fixture/documentation patch bound in [results.json](results.json).
Production source and APIs are unchanged. [inputs.sha256](inputs.sha256) binds
the fixture, endpoint documentation and unchanged client; the complete private
source inventory is represented only by its digest and entry count.

The missing-executable case retains its cannot-start, unavailable and zero-spawn
assertions without the old 20 ms stopwatch. A companion holds a real worker
before Ready and requires independent startup-entered and spawn-returned events
before release. It then requires Ready, confirmed worker teardown, a completion
witness and native child exit zero. The successful path directly reaps that
exited child; it does not signal the exited group. Failed observations release
only for bounded cleanup. An exited child with a worker-reaped witness can be
reaped; otherwise the owned group is killed before its leader is reaped. EPERM
is never broadly ignored. Cleanup failure retains scratch and fails the control.

## Fixed Mac gate

Formatting, strict all-target Turn-end-wire Clippy and compilation pass.
The complete crate reports **18 passed / 0 failed / 0 ignored**: five library
cases and thirteen process-harness cases (zero doctests). The focused **2/0**
observation is separate, supplies the control baseline, and was not rerun or
added to the full total. Literal commands, source/artifact/log hashes and
selected terminal lines are retained in the accompanying small files.

## Controlled observations

| Source | Selected result | Meaning |
| --- | --- | --- |
| Original HEAD fixture + 400 ms host pause | 0/1, exit 1 | Original 20 ms assertion fails |
| Corrected fixture + 400 ms pauses at both spawn-return boundaries | 2/0, exit 0 | Host pauses do not break the event contract |
| Actual client spawn changed to wait while Loading | 0/1, exit 1 | Parent's named 15 s return-before-release watchdog fires |
| Exact restored source, freshly compiled | 2/0, exit 0 | Both selected cases pass again |

Each variant is freshly compiled and copied before execution. The deliberate
failures reach their named assertions; compilation failures or the outer
120-second controller timeout do not qualify. After the blocking control's
parent failure releases startup for cleanup, the child also reports Ready
versus Loading; that diagnostic is retained, not counted as another test.
All source bytes/modes are restored and the full-gate executable is preserved.

## Earlier failed fixture, retained

The first attempt passed formatting, strict Clippy and compilation, then its
focused run ended **1 passed / 1 failed**, exit 1. Group cleanup returned EPERM
(`Operation not permitted`) after child exit was observed. Its full suite did
not run. That attempt's source, actual binaries and receipts remain distinct;
the underlying OS cause is not established. The revised successful teardown
requires native exit zero plus the confirmed teardown witness before direct
reaping. No failed result is overwritten or relabelled.

These are host fixture observations, not physical latency, device/provider
acceptance or native ARM64 qualification. Long watchdogs diagnose stuck tests;
they are not hard bounds on storage or OS teardown. Unexpected failed receipts
refuse automatic publication. Raw logs, full inventories, controllers and
executables remain private and require separately arranged access; their hashes
are not a public-download claim. Current-head native CI remains separate.
