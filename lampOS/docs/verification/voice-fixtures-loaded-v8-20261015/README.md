# Voice fixture qualification v8: stopped at First's priority request after 9 full runs

Fourth's Intel development Mac: x86_64 macOS 26.6.2 (25G83), Core
i7-10700K, 64 GiB RAM, Rust/Cargo 1.96.0. This is not a lamp. The frozen
source is based on `cf2f5b43c8113a704324c2743af3cd92bbc4fdda`.
The exact task source was subsequently preserved as commit
`ddb13741b1971cf386087f0f980aef39f8e50727` on `fourth/voice-fixtures`,
after First directed a pause behind the MVP and reassigned Fourth to #749.
That parked branch has no ready PR. A push-triggered ARM64 run is a separate
result and does not complete this task's ten-run requirement.
`source.json` records all 2,367 file hashes; `source-changes.patch`
and the two `source-*` files preserve the uncommitted task changes.

V8 explicitly selects the existing bounded test privacy policy for four
finite real-worker endpoint ordering cases and the already-ignored provenance
case. Six stepped scenarios and three endpoint mute/cleanup scenarios retain
50 ms; normal builds also retain 50 ms. The selected fixture gate is capped
at 100 ms and has a real 12-second conversation watchdog. It excludes setup
and cleanup. Worker clocks, source timestamps, the unchanged 100 ms input/IPC
ceiling, 60 ms reopening and every original scenario assertion remain.
Clock guards reject logical time before real child construction. No Ready
barrier hides intentional missing/loading-worker behavior. Producer timing
counters remain bounded and do not measure coordinator consumption age.

The [failed v7 sequence](https://github.com/autonomous-ai/lampOS/tree/12fdc1c5f9e00959bf1dd8fee781092c5d1bf0f8/lampOS/docs/verification/voice-fixtures-loaded-v7-20261015)
retains the real hung-worker fixture's privacy closure. Its capture source
gap stayed 10 ms, but privacy producer timing and consumption age were not
recorded, so those capture figures cannot establish the privacy cause.
Earlier failures and the deliberate v5 integration stop remain in that
immutable log history. No earlier pass counts toward v8.
The [focused endpoint correction](https://github.com/autonomous-ai/lampOS/tree/747ef2b96482340b586ab9924beca08fca4c3b96/lampOS/docs/verification/voice-fixtures-endpoint-v8-20261015)
passed 17 tests with one existing ignore, separately from this loaded sequence.
All 16 original tests and 103 assertions remain; two policy/guard regressions
make 18 tests and 111 assertions. Independent review found no actionable issues.
The prior metadata correction and its real host-clock cleanup evidence remain
[separately retained](https://github.com/autonomous-ai/lampOS/tree/19cf238713ae9f9a63e150cc980fd872e5b0a73f/lampOS/docs/verification/voice-fixtures-metadata-v7-20261015).
Relative to frozen v7, the endpoint fixture, two docs and a coordinator comment
are the only changed files. Normal reminder handling and all other task source
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
| 1 | 0 | no | 1173 / 0 / 8 | 21 | 464.047 |
| 2 | 0 | no | 1173 / 0 / 8 | 21 | 387.771 |
| 3 | 0 | no | 1173 / 0 / 8 | 21 | 386.477 |
| 4 | 0 | no | 1173 / 0 / 8 | 21 | 392.185 |
| 5 | 0 | no | 1173 / 0 / 8 | 21 | 392.309 |
| 6 | 0 | no | 1173 / 0 / 8 | 21 | 397.021 |
| 7 | 0 | no | 1173 / 0 / 8 | 21 | 390.261 |
| 8 | 0 | no | 1173 / 0 / 8 | 21 | 400.918 |
| 9 | 0 | no | 1173 / 0 / 8 | 21 | 398.243 |

This sequence does not meet the ten-run requirement. Complete raw output, failed counts and any requested-stop/build-error state are retained; no failed or partial sequence counts as qualification. The explicit stop reason is preserved in stop-reason.txt.

`qualification-audit.json` checks raw hashes, the four named regressions,
all 101 results in the endpoint/pilot/EarlyEnd/motor cohorts, and overlap
with actual compiler phases. Missing test counts or an outer timeout in any
test/build command disqualifies the sequence even if its exit status is zero.
One of those 101 is the existing model-dependent
ignore. `ignore-audit.json` records 8 observed ignores; the inventory matches the eight-ignore V6 baseline: True. Six
pre-existing ignores cover optional model, manual latency and long-duration
provider/reference cases; two newly merged reminder timing probes explicitly
run alone on an idle host. Their files match the upstream base. This task
adds no ignore or filter. Overlap is measured from the first `Compiling`
line through completed build command, not as a CPU-utilization measurement.

The scratch evidence scripts inherited reviewed missing-count/timeout handling
checked in v6 with four synthetic cases that replay only one completed run. `synthetic-auditor-checks.json`
labels those earlier checks explicitly; they test failure handling and never count
as additional real runs or lamp evidence.
The V8 auditor additionally passed 50 synthetic checks, including seven CLI
invocations that wrote failure audit files for missing or partial output.
Those checks have explicit synthetic labels and cannot qualify. The real
audit binds its exact raw summary and source manifest, checks the frozen
file inventory, and still rejects raw/source hash corruption. Valid-hash
terminal compile failures remain archivable with false qualification.
The later-gate scripts were separately reviewed for exact source/audit binding,
fixed image identity, parsed failures, partial-report wording and terminal
cleanup. Their first synthetic run met 32 of 33 expectations and exposed an
unreadable container-ID receipt falsely releasing a target. That failure is
retained. After correction, 41 of 41 simulated cases met expectations,
including unexpected cleanup exceptions. Those use fake process/signal APIs;
they do not establish actual descendant or container-runtime cleanup.

The sequence recorded 25 ARM64 cross-build cycles in the
existing x86_64 Linux Podman VM (6 virtual CPUs, 8192 MiB RAM), as UID 1000
with network disabled, all capabilities dropped, read-only source/registry/git
and Fourth's own writable target volume. Each cycle cleans only ARM64
workspace package artifacts, retains third-party dependency caches, then
builds all 34 workspace packages' binaries/examples for
`aarch64-unknown-linux-gnu`. Commands, outcomes and the configured image identity remain
in `arm64-builds.json`. A read-only inspection during the sequence confirmed
that the local tag matched that image ID (`image-observation.json`); it is
not a retrospective inspection of each completed container. The Mac sequence starts after actual compilation
begins. On stopping, the orchestrator allows an active build to finish and
retains its result; a failed build can have ended earlier. This is
cross-compilation load, not native ARM64 execution or lamp evidence.

The scratch orchestrator outside the repository recorded UTC boundaries and
monotonic elapsed durations, checked source hashes and sampled load/free space every five
seconds. There are 724 samples; one-minute load ranged from
3.675 to 13.274,
with minimum available space 38,846,226,432 bytes.
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
