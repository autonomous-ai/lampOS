# Voice fixture qualification v5: five passes, then an integration stop

Fourth's Intel development Mac: x86_64 macOS 26.6.2 (25G83), Core
i7-10700K, 64 GiB RAM, Rust/Cargo 1.96.0. This is not a lamp. The frozen
source is based on `f2261ec48e35a0bcfc1fa6fbd95f8edbc23f6d4d`.
`source.json` records all 1,678 file hashes; `source-changes.patch`
and the two `source-*` files preserve the uncommitted task changes.

V5 corrects the pilot fixture's source timeline: capture begins at actual
StartCapture and retains immutable 10 ms acquisition timestamps through
delayed delivery. It keeps all original assertions, real service-cycle
clocks, the 64-block step bound, privacy/input bounds, deliberate faults
and real child cleanup. Three added deterministic regressions cover
continuity, stale-input refusal and genuine source loss. Producer failure
diagnostics are explicitly not coordinator consumption ages.

The [focused correction and permission failure](https://github.com/autonomous-ai/lampOS/tree/f34fa30af5bb9248e31f51b8f614766c5d37e169/lampOS/docs/verification/voice-fixtures-pilot-v5-20261015)
are retained separately. The earlier failed v1 through v4 sequences remain
on the same immutable evidence commit; their failures are not erased by
later passing runs. Final independent static review and gate-scope/link
audits are included here with their limitations.

Every full serial Mac command was:

```sh
cargo test --locked --offline -p lamp-voice --no-fail-fast -- --test-threads=1
```

`CARGO_BUILD_JOBS=4`, `RUST_TEST_THREADS=1`, Fourth's existing Mac target
cache, and a 30-minute outer watchdog per command. Formatting and strict
workspace Clippy passed before load started. Raw output was not edited.
No retry occurs inside the sequence; future runs stop at the first test,
source or build failure.

| Full run | Exit | Passed / failed / ignored | Suites | Cargo wall seconds |
|---|---|---|---|---|
| 1 | 0 | 1085 / 0 / 6 | 21 | 437.745 |
| 2 | 0 | 1085 / 0 / 6 | 21 | 372.530 |
| 3 | 0 | 1085 / 0 / 6 | 21 | 373.109 |
| 4 | 0 | 1085 / 0 / 6 | 21 | 374.362 |
| 5 | 0 | 1085 / 0 / 6 | 21 | 373.972 |

This sequence does not meet the ten-run requirement. Complete raw output, failed counts and any requested-stop/build-error state are retained; no failed or partial sequence is counted as qualification. This was an explicit integration stop after five passing runs: newly merged Schedule support changed Voice and its fixtures, so qualification must restart on the combined source. The stop reason is preserved in stop-reason.txt.

`qualification-audit.json` checks raw hashes, the four named regressions,
all 98 results in the endpoint/pilot/EarlyEnd/motor cohorts, and overlap
with actual compiler phases. One of those 98 is the existing model-dependent
ignore. The full suite's six ignores are pre-existing optional model,
manual latency and long-duration provider/reference cases. This task adds
no ignore or filter. Overlap is measured from the first `Compiling` line
through completed build command, not as a CPU-utilization measurement.

The sequence recorded 14 ARM64 cross-build cycles in the
existing x86_64 Linux Podman VM (6 virtual CPUs, 8192 MiB RAM), as UID 1000
with network disabled, all capabilities dropped, read-only source/registry/git
and Fourth's own writable target volume. Each cycle cleans only ARM64
workspace package artifacts, retains third-party dependency caches, then
builds all 34 workspace packages' binaries/examples for
`aarch64-unknown-linux-gnu`. Commands, outcomes and image identity remain
in `arm64-builds.json`. The Mac sequence starts after actual compilation
begins; the current build finishes after the Mac sequence stops. This is
cross-compilation load, not native ARM64 execution or lamp evidence.

The scratch orchestrator outside the repository recorded UTC and monotonic
boundaries, checked source hashes and sampled load/free space every five
seconds. There are 387 samples; one-minute load ranged from
4.153 to 10.814,
with minimum available space 44,494,864,384 bytes.
Sampling ends with the Mac sequence, before the final build finishes.
Standard libtest results were parsed into the retained count records.
No orchestrator script or binary is committed here.

Not shown by this folder: native ARM64 task-head qualification, immunity
to arbitrary host starvation, actual lamp privacy response or thermal
margins, acoustic stopping or hardware motion. The checked-in lamp unit
requests RR priority 20; its installed state was not read from a lamp.
The separate Body clock-seam question remains pending, with no Body edits.
