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
