# Voice fixture qualification v2: two passes, then a retained failure

This is Fourth's Intel development Mac, not a lamp: x86_64 macOS 26.6.2
(25G83), Core i7-10700K, 64 GiB RAM, Rust/Cargo 1.96.0. The frozen source
is based on `f60d6d6f9c7895c84e127cb6c1172eef46148992`. `source.json`
records every source-file hash; `source-changes.patch` and the two
`source-*` files retain the uncommitted task changes. This snapshot is not
the eventual task head.

The command for each complete serial Mac run was:

```sh
cargo test --locked --offline -p lamp-voice --no-fail-fast -- --test-threads=1
```

`CARGO_BUILD_JOBS=4` and `RUST_TEST_THREADS=1`; the existing Fourth Mac
Cargo target cache was used. Each command had a 30-minute outer watchdog.
Formatting and strict workspace Clippy passed first; their exact commands,
raw output and wall times are retained. Source hashes were unchanged after
every run. Counts include every completed suite, not just the library.

| Full run | Exit | Passed / failed / ignored | Suites | Cargo wall seconds |
|---|---|---|---|---|
| 1 | 0 | 1073 / 0 / 6 | 21 | 437.603 |
| 2 | 0 | 1073 / 0 / 6 | 21 | 371.372 |
| 3 | 101 | 1072 / 1 / 6 | 21 | 370.901 |

Run 3 failed
`endpoint_session::hung_worker_never_delays_capture_endpoint_or_session_end`
with `capture discontinuity` at `endpoint_session.rs:711`. The originally
reported cases and all motor cases passed in all three runs. The sequence
stopped after this first failure; it is not a ten-run success.

The raw log does not contain per-frame acquisition/delivery timestamps.
Source inspection subsequently found that the synthetic capture peer
stamped overdue scheduled blocks with the sending thread's current clock,
so a host scheduling gap could become a false sample discontinuity. That
mechanism is an inference from source, not a measured delay for this failed
block. A later fixture correction must preserve immutable acquisition times
and the existing stale-input refusal.

Nine overlapping ARM64 release cross-build cycles passed. These ran in the
existing x86_64 Linux Podman VM as UID 1000, with 6 virtual CPUs and 8192 MiB
RAM, network disabled, capabilities dropped, read-only source/registry/git,
and Fourth's own writable target volume. The image identity and every full
Podman/Cargo command are in `arm64-builds.json`; the first raw build records
user, kernel, architecture, compiler and CPU count. Each cycle cleaned only
workspace package artifacts for `aarch64-unknown-linux-gnu`, kept cached
third-party dependencies, then built all workspace binaries and examples in
release mode. The first Mac run began only after the ARM compiler emitted
its first `Compiling` line. The in-progress build finished after run 3
failed. This is cross-compilation load, not native ARM64 or lamp execution.

A scratch Python orchestrator outside lampOS recorded UTC boundaries and
monotonic command wall durations, checked source hashes, ran the bounded
commands, and sampled host load/free bytes every five seconds. It stopped
future runs on any test, source-hash or build failure and retained unmodified
stdout/stderr. No measuring script or binary is committed here. The JSON
counts were parsed from standard Cargo test-result lines, and failure names
were checked in the raw output.

Recorded 237 host samples: 1-minute load ranged from 5.021 to 10.136; minimum available filesystem space was 47,126,618,112 bytes. Sampling stopped when the Mac sequence ended, before the last ARM build finished.


A stop-after-current-run request was separately written during review to
allow focused clock-error coverage and a wording correction. The run failed
before that request was consulted at the next-run boundary; the harness
therefore correctly ended with exit 1 and `requested_stop:false`.

Not shown: ten successful runs, immunity of every timed fixture to arbitrary
host stalls, the lamp's real 50 ms privacy latency, hardware motion or
acoustic performance, or native ARM64 qualification of this task source.
The motor clock-seam ownership question remains unanswered at this record.
