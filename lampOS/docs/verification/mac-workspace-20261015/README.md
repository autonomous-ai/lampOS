# Complete Mac workspace failure and isolated follow-up

Ready for lamp-voice: no

Fourth ran the full workspace gate on an Intel i7-10700K / 64 GiB host,
x86_64 macOS 26.6.2, Rust 1.96.0. Source was
`5fa0f78d67442107bd5b9c000a9ff2e57d7c67d1`, based on
`dcc2e6b1ee67e11e2de89f893696432e47c4d8c5`. No competing local build/test
was started by Fourth. These are synthetic host fixtures, not lamp readings.

From `lampOS/`, with `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=4` and
`RUST_TEST_THREADS=1`, sequentially:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --offline --locked -- -D warnings
cargo test --workspace --offline --locked --no-fail-fast -- --test-threads=1
cargo build --workspace --offline --locked
```

Formatting and strict workspace Clippy passed. Tests finished with exit 101:
**3,940 passed / 4 failed / 30 ignored, 269 completed suites**, including
the custom turn-end-wire harness's 12 passing tests. The workspace build
was not reached. `full-gate.txt` is the unmodified complete output,
`results.json` counts every completed suite, and `gate.json` records the
terminal status and hashes. Cargo's inherited Controls/Expression
`replay_cost` output-name collision warnings are retained, not suppressed.
Health's six program tests now pass after the separately recorded disk
cleanup. Memory's real-store Voice and submit targets passed 8/0 and 4/0;
VoiceEval's final loopback and session targets passed 12/0 and 14/0.

## Failures kept

All five audited test/helper files match upstream at the tested base;
`source-sha256.json` records their hashes. This proves file identity, not
that Memory could not affect the wider test environment.

- Tasks `a_descendant_holding_pipes_is_killed_without_an_unbounded_drain`
  failed at `tests/harness.rs:509`: expected `Uncertain { pipes_open }`,
  got `NotSent { preflight_timeout }`. Its one-second `machines --json`
  preflight expired before the descendant assertions. The target was
  23/1. The earlier report is `../mac-tasks-preflight-20261015/`.
- Voice `coordinator::motor::tests::late_started_callback_cannot_restore_an_offer_after_owner_replacement`
  failed at `coordinator/motor/tests.rs:896`: the expected child-output
  file was absent after the worker was reaped (ENOENT).
- Voice `coordinator::motor::tests::motor_crash_preserves_actual_voice_owner_and_next_audio_dispatch`
  failed at `coordinator/motor/tests.rs:722`: the next control message did
  not match the expected authority snapshot with the original owner.
- Voice `coordinator::tests::endpoint_session::verdicts_after_the_result_timeout_fall_back_on_every_turn`
  failed at `coordinator/tests/endpoint_session.rs:972`: its session
  returned `physical privacy closed or lost freshness` before the
  endpoint-timeout assertions. This case runs for six seconds with an
  emulated privacy source; no physical control was used.

The Voice library finished 854 passed / 3 failed / 6 ignored in 452.75 s.
`voice-failure-excerpt.txt` is an explicitly labelled excerpt; the full
raw run is retained separately. Cause is not established by these messages.
No privacy freshness limit, timeout, source or assertion was relaxed.

## Twenty isolated executions, same binaries

After the full gate ended, each failed test was run five times directly
from its existing binary, without a Cargo rebuild, with
`--exact <test-name> --test-threads=1 --nocapture`. All 20 executions
finished with exit 0. `isolated.txt` retains every command, binary hash,
raw output and terminal status; `isolated-results.json` includes the
monotonic elapsed time around each child process. A 60-second outer bound
was available for each execution and none reached it.

This is follow-up evidence, not a passing full gate or a measured cause.
It neither proves a scheduling explanation nor repairs the failing cases.
The three Voice fixtures are outside Fourth's task scope and are reported
to First once with this evidence.

## Owner repair and next source

First answered the Tasks report on #698 at 00:39 ICT with
`1a38f2f3e6b8afd1e7896761a0da6149a2400f67`, changing the rig's ordinary
launch/read bounds from one to ten seconds while timeout cases keep their
own shorter bounds. First reports three Mac harness runs of 24/0; those
are First's measurements. Fourth has rebased onto that requested repair,
including the intervening task-news changes. These old Mac results remain
tied to the old source and the unrepaired one-second rig.

The new source needs its own ordinary-user Linux gate and native ARM64
head run. No Memory PR is ready on the strength of this report.

## Not shown

No lamp execution, physical latency or loaded-host root cause was shown.
The isolated passes do not prove workspace stability. Raw whitespace is
preserved in the `.txt` artifacts; exclude them from prose whitespace checks.
