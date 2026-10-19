# Second route stop and start: host verification, 19 October 2026

Part of #1620 and #1567, carried onto `LAMP_VOICE_ROUTE=second` (the owner:
Gemini Live is no longer used on lamps). Base:
`5aff09571944fa81deeb11d3901a7bdba2bc8ed0` (PR #1712's head) plus this
branch's fix, tests and docs. macOS ARM64 host results, not lamp results. #1864's
commit (`b45aaf45a`) is on `lamp-voice` but not in this base; its behaviour
is checked here for the second route by tests of its own.

## Audit

- **Which processes.** With route `second`, `spawn_rig` starts the second
  route's worker in `rig.provider` under the role `second`
  (`coordinator.rs`, `routes.workers`); no Gemini worker starts. Deepgram,
  DeepSeek and ElevenLabs are tokio tasks inside that one process
  (`second_provider/net/*`), not children. The endpoint is the session's
  (`EndpointRig::start`, unconditional) and keeps its 500 ms receipts on
  either route.
- **Same cleanup.** `Rig::shutdown` sends Stop to `rig.provider` with the
  rest, polls it, rechecks its exit at the deadline, kills and reaps it, and
  records `ForcedStop`/`WorkerExit` under `second` (`coordinator/cleanup.rs`
  keeps that owner). Nothing in the stop, the restart guard
  (`guard_released_on_stop`, `stop_before_fault`) or `finish_active_run` reads
  the route.
- **Start.** The second worker says Ready and its link Ready (session 1,
  `Initial`) as `run` begins, before any service is asked. A refused
  service fails each line (`TurnFailed`), never readiness: there is no link
  for `LAMP_START_OFFLINE` to wait for.
- **The gap, fixed.** `worker::serve` let its current-thread runtime drop
  when `run` returned on Stop. tokio's drop waits, unbounded, for running
  `spawn_blocking` work: tool calls (`run_call`, up to `ASK_DEADLINE` 1.5 s
  for the executive), a text's tool call (`texts_ends`), DNS lookups
  (hyper-util and tokio resolvers). Async network I/O is dropped at once.
  The Live worker's calls run on detached threads. `serve` now ends the
  runtime with `worker::finish` (`shutdown_background()`); the process exits
  right after, so blocking work left detached dies with it.

## Tests added (`coordinator/tests/second_route.rs`)

- `a_stop_while_the_second_route_alone_is_mid_request_is_clean`: muted while
  the second route holds a whole line unanswered; every child sees Stop and
  is reaped, nothing forced or failed.
- `a_second_worker_that_ignores_stop_is_killed_and_named_as_geminis_is`: for
  `provider` (Live) and `second` alike, one `worker_forced_shutdown` naming
  the role, "did not stop within shared 300 ms deadline; forced termination
  required; reaped with", in the run's error too; all children reaped.
- `the_second_route_alone_with_its_services_refused_stays_up_past_the_startup_budget`:
  every line fails (`Transport`), the voice runs to a mute 28 s in, past the
  25 s budget, with `LAMP_START_OFFLINE` on and off; no `waiting_for_link`.
- `the_second_workers_link_is_ready_before_any_service_is_asked`: the real
  `worker::run` with refusing services says Ready, then link Ready, asks no
  service, and returns at once on Stop.
- `a_stop_during_a_slow_tool_call_lets_the_second_worker_exit_within_its_deadline`:
  the real `tool_runner` asking an executive that never answers, plus a
  chat stream waiting for its first byte, on a runtime built as `serve`'s
  and ended by `serve`'s own `worker::finish`. Mutation: with `finish`'s
  body a plain drop it fails, "took 1.501640334s"; with
  `shutdown_background()` it passes.

## Gate

Rust 1.96.0 from `lampOS/`, own target directory, `RUSTC_WRAPPER=`,
`CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0`:

```sh
cargo +1.96.0 fmt --all --check
cargo +1.96.0 clippy -j2 --workspace --all-targets -- -D warnings
cargo +1.96.0 test -j2 -p lamp-voice --lib coordinator::tests::second_route:: -- --test-threads=1
cargo +1.96.0 test -j2 -p lamp-voice --lib coordinator::tests::endpoint_session:: -- --test-threads=1
cargo +1.96.0 test -j2 -p lamp-voice --lib second_provider::worker:: -- --test-threads=1
cargo +1.96.0 test -j2 -p lamp-voice --no-fail-fast -- --test-threads=1
cargo +1.96.0 build -j2 --workspace
```

- Formatting and workspace all-target clippy: passed.
- Focused: `second_route` 30 passed; `endpoint_session` 37 passed,
  1 ignored; `second_provider::worker` 45 passed, 1 failed (below).
- Full `-p lamp-voice`: 2,463 passed, 1 failed, 12 ignored in 32 summaries.
- Workspace build: passed.

The one failure is pre-existing and time-of-day dependent, not this
branch's: `second_provider::worker::tests::as_the_only_voice_the_days_bars_hold_a_name_heard_close_by_day`
fails alone too, run at 21:49 local. It leaves `Heeding::night` on the wall
clock, which its sibling `..._the_day_and_the_night_bars_are_read_as_on_live`
stubs, so it fails at night. Left as it is.

Logs: `/private/tmp/lampos-seventh-watch-01a15185/2ndstop-*.log`.

## Not shown

No lamp, network service or acoustic timing. The forced stop is the
coordinator's handling of a synthetic peer; the gap test reproduces the
worker's runtime and its real tool path, not the whole `serve` or a lamp's journal. Whether lamps on the second
route have hit it is for their stop records (`worker_forced_shutdown`,
worker `second`).
