# Voice fixtures on a loaded development host

Task [#739](https://github.com/autonomous-ai/lampOS/issues/739) concerns host
tests, not a change to the lamp's timing policy. `scenario_clock` and the
fixture helpers are compiled only under `cfg(test)`. There is no runtime
flag, environment setting or wider production deadline. Ordinary builds
continue to use the existing monotonic clock.

## Stepped scenarios

The six endpoint cases with an in-process classifier explicitly select
synchronous peers:

- `endpoint_session::complete_verdict_ends_early_but_resumed_speech_keeps_the_turn`
- `endpoint_session::verdicts_after_the_result_timeout_fall_back_on_every_turn`
- `endpoint_session::rapid_resumes_never_end_the_turn_early`
- `endpoint_session::a_complete_verdict_ends_at_one_second_and_its_answer_plays`
- `endpoint_session::a_worker_failure_falls_back_through_recovery_and_then_resumes`
- `endpoint_session::duplex_complete_verdict_ends_at_one_second_and_its_answer_plays`

The pilot fixture makes its mode explicit at every scenario. Its 28
single-cycle scenarios use stepped peers, including the original ring and
SIGTERM regressions and the deliberate privacy-expiry regression. Rapid
mute/reopen and the stalled metadata writer retain real-time peers because
they exercise real service waits or a separately running writer.

The production coordinator, Unix channels, packet identities/sequences,
privacy gate, input admission and speaker `BoundaryGuard` remain in use.
Before each coordinator iteration, every emulated peer completes one
bounded step; scenario time then advances by 2 ms. A clock read never
advances time or runs a peer. Capture retains its 10 ms frame schedule.
The pilot privacy peer's requested 5 ms cadence becomes 6 ms on this
2 ms grid; permitted freshness remains strictly below 50 ms. The endpoint
peer's 10 ms privacy schedule is unchanged.

The scope belongs to one thread and cannot be sent or shared. Concurrent
scopes are independent; newly spawned threads retain the host clock.
Nested scopes and recursive pumps are rejected. A callback panic releases
its state and restores the host clock; an old guard cannot clear a later
scope. Session lifetime, peer schedules, envelope timestamps and age checks
share one clock. The 50 ms privacy, 60 ms reopening and 100 ms channel
boundaries remain unchanged.

Real `ProcessClassifier` workers retain real-time peers. The stepped endpoint
entry accepts an in-process classifier double explicitly, including its
scripted failure/recovery timer. Original functional assertions remain.
Pilot speaker-stall and revocation assertions now measure policy time;
the separately printed conversation/cleanup elapsed duration remains host
time. This does not qualify every other timed test as immune to scheduling.

The endpoint fixture's synthetic capture stream has one immutable source
origin and a sample block every 10 ms. Both peer modes retain each block's
scheduled acquisition timestamp when delivering a backlog. A delayed thread
therefore cannot manufacture a gap in these continuous synthetic samples.
Envelope delivery, the real classifier and unscoped policy clocks stay real;
old source audio is never refreshed to the delivery time.

Two controlled-clock regressions send through actual Unix channels: a 60 ms
delivery pause preserves the original contiguous sample stream, and fresh
envelopes carrying source audio aged 98/100/102 ms retain the unchanged
admission result (accept/reject/reject). Failed scenarios print bounded
after-send source-gap, delivery-gap and source-age diagnostics. These are
producer observations, not coordinator consumption ages or lamp latency.

The pilot fixture uses the same continuous source model in both modes,
with its origin set when the capture peer receives `StartCapture`. Each
bounded peer step sends at most 64 due blocks, retaining their original
10 ms acquisition schedule. Three deterministic checks cover a 60 ms
delivery pause, input admission at 99,999 us versus refusal at 100,000 us,
and rejection of an actual dropped block or source gap. Rapid-reopen
failures print each unsuccessful cycle's diagnostics before the service
assertion, including the largest capture send-attempt gap and oldest source
age at an attempt. These include failed send attempts; they do not measure
coordinator consumption. Its real privacy peer and 50 ms bound remain
unchanged.

## Real-provider ordering scenarios

The EarlyEnd module runs the actual provider worker, Gemini supervisor and
a scripted WebSocket service on a real Tokio runtime. Its ten coordinator
scenarios run eleven `Setup` conversations (the option comparison runs
twice). Every conversation explicitly selects a per-instance privacy
freshness bound of 100,000 us, matching the existing worker IPC ceiling,
and a 12-second real conversation watchdog. This is bounded host scheduling
tolerance, not an arbitrary-load guarantee. The test-only constructor
accepts positive bounds at most 100,000 us; it and the override field do not
exist in normal builds. Every ordinary gate remains at 50,000 us, including
unrelated tests.

Capture/privacy/speaker producers start after the actual provider Ready
barrier. Slow provider setup cannot fill their queues with old samples.
The synthetic capture source retains its immutable 10 ms acquisition
schedule even when delivery is delayed. Original source timestamps, packet
order, 100 ms IPC/capture bounds, 60 ms stable reopening and the shared
300 ms shutdown remain intact. The 12-second watchdog begins immediately
before conversation; it excludes the existing three-second provider
readiness wait and subsequent cleanup. The fixture's sleeping child
processes still require forced termination, as before; their teardown is
not claimed as cooperative shutdown evidence.

The provider and scripted service share one current-thread Tokio runtime.
The fixture obtains the service's abort handle within the existing Ready
budget, then requests its cancellation before coordinator shutdown sends
provider Stop. Capture, privacy and provider peers retain their normal
cleanup order and budget. Both Tokio tasks are joined; unexpected task
panics propagate, and only cancellation requested by the fixture is accepted.
This prevents a closing provider connection from racing further scripted
answer sends, without swallowing a background failure.

The separate Tokio name-note test exercises the Gemini supervisor directly;
it does not construct `Setup`, capture peers or a privacy gate. It retains
its original continued/abandoned request flow. Both fixture entrypoints
reject an active scenario clock before starting child or provider resources,
so provider, envelope and coordinator clocks remain real and coherent.

Six privacy regressions cover exact observation, state and continuity
boundaries; per-instance/default isolation; immediate mute and 60 ms
reopening; invalid configuration; and future, stale and reordered
observations. Two EarlyEnd fixture regressions prove rejection of a scoped
clock and continuous source timestamps after delayed delivery, including
actual input admission at 99,999 us and refusal at exactly 100,000 us.
Three further checks cover requested cancellation, unexpected cancellation
and propagation of a background panic.
Every original scenario assertion remains, including the lone-name case's
one request with audio blocks 76 through 290, one early end, no abandoned
request and 48 speaker chunks. These tests do not measure the lamp's 50 ms
privacy response.

## Actual processes and cleanup

Child startup, exit observation and the shared shutdown watchdog keep real
host deadlines. Scoped protocol deadlines, including camera teardown, use
logical time. Stepped pilot children are
actual `cat` processes: a bounded echo proves that each has run and read its
input before scenario time starts. After closing input, bounded
`waitid(WNOWAIT)` observes successful exit of that exact PID. Nonzero exit,
signal death, timeout and an already reaped child are errors. The production
supervisor still owns the actual reap and exit-status checks.

Shutdown pumps receive the coordinator's existing shared 300 ms real
deadline (750 ms for diagnostic cleanup). They neither grant a fresh budget
per child nor advance scenario time or bypass the final receipt drain.
Fault injection cannot advance the shutdown clock. SIGTERM still signals
the actual test process. Real wall-clock watchdogs remain; logical scenario
durations and revocation intervals are not host or lamp latency measurements.
The shared real deadline still terminates a child that has not exited; these
stepped cases do not measure the camera's separate teardown deadline.

Clock tests cover host pauses, independent scopes, unscoped threads,
reentrancy, panic cleanup, callback-error recovery, watchdog expiry before and
after a peer step, frozen shutdown time and unchanged channel expiry.
A stepped coordinator regression advances exactly 50 ms after an accepted
privacy lease, then queues a fresh mute on the real channel. The existing
pre-drain freshness check must fail without a restartable physical-mute
token. Consumption also includes the ordinary 2 ms step and the lease's prior
age; this proves expiry and ordering, not exact consumption age. Actual
cleanup and reaping are still asserted.

## Motor setup

The motor fixture executes and reaps an inert preflight arm of its exact
frozen wrapper before starting the real supervisor protocol. It requires
the one-byte marker and successful exit; missing/wrong marker, nonzero exit
or timeout fail and reap the owned child. Preflight does not start a worker,
bind its sockets, open a device or establish motor readiness. The frozen
executable's identity and hash remain checked.

This separates a measured fresh-script execution cost on Fourth's Intel
Mac from Ready/authority tests. It does not explain every historical timeout
or qualify cold production startup. Original two-second fixture predicates
and production startup, progress, inhibit, permit and teardown bounds remain.
Failure output includes child phase, connection/Ready/reap state and the
observed parent poll gap.

The crash-preserves-voice fixture creates its active reply after config
freezing and unrelated child setup. While servicing the optional motor it
refreshes that same owner's existing 500 ms capture/admission lease, as live
capture would. It neither creates a replacement owner nor enlarges the
lease. Original audio-dispatch and cleanup assertions remain.

## Qualification and limits

Run from `lampOS/`:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
cargo test --locked --offline -p lamp-voice -- --test-threads=1
cargo test --locked --offline -p lamp-voice-eval -- --test-threads=1
cargo test --locked --offline -p lamp-memory -- --test-threads=1
cargo build --workspace --locked --offline
```

The issue also requires ten full serial `lamp-voice` runs on the Mac beside
a Podman ARM64 build, and a green exact-head native ARM64 workspace workflow.
Source/build identity, raw results, load boundaries and failures belong in
the task's verification folder and Fourth's log. An isolated passing case
is not a passing full gate.

No lamp is accessed. A privacy peer's send-side gap is not the coordinator's
consumption age. The real 50 ms bound may still be tight on a loaded, hot
lamp; host fixtures cannot settle that question. First must measure physical
observation through coordinator consumption on the lamp with its actual
services and temperature, without widening the bound on synthetic evidence.
