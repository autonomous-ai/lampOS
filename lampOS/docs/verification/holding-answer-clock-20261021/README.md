# Holding-answer reminder fixture clock

For [#1569, row 77](https://github.com/autonomous-ai/lampOS/issues/1569).
The controls and initial gate below were measured on base
`88297758e2fc5c04a6d74fdb50d3143185d66903`. The change was then carried
unchanged onto `bc2f62fca65dac6f01fcf91fec878fb3eb8d5eee` for publication.
Host: `x86_64-apple-darwin`; Rust 1.96.0 (`ac68faa20`, 2026-05-25).

## Failure and change

The reminder scenario finishes its first turn with
`rig.owner.complete_turn(real_time(), reminder)`. The rig's capture and
admission leases last 100 ms. Previously, a host pause after the last
heartbeat could expire those leases, revoke the owner and return
`StaleOwner`, even though the test's reminder observations were correct.

The scenario now uses the existing paused Tokio runtime and
`logical_clock()` fixture. It takes the shared worker-state lock before
starting the clock's host watchdog. Only the rig's explicit ticks advance
lease age; all original assertions and the 100 ms lease remain. The
production worker, controller, privacy policy and other tests are unchanged.

## Controls

| Source and control | Runs | Result |
|---|---:|---|
| Original test; one 150 ms host sleep immediately before `complete_turn` | 3 | All fail at `complete_turn` with `StaleOwner` |
| Fixed test; identical 150 ms sleep at the same point | 3 | All pass |
| Fixed test without the sleep; remove the production holding-answer reminder-origin guard temporarily | 2 | Both fail the original assertion on an unexpected `HoldingAnswer` for the reminder |

The sleep and production mutation are absent from the final source.
`baseline-probe.patch` and `logical-probe.patch` each apply to the named
original base, independently. `reminder-origin-mutant.patch` applies on top of the
final test change. Representative raw outputs are the three `.log` files;
`controls.json` records every run and its exact command. Its elapsed values
include the Cargo invocation and process launch; they are not test-only or
lamp latency measurements.

The exact scenario used for every control was:

```sh
cargo test --offline --locked -q -p lamp-voice --lib \
  provider_worker::tests::holding_answer::a_reminders_holding_words_grant_nothing_and_the_next_heard_request_still_qualifies \
  -- --exact --nocapture
```

## Local validation

Commands run from the standalone `lampOS/` directory, with an isolated
target directory, four Cargo build jobs, debug information disabled for
development/test profiles, and incremental compilation disabled:

```sh
export CARGO_TARGET_DIR=/private/tmp/tests-codex-1569-target
export CARGO_BUILD_JOBS=4 CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0
cargo fmt --all --check
cargo clippy --offline --locked -q --message-format=short -p lamp-voice --all-targets -- -D warnings
cargo test --offline --locked -q -p lamp-voice
cargo test --offline --locked -q -p lamp-voice -- --test-threads=1
cargo build --offline --locked -q --workspace
```

Formatting, all-target Clippy and the workspace build passed. The ordinary parallel test run
failed: 2,971 library tests passed, 16 failed and 14 were ignored. All 16
failures were in the unchanged `coordinator::motor::tests` files: 11 fixture
preflight timeouts, four stepped-child timeouts and one cleanup assertion.
Their complete panic details are in `parallel-voice-failures.log`. The
reminder scenario and the row-72 EarlyEnd scenario passed in that run.

To check the effect of this run's concurrency, the full crate was rerun with
`--test-threads=1`: **3,252 passed, zero failed, 14 ignored** across its
library, binary, integration and example tests. All 16 motor cases passed.
The ignored tests are the existing ones; this change adds no exclusion or
retry. This serial pass does not erase the failed parallel result or prove
the precise cause of every motor failure. `gates.json` records the commands,
exit codes and elapsed times, including compilation and process launch.

The initial sandboxed control stopped before exercising the scenario because
macOS denied creation of its Unix socket. All reported control runs used
local Unix-socket access. No cloud service or lamp was contacted.

## Comparison with current integration head

The unchanged head `bc2f62fca65dac6f01fcf91fec878fb3eb8d5eee` was checked
with the same `cargo test --offline --locked -q -p lamp-voice` command,
profile settings and parallel test runner. Its library passed **3,004 tests,
zero failed, 14 ignored**, including all 16 motor cases. Its integration
binary passed 259 and failed
`light_call::twenty_light_calls_are_each_answered_once_the_ring_says_it_shows_them`
at `tests/it/light_call.rs:90`; that failure occurred without this change.
The raw log and command record are `baseline-current-parallel-voice.log`
and `baseline-current-parallel-voice.json`.

This run did not reproduce the earlier motor failures. The scenario clock
is thread-local, its scope cannot cross threads, and the motor fixtures do
not acquire the provider-worker state lock. That isolates their state, but
does not rule out host scheduling effects or establish the cause of the
earlier failures.

The two library executables were then preserved from that same base, one
unchanged and one with only the reminder test changed. The original 16
motor cases and the reminder scenario ran together in one parallel libtest
process, in order **unchanged, changed, changed, unchanged**: **17/17 passed
in each of the four runs**. The exact test names, commands, exit codes and
raw outputs are in `motor-comparison-tests.json`, `motor-comparison.json`
and the four `motor-comparison-*.log` files. No extra host load was injected.
The comparison found no motor regression attributable to this change;
the old 16 failures remain unexplained, not claimed as reproduced on the
unchanged head.

On the publication base, `cargo fmt --all --check`, the same all-target
Clippy command and `cargo build --offline --locked -q --workspace` also
passed. `current-gates.json` keeps those results and the command used to
preserve the modified library executable. The 3,252-test serial result above
remains the original-base result; it is not relabeled as a current-head run.

## Slow-test census

The separate #1806 census fell from **212.6 s to 187.3 s**: the summed
durations of tests taking at least one second outside the realtime group,
from full Linux ARM64 run
[39592616556](https://github.com/autonomous-ai/lampOS/actions/runs/39592616556)
at `9752b0f86` to
[39612385215](https://github.com/autonomous-ai/lampOS/actions/runs/39612385215)
at `64b2cb461bc593461b69779c515deea6813797b3`.
The count fell from 131 to 119; tests taking at least three seconds fell
from eight to six. This is aggregate progress from changes already on
`lamp-voice`, not a speedup attributed to this reminder-clock patch.

## Not shown

The pull request records its exact-head Linux ARM64 and Linux Clippy runs;
no ARM64 build was run on the Mac. No device, acoustic latency or motor
behavior was measured.
The separate `early_end` failure in row 72 is not claimed fixed here.
