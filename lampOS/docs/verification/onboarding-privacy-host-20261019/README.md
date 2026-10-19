# Privacy onboarding: host verification, 19 October 2026

Part of #1802, continued from #841. Original integration base: `981bf101a`. The feature remains off by default.
These are macOS ARM64 host checks; no lamp was accessed for this batch.

## Scope and boundaries

The privacy step needs whole invitation playback, a fresh Controls-qualified
close, verified microphone/camera teardown, and a fresh same-source open with
normal input readiness. The model can skip, but cannot certify it. The core
skill uses an exact current invitation token; the runner keeps evidence and
acknowledged progress. A hardware-owner profile supplies switch location;
missing or invalid metadata never becomes guessed wording.

One bounded storage worker spans the service's voice cycles. The coordinator
only polls/submits; progress, budget, checkpoint and mailbox I/O run off its
interaction loop. Invalidation fences hide stale results immediately and order
the durable barrier after an old write. Shutdown retains accepted barriers;
Pending is not durable completion or cancellation. Reset owns the canonical
state directory and preserves the installed core package.

## Regressions

- Delayed storage initialization, stale/late Done versus redo, a newer desired
  snapshot versus an older completion, fast reopen before Closed is committed,
  mailbox acknowledgment across cycles, explicit redo after storage failure,
  and the persisted two-invitation budget.
- Source/clock/reader mismatch, stale/duplicate acquisitions, five-minute expiry
  evaluated on opening, model-only Done rejection, pause/skip followed by an
  immediate reclose, invalidation failure and service restart.
- A held old Done write followed by redo and shutdown: shutdown stays Pending
  until the barrier is drained, and the next initialization reads reset state.
- `pilot_faults` real-conversation fixtures hold the storage worker, verify at
  least 30 validated capture frames reach the hearing path and onboarding is
  polled, then verify Stop/physical mute and audio-worker reap before release.
  These are controlled host fixtures, not acoustic/device latency measurements.
- Bounded profile loading, opt-in topic publication, core-skill replay/token
  matching, private state permissions and full reset coverage.

## Commands and current results

Rust 1.96.0, from `lampOS/`, one target directory for this checkout:

```sh
export RUSTC_WRAPPER=''
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export CARGO_INCREMENTAL=0
cargo +1.96.0 fmt --all --check
cargo +1.96.0 test --offline -j2 -p lamp-voice onboarding -- --test-threads=1
cargo +1.96.0 clippy --offline -j2 --workspace --all-targets -- -D warnings
cargo +1.96.0 test --offline -j2 --workspace --no-fail-fast -- --test-threads=1
cargo +1.96.0 build --offline -j2 --workspace
```

Final-pass results: formatting passed; focused tests 52 passed, zero failed;
strict workspace all-target clippy passed. Full workspace tests: 7,246 passed,
0 failed, 44 ignored across 369 summaries. Workspace build passed. Earlier infrastructure-only tests passed 433 with zero
failures and one existing ignored test, before the async voice integration;
they do not replace the final workspace run.

Development checks caught a missing field in a test initializer, reset fixture
array length, formatting/lint diagnostics, synchronous storage on the owner
loop, and a shutdown path that discarded a queued invalidation barrier. Those
were fixed before the final pass; earlier failures are not counted as successes.

## Not verified

Native Linux ARM64 CI and the physical switch walkthrough remain outstanding.
The target is acknowledgment within 500 ms after accepted proof; slow storage
can delay durability, but must not block capture or privacy processing. No
hardware first-use/repeat/slow-storage measurements are claimed here.

## Integration refresh

Rebased onto `584e0d471f67f69dd3b8873912b3ed8dc3b67a54`. Resolutions retain
upstream Reports registration, skill Words/SAY_PRIORITY behavior and upstream
skill tests together with the onboarding bridge. No feature switch was enabled.

On this refreshed source, formatting and strict all-target workspace clippy
passed. Complete touched/dependent crate suites passed: 3,606 tests, zero
failures, 18 ignored across 121 summaries. Workspace build passed. The command
was:

```sh
cargo +1.96.0 test --offline -j2 -p lamp-msgs -p lamp-skills -p lamp-voice -p lamp-voice-eval -p lamp-memory -p lamp-cloud -p lamp-setup -p lamp-update --no-fail-fast -- --test-threads=1
```

The original full-workspace result above is retained as original-base evidence;
it is not relabelled as the refreshed source's test run. Exact refreshed-head
ARM64 remains required before the PR is ready.

## Catalog refresh

Rebased again onto `e2a8dd479`. The two mechanical conflict resolutions retain
upstream familiar-topic registration and the not-feeling-well core skill along
with onboarding. The onboarding registry entries now sit beside skills entries
to avoid the shared append location. No privacy runtime behavior changed.

Formatting, strict all-target workspace clippy and workspace build passed.
Focused onboarding tests passed 52 with zero failures. Complete msgs, skills,
setup and update suites passed 461 with zero failures and one ignored test
across 34 summaries:

```sh
cargo +1.96.0 test --offline -j2 -p lamp-msgs -p lamp-skills -p lamp-setup -p lamp-update --no-fail-fast -- --test-threads=1
```

Earlier full-workspace and eight-crate results retain their stated bases.
Exact-head ARM64 and physical evidence remain outstanding.
