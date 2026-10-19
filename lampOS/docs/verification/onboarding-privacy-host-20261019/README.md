# Privacy onboarding: host verification, 19 October 2026

Part of #1802, continued from #841. Original integration base: `981bf101a`. The feature remains off by default.
These are macOS ARM64 host checks; no lamp was accessed for this batch.

## Scope and boundaries

The privacy step needs whole invitation playback, a fresh Controls-qualified
close, the voice's verified microphone teardown (the camera's close is not
observed; see Review fixes), and a fresh same-source open with
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

## Review fixes (19 October 2026, Third's privacy review on PR #1792)

On head `76e6342ee`, uncommitted:

- Privacy is available only with a receipt source (`LAMP_PRIVACY_CONTROLS=on`);
  with the GPIO worker it is `not_available` and the walk goes on
  (`without_a_receipt_source_privacy_is_unavailable_and_the_walk_goes_on`).
- The camera's close is not proven by this step: the voice verifies its own
  teardown, and the eyes close the camera on their 1.5 s permit timeout, which
  nothing here observes. The eyes' 1 Hz `diagnostics` health report names the
  camera's state but is not a privacy-grade signal and the voice doesn't read
  it. Done's note now claims the microphones only. Proving the camera's close
  remains open on #1802.
- A close read just before the invitation's end is processed is offered again,
  not failed (`a_close_read_just_before_arming_is_offered_again_not_failed`).
- Start-up removes `privacy-pending.json.next-*` left by a crash mid-write
  (`start_up_removes_temporaries_a_crash_left_mid_write`).

Gates on this change, from `lampOS/` with the environment above, one fresh
target directory, run in this order:

```sh
cargo +1.96.0 fmt --all --check
cargo +1.96.0 clippy --offline -j2 --workspace --all-targets -- -D warnings
cargo +1.96.0 test --offline -j2 -p lamp-voice --lib -- --test-threads=1 onboarding privacy say::
cargo +1.96.0 test --offline -j2 -p lamp-voice --lib -- --test-threads=1 without_a_receipt_source_privacy_is_unavailable_and_the_walk_goes_on a_close_read_just_before_arming_is_offered_again_not_failed start_up_removes_temporaries_a_crash_left_mid_write
cargo +1.96.0 test --offline -j2 -p lamp-msgs -p lamp-skills -p lamp-setup -- --test-threads=1 onboarding
cargo +1.96.0 test --offline -j2 -p lamp-update --test install -- --test-threads=1
cargo +1.96.0 test --offline -j2 -p lamp-voice --no-fail-fast -- --test-threads=1
cargo +1.96.0 build --offline -j2 --workspace
```

Formatting and strict all-target workspace clippy passed. Focused lamp-voice
tests: 157 passed, 0 failed; the three new tests: 3 passed. msgs, skills and
setup onboarding tests: 5 passed, 0 failed across 26 summaries. Install: 52
passed, 0 failed. Full lamp-voice: 2,331 passed, 0 failed, 12 ignored across
32 summaries. Workspace build passed. The counts in earlier sections keep
their stated bases; exact-head ARM64 and physical evidence remain outstanding.

## Camera receipt (19 October 2026, follow-up to Third's P2-2, #1802)

The gap: on a physical mute the voice cycle returned `PhysicalMute` from
`conversation_with_gate` before reaching the per-loop `eyes.permit(...)`
call, so the eyes were never told `{"camera":false}`; the camera closed only
on the eyes' 1.5 s permit timeout (1.0 to 1.5 s after the mute, since the
last yes was at most 500 ms earlier), and nothing tied a close to the step.

The change: the voice tells the eyes no first at every physical or software
mute exit (`EyesLink::close`), and on drop for any other end of a cycle. When
a close is taken as evidence and the eyes' `eyes/camera_closed` publisher is
alive on the bus, it then sends `{"close_receipt":N}` (fresh random N, kept
with the close in the checkpoint). The eyes publish
`lamp_msgs::CameraClosed {close:N, released}` only once the camera is shut,
after the device's stop has returned; `released` is that stop's result. On
reopening, Done needs the receipt from source `eyes`, for N, released, within
2 s of the ask, and then says "microphones and the camera"; a receipt for
another close, a faulted stop, a late one or none makes the step Failed.
Without eyes or their publisher the camera is `not_available` and Done claims
the microphones only, as before. Muting never waits on the eyes.

Gates, from `lampOS/` with the environment above, in this order:

```sh
cargo +1.96.0 fmt --all --check
cargo +1.96.0 clippy -j2 --workspace --all-targets -- -D warnings
cargo-zigbuild clippy -j2 --target aarch64-unknown-linux-gnu -p lamp-eyes --all-targets -- -D warnings  # the eyes' loop is Linux-only
cargo +1.96.0 test -j2 -p lamp-voice --lib -- --test-threads=1 onboarding privacy body_link
cargo +1.96.0 test -j2 -p lamp-eyes -p lamp-msgs -- --test-threads=1
cargo +1.96.0 test -j2 -p lamp-voice -- --test-threads=1
cargo +1.96.0 build -j2 --workspace
```

All passed. Focused lamp-voice: 146 passed, 0 failed. lamp-eyes and
lamp-msgs: 278 passed, 0 failed, 2 ignored across 17 summaries. Full
lamp-voice: 2,408 passed, 0 failed, 12 ignored across 32 summaries. New
tests: the voice says no at once and then asks the receipt, and a dropped
cycle says no (`body_link`); a receipt is said once, only after the stop, with
a faulted stop as `released: false` (`permit`); the receipt token is stored
only with its close and survives a voice cycle (`onboarding::privacy`); Done
with the matching receipt claims the camera, another close's, a faulted or
no receipt fails, no eyes on the bus stays microphones-only, and a late or
foreign receipt proves nothing (`privacy_flow_tests`).

Not verified: anything on a lamp. The time from the switch to the camera
released was not measured; on the host only the voice's side is bounded (no
and the ask are sent within 50 ms in the test, with no wait on the eyes). On
the lamp the eyes notice the no on their next loop turn, which can be behind
a detector run or a blocking open (up to 2 s, `OPEN_BUDGET_US`), then stop
the device within its teardown budget. A receipt proves the eyes' process
stopped and closed the V4L2 device without a fault, not that the sensor or
its light lost power.
