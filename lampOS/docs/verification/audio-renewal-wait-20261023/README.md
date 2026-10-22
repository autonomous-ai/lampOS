# Requested-audio renewal refusal fixture — 23 October 2026

Second's one-commit source is `f913461d92b7928e5361c51de357ca7aef4b1a8d`,
on `3989219969f2460e6cfd69d32d06a58dd5c2e269` (public `lamp-voice` at push).
The change is confined to Voice's requested-audio tests and their documentation.
The runtime prefix before `#[cfg(test)] mod tests` is byte-identical to base.
There is no production timing, permission, cache or protocol change.

## What failed and what the fixture now establishes

Original ARM64 attempt 1 failed at the assertion that the operation after
Start was Renew:

| Original run | PR head | Passed / failed / skipped | Unrun |
| --- | --- | --- | --- |
| [39823420807](https://github.com/autonomous-ai/lampOS/actions/runs/39823420807/job/119980313890) | `839c74498` | 8,375 / 1 / 67 | 2,207 |
| [39827406552](https://github.com/autonomous-ai/lampOS/actions/runs/39827406552/job/119994248943) | `7b393982b` | 8,392 / 1 / 67 | 2,216 |

The original assertion did not print the received operation. The root cause
of either original failure is not established. Later reruns do not replace
these failures. `original-failures.json` binds each original attempt/job and
the retained full raw log's byte count and SHA-256.

An independently scheduled monitor can legitimately cancel on speaker-cache
contention, missed freshness or lease expiry before renewal. The refusal
fixture now uses the existing acknowledged worker steps and synthetic
monotonic clock, with the real worker, WAV and Unix socket. Each step refreshes
the cache while the worker is parked. It expects no renewal before 200 ms,
then exact Start / Renew / Cancelled operations and joined worker completion.
For a bad digest it expects Start / Cancelled with no prepared samples.
The valid-recording branch retains prepared samples and an unexpired original
grant, then checks that revocation denies playback, sample access and completion.
Separate contention and expiry tests remain in the full crate gate.

## Mac gate and source binding

Host: native **Darwin arm64**, Rust **1.96.0**,
`aarch64-apple-darwin`. Commands ran from `lampOS/`, offline with two Cargo
build jobs and the existing Mac target cache. No local ARM64 Linux gate ran.

| Command on final head | Exit | Elapsed | Result |
| --- | --- | --- | --- |
| `cargo +1.96.0 fmt --all --check` | 0 | 5.443 s | Pass |
| `cargo +1.96.0 clippy -q -p lamp-voice --all-targets --offline -- -D warnings` | 0 | 28.489 s | Pass |
| `cargo +1.96.0 test -q -p lamp-voice --offline --no-fail-fast -- --test-threads=2` | 0 | 313.247 s | **3,728 passed / 0 failed / 14 ignored** |

`mac-current.json` records six bound source/manifest/toolchain hashes, all
unchanged after the gate, the exact commands, durations and complete output
hashes. `mac-current-*.txt` are the complete outputs. Aggregate counts sum
Cargo's six result lines; no test was filtered.

Before the latest-base rebase, the same runtime/documentation bytes passed
formatting, strict Clippy and **3,727 / 0 / 14 ignored** with one test thread
on `23132abf9`; `mac-gate.json` and `mac-gate-*.txt` retain that separate run.
The rebase has an equal range-diff (`rebase.json`). The latest base adds one
Voice coordinator test, accounting for the extra final-head pass.
The initial runtime-only check passed **18 / 0**, retained as `focused.json`
and `focused-*.txt` (the local Cargo executable path is normalized to
`cargo` in the receipt); it is not a substitute for either complete crate gate.

The changed-name search across `crates/` finds `WorkerSteps`,
`SteppedWorkerTime`, `resume_next` and the regression name only in Voice's
requested-audio runtime. No other crate consumes a changed API.

## Negative controls

Both commands select the exact test with `--lib --offline`,
`-- --exact --nocapture`; their full argument lists and mutated source hashes
are in `controls.json`. The candidate was restored byte-for-byte before the
full gates.

1. **Old fixture, cache held after Start:** exit **101**, **0 passed / 1 failed**.
   The actual next operation was Cancelled Finish rather than Renew. This
   demonstrates one legitimate cancellation route; it does not attribute
   the original CI failures to contention. Apply `base-cache-contention.patch`
   to the original runtime on base `23132abf9` to reproduce the mutation.
2. **New fixture, renewal allowed:** exit **101**, **0 passed / 1 failed**.
   The required Cancelled operation never arrived; bounded worker/peer waits
   failed. Apply `candidate-renewal-accepted.patch` to the final runtime.
   The fixture cannot pass by silently accepting renewed permission.

The two complete outputs are `base-cache-contention.txt` and
`candidate-renewal-accepted.txt`. These are deliberate failing controls,
not passing product tests.

## Limits and review

This proves the targeted protocol and retirement checks under acknowledged
steps, not independent monitor scheduling or real elapsed renewal latency.
Production code is unchanged. No lamp, microphone, speaker, cloud service,
real grant, device install or release operation was used. The official PR's Linux ARM64 and strict Linux Clippy jobs passed as recorded
below. First's combined latest-base integration gate is separate.

Self-read checked wrong-person data paths, stop/forget cleanup, off-switch
behavior and census impact: the implementation changes only the existing
`#[cfg(test)]` module and adds no runtime data path, switch, unit, topic or
message field. Revoked authority, retained samples, capacity release and
worker exit are asserted explicitly. First owns integration and device work.

## Official PR #3105 gates and integration carry

Both official attempt-1 workflows and their required jobs completed successfully
on source `f913461d9`, checked as merge `1f6f1e868` into `398921996`.

- [ARM64 39836153263](https://github.com/autonomous-ai/lampOS/actions/runs/39836153263/job/120024921759):
  **6,925 passed / 0 failed / 31 skipped**, one existing leaky journal pass,
  nextest **127.964 s**. The workflow selected **12 of 46 crates**: Voice,
  its dependents and the three file readers. This is the PR selection, not a
  full-workspace test claim. Both affected runtime fixtures passed at **0.009 s**.
- [Strict Linux Clippy 39836153301](https://github.com/autonomous-ai/lampOS/actions/runs/39836153301/job/120024921893): pass.
- `native.json` binds source/base/checked merge, workflow and job terminal
  statuses, original logs and SHA-256, named test results and native watcher
  exit 0. Nested Cargo result lines are retained without double-counting them
  into nextest totals. Publication was correctly skipped on this PR.

The updated integration head `001a3d714` has a clean merge-tree preflight,
with identical patch and both changed files (`carry-preflight.json`).
First carried the exact commit as `75dfd7cb1` in [train 7, #3106](https://github.com/autonomous-ai/lampOS/pull/3106);
`train7-carry.json` records an equal range-diff and matching hashes. The train's
combined gate is running; this receipt does not pre-claim its result.
Fourteen's source read found no P1/P2. Its service-admission scope note is
retained in the PR: this fixture calls the worker directly, while adjacent
service tests cover `Service::offer`. Original failed CI causes remain unproven.
