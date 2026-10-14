# Voice fixture qualification v3: one pass, then a retained failure

Fourth's Intel development Mac: x86_64 macOS 26.6.2 (25G83), Core
i7-10700K, 64 GiB RAM, Rust/Cargo 1.96.0. This is not a lamp. The frozen
source is based on `1337e7e1d8b88e102ec9fd12f544841569856f75`.
`source.json` records all 1,663 file hashes; `source-changes.patch` and the
two `source-*` files preserve the uncommitted task changes. Every task file
was byte-identical before and after rebase from the preceding base.

V3 adds the immutable scheduled capture timeline and two controlled-delay
regressions to the endpoint fixture, two clock-error recovery regressions,
and corrected cleanup-clock wording. All original endpoint scenario test
bodies were unchanged. Their focused gate passed 15 / 0 / 1 model-required
ignore; seven clock tests passed. Those focused passes do not replace this
full qualification failure.

Every full serial Mac command was:

```sh
cargo test --locked --offline -p lamp-voice --no-fail-fast -- --test-threads=1
```

`CARGO_BUILD_JOBS=4`, `RUST_TEST_THREADS=1`, Fourth's existing Mac target
cache, and a 30-minute outer watchdog per command. Formatting and strict
workspace Clippy passed before the load started. Source hashes were
unchanged after both runs; raw output was not edited.

| Full run | Exit | Passed / failed / ignored | Suites | Cargo wall seconds |
|---|---|---|---|---|
| 1 | 0 | 1077 / 0 / 6 | 21 | 433.462 |
| 2 | 101 | 1076 / 1 / 6 | 21 | 369.994 |

Run 2 failed
`early_end::an_answer_with_nothing_said_before_the_lamp_is_sure_is_never_played`
with `physical privacy closed or lost freshness`. That real-provider case
still selected the ordinary 50 ms test gate. The explicitly tolerant
lone-name EarlyEnd case, all endpoint tests (including the previously failed
hung-worker case), and all motor tests passed in both full runs. The error
output also preserves the shared 300 ms shutdown's forced-termination
records; it is not evidence of cooperative child cleanup. The sequence
stopped on this failure and is not a ten-run success. No exact consumption
age is inferred from this error alone.

Six overlapping release ARM64 cross-build cycles passed in the existing
x86_64 Linux Podman VM (6 virtual CPUs, 8192 MiB RAM), as UID 1000 with
network disabled, all capabilities dropped, read-only source/registry/git,
and Fourth's own writable target volume. Each cycle cleaned only ARM64
workspace package artifacts, retained third-party dependency caches, then
built all workspace binaries and examples for
`aarch64-unknown-linux-gnu`. Commands and image identity are retained in
`arm64-builds.json`; the first raw build records compiler and environment.
The Mac loop started only after actual ARM compilation began. The current
ARM build finished after the Mac failure. This is build load, not native
ARM64 execution or a measurement on a lamp.

A scratch Python orchestrator outside the repository ran the commands,
recorded UTC boundaries and monotonic command durations, checked source
hashes, and sampled host load/free space every five seconds. It stopped
future runs at the first test/source/build failure. Standard test-result
lines were parsed into the retained counts; all failed names were checked
against raw output. No script or binary is committed in this folder.

There are 162 host samples. The 1-minute load ranged from 5.724 to 9.908; minimum available filesystem space was 45,796,216,832 bytes. Sampling ends with the Mac sequence, before the last ARM build finishes.


Not shown: ten successful runs, immunity to arbitrary host scheduling
stalls, actual lamp privacy response or thermal margins, hardware motion,
or native ARM64 qualification of this task head. The checked-in lamp unit
requests round-robin scheduling at priority 20; its installed state and
scheduler behavior were not read from a lamp. Neither that setting nor a
Mac freshness failure settles the lamp's real 50 ms bound. The separate
Body clock-seam question still awaits an ownership answer.
