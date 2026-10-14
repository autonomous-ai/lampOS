# Voice fixture qualification v4: three passes, then a retained failure

Fourth's Intel development Mac: x86_64 macOS 26.6.2 (25G83), Core
i7-10700K, 64 GiB RAM, Rust/Cargo 1.96.0. This is not a lamp. The frozen
source is based on `78b984b69fb58b42448ecb16812bc4cab668f1e5`.
`source.json` records all 1,678 file hashes; `source-changes.patch` and the
two `source-*` files preserve the uncommitted task changes.

V4 makes the real-provider EarlyEnd cohort select explicit per-instance
test timing, preserves its original scheduled capture source, guards against
mixed clock domains, and checks background task cancellation/panics. Its
focused correction and the earlier exposed teardown panic are retained in
[the focused evidence](../voice-fixtures-early-end-v4-20261015/README.md).
All eleven original EarlyEnd tests and 86 assertions remain. Normal Voice
and unrelated tests retain their existing privacy bounds.

Every full serial Mac command was:

```sh
cargo test --locked --offline -p lamp-voice --no-fail-fast -- --test-threads=1
```

`CARGO_BUILD_JOBS=4`, `RUST_TEST_THREADS=1`, Fourth's existing Mac target
cache, and a 30-minute outer watchdog per command. Formatting and strict
workspace Clippy passed before load started. Source hashes were unchanged
after all four runs; raw output was not edited. No retry occurred within
this sequence.

| Full run | Exit | Passed / failed / ignored | Suites | Cargo wall seconds |
|---|---|---|---|---|
| 1 | 0 | 1082 / 0 / 6 | 21 | 435.603 |
| 2 | 0 | 1082 / 0 / 6 | 21 | 372.849 |
| 3 | 0 | 1082 / 0 / 6 | 21 | 373.223 |
| 4 | 101 | 1081 / 1 / 6 | 21 | 371.137 |

Run 4 failed
`pilot_faults::rapid_mute_and_reopen_never_overlaps_workers_or_carries_speech_across`
with `capture discontinuity`. Four listening-ready records precede the
assertion failure. This scenario retains real-time peers to exercise the
actual service privacy wait across four cycles. Source inspection finds fixed 10 ms block scheduling but acquisition
timestamps assigned at delivery. A delayed peer can therefore manufacture
a source discontinuity. The raw failure does not record actual block
timestamps or consumption age; this explanation is inferred from code,
not a measured scheduling pause in run 4.
The four named regressions in #739, every EarlyEnd scenario and all motor
scenarios passed all four runs. `qualification-audit.json` checks raw
hashes, named results and overlapping compiler phases. The sequence stopped
at the first failure and is not a ten-run success.

Eleven overlapping release ARM64 cross-build cycles passed in the existing
x86_64 Linux Podman VM (6 virtual CPUs, 8192 MiB RAM), as UID 1000 with
network disabled, all capabilities dropped, read-only source/registry/git,
and Fourth's own writable target volume. Each cycle cleaned only ARM64
workspace package artifacts, retained third-party dependency caches, then
built all 34 workspace packages' binaries/examples for
`aarch64-unknown-linux-gnu`. Commands and image identity are retained in
`arm64-builds.json`; the first raw build records compiler and environment.
The Mac loop started after actual ARM compilation began. The current ARM
build finished after the Mac failure. This is build load, not native ARM64
execution or a measurement on a lamp.

A scratch orchestrator outside the repository ran the commands, recorded
UTC and monotonic boundaries, checked hashes and sampled host load/free
space every five seconds. It stopped future runs at the first test/source/
build failure. Standard test-result lines were parsed into the retained
counts and failed names were checked against raw output. No orchestrator
script or binary is committed here. The six ignores are pre-existing
optional model, manual latency and long-duration provider/reference tests;
this task adds no ignore.

There are 311 host samples. One-minute load ranged from 3.813 to 8.711; minimum available filesystem space was 45,003,202,560 bytes. Sampling ends with the Mac sequence, before the last ARM build finishes.


Not shown: ten successful runs, immunity to arbitrary host scheduling
stalls, actual lamp privacy response or thermal margins, hardware motion,
or native ARM64 qualification of this task head. The checked-in lamp unit
requests RR priority 20; its installed state was not read from a lamp.
The separate Body clock-seam question remains unanswered, with no Body edits.
