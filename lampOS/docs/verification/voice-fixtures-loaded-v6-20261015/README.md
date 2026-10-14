# Voice fixture qualification v6: three passes, then metadata-stall failure

Fourth's Intel development Mac: x86_64 macOS 26.6.2 (25G83), Core
i7-10700K, 64 GiB RAM, Rust/Cargo 1.96.0. This is not a lamp. The frozen
source is based on `cf2f5b43c8113a704324c2743af3cd92bbc4fdda`.
`source.json` records all 2,367 file hashes; `source-changes.patch`
and the two `source-*` files preserve the uncommitted task changes.

This sequence includes the newly merged Voice reminder integration. The
normal coordinator wrapper forwards the existing reminder state; only
explicit test helpers supply `None`. Endpoint and pilot fixtures reject
unexpected reminder input. Independent review of the four integrated
files, a comparison with the previously reviewed unchanged files, and
gate-scope/link audits are retained here with their limitations.

The five passing v5 runs ended deliberately for this integration; they do
not count toward this sequence. That stop, the exact rebase, the verified
locked dependency fetch and earlier v1-v4 failures remain on
[the preceding evidence commit](https://github.com/autonomous-ai/lampOS/tree/526745f798bc96815a09c3db1e88e509ca22c730/lampOS/docs/verification).
The [focused pilot correction and permission failure](https://github.com/autonomous-ai/lampOS/tree/f34fa30af5bb9248e31f51b8f614766c5d37e169/lampOS/docs/verification/voice-fixtures-pilot-v5-20261015)
are also retained. Later passes do not erase those failures or establish
the exact cause of a failed run whose timing was not recorded.

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
| 1 | 0 | no | 1170 / 0 / 8 | 21 | 462.988 |
| 2 | 0 | no | 1170 / 0 / 8 | 21 | 386.692 |
| 3 | 0 | no | 1170 / 0 / 8 | 21 | 387.171 |
| 4 | 101 | no | 1169 / 1 / 8 | 21 | 386.332 |

Run 4 failed in
`pilot_faults::sigterm_while_metadata_storage_is_stalled_stops_cleanly_and_reports_the_loss`
with `service privacy freshness lost before next observation`. The real privacy
producer gap was 50,902 us against the unchanged 50,000 us bound; exact
coordinator consumption age was not recorded. No speaker chunk had been
accepted and the scripted SIGTERM had not fired. The failure is consistent
with producer scheduling delay; the producer statistic is not a direct
measurement of coordinator age or lamp behavior. A later `RecvError` panic
came from the still-blocked metadata writer when the assertion unwound before
its release. `failure-analysis.json` separates observations from source
inspection and the proposed correction. All four named regressions and the
other 97 affected-cohort outcomes matched expectations in all four runs.

This sequence does not meet the ten-run requirement. Complete raw output, failed counts and any requested-stop/build-error state are retained; no failed or partial sequence counts as qualification.

`qualification-audit.json` checks raw hashes, the four named regressions,
all 98 results in the endpoint/pilot/EarlyEnd/motor cohorts, and overlap
with actual compiler phases. Missing test counts or an outer timeout in any
test/build command disqualifies the sequence even if its exit status is zero.
One of those 98 is the existing model-dependent
ignore. `ignore-audit.json` records all eight full-suite ignores. Six
pre-existing ignores cover optional model, manual latency and long-duration
provider/reference cases; two newly merged reminder timing probes explicitly
run alone on an idle host. Their files match the upstream base. This task
adds no ignore or filter. Overlap is measured from the first `Compiling`
line through completed build command, not as a CPU-utilization measurement.

The scratch evidence scripts were reviewed and checked with four synthetic
cases that replay only one completed run. `synthetic-auditor-checks.json`
labels those checks explicitly; they test failure handling and never count
as additional real runs or lamp evidence.

The sequence recorded 12 ARM64 cross-build cycles in the
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
seconds. There are 326 samples; one-minute load ranged from
4.577 to 9.165,
with minimum available space 42,612,764,672 bytes.
Sampling ends with the Mac sequence; any still-active build is then joined.
Standard libtest results were parsed into the retained count records.
No orchestrator script or binary is committed here.

The ordinary-library integration tests in `tests/privacy_state.rs` and
`tests/core.rs` passed in the first full run; their separate target results
are extracted in `ordinary-library-evidence.json`. They compile the normal
library, while the test clock and per-instance override exist only in unit
tests. They are not a measurement of physical privacy latency.

Not shown by this folder: native ARM64 task-head qualification, immunity
to arbitrary host starvation, actual lamp privacy response or thermal
margins, acoustic stopping or hardware motion. `lamp-handoff.md` gives
the exact read-only checks for First's required compatibility minute and
the unresolved physical measurement boundary. The checked-in lamp unit
requests RR priority 20; its installed state was not read from a lamp.
The separate Body clock-seam question remains pending, with no Body edits.
