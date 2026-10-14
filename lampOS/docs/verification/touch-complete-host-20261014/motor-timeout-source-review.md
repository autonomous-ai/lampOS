# Motor timeout audit — 2026-10-14

Read-only at frozen head `08a32c9d8`; no source edits, builds, tests, network
or device access. Paths below are relative to `lampOS/`.

## Observed evidence

- `/private/tmp/lamp-third-touch-final-buffered-gate-tests.txt:3705–3715`
  records the named Voice test failing at `motor/tests.rs:441` with
  `owned child timed out: None`; Voice total: 830 passed, 1 failed, 6 ignored,
  462.28 s. It contains no mode, handshake stage, poll timestamps or backtrace.
- The retained original-binary reruns each passed this exact test, with
  harness-reported durations 3.15, 2.65 and 2.63 s. The separate process-wall
  durations in `lamp-third-touch-original-motor-reruns/results.json` are
  4.084655972, 3.582238322 and 3.563615848 s. These are whole two-mode tests,
  not measurements of either child's startup, readiness or reap interval.
- That artifact identifies the original binary SHA-256 as
  `de05a4465230f41753ce3285ee20536c1c9681e5c2ea3605142c7734160d0ced`.
  The parent separately reports a focused Cargo pass and native ARM64 run
  38573575450 passing 4054 tests; I did not independently query that run.

## Source facts and narrower interpretation

1. `crates/voice/src/coordinator/motor/tests.rs:642–651` runs `crash`, then
   `hang`. Each first waits directly for `OptionalMotor::reaped`; it does not
   separately establish Ready before starting the two-second wait. Later
   cleanup already has a reaped child, and another start is refused because
   the existing child remains owned. The panic does not reveal which mode.
2. The helper at `tests.rs:438–451` uses a real two-second deadline. It checks
   that deadline **before** refreshing authority and calling `motor.poll`.
   Its 1 ms `park_timeout` is a requested pause, not a measured poll cadence.
   A delayed test thread can panic without polling an exit/fault already due.
3. `OptionalMotor::failure()` (`motor.rs:471`) only exposes its latched error.
   `poll` copies a Supervisor error even after `Ok(None)` (`motor.rs:702–714`).
   Thus `None` means no error was latched at the last completed poll, not that
   the child was healthy, still running, or stuck in any particular operation.
   It does not support a claim that a discovered failure was silently dropped.
4. Production `Supervisor::STARTUP_US` is **3,000,000 us**, while progress after
   Ready is bounded to 50,000 us (`body/src/runtime/supervisor.rs:18–24`).
   `enforce` uses the startup deadline while not ready (`:424–429`); it does
   not impose the shorter progress deadline before Ready. Startup is measured
   from just before `spawn`, and enforcement/reaping happen only when polled.
   Therefore the two-second helper can expire before a legitimate production
   startup deadline, with no latched failure. This is a definite deadline
   mismatch, not proof that this failed run spent two seconds starting.
5. The fixture launches a shell wrapper which execs this same test binary's
   `inert_child` (`tests.rs:393–399`). The child reads/validates configuration,
   binds two sockets, connects, builds/qualifies the inert core, then runs the
   worker. It has its own two-second connection loop (`:320–324`). Production
   native connection setup instead allows three seconds (`native.rs:167–172`).
6. `body/src/runtime/channel.rs:130–180` deliberately waits for both peer paths
   and private modes; bind and chmod are separate calls. Missing paths or a
   not-yet-private peer return `false`, not an error. Connection is stepped,
   without an internal blocking wait or automatic rebind. This can leave the
   supervisor pre-Ready with `failure == None`, but no infinite-loop defect was
   identified: the supervisor's own startup deadline still applies on poll.
7. Worker Ready requires a consumed authority snapshot (`worker.rs:431–451`).
   The fixture sends Ready before exiting 7 (`crash`) or parking forever
   (`hang`), at `tests.rs:270–291`. Once Ready is observed and polls continue,
   hang has no progress packets and should enter the 50 ms failure path;
   crash is detected by `try_wait`. Both paths latch a failure before claiming
   clean cleanup. Packet age/order faults likewise produce explicit errors.

## Conclusion and remaining uncertainty

No deterministic production defect or Touch-dependent cause was identified.
The strongest source-supported explanation is a real-time test budget that
includes startup but is shorter than production's startup allowance, possibly
combined with delayed parent/child scheduling. Host contention, filesystem or
loader delay are possibilities, not measured causes. Serial execution alone
does not establish their absence. The unchanged passing reruns narrow this to
an intermittent failure; they do not replace the pending full gate. Identifying
the exact cause needs mode/stage and last-poll/Ready/reap timestamps, absent from
the retained failure. No ownership change or source patch is proposed here.
