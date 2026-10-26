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
assertion relaxation or product durability change was used. The exact blocking
operation is not yet established; a separate bounded stage trace is running.
This is host fixture evidence, not lamp performance or acceptance.
