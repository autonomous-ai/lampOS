# Requested-audio asset scan, 21 October 2026

Status: before/after measurements and focused regressions passed; complete
consumer gate in progress.

The host debug fixture has sixteen independent 60-second, 24 kHz mono PCM16
recordings, each 1,440,000 frames. All three files per skill total 46,085,516
bytes. The first scan follows loading; two repeats use fresh Authority snapshots.
Only `Scan::unchanged` is timed. Fixture creation, loading, cloning, copyback and
logging are excluded. No microphone, speaker, provider or lamp is used.

[Before record](before.json): the rebased pre-fix implementation reads all three
files per skill on every successful scan. The test passed **1 / 0 failed /
0 ignored / 0 filtered**. The helper changes only diagnostic byte accounting
for the old API and removes the new zero-read assertion. All baseline source
and raw logs are retained privately; the complete final record follows the gate.

The revised implementation must prove zero payload reads on unchanged files,
recheck bytes after metadata changes, reject restored-mtime edits and never
restore revoked inventory from a stale scan. Host timing does not establish
ARM64 scan latency, acoustic onset or the audible behavior of Music.

The [current frozen run](scan-benchmark.json) measured **1.481916 ms** on the
first scan after loading and **0.749791 / 0.762500 ms** on repeats, reading
**zero payload bytes** each time. The exact focused result was **1 passed /
0 failed / 0 ignored / 29 filtered out**; those filters select this benchmark
only. Its full 30-test integration file also passed. Complete consumer counts
and native qualification remain pending. The before/after implementations use
the same fixture and measured boundary; this is three samples on one host,
with no cold-cache or lamp-latency claim.
