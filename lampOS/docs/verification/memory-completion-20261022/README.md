# Memory fixtures finish on their observations

Part of #1806. Base `4efcf4eb0cda0ef730d30658572f49da7592a0b8`.
Test harness only: no product code, report period, client deadline or
original scenario assertion changes.

The newer-database scenario uses the existing explicit fixture binary and
report controller. That binary includes the ordinary application body and
only supplies its controlled reporter schedule. The test still requires an
actual Diagnostic with `set_aside`, a warning level, the original file name
prefix and bytes, and a successful remember request. A controller receipt
cannot satisfy the report predicate.

The late rule-acknowledgement scenario commits through the actual store,
then holds its answer until the client's original 1.5-second call returns
and the existing unsuccessful-result assertion passes. Dropping a channel
sender releases the worker; its declaration after the fixture also releases
it before the fixture's join during an assertion unwind. The ten-second
receive timeout is a failure bound, not a release delay. Worker failures
still propagate on a normal join; an existing assertion unwind no longer
causes a second panic merely because that worker also failed. Rule listing,
unlinked-journal rejection, forgetting and empty recall remain unchanged.

## Same-base host measurements

Three serial processes per case before and after. Timings include process
launch, test execution, cleanup and captured output, excluding compilation.
The first invocation is retained. These are Mac fixture durations, not lamp
response times. [Environment and planned targets](environment.json),
[before](before-host-timings.json), [after](after-host-timings.json),
[before output](before.txt), [after output](after.txt).

| Case | Before (s), runs 1 / 2 / 3 | After (s), runs 1 / 2 / 3 |
|---|---|---|
| Set-aside health | 2.3883 / 1.0952 / 1.0938 | 2.4220 / 1.0462 / 1.0560 |
| Held rule acknowledgement | 2.8086 / 2.7496 / 2.7495 | 2.5028 / 2.4877 / 2.4808 |

All twelve runs passed. The initial targets, health below 0.5 s and held-rule
below 1.65 s, are not met on this Mac. The health case's first run is slower
after the change; the complete series is retained, without a causal claim
about host overhead. The real 1.5-second deadline remains in the held-rule
case. No unrelated product timing is shortened to reach the target.

## Failure controls

Each temporary mutation compiled, then exited 101; all source bytes were
restored before the final gate. [Raw control output](mutation-controls.txt)
and [restored source hashes](source-sha256.json) are retained.

- [Omit the set-aside field](mutant-no-set-aside.patch): the original report
  predicate cannot match. The existing controller exhausts its logical
  window with a real warning report lacking the required field. The outer
  evidence script initially expected the host-watchdog wording; after
  inspecting the actual failure it retained this result and corrected only
  that diagnostic-text expectation. [Record](control-expectation-note.txt).
- [Release before the call](mutant-early-release.patch): the call succeeds
  and the original unsuccessful-result assertion fails.
- [Bypass uncertain-write journal rejection](mutant-unlinked-journal.patch):
  the original `finished_exchange_for` rejection assertion fails.
- [Unwind before normal release](mutant-unwind-before-release.patch): the
  intentional assertion fails, releases the worker and exits 101 without
  consuming the ten-second watchdog.
- [Leak the release token](mutant-leaked-release.patch): the original list
  assertion fails, the worker's ten-second watchdog also fails, and cleanup
  exits 101 without a double-panic abort.

No mutation ships. Patch paths are relative to the repository root and
apply to the final fixture/product source for replay.

## Validation

Memory tests passed: 282 passed, zero failed, six existing ignored
measurement/manual fixture cases. Dependent Voice-eval tests passed: 608
passed, zero failed or ignored. Formatting, both all-target Clippy commands,
the workspace build and diff check passed. Final local results are recorded in [commands](commands.json) and
[gate output](local-gate.txt). All commands run from `lampOS/`, sequentially:

```text
cargo fmt --all --check
cargo clippy -q -p lamp-memory --all-targets --message-format=short -- -Dwarnings
cargo test -q -p lamp-memory
cargo clippy -q -p lamp-voice-eval --all-targets --message-format=short -- -Dwarnings
cargo test -q -p lamp-voice-eval
cargo build -q --workspace
git diff --check
```

Voice-eval depends on Memory. No ARM64 build runs on the Mac and no device is
contacted. The PR remains draft until its exact-head Linux ARM64 and Linux
all-target Clippy runs pass. Measuring scripts remain outside the repository;
only text evidence is committed. Compiler JSON output is reduced to the
selected tested artifact and build-finished record; test output is retained.

The earlier full-main [ARM64 #5220](https://github.com/autonomous-ai/lampOS/actions/runs/39639887119)
recorded 1.014 s for health and 1.731 s for the held rule acknowledgement.
[Extracted rows](arm64-before-5220.json) are before-only context. The Memory
crate and Voice's remember module are unchanged between that base and this
one. Native runner/base differences remain; use the same-base series above
for an isolated comparison. #1806 remains open.
