# Install fixture parallel timeout investigation

First requested this investigation on Queue #3504, comment 6412300200.
The unchanged 1aa409fcb install test executable, run on the development Mac
with `--test-threads=16 --nocapture`, returned 96 passed / 5 failed / 0 ignored.
The five successful-install head-touch preflight cases reached their existing
30-second deadline. The baseline overlapped another durable-filesystem test
suite; this is not an isolated load measurement.

A diagnostic-only fixture patch added fixed timestamps for ready, installation
return, assertions and cleanup. One full run again returned 96 / 5 / 0.
All five children reached ready and timed out before install returned. Their
finished markers were absent. The parent killed and reaped each child; the
outer driver was reaped and its process group was absent after completion.
No assertion or cleanup delay was observed. This second run did not overlap
the earlier durable suite, but ambient compiler load may differ.

The receipts retain actual driver hashes, log hashes, durations and unchanged
source checks. Original logs, executable copies, diagnostic patches and source
manifests remain in private builder scratch. No retry, deadline increase,
assertion relaxation or product durability change was used. The individual blocking syscall is not established. The separate stage trace
shows all five preflights completed in 154–396 ms; builds and links took
7.8–17.2 seconds, and unit installation was unfinished at the cutoff.
This is host fixture evidence, not lamp performance or acceptance.

The fix at 32cc833686 on aa7cf9029 keeps the four refusal watchdogs and
runs five safe compatibility cases through ordinary fresh-install execution,
with all original assertions. Formatting, strict Update Clippy, workspace
build and full parallel-16 install passed: 101/0/0, 383.49 seconds libtest,
384.16 seconds outer runner, within its 900-second bound. All five positive
cases produced libtest over-60-second notices; no exact individual duration
is claimed. The source/driver stayed fixed and owned processes were reaped.
The three original failed runs remain failed; no retry erased them.

## Delivered source qualification

Train 170 landed this fix as ba950f289. Its candidate, tested preview,
landing and clean local b63c163d9 all have the same complete tree. The
corrective local gate invalidated all 46 workspace packages plus patched
local tough/sonora in a permanently dedicated target. Compiler JSON records
all 48 local libraries freshly compiled. Full parallel-16 install passed
101/0/0 in 380.23 s libtest (380.91 s outer), with source, driver and cleanup
guards. The prior shared-target carry result is preserved but excluded from
source qualification.

Original train ARM64 40307005142 and Linux Clippy 40307005161 passed. ARM
ran 7,501 selected cases with 53 skips, including all 101 install tests and
all nine preflight cases once. Separate docs 4/0, process 13/0 and relay
passed. This is a test-fixture repair, not a lamp timing result.
