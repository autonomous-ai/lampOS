# Voice fixture qualification v7: real endpoint-worker privacy failure

Fourth's Intel development Mac: x86_64 macOS 26.6.2 (25G83), Core
i7-10700K, 64 GiB RAM, Rust/Cargo 1.96.0. This is not a lamp. The frozen
source is based on `cf2f5b43c8113a704324c2743af3cd92bbc4fdda`.
`source.json` records all 2,367 file hashes; `source-changes.patch`
and the two `source-*` files preserve the uncommitted task changes.

V7 corrects the stalled-metadata fixture without changing the real writer,
SIGTERM or cleanup boundaries. It waits for the actual writer to enter its
first blocked event before starting children or scoped policy time. The
synthetic peers then use explicit stepped time. Host instants separately
measure signal send through verified child exit/reap, and logging finish
keeps its real deadline. The unacknowledged report must show zero written
events. Normal release obtains a later real writer-finish receipt, while a
release guard handles assertion unwind without replacing the original
failure. A typed-panic regression covers that failure path.

The [failed v6 sequence](https://github.com/autonomous-ai/lampOS/tree/157adef7a64635cba38428dd2d22f4f227e55b78/lampOS/docs/verification/voice-fixtures-loaded-v6-20261015)
retains the actual 50,902 us producer gap, missing SIGTERM injection and
secondary writer panic; coordinator consumption age was not measured.
Earlier v1-v5 sequences and the deliberate reminder-integration stop remain
on that same immutable evidence commit. No earlier pass counts toward v7.
Independent metadata review preserves all 35 existing pilot tests and their
assertions; 34 bodies are unchanged and one regression makes 36 tests.
The [focused correction and actual host timing](https://github.com/autonomous-ai/lampOS/tree/19cf238713ae9f9a63e150cc980fd872e5b0a73f/lampOS/docs/verification/voice-fixtures-metadata-v7-20261015)
are retained separately from this loaded sequence.
Normal reminder handling, production freshness limits and other task source
remain as reviewed. Native ARM64 and lamp qualification are separate.

Every full serial Mac command was:

```sh
cargo test --locked --offline -p lamp-voice --no-fail-fast -- --test-threads=1
```

`CARGO_BUILD_JOBS=4`, `RUST_TEST_THREADS=1`, Fourth's existing Mac target
cache, and a 30-minute outer watchdog per command. Formatting and strict
workspace Clippy passed before load started. Raw output was not edited.
No retry occurs inside the sequence; future runs stop at the first test,
source or build failure.

| Full run | Exit | Outer timeout | Passed / failed / ignored | Suites | Cargo wall seconds |
|---|---|---|---|---|---|
| 1 | 101 | no | 1170 / 1 / 8 | 21 | 453.303 |

The first full run failed in
`endpoint_session::hung_worker_never_delays_capture_endpoint_or_session_end`
with `physical privacy closed or lost freshness`, after 4.240981188 real
seconds of its requested six-second session. The capture producer retained
10,000 us source gaps; its maximum after-send delivery gap and source age
were 48,361 us and 39,164 us. No privacy producer or consumption timing was
recorded, so those capture figures do not establish the privacy timing
cause. `failure-analysis.json` separates that limit from the proposed fix.
All four named regressions, all pilot/EarlyEnd/motor cases and the other
98 affected-cohort outcomes matched expectations. The corrected metadata
scenario and its new unwind regression passed. Four actual ARM64 cross-build
cycles passed; no source changed. No earlier run counts toward a success.

This sequence does not meet the ten-run requirement. Complete raw output, failed counts and any requested-stop/build-error state are retained; no failed or partial sequence counts as qualification.

`qualification-audit.json` checks raw hashes, the four named regressions,
all 99 results in the endpoint/pilot/EarlyEnd/motor cohorts, and overlap
with actual compiler phases. Missing test counts or an outer timeout in any
test/build command disqualifies the sequence even if its exit status is zero.
One of those 99 is the existing model-dependent
ignore. `ignore-audit.json` records all eight full-suite ignores. Six
pre-existing ignores cover optional model, manual latency and long-duration
provider/reference cases; two newly merged reminder timing probes explicitly
run alone on an idle host. Their files match the upstream base. This task
adds no ignore or filter. Overlap is measured from the first `Compiling`
line through completed build command, not as a CPU-utilization measurement.

The scratch evidence scripts inherited reviewed missing-count/timeout handling
checked in v6 with four synthetic cases that replay only one completed run. `synthetic-auditor-checks.json`
labels those earlier checks explicitly; they test failure handling and never count
as additional real runs or lamp evidence.

The sequence recorded 4 ARM64 cross-build cycles in the
existing x86_64 Linux Podman VM (6 virtual CPUs, 8192 MiB RAM), as UID 1000
with network disabled, all capabilities dropped, read-only source/registry/git
and Fourth's own writable target volume. Each cycle cleans only ARM64
workspace package artifacts, retains third-party dependency caches, then
builds all 34 workspace packages' binaries/examples for
`aarch64-unknown-linux-gnu`. Commands, outcomes and image identity remain
in `arm64-builds.json`. The Mac sequence starts after actual compilation
begins. On stopping, the orchestrator allows an active build to finish and
retains its result; a failed build can have ended earlier. This is
cross-compilation load, not native ARM64 execution or lamp evidence.

The scratch orchestrator outside the repository recorded UTC boundaries and
monotonic elapsed durations, checked source hashes and sampled load/free space every five
seconds. There are 92 samples; one-minute load ranged from
4.181 to 9.181,
with minimum available space 40,765,296,640 bytes.
Sampling ends with the Mac sequence; any still-active build is then joined.
Standard libtest results were parsed into the retained count records.
No orchestrator script or binary is committed here.

The ordinary-library integration tests in `tests/privacy_state.rs` and `tests/core.rs` passed in the first full run; their separate target results are extracted in `ordinary-library-evidence.json`.
These targets compile the normal library, while the test clock and
per-instance override exist only in unit tests. They are not a measurement
of physical privacy latency.

Not shown by this folder: native ARM64 task-head qualification, immunity
to arbitrary host starvation, actual lamp privacy response or thermal
margins, acoustic stopping or hardware motion. `lamp-handoff.md` gives
the exact read-only checks for First's required compatibility minute and
the unresolved physical measurement boundary. The checked-in lamp unit
requests RR priority 20; its installed state was not read from a lamp.
The separate Body clock-seam question remains pending, with no Body edits.
