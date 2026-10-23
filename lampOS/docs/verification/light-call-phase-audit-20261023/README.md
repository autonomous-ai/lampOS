# Light-call source audit at 09f8e7412

No missing construction-readiness event is established. No source was edited,
compiled or executed. The two complete retained failures were read: peer #3271
upper-middle 59.537973 ms, Expression #3273 40.232748 ms, both against strict
`<25 ms`. All 20 Done assertions passed before each final latency failure.
Fourteen peer samples and thirteen Expression samples are at least 25 ms, so
a single slow first call cannot by itself explain either sample set. Sorting
lost their call order; it does not tell us whether slowdown clustered early.

## Exact boundary and readiness

- `crates/voice/tests/it/light_call.rs:41–67`: a fresh process-specific root is
  created, the ring thread is spawned, and the caller waits until `light/state`
  actually exists. The 5 s readiness watchdog and 10 ms readiness polling are
  before the measured loop. `BusLight::new` also completes before it.
- `ring_light.rs:334–377`: construction installs the command/test subscribers
  and state/drawn publishers before returning. `ring_light.rs:600–607` publishes
  the first state from a tick, so the observed state is evidence of a completed
  constructor and executed ring pass. No asynchronous registration remains
  behind a declared ready marker in this path.
- `light_call.rs:69–79`: every interval starts before construction of the JSON
  argument and the synchronous `Executive::call("light_colour", ...)`, ending
  at `started.elapsed()` after return. The subsequent Done assertion is outside
  that interval. Red/blue alternate, so each asks for a real state change.
- `light_call.rs:81–90`: stop/join/root removal finish before sorting and
  selecting index 10 of 20. Cleanup is outside every measured interval.
- `actions/src/lib.rs:281–296` and `actions/src/light.rs:95–118,309–334`: the
  interval includes current-state availability/readback, request publication,
  and confirmation. `light.rs:44–67` retains real 2 ms polling and the 600 ms
  unsuccessful-confirmation bound. `ring_light.rs:65,437–463` checks commands
  every 2,000 us. These waits are part of the path the test measures.
- `bus/src/lib.rs:268–278`, `bus/src/wire.rs:166–174`, and
  `bus/src/publish.rs:647–690,834–845` show retained-file reads, socket delivery,
  presence/subscriber discovery, and write/rename work inside the path. There
  is no captured evidence allocating the observed elapsed time among them.

The fixture sleeps a full 2 ms after each tick (`light_call.rs:49–51`). The
actual ring loop records its pass start (`ring_worker.rs:572`) and sleeps only
the remainder of its 2 ms budget (`ring_worker.rs:763–765`), with the light
checked at lines 724–725. This is a real fixture/production pacing difference,
but no measured tick costs or wake delays establish its contribution. Changing
it now would be a timing change, not an evidenced missing-readiness repair.

## Prior evidence already exists

The original implementation commit `2750a8c04d` reduced the ring look from
50 ms to 2 ms and the executive read from 15 ms to 2 ms. Its existing focused
unit checks still cover next-pass acceptance and a delayed second read. Current
`docs/actions.md:252–257` describes that historical improvement; it does not
qualify this shared-host workload.

`docs/verification/holding-answer-clock-20261021/README.md:83–94` already
records a light-call failure on an unchanged integration head.
`memory-note-retry-20261022/README.md:105–114` retains a 56.693456 ms parallel
failure and a later serial pass, explicitly without waiver. The boot-clock
comparison receipts likewise retain failures on both sides. The scoped search
found no prior phase-timing diagnostic or proven missing readiness condition;
it was not an exhaustive search of all remote issue comments.

## Recommended next step, requiring a separate grant

Keep the 20 calls, both real 2 ms pacing constants, clock, 600 ms bound, strict
25 ms upper-middle assertion and default-parallel gate unchanged. A useful next
observation is one temporary, fixture-only diagnostic in this integration file:

1. Wrap the existing `BusLight` in a delegating `Light` wrapper that records
   entry/exit of `there`, `shown`, `ask`, and `covered`, preserving returned
   values and every original call. Keep ordered call index/start/end as well as
   the original sorted final list.
2. Record the ring fixture's tick entry/exit, returned change, and pre/post
   `sleep(PASS)` timestamps against one real Instant origin. Use preallocated,
   bounded per-thread buffers and print them after join; no logging locks or
   file I/O in measured methods, added sleeps, warm-up calls or republishing.
3. Run one fixed, declared diagnostic observation in the same integration target
   and default-parallel workload. Preserve every outcome, trace overflow and
   instrumentation-overhead limitation. A passing diagnostic is not a retry
   that clears the original failure. It cannot identify a kernel scheduling
   cause, but it can separate tick time, gaps between ticks, method work and
   gaps between confirmation reads without instrumenting product code.

This is more discriminating than adding a ready event already established by
state publication. A future deterministic calibration can withhold the fixture
ring only after a real command publication and record that hold separately;
it must still fail the original bound and cannot become shipping behavior.
Do not substitute virtual time, wait for a ring pass before each timed call,
shorten a product poll, increase limits, drop slow samples, add warm-ups or
serialize the suite as an alleged readiness fix.

If a change to ring cadence or action confirmation is proposed, First must
approve that specific scope and qualification environment. Until phase evidence
exists, retain the failure and treat scheduler, filesystem and product-loop
contributions as unresolved. No cause, infrastructure classification or waiver
is claimed by this audit.
