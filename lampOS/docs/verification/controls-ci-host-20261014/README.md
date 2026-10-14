# Controls CI experiment — superseded, 2026-10-14

First fixed the missing `/bin/kill` dependency by installing `procps` in CI
(`f1ff6e857`) and explicitly said in [Queue #705](https://github.com/autonomous-ai/lampOS/issues/705#issuecomment-6166373831)
not to change the test. Third read that answer at the next half-hour check.
The uncommitted four-file experiment was discarded. **No code, manifest or
Controls document change from this experiment is being handed back.**
[The trial diff](superseded.patch) is retained only to identify what the
following checks exercised. The original test remains unchanged.

## Host and checks

Same Intel i7-10700K Mac, 16 logical CPUs, 64 GiB, macOS 26.6.2 / Darwin
25.6.0 x86_64, Rust/Cargo 1.96.0, APFS as the
[Tasks host record](../tasks-host-20261014/host.json). No other Third build/test
ran concurrently. Other host load was uncontrolled. No lamp or physical
control was used. Command durations below are execution records, not response
latency or lamp benchmarks.

The trial replaced the test's external `/bin/kill` with a test-only Rust
signal wrapper. It preserved the child, real SIGTERM and recording/owner
release assertions, and changed no runtime implementation. Controls' first
acquisition record is queued before the diagnostic that the test waits for.

The dependency walk selected Controls, Voice and Voice-eval. Exact argv,
timestamps and command durations are in [results](results.json) and
[the supplement](supplement.json), with unedited output alongside them:

| Check | Result | Command seconds |
|---|---|---:|
| Workspace formatting | pass | 3.632855930 |
| Strict all-target Clippy for all three crates | pass | 31.396059767 |
| Combined serial tests | **failed**, as described below | 529.307053309 |
| One isolated rerun of the failed Voice test | 1 passed | 1.030073565 |
| Independent Voice-eval tests | 284 passed, 0 failed/ignored | 797.166813347 |
| Full workspace build | pass | 3.134352658 |

The combined command passed all **144 Controls tests**. It then stopped in
Voice's library with **816 passed, 1 failed, 6 inherited ignored**; later
Voice binaries and Voice-eval were not reached by that command. Its partial
total is **960 passed, 1 failed, 6 ignored**, not a passing gate. The independent
Voice-eval command above supplies its separate coverage. [Counts](test-counts.json)
are derived from completed test harness summaries, without pooling the rerun
into the original result.

## Found in unchanged Voice code

`coordinator::motor::tests::motor_crash_preserves_actual_voice_owner_and_next_audio_dispatch`
failed at `coordinator/motor/tests.rs:722`: a peer's next authority message
did not match the original owner. The [full failed run](tests.txt) is retained.
One [isolated rerun](voice-isolated.txt) passed (0.30 seconds in its test
harness). That rerun does not turn the combined gate green.

Voice's source matched the integration base byte-for-byte; it was not edited.
One place for First to inspect is `active_reply()` in `coordinator.rs`: it
grants capture/admission leases for 500,000 microseconds before the failing
test creates its motor and four peer workers. The failed run did not capture
setup elapsed time or the actual received authority payload, so expiry is a
hypothesis, not an established cause. No production deadline was changed.

## Standalone trial copy

A fresh copy of 1,336 tracked files at
`/private/tmp/lamp-third-controls-ci-source-20261014` had no `.git` or previous
`target`. [Metadata and source checks](copy-check.json) verified all 31 workspace
packages/local dependencies stay inside it and the four relevant source/manifest
hashes match the trial. Ordinary cached Rust dependencies remain explicit.

`cargo build --offline --locked --workspace` passed in 82.442267836 seconds.
`cargo test --offline --locked -p lamp-controls -- --test-threads=1` passed
**144 tests, 0 failed, 0 ignored**, in 22.718654877 seconds. See
[standalone commands](standalone.json), [build](standalone-build.txt) and
[tests](standalone-controls-tests.txt). These qualify the discarded trial
copy only. No binary is committed. [The evidence manifest](evidence-manifest.json)
identifies and hashes all unedited copied logs.

## What has not been shown

No complete passing dependent gate, native ARM64 result for the trial,
physical SIGTERM timing, GPIO behavior or lamp measurement is established.
The Voice failure's cause remains unproven. The trial source was discarded
after First's explicit CI fix; its passing subset is not a shipped change.
